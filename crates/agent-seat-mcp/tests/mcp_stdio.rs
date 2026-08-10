//! Process-boundary MCP lifecycle and lazy-connection tests.

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::num::NonZeroU64;
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::thread;

use agent_seat_proto::{
    Assurance, Backend, BoundedList, BoundedText, Capability, ClientMessage, ErrorCode, Feature,
    Limits, MAX_EVENTS, MAX_POLL_WAIT_MS, MAX_REQUEST_FRAME_BYTES, MAX_RESPONSE_FRAME_BYTES,
    Outcome, PROTOCOL_NAME, PROTOCOL_REVISION, ProtocolError, ProviderInfo, ReadFrame, Response,
    Retry, Sequence, ServerMessage, SessionId, Welcome, read_frame, write_frame,
};
use serde_json::{Value, json};

fn run(input: &str, arguments: &[&str]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-seat-mcp"))
        .args(arguments)
        .env_remove("DISPLAY")
        .env_remove("AGENT_SEAT_SOCKET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn companion");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input.as_bytes())
        .expect("write requests");
    child.wait_with_output().expect("wait for companion")
}

fn lines(output: &std::process::Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "companion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect()
}

fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{},
        "io.modelcontextprotocol/clientInfo":{"name":"test","version":"1"}
    })
}

#[test]
fn initialization_and_tools_are_desktop_free_but_calls_resolve_lazily() {
    let requests = [
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":"2025-11-25",
                "capabilities":{},
                "clientInfo":{"name":"test","version":"1"}
            }
        }),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"seat_status","arguments":{}}}),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let output = run(&requests, &[]);
    let responses = lines(&output);

    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert!(
        responses[0]["result"]["instructions"]
            .as_str()
            .is_some_and(|instructions| instructions.contains("never mutate XKB"))
    );
    assert!(responses[0]["result"]["resultType"].is_null());
    assert_eq!(
        responses[0]["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(
        responses[1]["result"]["tools"]
            .as_array()
            .expect("tool array")
            .len(),
        22
    );
    let tool_names = responses[1]["result"]["tools"]
        .as_array()
        .expect("tool array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(tool_names.contains(&"pointer_click"));
    assert!(tool_names.contains(&"keyboard_type"));
    assert!(tool_names.contains(&"keyboard_key"));
    assert!(tool_names.contains(&"keyboard_write"));
    assert!(tool_names.contains(&"pointer_slot_save"));
    assert!(tool_names.contains(&"pointer_slot_replay"));
    assert!(tool_names.contains(&"pointer_slots_list"));
    assert!(tool_names.contains(&"capture_obscured"));
    assert!(tool_names.contains(&"capture_region"));
    let keyboard_write = responses[1]["result"]["tools"]
        .as_array()
        .expect("tool array")
        .iter()
        .find(|tool| tool["name"] == "keyboard_write")
        .expect("keyboard_write tool");
    assert!(
        keyboard_write["description"]
            .as_str()
            .is_some_and(|description| description.contains("first unavailable scalar"))
    );
    assert!(responses[1]["result"]["resultType"].is_null());
    assert!(responses[1]["result"]["ttlMs"].is_null());
    assert_eq!(responses[2]["result"]["isError"], true);
    assert_eq!(
        responses[2]["result"]["structuredContent"]["body"]["code"],
        "unavailable"
    );
}

#[test]
fn modern_discovery_and_tool_listing_are_stateless_cacheable_and_desktop_free() {
    let requests = [
        json!({
            "jsonrpc":"2.0",
            "id":"discover",
            "method":"server/discover",
            "params":{"_meta":modern_meta()}
        }),
        json!({
            "jsonrpc":"2.0",
            "id":"tools",
            "method":"tools/list",
            "params":{"_meta":modern_meta()}
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let responses = lines(&run(&requests, &[]));

    assert_eq!(responses.len(), 2);
    let discovery = &responses[0]["result"];
    assert_eq!(discovery["resultType"], "complete");
    assert_eq!(
        discovery["supportedVersions"],
        json!(["2026-07-28", "2025-11-25"])
    );
    assert_eq!(discovery["ttlMs"], 3_600_000);
    assert_eq!(discovery["cacheScope"], "public");
    assert!(
        discovery["instructions"]
            .as_str()
            .is_some_and(|instructions| instructions.contains("never mutate XKB"))
    );
    assert_eq!(
        discovery["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "agent-seat-mcp"
    );

    let listing = &responses[1]["result"];
    assert_eq!(listing["resultType"], "complete");
    assert_eq!(listing["ttlMs"], 3_600_000);
    assert_eq!(listing["cacheScope"], "public");
    let tools = listing["tools"].as_array().expect("modern tool array");
    assert_eq!(tools.len(), 23);
    let status = tools
        .iter()
        .find(|tool| tool["name"] == "seat_status")
        .expect("modern status tool");
    assert!(status["inputSchema"]["properties"]["context"].is_null());
    let snapshot = tools
        .iter()
        .find(|tool| tool["name"] == "desktop_snapshot")
        .expect("modern snapshot tool");
    assert!(
        snapshot["inputSchema"]["required"]
            .as_array()
            .is_some_and(|required| required.contains(&json!("context")))
    );
    assert!(tools.iter().any(|tool| tool["name"] == "seat_release"));
}

#[test]
fn modern_metadata_and_protocol_versions_fail_with_standard_errors() {
    let requests = [
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"server/discover",
            "params":{"_meta":modern_meta()}
        }),
        json!({"jsonrpc":"2.0","id":2,"method":"server/discover","params":{}}),
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"server/discover",
            "params":{"_meta":{
                "io.modelcontextprotocol/protocolVersion":"1900-01-01",
                "io.modelcontextprotocol/clientCapabilities":{}
            }}
        }),
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/list",
            "params":{"_meta":{
                "io.modelcontextprotocol/protocolVersion":"2026-07-28"
            }}
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let responses = lines(&run(&requests, &[]));

    assert_eq!(responses[0]["result"]["resultType"], "complete");
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert_eq!(responses[2]["error"]["code"], -32022);
    assert_eq!(responses[2]["error"]["data"]["requested"], "1900-01-01");
    assert_eq!(
        responses[2]["error"]["data"]["supported"],
        json!(["2026-07-28", "2025-11-25"])
    );
    assert_eq!(responses[3]["error"]["code"], -32602);
}

#[test]
fn modern_tool_results_identify_the_server_without_legacy_initialization() {
    let request = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"tools/call",
        "params":{"name":"seat_status","arguments":{},"_meta":modern_meta()}
    });
    let responses = lines(&run(
        &(request.to_string() + "\n"),
        &["--socket", "/tmp/agent-seat-deliberately-absent.sock"],
    ));

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["resultType"], "complete");
    assert_eq!(responses[0]["result"]["isError"], true);
    assert_eq!(
        responses[0]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "agent-seat-mcp"
    );
    assert_eq!(
        responses[0]["result"]["structuredContent"]["body"]["code"],
        "unavailable"
    );
}

#[test]
fn modern_provider_continuity_uses_an_explicit_releasable_context() {
    let path = std::env::temp_dir().join(format!(
        "agent-seat-modern-context-{}.sock",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind context provider");
    let provider = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept companion");
        assert!(matches!(
            read_frame(&mut stream, MAX_REQUEST_FRAME_BYTES).expect("read hello"),
            ReadFrame::Message(ClientMessage::Hello(_))
        ));
        write_frame(
            &mut stream,
            &ServerMessage::Welcome(Welcome {
                protocol: BoundedText::new(PROTOCOL_NAME).expect("protocol text"),
                revision: PROTOCOL_REVISION,
                session: SessionId::new(NonZeroU64::MIN),
                provider: ProviderInfo {
                    name: BoundedText::new("context-fixture").expect("provider name"),
                    version: BoundedText::new("1").expect("provider version"),
                },
                backend: Backend::X11Ewmh,
                assurance: Assurance::Tier0,
                features: BoundedList::new(vec![Feature::EwmhObservation])
                    .expect("provider features"),
                granted: BoundedList::new(vec![
                    Capability::ObserveStructure,
                    Capability::LaunchList,
                    Capability::InputPointer,
                ])
                .expect("provider grants"),
                limits: Limits {
                    request_frame_bytes: MAX_REQUEST_FRAME_BYTES as u32,
                    response_frame_bytes: MAX_RESPONSE_FRAME_BYTES as u32,
                    events_per_poll: MAX_EVENTS as u16,
                    poll_wait_ms: MAX_POLL_WAIT_MS,
                },
            }),
            MAX_RESPONSE_FRAME_BYTES,
        )
        .expect("write welcome");
        for expected in ["seat_status", "pointer_click", "applications_list"] {
            let request = match read_frame(&mut stream, MAX_REQUEST_FRAME_BYTES)
                .expect("read provider request")
            {
                ReadFrame::Message(ClientMessage::Request(request)) => request,
                other => panic!("unexpected provider frame: {other:?}"),
            };
            assert!(matches!(
                (&request.call, expected),
                (agent_seat_proto::Call::SeatStatus(_), "seat_status")
                    | (agent_seat_proto::Call::PointerClick(_), "pointer_click")
                    | (
                        agent_seat_proto::Call::ApplicationsList(_),
                        "applications_list"
                    )
            ));
            if let agent_seat_proto::Call::PointerClick(action) = &request.call {
                assert_eq!(action.target.generation.get(), 5);
                assert_eq!((action.x, action.y), (120, 80));
            }
            let outcome = if expected == "pointer_click" {
                Outcome::Ok(agent_seat_proto::Reply::Input(
                    agent_seat_proto::InputReply {
                        completed: 1,
                        requested: 1,
                        terminal: agent_seat_proto::InputTerminal::Queued,
                    },
                ))
            } else {
                Outcome::Error(ProtocolError {
                    code: ErrorCode::Unsupported,
                    retry: Retry::Never,
                    field: None,
                    message: None,
                    current_generation: None,
                    current_sequence: Some(Sequence::new(0)),
                })
            };
            write_frame(
                &mut stream,
                &ServerMessage::Response(Response {
                    id: request.id,
                    outcome,
                }),
                MAX_RESPONSE_FRAME_BYTES,
            )
            .expect("write provider response");
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-seat-mcp"))
        .args(["--socket", path.to_str().expect("UTF-8 socket")])
        .env_remove("DISPLAY")
        .env_remove("AGENT_SEAT_SOCKET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn modern companion");
    let mut input = child.stdin.take().expect("companion input");
    let mut output = BufReader::new(child.stdout.take().expect("companion output"));
    let request = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"tools/call",
        "params":{"name":"seat_status","arguments":{},"_meta":modern_meta()}
    });
    writeln!(input, "{request}").expect("request context");
    input.flush().expect("flush context request");
    let mut line = String::new();
    output.read_line(&mut line).expect("read context response");
    let status: Value = serde_json::from_str(&line).expect("context response JSON");
    let context = status["result"]["structuredContent"]["context"]
        .as_u64()
        .expect("explicit context");
    assert_eq!(status["result"]["resultType"], "complete");

    for request in [
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"pointer_slot_save",
                "arguments":{"context":context,"name":"menu.download","client":7,"generation":4,"x":120,"y":80,"button":"primary"},
                "_meta":modern_meta()
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{
                "name":"pointer_slots_list",
                "arguments":{"context":context},
                "_meta":modern_meta()
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/call",
            "params":{
                "name":"pointer_slot_replay",
                "arguments":{"context":context,"name":"menu.download","generation":5},
                "_meta":modern_meta()
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":5,
            "method":"tools/call",
            "params":{
                "name":"applications_list",
                "arguments":{"context":context,"limit":1},
                "_meta":modern_meta()
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":6,
            "method":"tools/call",
            "params":{
                "name":"seat_release",
                "arguments":{"context":context},
                "_meta":modern_meta()
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":7,
            "method":"tools/call",
            "params":{
                "name":"pointer_slots_list",
                "arguments":{"context":context},
                "_meta":modern_meta()
            }
        }),
    ] {
        writeln!(input, "{request}").expect("write context call");
    }
    input.flush().expect("flush context calls");
    for expected_id in [2, 3, 4, 5, 6, 7] {
        line.clear();
        output.read_line(&mut line).expect("read context call");
        let response: Value = serde_json::from_str(&line).expect("context call JSON");
        assert_eq!(response["id"], expected_id);
        assert_eq!(response["result"]["resultType"], "complete");
        assert!(response["result"]["structuredContent"]["context"].is_null());
        if expected_id == 2 {
            assert_eq!(
                response["result"]["structuredContent"]["body"]["kind"],
                "pointer_slot_saved"
            );
        }
        if expected_id == 3 {
            assert_eq!(
                response["result"]["structuredContent"]["body"]["value"]["slots"][0]["name"],
                "menu.download"
            );
        }
        if expected_id == 4 {
            assert_eq!(
                response["result"]["structuredContent"]["body"]["kind"],
                "input"
            );
        }
        if expected_id == 6 {
            assert_eq!(
                response["result"]["structuredContent"]["body"]["value"]["context"],
                context
            );
        }
        if expected_id == 7 {
            assert_eq!(response["result"]["isError"], true);
            assert_eq!(
                response["result"]["structuredContent"]["body"]["code"],
                "stale_context"
            );
        }
    }
    drop(input);
    let output = child.wait_with_output().expect("wait for modern companion");
    assert!(
        output.status.success(),
        "modern companion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    provider.join().expect("provider fixture");
    fs::remove_file(path).expect("remove context socket");
}

#[test]
fn malformed_json_does_not_poison_the_next_initialize() {
    let input = concat!(
        "not-json\n",
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"future\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n"
    );
    let output = run(input, &[]);
    let responses = lines(&output);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn null_request_ids_are_invalid_but_notifications_remain_silent() {
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"server/discover\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\"}\n"
    );
    let responses = lines(&run(input, &[]));

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["id"], Value::Null);
    assert_eq!(responses[0]["error"]["code"], -32600);
}

#[test]
fn notifications_are_silent_and_invalid_tool_arguments_do_not_resolve_a_seat() {
    let requests = [
        json!({"jsonrpc":"2.0","method":"unknown/notification"}),
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":"2025-11-25",
                "capabilities":{},
                "clientInfo":{"name":"test","version":"1"}
            }
        }),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"events_poll",
                "arguments":{"after":0,"limit":8,"wait_ms":30001}
            }
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let responses = lines(&run(&requests, &[]));

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[1]["result"]["isError"], true);
    assert_eq!(
        responses[1]["result"]["structuredContent"]["body"]["code"],
        "invalid_argument"
    );
}

#[test]
fn duplicate_socket_arguments_are_rejected() {
    let output = run("", &["--socket", "/tmp/a", "--socket", "/tmp/b"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only once"));
}

#[test]
fn print_config_is_copyable_without_a_desktop() {
    let output = run("", &["--print-mcp-config"]);
    let config: Value = serde_json::from_slice(&output.stdout).expect("config JSON");
    assert_eq!(
        config["mcpServers"]["agent-seat"]["command"],
        "agent-seat-mcp"
    );
}

#[test]
fn private_config_is_explicit_and_confines_the_companion() {
    let socket = "/run/user/1000/agent-seat/x11-input.sock";
    let output = run("", &["--socket", socket, "--print-private-mcp-config"]);
    let config: Value = serde_json::from_slice(&output.stdout).expect("private config JSON");
    let server = &config["mcpServers"]["agent-seat"];
    assert_eq!(server["command"], "/usr/bin/systemd-run");
    let arguments = server["args"].as_array().expect("systemd argument array");
    for required in [
        "--user",
        "--pipe",
        "--wait",
        "--collect",
        "--service-type=exec",
        "--property=PrivateDevices=yes",
        "--property=DevicePolicy=strict",
        "--property=PrivateNetwork=yes",
        "--property=RestrictAddressFamilies=AF_UNIX",
        "--property=NoNewPrivileges=yes",
        "--property=CapabilityBoundingSet=",
        "--property=ProtectHome=yes",
        "--property=PrivateTmp=yes",
        "--property=ProtectProc=invisible",
        "--property=TemporaryFileSystem=/run:ro",
        "--property=NoExecPaths=/",
        "--property=TasksMax=2",
        "--property=LimitNOFILE=32",
        "--property=LimitCORE=0",
        "--property=MemoryMax=64M",
        "--property=Restart=no",
    ] {
        assert!(arguments.contains(&json!(required)), "missing {required}");
    }
    assert!(arguments.contains(&json!(format!(
        "--property=OpenFile={socket}:agent-seat-provider"
    ))));
    let separator = arguments
        .iter()
        .position(|argument| argument == "--")
        .expect("systemd command separator");
    assert_eq!(
        &arguments[separator..],
        ["--", "/usr/bin/agent-seat-mcp", "--provider-fd"]
            .map(Value::from)
            .as_slice()
    );
    assert!(arguments.iter().all(|argument| argument != "sh"));
}

#[test]
fn private_config_rejects_a_relative_socket() {
    let output = run(
        "",
        &["--socket", "relative.sock", "--print-private-mcp-config"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid private-profile socket"));
}

#[test]
fn inherited_provider_mode_fails_without_one_exact_named_descriptor() {
    let output = run("", &["--provider-fd"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("inherited provider descriptor environment is not exact")
    );

    let combined = run("", &["--socket", "/tmp/provider.sock", "--provider-fd"]);
    assert!(!combined.status.success());
    assert!(String::from_utf8_lossy(&combined.stderr).contains("cannot be combined"));
}

#[test]
fn oversized_stdio_messages_fail_before_json_parsing() {
    let input = "x".repeat(1024 * 1024 + 1);
    let output = run(&input, &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds 1048576 bytes"));
}
