//! Minimal MCP 2025-11-25 stdio server.

use std::io::{self, BufRead as _, BufReader, BufWriter, Read as _};
use std::path::PathBuf;
use std::sync::OnceLock;

use agent_seat_proto::{
    ApplicationLaunchRequest, ApplicationListRequest, Call, ClientGeometryRequest,
    ClientStateRequest, ClientWorkspaceRequest, Empty, KeyboardTypeRequest, Outcome,
    PointerClickRequest, PointerMoveRequest, PollRequest, Reply, SubscribeRequest, TargetRequest,
    Validate as _, WorkspaceRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::discovery;
use crate::seat::Seat;

const MCP_VERSION: &str = "2025-11-25";
const MAX_MCP_LINE_BYTES: usize = 1024 * 1024;

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
        let request = match serde_json::from_str::<RpcRequest>(&line) {
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
                write_error(&mut output, Value::Null, -32700, "parse error")?;
                continue;
            }
        };
        server.handle(request, &mut output)?;
    }
}

struct Server {
    socket: Option<PathBuf>,
    seat: Option<Seat>,
    negotiated: bool,
    initialized: bool,
}

impl Server {
    fn new(socket: Option<PathBuf>, seat: Option<Seat>) -> Self {
        Self {
            socket,
            seat,
            negotiated: false,
            initialized: false,
        }
    }

    fn handle<W: io::Write>(&mut self, request: RpcRequest, output: &mut W) -> Result<(), String> {
        if request.method == "notifications/initialized" {
            if self.negotiated {
                self.initialized = true;
            }
            return Ok(());
        }
        if request.method == "notifications/cancelled" {
            return Ok(());
        }

        let Some(id) = request.id else {
            return Ok(());
        };
        match request.method.as_str() {
            "initialize" => self.initialize(id, request.params, output),
            "ping" => write_result(output, id, &json!({})),
            "tools/list" if self.initialized => self.list_tools(id, request.params, output),
            "tools/call" if self.initialized => self.call_tool(id, request.params, output),
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
        if self.negotiated {
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
        self.negotiated = true;
        let version = if params.protocol_version == MCP_VERSION {
            params.protocol_version
        } else {
            MCP_VERSION.to_owned()
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
                "instructions": "Use seat_status first. Observe before mutation; treat stale and timed_out results as requiring a fresh observation. The provider, not this companion, owns every grant and policy decision."
            }),
        )
    }

    fn list_tools<W: io::Write>(
        &self,
        id: Value,
        params: Value,
        output: &mut W,
    ) -> Result<(), String> {
        let params = if params.is_null() { json!({}) } else { params };
        let _params: ListToolsParams = match serde_json::from_value::<ListToolsParams>(params) {
            Ok(params) if params.cursor.is_none() => params,
            _ => return write_error(output, id, -32602, "invalid tools/list parameters"),
        };
        let result = ToolList { tools: tools() };
        write_result(output, id, &result)
    }

    fn call_tool<W: io::Write>(
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

        let result = match self.provider_call(call) {
            Ok(outcome) => tool_outcome(outcome),
            Err(error) => tool_error(error.code, error.retry, &error.message),
        };
        write_result(output, id, &result)
    }

    fn provider_call(&mut self, call: Call) -> Result<Outcome, ProviderFailure> {
        if self.seat.is_none() {
            let path = discovery::resolve(self.socket.as_deref())
                .map_err(|error| ProviderFailure {
                    code: error.code(),
                    retry: error.retry(),
                    message: error.to_string(),
                })?
                .ok_or_else(|| ProviderFailure {
                    code: "unavailable",
                    retry: "reconnect",
                    message: "no live Agent Seat provider is advertised".to_owned(),
                })?;
            self.seat = Some(Seat::connect(&path).map_err(|error| ProviderFailure {
                code: error.code(),
                retry: error.retry(),
                message: error.to_string(),
            })?);
        }
        let result = self
            .seat
            .as_mut()
            .ok_or_else(|| ProviderFailure {
                code: "unavailable",
                retry: "reconnect",
                message: "provider session is unavailable".to_owned(),
            })?
            .call(call);
        match result {
            Ok(response) => Ok(response.outcome),
            Err(error) => {
                self.seat = None;
                Err(ProviderFailure {
                    code: error.code(),
                    retry: error.retry(),
                    message: error.to_string(),
                })
            }
        }
    }
}

struct ProviderFailure {
    code: &'static str,
    retry: &'static str,
    message: String,
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
            error: ErrorBody { code, message },
        },
    )
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
        None | Some(Value::Null | Value::String(_)) => true,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
}

fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Box<[Tool]>> = OnceLock::new();
    TOOLS.get_or_init(build_tools)
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
            name: "keyboard_type",
            title: "Type text into client",
            description: "Type bounded text through the current X11 keyboard layout only when the target already owns keyboard focus.",
            input_schema: object_with_target(
                json!({"text":{"type":"string","minLength":1,"maxLength":256}}),
                &["text"],
            ),
        },
        Tool {
            name: "capture_obscured",
            title: "Capture client pixels",
            description: "Capture one freshly observed client's own pixels, including content currently covered by other windows.",
            input_schema: target(),
        },
    ]
    .into_boxed_slice()
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
        "capture_obscured" => arguments!(TargetRequest, CaptureObscured),
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
        CaptureData, CaptureFormat, CaptureReply, ClientId, Generation, Validate as _,
    };

    use super::*;

    #[test]
    fn every_tool_has_a_closed_object_schema() {
        assert_eq!(tools().len(), 16);
        for tool in tools() {
            assert_eq!(tool.input_schema["type"], "object");
            assert_eq!(tool.input_schema["additionalProperties"], false);
        }
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
            ("capture_obscured", json!({"client":1,"generation":0})),
        ];
        for (name, arguments) in calls {
            let call = translate_call(name, Some(arguments)).expect("valid tool fixture");
            call.validate().expect("valid wire call");
        }
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
    }
}
