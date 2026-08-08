//! Process-boundary MCP lifecycle and lazy-connection tests.

use std::io::Write as _;
use std::process::{Command, Stdio};

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
    assert_eq!(
        responses[0]["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(
        responses[1]["result"]["tools"]
            .as_array()
            .expect("tool array")
            .len(),
        13
    );
    assert_eq!(responses[2]["result"]["isError"], true);
    assert_eq!(
        responses[2]["result"]["structuredContent"]["body"]["code"],
        "unavailable"
    );
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
fn oversized_stdio_messages_fail_before_json_parsing() {
    let input = "x".repeat(1024 * 1024 + 1);
    let output = run(&input, &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds 1048576 bytes"));
}
