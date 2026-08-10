//! Minimal dual-era MCP stdio server.

use std::collections::BTreeMap;
use std::io::{self, BufRead as _, BufReader, BufWriter, Read as _};
use std::path::PathBuf;
use std::sync::OnceLock;

use agent_seat_proto::{
    ApplicationLaunchRequest, ApplicationListRequest, Call, CaptureRegionRequest,
    ClientGeometryRequest, ClientId, ClientStateRequest, ClientWorkspaceRequest, Empty, Generation,
    KeyboardKeyRequest, KeyboardTypeRequest, KeyboardWriteRequest, Outcome, PointerButton,
    PointerClickRequest, PointerMoveRequest, PollRequest, Reply, SubscribeRequest, TargetRequest,
    TextInsertRequest, Validate as _, WorkspaceRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::discovery;
use crate::seat::Seat;

const MODERN_MCP_VERSION: &str = "2026-07-28";
const LEGACY_MCP_VERSION: &str = "2025-11-25";
const SUPPORTED_MCP_VERSIONS: [&str; 2] = [MODERN_MCP_VERSION, LEGACY_MCP_VERSION];
const MODERN_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const MODERN_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const MODERN_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const STATIC_RESULT_TTL_MS: u64 = 3_600_000;
const MAX_MODERN_CONTEXTS: usize = 8;
const MAX_POINTER_SLOTS: usize = 32;
const MAX_POINTER_SLOT_NAME_BYTES: usize = 64;
const MAX_MCP_LINE_BYTES: usize = 1024 * 1024;
const KEYBOARD_KEYS: [&str; 63] = [
    "backspace",
    "delete",
    "enter",
    "escape",
    "tab",
    "space",
    "insert",
    "home",
    "end",
    "page_up",
    "page_down",
    "arrow_left",
    "arrow_right",
    "arrow_up",
    "arrow_down",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
];

pub(crate) fn serve(socket: Option<PathBuf>, seat: Option<Seat>) -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());
    let mut server = Server::new(socket, seat);
    let mut line = String::new();

    loop {
        line.clear();
        let mut bounded = input.by_ref().take((MAX_MCP_LINE_BYTES + 1) as u64);
        let count = bounded
            .read_line(&mut line)
            .map_err(|error| format!("cannot read MCP stdio: {error}"))?;
        if count == 0 {
            return Ok(());
        }
        if count > MAX_MCP_LINE_BYTES {
            return Err(format!("MCP message exceeds {MAX_MCP_LINE_BYTES} bytes"));
        }
        while line.ends_with(['\n', '\r']) {
            line.pop();
        }
        if line.is_empty() {
            write_error(&mut output, Value::Null, -32600, "empty request")?;
            continue;
        }
        let value = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(_) => {
                write_error(&mut output, Value::Null, -32700, "parse error")?;
                continue;
            }
        };
        if value.get("id").is_some_and(Value::is_null) {
            write_error(&mut output, Value::Null, -32600, "invalid JSON-RPC request")?;
            continue;
        }
        let request = match serde_json::from_value::<RpcRequest>(value) {
            Ok(request)
                if request.jsonrpc == "2.0"
                    && !request.method.is_empty()
                    && request.method.len() <= 128
                    && valid_id(&request.id) =>
            {
                request
            }
            Ok(_) => {
                write_error(&mut output, Value::Null, -32600, "invalid JSON-RPC request")?;
                continue;
            }
            Err(_) => {
                write_error(&mut output, Value::Null, -32600, "invalid JSON-RPC request")?;
                continue;
            }
        };
        server.handle(request, &mut output)?;
    }
}

struct Server {
    socket: Option<PathBuf>,
    inherited_seat: Option<Seat>,
    legacy_session: Option<ProviderSession>,
    legacy_negotiated: bool,
    legacy_initialized: bool,
    modern_contexts: Contexts<ProviderSession>,
}

impl Server {
    fn new(socket: Option<PathBuf>, seat: Option<Seat>) -> Self {
        Self {
            socket,
            inherited_seat: seat,
            legacy_session: None,
            legacy_negotiated: false,
            legacy_initialized: false,
            modern_contexts: Contexts::new(MAX_MODERN_CONTEXTS),
        }
    }

    fn handle<W: io::Write>(&mut self, request: RpcRequest, output: &mut W) -> Result<(), String> {
        if request.method == "notifications/initialized" {
            if self.legacy_negotiated {
                self.legacy_initialized = true;
            }
            return Ok(());
        }
        if request.method == "notifications/cancelled" {
            return Ok(());
        }

        let Some(id) = request.id else {
            return Ok(());
        };
        if request.method == "initialize" {
            return self.initialize(id, request.params, output);
        }
        if has_modern_version(&request.params) || request.method == "server/discover" {
            return self.handle_modern(id, &request.method, request.params, output);
        }
        match request.method.as_str() {
            "ping" => write_result(output, id, &json!({})),
            "tools/list" if self.legacy_initialized => {
                self.list_tools(id, request.params, Era::Legacy, output)
            }
            "tools/call" if self.legacy_initialized => {
                self.call_tool_legacy(id, request.params, output)
            }
            "tools/list" | "tools/call" => {
                write_error(output, id, -32002, "server is not initialized")
            }
            _ => write_error(output, id, -32601, "method not found"),
        }
    }

    fn initialize<W: io::Write>(
        &mut self,
        id: Value,
        params: Value,
        output: &mut W,
    ) -> Result<(), String> {
        if self.legacy_negotiated {
            return write_error(output, id, -32600, "initialize was already completed");
        }
        let params: InitializeParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(_) => {
                return write_error(output, id, -32602, "invalid initialize parameters");
            }
        };
        if !params.capabilities.is_object() || !valid_implementation(&params.client_info) {
            return write_error(output, id, -32602, "invalid initialize parameters");
        }
        self.legacy_negotiated = true;
        let version = if params.protocol_version == LEGACY_MCP_VERSION {
            params.protocol_version
        } else {
            LEGACY_MCP_VERSION.to_owned()
        };
        write_result(
            output,
            id,
            &json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "agent-seat-mcp",
                    "title": "Agent Seat",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "Use seat_status first. Observe before mutation; prefer focused key commands, titles, and small capture regions. Use text_insert for exact multilingual or long text; it replaces X11 clipboard ownership and reports delivery, not application insertion. Never mutate XKB or bypass Agent Seat through a shell/browser clipboard. Save a pointer slot only after verifying that click, and reobserve when UI changes. Treat stale or timed_out as requiring fresh observation. The provider owns grants and policy."
            }),
        )
    }

    fn handle_modern<W: io::Write>(
        &mut self,
        id: Value,
        method: &str,
        params: Value,
        output: &mut W,
    ) -> Result<(), String> {
        match validate_modern_metadata(&params) {
            Ok(()) => {}
            Err(ModernMetadataError::Invalid) => {
                return write_error(output, id, -32602, "invalid modern request metadata");
            }
            Err(ModernMetadataError::Unsupported(requested)) => {
                return write_error_data(
                    output,
                    id,
                    -32022,
                    "Unsupported protocol version",
                    json!({"supported":SUPPORTED_MCP_VERSIONS,"requested":requested}),
                );
            }
        }
        match method {
            "server/discover" => self.discover(id, params, output),
            "tools/list" => self.list_tools(id, params, Era::Modern, output),
            "tools/call" => self.call_tool_modern(id, params, output),
            _ => write_error(output, id, -32601, "method not found"),
        }
    }

    fn discover<W: io::Write>(
        &self,
        id: Value,
        params: Value,
        output: &mut W,
    ) -> Result<(), String> {
        if serde_json::from_value::<DiscoverParams>(params).is_err() {
            return write_error(output, id, -32602, "invalid server/discover parameters");
        }
        write_result(
            output,
            id,
            &modern_result(json!({
                "supportedVersions": SUPPORTED_MCP_VERSIONS,
                "capabilities": {"tools":{"listChanged":false}},
                "instructions": "Check the seat; observe before acting. Prefer focused key commands, titles, and small capture regions. Use text_insert for exact multilingual or long text; it replaces X11 clipboard ownership and reports delivery, not application insertion. Never mutate XKB or bypass Agent Seat through a shell/browser clipboard. Save a pointer slot only after verifying that click; reobserve changed UI. Report qualified work exactly.",
                "ttlMs": STATIC_RESULT_TTL_MS,
                "cacheScope": "public"
            })),
        )
    }

    fn list_tools<W: io::Write>(
        &self,
        id: Value,
        params: Value,
        era: Era,
        output: &mut W,
    ) -> Result<(), String> {
        let params = if params.is_null() { json!({}) } else { params };
        let _params: ListToolsParams = match serde_json::from_value::<ListToolsParams>(params) {
            Ok(params) if params.cursor.is_none() => params,
            _ => return write_error(output, id, -32602, "invalid tools/list parameters"),
        };
        let result = ToolList { tools: tools(era) };
        match era {
            Era::Legacy => write_result(output, id, &result),
            Era::Modern => {
                let value = serde_json::to_value(result)
                    .map_err(|error| format!("cannot encode MCP tool list: {error}"))?;
                write_result(
                    output,
                    id,
                    &modern_result_with(
                        value,
                        json!({"ttlMs":STATIC_RESULT_TTL_MS,"cacheScope":"public"}),
                    )?,
                )
            }
        }
    }

    fn call_tool_legacy<W: io::Write>(
        &mut self,
        id: Value,
        params: Value,
        output: &mut W,
    ) -> Result<(), String> {
        let params: CallToolParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(_) => {
                return write_error(output, id, -32602, "invalid tools/call parameters");
            }
        };
        if is_pointer_slot_tool(&params.name) {
            let result = match self.pointer_slot_legacy(&params.name, params.arguments) {
                Ok(result) => result,
                Err(error) => tool_error(error.code, error.retry, &error.message),
            };
            return write_result(output, id, &result);
        }
        let call = match translate_call(&params.name, params.arguments) {
            Ok(call) => call,
            Err(CallError::UnknownTool) => {
                return write_error(output, id, -32602, "unknown tool");
            }
            Err(CallError::Arguments(message)) => {
                return write_result(
                    output,
                    id,
                    &tool_error("invalid_argument", "never", &message),
                );
            }
        };

        let result = match self.provider_call_legacy(call) {
            Ok(outcome) => tool_outcome(outcome),
            Err(error) => tool_error(error.code, error.retry, &error.message),
        };
        write_result(output, id, &result)
    }

    fn call_tool_modern<W: io::Write>(
        &mut self,
        id: Value,
        params: Value,
        output: &mut W,
    ) -> Result<(), String> {
        let params: CallToolParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(_) => {
                return write_error(output, id, -32602, "invalid tools/call parameters");
            }
        };
        let (context, arguments) = match take_context(params.arguments) {
            Ok(value) => value,
            Err(message) => {
                return write_result(
                    output,
                    id,
                    &modern_tool_result(tool_error("invalid_argument", "never", message)),
                );
            }
        };
        if params.name == "seat_release" {
            if arguments
                .as_ref()
                .and_then(Value::as_object)
                .is_some_and(|arguments| !arguments.is_empty())
            {
                return write_result(
                    output,
                    id,
                    &modern_tool_result(tool_error(
                        "invalid_argument",
                        "never",
                        "seat_release accepts only context",
                    )),
                );
            }
            let result = match context {
                Some(context) if self.modern_contexts.remove(context).is_some() => tool_result(
                    json!({"status":"ok","body":{"kind":"context_released","value":{"context":context}}}),
                    false,
                ),
                Some(_) => tool_error(
                    "stale_context",
                    "reconnect",
                    "Agent Seat context is unknown or expired",
                ),
                None => tool_error("invalid_argument", "never", "context is required"),
            };
            return write_result(output, id, &modern_tool_result(result));
        }
        if params.name == "seat_status" {
            if context.is_some() {
                return write_result(
                    output,
                    id,
                    &modern_tool_result(tool_error(
                        "invalid_argument",
                        "never",
                        "seat_status does not accept a context",
                    )),
                );
            }
            let call = match translate_call(&params.name, arguments) {
                Ok(call) => call,
                Err(error) => {
                    return self.write_modern_call_error(id, error, output);
                }
            };
            let result = match self.create_modern_context(call) {
                Ok((context, outcome)) => tool_outcome(outcome).with_context(context),
                Err(error) => tool_error(error.code, error.retry, &error.message),
            };
            return write_result(output, id, &modern_tool_result(result));
        }
        if is_pointer_slot_tool(&params.name) {
            let Some(context) = context else {
                return write_result(
                    output,
                    id,
                    &modern_tool_result(tool_error(
                        "invalid_argument",
                        "never",
                        "context is required",
                    )),
                );
            };
            let result = match self.pointer_slot_modern(context, &params.name, arguments) {
                Ok(result) => result,
                Err(error) => tool_error(error.code, error.retry, &error.message),
            };
            return write_result(output, id, &modern_tool_result(result));
        }
        let call = match translate_call(&params.name, arguments) {
            Ok(call) => call,
            Err(error) => return self.write_modern_call_error(id, error, output),
        };
        let Some(context) = context else {
            return write_result(
                output,
                id,
                &modern_tool_result(tool_error(
                    "invalid_argument",
                    "never",
                    "context is required",
                )),
            );
        };
        let result = match self.provider_call_modern(context, call) {
            Ok(outcome) => tool_outcome(outcome),
            Err(error) => tool_error(error.code, error.retry, &error.message),
        };
        write_result(output, id, &modern_tool_result(result))
    }

    fn write_modern_call_error<W: io::Write>(
        &self,
        id: Value,
        error: CallError,
        output: &mut W,
    ) -> Result<(), String> {
        match error {
            CallError::UnknownTool => write_error(output, id, -32602, "unknown tool"),
            CallError::Arguments(message) => write_result(
                output,
                id,
                &modern_tool_result(tool_error("invalid_argument", "never", &message)),
            ),
        }
    }

    fn create_modern_context(&mut self, call: Call) -> Result<(u64, Outcome), ProviderFailure> {
        if self.modern_contexts.is_full() {
            return Err(ProviderFailure {
                code: "capacity",
                retry: "reconnect",
                message: format!("modern Agent Seat context limit {MAX_MODERN_CONTEXTS} reached"),
            });
        }
        let mut session = ProviderSession::new(self.connect_seat()?);
        let outcome = call_seat(&mut session.seat, call)?;
        let context = self.modern_contexts.insert(session)?;
        Ok((context, outcome))
    }

    fn provider_call_modern(
        &mut self,
        context: u64,
        call: Call,
    ) -> Result<Outcome, ProviderFailure> {
        let result = match self.modern_contexts.get_mut(context) {
            Some(session) => call_seat(&mut session.seat, call),
            None => {
                return Err(ProviderFailure {
                    code: "stale_context",
                    retry: "reconnect",
                    message: "Agent Seat context is unknown or expired".to_owned(),
                });
            }
        };
        if result.is_err() {
            self.modern_contexts.remove(context);
        }
        result
    }

    fn provider_call_legacy(&mut self, call: Call) -> Result<Outcome, ProviderFailure> {
        if self.legacy_session.is_none() {
            self.legacy_session = Some(ProviderSession::new(self.connect_seat()?));
        }
        let result = call_seat(
            &mut self
                .legacy_session
                .as_mut()
                .ok_or_else(unavailable_provider_session)?
                .seat,
            call,
        );
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                self.legacy_session = None;
                Err(error)
            }
        }
    }

    fn pointer_slot_modern(
        &mut self,
        context: u64,
        name: &str,
        arguments: Option<Value>,
    ) -> Result<ToolResult, ProviderFailure> {
        let result = match self.modern_contexts.get_mut(context) {
            Some(session) => pointer_slot_call(session, name, arguments),
            None => {
                return Err(ProviderFailure {
                    code: "stale_context",
                    retry: "reconnect",
                    message: "Agent Seat context is unknown or expired".to_owned(),
                });
            }
        };
        if result.is_err() {
            self.modern_contexts.remove(context);
        }
        result
    }

    fn pointer_slot_legacy(
        &mut self,
        name: &str,
        arguments: Option<Value>,
    ) -> Result<ToolResult, ProviderFailure> {
        if self.legacy_session.is_none() {
            self.legacy_session = Some(ProviderSession::new(self.connect_seat()?));
        }
        let result = pointer_slot_call(
            self.legacy_session
                .as_mut()
                .ok_or_else(unavailable_provider_session)?,
            name,
            arguments,
        );
        if result.is_err() {
            self.legacy_session = None;
        }
        result
    }

    fn connect_seat(&mut self) -> Result<Seat, ProviderFailure> {
        if let Some(seat) = self.inherited_seat.take() {
            return Ok(seat);
        }
        let path = discovery::resolve(self.socket.as_deref())
            .map_err(provider_failure)?
            .ok_or_else(|| ProviderFailure {
                code: "unavailable",
                retry: "reconnect",
                message: "no live Agent Seat provider is advertised".to_owned(),
            })?;
        Seat::connect(&path).map_err(provider_failure)
    }
}

fn call_seat(seat: &mut Seat, call: Call) -> Result<Outcome, ProviderFailure> {
    seat.call(call)
        .map(|response| response.outcome)
        .map_err(provider_failure)
}

fn provider_failure(error: impl ProviderError) -> ProviderFailure {
    ProviderFailure {
        code: error.code(),
        retry: error.retry(),
        message: error.to_string(),
    }
}

trait ProviderError: std::fmt::Display {
    fn code(&self) -> &'static str;
    fn retry(&self) -> &'static str;
}

impl ProviderError for discovery::DiscoveryError {
    fn code(&self) -> &'static str {
        self.code()
    }

    fn retry(&self) -> &'static str {
        self.retry()
    }
}

impl ProviderError for crate::seat::SeatError {
    fn code(&self) -> &'static str {
        self.code()
    }

    fn retry(&self) -> &'static str {
        self.retry()
    }
}

fn unavailable_provider_session() -> ProviderFailure {
    ProviderFailure {
        code: "unavailable",
        retry: "reconnect",
        message: "provider session is unavailable".to_owned(),
    }
}

#[derive(Debug)]
struct ProviderFailure {
    code: &'static str,
    retry: &'static str,
    message: String,
}

struct ProviderSession {
    seat: Seat,
    pointer_slots: BTreeMap<String, PointerClickRequest>,
}

impl ProviderSession {
    fn new(seat: Seat) -> Self {
        Self {
            seat,
            pointer_slots: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerSlotSaveRequest {
    name: String,
    client: ClientId,
    generation: Generation,
    x: u32,
    y: u32,
    button: PointerButton,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerSlotReplayRequest {
    name: String,
    generation: Generation,
}

fn is_pointer_slot_tool(name: &str) -> bool {
    matches!(
        name,
        "pointer_slot_save" | "pointer_slot_replay" | "pointer_slots_list"
    )
}

fn pointer_slot_call(
    session: &mut ProviderSession,
    name: &str,
    arguments: Option<Value>,
) -> Result<ToolResult, ProviderFailure> {
    let arguments = arguments.unwrap_or_else(|| json!({}));
    match name {
        "pointer_slot_save" => {
            let request = match serde_json::from_value::<PointerSlotSaveRequest>(arguments) {
                Ok(request) => request,
                Err(error) => {
                    return Ok(tool_error("invalid_argument", "never", &error.to_string()));
                }
            };
            if !valid_pointer_slot_name(&request.name) {
                return Ok(tool_error(
                    "invalid_argument",
                    "never",
                    "slot name must be 1 to 64 bytes of ASCII letters, digits, '.', '_', or '-'",
                ));
            }
            let action = PointerClickRequest {
                target: TargetRequest {
                    client: request.client,
                    generation: request.generation,
                },
                x: request.x,
                y: request.y,
                button: request.button,
            };
            if let Err(error) = action.validate() {
                return Ok(tool_error("invalid_argument", "never", error));
            }
            if session.pointer_slots.len() >= MAX_POINTER_SLOTS
                && !session.pointer_slots.contains_key(&request.name)
            {
                return Ok(tool_error(
                    "capacity",
                    "never",
                    "pointer action slot limit 32 reached",
                ));
            }
            let replaced = session
                .pointer_slots
                .insert(request.name.clone(), action)
                .is_some();
            Ok(tool_result(
                json!({
                    "status":"ok",
                    "body":{"kind":"pointer_slot_saved","value":{
                        "name":request.name,
                        "action":action,
                        "replaced":replaced
                    }}
                }),
                false,
            ))
        }
        "pointer_slot_replay" => {
            let request = match serde_json::from_value::<PointerSlotReplayRequest>(arguments) {
                Ok(request) => request,
                Err(error) => {
                    return Ok(tool_error("invalid_argument", "never", &error.to_string()));
                }
            };
            if !valid_pointer_slot_name(&request.name) {
                return Ok(tool_error(
                    "invalid_argument",
                    "never",
                    "slot name is invalid",
                ));
            }
            let Some(mut action) = session.pointer_slots.get(&request.name).copied() else {
                return Ok(tool_error(
                    "not_found",
                    "refresh",
                    "pointer action slot is unknown in this provider session",
                ));
            };
            action.target.generation = request.generation;
            call_seat(&mut session.seat, Call::PointerClick(action)).map(tool_outcome)
        }
        "pointer_slots_list" => {
            if serde_json::from_value::<Empty>(arguments).is_err() {
                return Ok(tool_error(
                    "invalid_argument",
                    "never",
                    "pointer_slots_list accepts no arguments",
                ));
            }
            let slots = session
                .pointer_slots
                .iter()
                .map(|(name, action)| json!({"name":name,"action":action}))
                .collect::<Vec<_>>();
            Ok(tool_result(
                json!({
                    "status":"ok",
                    "body":{"kind":"pointer_slots","value":{
                        "limit":MAX_POINTER_SLOTS,
                        "slots":slots
                    }}
                }),
                false,
            ))
        }
        _ => Ok(tool_error(
            "invalid_argument",
            "never",
            "unknown local tool",
        )),
    }
}

fn valid_pointer_slot_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_POINTER_SLOT_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

struct Contexts<T> {
    limit: usize,
    next: u64,
    entries: BTreeMap<u64, T>,
}

impl<T> Contexts<T> {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            next: 1,
            entries: BTreeMap::new(),
        }
    }

    fn is_full(&self) -> bool {
        self.entries.len() >= self.limit
    }

    fn insert(&mut self, value: T) -> Result<u64, ProviderFailure> {
        if self.is_full() {
            return Err(ProviderFailure {
                code: "capacity",
                retry: "reconnect",
                message: format!("context limit {} reached", self.limit),
            });
        }
        let context = self.next;
        self.next = self.next.checked_add(1).ok_or_else(|| ProviderFailure {
            code: "capacity",
            retry: "reconnect",
            message: "context identity space is exhausted".to_owned(),
        })?;
        self.entries.insert(context, value);
        Ok(context)
    }

    fn get_mut(&mut self, context: u64) -> Option<&mut T> {
        self.entries.get_mut(&context)
    }

    fn remove(&mut self, context: u64) -> Option<T> {
        self.entries.remove(&context)
    }
}

#[derive(Clone, Copy)]
enum Era {
    Legacy,
    Modern,
}

enum ModernMetadataError {
    Invalid,
    Unsupported(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitializeParams {
    protocol_version: String,
    capabilities: Value,
    client_info: Value,
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoverParams {
    #[serde(rename = "_meta")]
    _meta: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListToolsParams {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallToolParams {
    name: String,
    #[serde(default)]
    arguments: Option<Value>,
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}

#[derive(Serialize)]
struct RpcResult<'a, T> {
    jsonrpc: &'static str,
    id: Value,
    result: &'a T,
}

#[derive(Serialize)]
struct RpcError<'a> {
    jsonrpc: &'static str,
    id: Value,
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: i32,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

fn write_result<W: io::Write, T: Serialize>(
    output: &mut W,
    id: Value,
    result: &T,
) -> Result<(), String> {
    write_json(
        output,
        &RpcResult {
            jsonrpc: "2.0",
            id,
            result,
        },
    )
}

fn write_error<W: io::Write>(
    output: &mut W,
    id: Value,
    code: i32,
    message: &str,
) -> Result<(), String> {
    write_json(
        output,
        &RpcError {
            jsonrpc: "2.0",
            id,
            error: ErrorBody {
                code,
                message,
                data: None,
            },
        },
    )
}

fn write_error_data<W: io::Write>(
    output: &mut W,
    id: Value,
    code: i32,
    message: &str,
    data: Value,
) -> Result<(), String> {
    write_json(
        output,
        &RpcError {
            jsonrpc: "2.0",
            id,
            error: ErrorBody {
                code,
                message,
                data: Some(data),
            },
        },
    )
}

fn has_modern_version(params: &Value) -> bool {
    params
        .get("_meta")
        .and_then(Value::as_object)
        .is_some_and(|metadata| metadata.contains_key(MODERN_PROTOCOL_VERSION))
}

fn validate_modern_metadata(params: &Value) -> Result<(), ModernMetadataError> {
    let metadata = params
        .get("_meta")
        .and_then(Value::as_object)
        .ok_or(ModernMetadataError::Invalid)?;
    let requested = metadata
        .get(MODERN_PROTOCOL_VERSION)
        .and_then(Value::as_str)
        .ok_or(ModernMetadataError::Invalid)?;
    if requested != MODERN_MCP_VERSION {
        return Err(ModernMetadataError::Unsupported(requested.to_owned()));
    }
    if !metadata
        .get(MODERN_CLIENT_CAPABILITIES)
        .is_some_and(Value::is_object)
    {
        return Err(ModernMetadataError::Invalid);
    }
    if metadata
        .get(MODERN_CLIENT_INFO)
        .is_some_and(|client| !valid_implementation(client))
    {
        return Err(ModernMetadataError::Invalid);
    }
    Ok(())
}

fn server_info() -> Value {
    json!({
        "name":"agent-seat-mcp",
        "title":"Agent Seat",
        "version":env!("CARGO_PKG_VERSION")
    })
}

fn modern_result(mut value: Value) -> Value {
    if let Value::Object(object) = &mut value {
        object.insert(
            "resultType".to_owned(),
            Value::String("complete".to_owned()),
        );
        object.insert(
            "_meta".to_owned(),
            json!({"io.modelcontextprotocol/serverInfo":server_info()}),
        );
    }
    value
}

fn modern_result_with(mut value: Value, extra: Value) -> Result<Value, String> {
    let Value::Object(object) = &mut value else {
        return Err("modern MCP result must be an object".to_owned());
    };
    let Value::Object(extra) = extra else {
        return Err("modern MCP result fields must be an object".to_owned());
    };
    object.extend(extra);
    Ok(modern_result(value))
}

fn modern_tool_result(result: ToolResult) -> Value {
    match serde_json::to_value(result) {
        Ok(value) => modern_result(value),
        Err(error) => modern_result(json!({
            "content":[{"type":"text","text":error.to_string()}],
            "isError":true
        })),
    }
}

fn take_context(arguments: Option<Value>) -> Result<(Option<u64>, Option<Value>), &'static str> {
    let mut arguments = match arguments {
        None => serde_json::Map::new(),
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Err("tool arguments must be an object"),
    };
    let context = match arguments.remove("context") {
        Some(Value::Number(number)) => Some(
            number
                .as_u64()
                .filter(|value| *value != 0)
                .ok_or("context must be a positive integer")?,
        ),
        Some(_) => return Err("context must be a positive integer"),
        None => None,
    };
    Ok((context, Some(Value::Object(arguments))))
}

fn write_json<W: io::Write, T: Serialize>(output: &mut W, value: &T) -> Result<(), String> {
    serde_json::to_writer(&mut *output, value)
        .map_err(|error| format!("cannot encode MCP response: {error}"))?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|error| format!("cannot write MCP response: {error}"))
}

fn valid_id(id: &Option<Value>) -> bool {
    match id {
        None | Some(Value::String(_)) => true,
        Some(Value::Number(number)) => number.as_i64().is_some() || number.as_u64().is_some(),
        _ => false,
    }
}

fn valid_implementation(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    ["name", "version"].into_iter().all(|field| {
        object
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty() && value.len() <= 128)
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolList<'a> {
    tools: &'a [Tool],
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
}

fn tools(era: Era) -> &'static [Tool] {
    static LEGACY_TOOLS: OnceLock<Box<[Tool]>> = OnceLock::new();
    static MODERN_TOOLS: OnceLock<Box<[Tool]>> = OnceLock::new();
    match era {
        Era::Legacy => LEGACY_TOOLS.get_or_init(build_tools),
        Era::Modern => MODERN_TOOLS.get_or_init(build_modern_tools),
    }
}

fn build_tools() -> Box<[Tool]> {
    let empty = || json!({"type":"object","additionalProperties":false});
    let target = || {
        json!({
            "type":"object",
            "properties":{
                "client":{"type":"integer","minimum":1},
                "generation":{"type":"integer","minimum":0}
            },
            "required":["client","generation"],
            "additionalProperties":false
        })
    };
    vec![
        Tool {
            name: "seat_status",
            title: "Agent Seat status",
            description: "Resolve and authenticate the current Agent Seat provider. Call this before desktop work.",
            input_schema: empty(),
        },
        Tool {
            name: "desktop_snapshot",
            title: "Desktop snapshot",
            description: "Observe the bounded Tier 0 desktop before choosing a target or mutation.",
            input_schema: empty(),
        },
        Tool {
            name: "events_subscribe",
            title: "Subscribe to desktop changes",
            description: "Start a bounded event stream; an empty kinds list subscribes to every core event.",
            input_schema: json!({
                "type":"object",
                "properties":{"kinds":{"type":"array","maxItems":8,"uniqueItems":true,"items":{"enum":["client_added","client_changed","client_removed","active_changed","workspace_changed","applications_changed"]}}},
                "additionalProperties":false
            }),
        },
        Tool {
            name: "events_poll",
            title: "Poll desktop changes",
            description: "Read bounded changes after a provider sequence cursor.",
            input_schema: json!({
                "type":"object",
                "properties":{
                    "after":{"type":"integer","minimum":0},
                    "limit":{"type":"integer","minimum":1,"maximum":1024},
                    "wait_ms":{"type":"integer","minimum":0,"maximum":30000}
                },
                "required":["after","limit","wait_ms"],
                "additionalProperties":false
            }),
        },
        Tool {
            name: "client_activate",
            title: "Activate client",
            description: "Request advertised EWMH activation of a freshly observed client, then report what was observed.",
            input_schema: target(),
        },
        Tool {
            name: "client_close",
            title: "Close client politely",
            description: "Request polite close only when the client advertises WM_DELETE_WINDOW, then report what was observed.",
            input_schema: target(),
        },
        Tool {
            name: "workspace_switch",
            title: "Switch workspace",
            description: "Request an advertised current-workspace change from a fresh snapshot.",
            input_schema: json!({
                "type":"object",
                "properties":{"workspace":{"type":"integer","minimum":0,"maximum":65535},"sequence":{"type":"integer","minimum":0}},
                "required":["workspace","sequence"],
                "additionalProperties":false
            }),
        },
        Tool {
            name: "client_workspace",
            title: "Send client to workspace",
            description: "Request moving a freshly observed client to an advertised workspace.",
            input_schema: object_with_target(json!({"workspace":{"type":"integer","minimum":0,"maximum":65535}}), &["workspace"]),
        },
        Tool {
            name: "client_state",
            title: "Change client state",
            description: "Request one supported EWMH state transition on a freshly observed client.",
            input_schema: object_with_target(
                json!({
                    "state":{"enum":["above","below","fullscreen","hidden","maximized_horizontal","maximized_vertical","demands_attention","sticky","shaded"]},
                    "action":{"enum":["add","remove","toggle"]}
                }),
                &["state","action"]
            ),
        },
        Tool {
            name: "client_geometry",
            title: "Change client geometry",
            description: "Request supported EWMH frame geometry for a freshly observed client.",
            input_schema: object_with_target(
                json!({"frame":{"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer"},"width":{"type":"integer","minimum":1,"maximum":4294967295_u64},"height":{"type":"integer","minimum":1,"maximum":4294967295_u64}},"required":["x","y","width","height"],"additionalProperties":false}}),
                &["frame"]
            ),
        },
        Tool {
            name: "applications_list",
            title: "List launchable applications",
            description: "List a bounded page of desktop entries visible under current provider policy.",
            input_schema: json!({
                "type":"object",
                "properties":{"cursor":{"type":"integer","minimum":0,"maximum":4294967295_u64,"default":0},"limit":{"type":"integer","minimum":1,"maximum":256}},
                "required":["limit"],
                "additionalProperties":false
            }),
        },
        Tool {
            name: "application_launch",
            title: "Launch desktop application",
            description: "Launch one current policy-approved desktop ID without a shell.",
            input_schema: json!({
                "type":"object",
                "properties":{"application":{"type":"string","minLength":9,"maxLength":256,"pattern":"^[^/\\u0000]+\\.desktop$"}},
                "required":["application"],
                "additionalProperties":false
            }),
        },
        Tool {
            name: "pointer_move",
            title: "Move pointer within client",
            description: "Move the pointer once to a currently visible client-relative point while the operator seat remains enabled.",
            input_schema: object_with_target(
                json!({
                    "x":{"type":"integer","minimum":0,"maximum":4294967295_u64},
                    "y":{"type":"integer","minimum":0,"maximum":4294967295_u64}
                }),
                &["x", "y"],
            ),
        },
        Tool {
            name: "pointer_click",
            title: "Click within client",
            description: "Move to and click one currently visible client-relative point while the operator seat remains enabled.",
            input_schema: object_with_target(
                json!({
                    "x":{"type":"integer","minimum":0,"maximum":4294967295_u64},
                    "y":{"type":"integer","minimum":0,"maximum":4294967295_u64},
                    "button":{"enum":["primary","middle","secondary"]}
                }),
                &["x", "y", "button"],
            ),
        },
        Tool {
            name: "pointer_slot_save",
            title: "Remember pointer click",
            description: "Save one previously verified client-relative click in the current provider session. A slot remembers coordinates, not a UI element; saving does not perform the click.",
            input_schema: object_with_target(
                json!({
                    "name":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9._-]+$"},
                    "x":{"type":"integer","minimum":0,"maximum":4294967295_u64},
                    "y":{"type":"integer","minimum":0,"maximum":4294967295_u64},
                    "button":{"enum":["primary","middle","secondary"]}
                }),
                &["name", "x", "y", "button"],
            ),
        },
        Tool {
            name: "pointer_slot_replay",
            title: "Replay remembered click",
            description: "Replay one remembered click using the target's freshly observed generation and all current provider geometry, visibility, hit-test, grant, and seat checks. Reobserve before relying on a slot after UI changes.",
            input_schema: json!({
                "type":"object",
                "properties":{
                    "name":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9._-]+$"},
                    "generation":{"type":"integer","minimum":0}
                },
                "required":["name","generation"],
                "additionalProperties":false
            }),
        },
        Tool {
            name: "pointer_slots_list",
            title: "List remembered clicks",
            description: "List the at most 32 pointer clicks remembered only in the current provider session.",
            input_schema: json!({
                "type":"object",
                "properties":{},
                "required":[],
                "additionalProperties":false
            }),
        },
        Tool {
            name: "keyboard_type",
            title: "Type text into client",
            description: "Type bounded text through the current X11 keyboard layout only when the target already owns keyboard focus.",
            input_schema: object_with_target(
                json!({"text":{"type":"string","minLength":1,"maxLength":256}}),
                &["text"],
            ),
        },
        Tool {
            name: "keyboard_key",
            title: "Send key to client",
            description: "Send one complete layout-aware key or shortcut only when the target already owns keyboard focus.",
            input_schema: object_with_target(
                json!({
                    "key":{"enum":KEYBOARD_KEYS.as_slice()},
                    "modifiers":{
                        "type":"array",
                        "items":{"enum":["control","alt","shift","super"]},
                        "maxItems":4,
                        "uniqueItems":true,
                        "default":[],
                        "description":"Use canonical order: control, alt, shift, super."
                    }
                }),
                &["key"],
            ),
        },
        Tool {
            name: "keyboard_write",
            title: "Write long-form text into client",
            description: "Type up to 4,096 characters and 16 KiB of preflighted multiline text through direct symbols in the current X11 keyboard layout. Refusal identifies the first unavailable scalar. Never change the user's XKB layout or mapping as a workaround. The target must retain keyboard focus; completion is reported exactly and may be interrupted.",
            input_schema: object_with_target(
                json!({"text":{"type":"string","minLength":1,"maxLength":4096}}),
                &["text"],
            ),
        },
        Tool {
            name: "text_insert",
            title: "Transfer exact text to client",
            description: "Offer up to 16,384 Unicode scalars and 32 KiB of exact UTF-8 to one freshly observed focused target. This separately granted write-only operation replaces X11 clipboard ownership; a clipboard manager may retain the text. It never reads the prior clipboard and reports selection delivery, not application insertion.",
            input_schema: object_with_target(
                json!({"text":{"type":"string","minLength":1,"maxLength":16384}}),
                &["text"],
            ),
        },
        Tool {
            name: "capture_obscured",
            title: "Capture client pixels",
            description: "Capture one freshly observed client's own pixels, including content currently covered by other windows.",
            input_schema: target(),
        },
        Tool {
            name: "capture_region",
            title: "Capture part of client",
            description: "Capture a bounded client-relative rectangle, including content covered by other windows. The region may be at most 1,024 pixels on either side and 262,144 pixels total.",
            input_schema: object_with_target(
                json!({
                    "x":{"type":"integer","minimum":0,"maximum":2047},
                    "y":{"type":"integer","minimum":0,"maximum":2047},
                    "width":{"type":"integer","minimum":1,"maximum":1024},
                    "height":{"type":"integer","minimum":1,"maximum":1024}
                }),
                &["x", "y", "width", "height"],
            ),
        },
    ]
    .into_boxed_slice()
}

fn build_modern_tools() -> Box<[Tool]> {
    let mut tools = build_tools().into_vec();
    for tool in &mut tools {
        if tool.name == "seat_status" {
            tool.description = "Open one of at most eight Agent Seat contexts and report status; carry the returned context until seat_release, provider failure, or companion exit.";
        } else {
            add_context_schema(&mut tool.input_schema);
        }
    }
    tools.push(Tool {
        name: "seat_release",
        title: "Release Agent Seat context",
        description: "Release one explicit Agent Seat context when desktop work is complete.",
        input_schema: json!({
            "type":"object",
            "properties":{"context":{"type":"integer","minimum":1}},
            "required":["context"],
            "additionalProperties":false
        }),
    });
    tools.into_boxed_slice()
}

fn add_context_schema(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let properties = object.entry("properties").or_insert_with(|| json!({}));
    if let Some(properties) = properties.as_object_mut() {
        properties.insert("context".to_owned(), json!({"type":"integer","minimum":1}));
    }
    let required = object.entry("required").or_insert_with(|| json!([]));
    if let Some(required) = required.as_array_mut() {
        required.push(Value::String("context".to_owned()));
    }
}

fn object_with_target(extra: Value, extra_required: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("client".to_owned(), json!({"type":"integer","minimum":1}));
    properties.insert(
        "generation".to_owned(),
        json!({"type":"integer","minimum":0}),
    );
    if let Value::Object(extra) = extra {
        properties.extend(extra);
    }
    let mut required = vec![
        Value::String("client".to_owned()),
        Value::String("generation".to_owned()),
    ];
    required.extend(
        extra_required
            .iter()
            .map(|field| Value::String((*field).to_owned())),
    );
    json!({
        "type":"object",
        "properties":properties,
        "required":required,
        "additionalProperties":false
    })
}

#[derive(Debug)]
enum CallError {
    UnknownTool,
    Arguments(String),
}

fn translate_call(name: &str, arguments: Option<Value>) -> Result<Call, CallError> {
    let arguments = arguments.unwrap_or_else(|| json!({}));
    macro_rules! arguments {
        ($type:ty, $variant:ident) => {
            serde_json::from_value::<$type>(arguments)
                .map(Call::$variant)
                .map_err(|error| CallError::Arguments(error.to_string()))
        };
    }
    let call = match name {
        "seat_status" => arguments!(Empty, SeatStatus),
        "desktop_snapshot" => arguments!(Empty, DesktopSnapshot),
        "events_subscribe" => arguments!(SubscribeRequest, EventsSubscribe),
        "events_poll" => arguments!(PollRequest, EventsPoll),
        "client_activate" => arguments!(TargetRequest, ClientActivate),
        "client_close" => arguments!(TargetRequest, ClientClose),
        "workspace_switch" => arguments!(WorkspaceRequest, WorkspaceSwitch),
        "client_workspace" => arguments!(ClientWorkspaceRequest, ClientWorkspace),
        "client_state" => arguments!(ClientStateRequest, ClientState),
        "client_geometry" => arguments!(ClientGeometryRequest, ClientGeometry),
        "applications_list" => arguments!(ApplicationListRequest, ApplicationsList),
        "application_launch" => arguments!(ApplicationLaunchRequest, ApplicationLaunch),
        "pointer_move" => arguments!(PointerMoveRequest, PointerMove),
        "pointer_click" => arguments!(PointerClickRequest, PointerClick),
        "keyboard_type" => arguments!(KeyboardTypeRequest, KeyboardType),
        "keyboard_write" => arguments!(KeyboardWriteRequest, KeyboardWrite),
        "keyboard_key" => arguments!(KeyboardKeyRequest, KeyboardKey),
        "text_insert" => arguments!(TextInsertRequest, TextInsert),
        "capture_obscured" => arguments!(TargetRequest, CaptureObscured),
        "capture_region" => arguments!(CaptureRegionRequest, CaptureRegion),
        _ => Err(CallError::UnknownTool),
    }?;
    call.validate()
        .map_err(|error| CallError::Arguments(error.to_owned()))?;
    Ok(call)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolResult {
    content: Vec<ToolContent>,
    structured_content: Value,
    is_error: bool,
}

impl ToolResult {
    fn with_context(mut self, context: u64) -> Self {
        if let Value::Object(object) = &mut self.structured_content {
            object.insert("context".to_owned(), Value::from(context));
        }
        if let Some(ToolContent::Text(content)) = self.content.first_mut() {
            content.text = self.structured_content.to_string();
        }
        self
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum ToolContent {
    Text(TextContent),
    Image(ImageContent),
}

#[derive(Serialize)]
struct TextContent {
    r#type: &'static str,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageContent {
    r#type: &'static str,
    data: String,
    mime_type: &'static str,
}

fn tool_outcome(outcome: Outcome) -> ToolResult {
    if let Outcome::Ok(Reply::Capture(capture)) = outcome {
        let structured_content = json!({
            "status":"ok",
            "body":{
                "kind":"capture",
                "value":{
                    "target":capture.target,
                    "width":capture.width,
                    "height":capture.height,
                    "format":capture.format
                }
            }
        });
        return ToolResult {
            content: vec![ToolContent::Image(ImageContent {
                r#type: "image",
                data: capture.data.into_string(),
                mime_type: "image/png",
            })],
            structured_content,
            is_error: false,
        };
    }
    if let Outcome::Ok(Reply::CaptureRegion(capture)) = outcome {
        let structured_content = json!({
            "status":"ok",
            "body":{
                "kind":"capture_region",
                "value":{
                    "target":capture.target,
                    "region":capture.region,
                    "format":capture.format
                }
            }
        });
        return ToolResult {
            content: vec![ToolContent::Image(ImageContent {
                r#type: "image",
                data: capture.data.into_string(),
                mime_type: "image/png",
            })],
            structured_content,
            is_error: false,
        };
    }
    let is_error = matches!(outcome, Outcome::Error(_));
    match serde_json::to_value(outcome) {
        Ok(structured_content) => tool_result(structured_content, is_error),
        Err(error) => tool_error("internal", "never", &error.to_string()),
    }
}

fn tool_error(code: &str, retry: &str, message: &str) -> ToolResult {
    tool_result(
        json!({
            "status":"error",
            "body":{"code":code,"retry":retry,"message":message}
        }),
        true,
    )
}

fn tool_result(structured_content: Value, is_error: bool) -> ToolResult {
    let text = structured_content.to_string();
    ToolResult {
        content: vec![ToolContent::Text(TextContent {
            r#type: "text",
            text,
        })],
        structured_content,
        is_error,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use agent_seat_proto::{
        CaptureData, CaptureFormat, CaptureRegion, CaptureRegionReply, CaptureReply, ClientId,
        Generation, Validate as _,
    };

    use super::*;

    #[test]
    fn every_tool_has_a_closed_object_schema() {
        assert_eq!(tools(Era::Legacy).len(), 23);
        for tool in tools(Era::Legacy) {
            assert_eq!(tool.input_schema["type"], "object");
            assert_eq!(tool.input_schema["additionalProperties"], false);
        }
    }

    #[test]
    fn modern_tools_make_provider_continuity_explicit() {
        let tools = tools(Era::Modern);
        assert_eq!(tools.len(), 24);
        for tool in tools {
            assert_eq!(tool.input_schema["type"], "object");
            assert_eq!(tool.input_schema["additionalProperties"], false);
            if tool.name == "seat_status" {
                assert!(tool.input_schema["properties"]["context"].is_null());
            } else {
                assert_eq!(tool.input_schema["properties"]["context"]["minimum"], 1);
                assert!(
                    tool.input_schema["required"]
                        .as_array()
                        .is_some_and(|required| required.contains(&json!("context")))
                );
            }
        }
        assert!(tools.iter().any(|tool| tool.name == "seat_release"));
    }

    #[test]
    fn modern_context_store_is_bounded_and_never_reuses_an_identity() {
        let mut contexts = Contexts::new(2);
        let first = contexts.insert("first").expect("first context");
        let second = contexts.insert("second").expect("second context");
        assert_eq!((first, second), (1, 2));
        assert!(contexts.insert("over capacity").is_err());
        assert_eq!(contexts.remove(first), Some("first"));
        let third = contexts.insert("third").expect("replacement context");
        assert_eq!(third, 3);
        assert_eq!(contexts.get_mut(second), Some(&mut "second"));
    }

    #[test]
    fn tool_arguments_translate_to_valid_wire_calls() {
        let calls = [
            ("seat_status", json!({})),
            ("desktop_snapshot", json!({})),
            ("events_subscribe", json!({"kinds":[]})),
            ("events_poll", json!({"after":0,"limit":8,"wait_ms":0})),
            ("client_activate", json!({"client":1,"generation":0})),
            ("client_close", json!({"client":1,"generation":0})),
            ("workspace_switch", json!({"workspace":0,"sequence":0})),
            (
                "client_workspace",
                json!({"client":1,"generation":0,"workspace":0}),
            ),
            (
                "client_state",
                json!({"client":1,"generation":0,"state":"fullscreen","action":"add"}),
            ),
            (
                "client_geometry",
                json!({"client":1,"generation":0,"frame":{"x":0,"y":0,"width":1,"height":1}}),
            ),
            ("applications_list", json!({"limit":16})),
            (
                "application_launch",
                json!({"application":"example.desktop"}),
            ),
            (
                "pointer_move",
                json!({"client":1,"generation":0,"x":10,"y":20}),
            ),
            (
                "pointer_click",
                json!({"client":1,"generation":0,"x":10,"y":20,"button":"primary"}),
            ),
            (
                "keyboard_type",
                json!({"client":1,"generation":0,"text":"Agent Seat\n"}),
            ),
            (
                "keyboard_write",
                json!({"client":1,"generation":0,"text":"verse one\nverse two\n"}),
            ),
            (
                "keyboard_key",
                json!({"client":1,"generation":0,"key":"l","modifiers":["control"]}),
            ),
            (
                "text_insert",
                json!({"client":1,"generation":0,"text":"Canción íntima\nMañana será mejor.\n"}),
            ),
            ("capture_obscured", json!({"client":1,"generation":0})),
            (
                "capture_region",
                json!({"client":1,"generation":0,"x":10,"y":20,"width":64,"height":32}),
            ),
        ];
        for (name, arguments) in calls {
            let call = translate_call(name, Some(arguments)).expect("valid tool fixture");
            call.validate().expect("valid wire call");
        }
    }

    #[test]
    fn every_published_keyboard_key_translates_to_the_wire_enum() {
        let mut unique = std::collections::BTreeSet::new();
        for key in KEYBOARD_KEYS {
            assert!(unique.insert(key), "duplicate keyboard key {key}");
            let call = translate_call(
                "keyboard_key",
                Some(json!({"client":1,"generation":0,"key":key})),
            )
            .expect("published keyboard key must translate");
            call.validate()
                .expect("published keyboard key must validate");
        }
        assert_eq!(unique.len(), KEYBOARD_KEYS.len());
    }

    #[test]
    fn extra_tool_arguments_are_rejected() {
        assert!(matches!(
            translate_call("seat_status", Some(json!({"extra":true}))),
            Err(CallError::Arguments(_))
        ));
    }

    #[test]
    fn capture_results_emit_one_image_without_structured_data_duplication() {
        let result = tool_outcome(Outcome::Ok(Reply::Capture(CaptureReply {
            target: TargetRequest {
                client: ClientId::new(NonZeroU64::MIN),
                generation: Generation::new(0),
            },
            width: 1,
            height: 1,
            format: CaptureFormat::Png,
            data: CaptureData::new("iVBORw0KGgo=").expect("capture fixture"),
        })));
        let value = serde_json::to_value(result).expect("MCP capture result");
        assert_eq!(value["content"][0]["type"], "image");
        assert_eq!(value["content"][0]["mimeType"], "image/png");
        assert_eq!(value["content"][0]["data"], "iVBORw0KGgo=");
        assert_eq!(value["structuredContent"]["body"]["value"]["width"], 1);
        assert!(value["structuredContent"]["body"]["value"]["data"].is_null());

        let result = tool_outcome(Outcome::Ok(Reply::CaptureRegion(CaptureRegionReply {
            target: TargetRequest {
                client: ClientId::new(NonZeroU64::MIN),
                generation: Generation::new(0),
            },
            region: CaptureRegion {
                x: 10,
                y: 20,
                width: 1,
                height: 1,
            },
            format: CaptureFormat::Png,
            data: CaptureData::new("iVBORw0KGgo=").expect("capture fixture"),
        })));
        let value = serde_json::to_value(result).expect("MCP region result");
        assert_eq!(value["content"][0]["type"], "image");
        assert_eq!(
            value["structuredContent"]["body"]["value"]["region"]["x"],
            10
        );
        assert!(value["structuredContent"]["body"]["value"]["data"].is_null());
    }

    #[test]
    fn pointer_slot_names_are_small_and_portable() {
        assert!(valid_pointer_slot_name("suno.download"));
        assert!(valid_pointer_slot_name("menu_to_download-2"));
        assert!(!valid_pointer_slot_name(""));
        assert!(!valid_pointer_slot_name("contains space"));
        assert!(!valid_pointer_slot_name(&"a".repeat(65)));
    }
}
