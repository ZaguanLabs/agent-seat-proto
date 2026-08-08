//! Isolated-X11 discovery precedence and ownership tests.

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, Command, Stdio};

use agent_seat_proto::{ADVERTISEMENT_PROPERTY, Advertisement};
use serde_json::{Value, json};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, PropMode, WindowClass};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Self {
        let mut child = Command::new("Xvfb")
            .args([
                "-screen",
                "0",
                "800x600x24",
                "-nolisten",
                "tcp",
                "-displayfd",
                "1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Xvfb is required by the E1 discovery gate");
        let mut display_number = String::new();
        BufReader::new(child.stdout.take().expect("Xvfb display pipe"))
            .read_line(&mut display_number)
            .expect("read Xvfb display number");
        let display_number = display_number.trim();
        assert!(!display_number.is_empty(), "Xvfb did not publish a display");
        Self {
            child,
            display: format!(":{display_number}"),
        }
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn requests() -> String {
    [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"seat_status","arguments":{}}}),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n"
}

fn call(display: &str, explicit: Option<&str>, environment: Option<&str>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-seat-mcp"));
    if let Some(path) = explicit {
        command.args(["--socket", path]);
    }
    command
        .env("DISPLAY", display)
        .env("XDG_RUNTIME_DIR", "/tmp/agent-seat-must-not-be-synthesized")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = environment {
        command.env("AGENT_SEAT_SOCKET", path);
    } else {
        command.env_remove("AGENT_SEAT_SOCKET");
    }
    let mut child = command.spawn().expect("spawn companion");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(requests().as_bytes())
        .expect("write MCP requests");
    let output = child.wait_with_output().expect("wait for companion");
    assert!(
        output.status.success(),
        "companion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("MCP response"))
        .collect();
    assert_eq!(responses.len(), 2);
    responses[1]["result"]["structuredContent"].clone()
}

fn message(result: &Value) -> &str {
    result["body"]["message"]
        .as_str()
        .expect("tool error message")
}

fn code(result: &Value) -> &str {
    result["body"]["code"].as_str().expect("tool error code")
}

#[test]
fn discovery_is_selection_bound_and_obeys_exact_precedence() {
    let xvfb = Xvfb::start();
    let (connection, screen_index) = x11rb::connect(Some(&xvfb.display)).expect("connect to Xvfb");
    let root = connection.setup().roots[screen_index].root;
    let owner = connection.generate_id().expect("owner window ID");
    connection
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            owner,
            root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .expect("create owner")
        .check()
        .expect("owner creation accepted");
    let selection = connection
        .intern_atom(false, format!("_AGENT_SEAT_S{screen_index}").as_bytes())
        .expect("intern selection")
        .reply()
        .expect("selection atom")
        .atom;
    let property = connection
        .intern_atom(false, ADVERTISEMENT_PROPERTY.as_bytes())
        .expect("intern property")
        .reply()
        .expect("property atom")
        .atom;
    let utf8 = connection
        .intern_atom(false, b"UTF8_STRING")
        .expect("intern UTF8_STRING")
        .reply()
        .expect("UTF8_STRING atom")
        .atom;

    let root_path = format!("/tmp/agent-seat-e1-{}.sock", std::process::id());
    let encoded = Advertisement::new(&root_path)
        .expect("bounded root fixture")
        .encode();
    connection
        .set_selection_owner(owner, selection, CURRENT_TIME)
        .expect("claim selection")
        .check()
        .expect("selection claim accepted");
    for window in [owner, root] {
        connection
            .change_property8(
                PropMode::REPLACE,
                window,
                property,
                utf8,
                encoded.as_bytes(),
            )
            .expect("set advertisement")
            .check()
            .expect("advertisement accepted");
    }
    connection.flush().expect("publish fixture");

    let explicit = "/tmp/agent-seat-explicit.sock";
    let environment = "/tmp/agent-seat-environment.sock";
    assert!(message(&call(&xvfb.display, Some(explicit), Some(environment))).contains(explicit));
    assert!(message(&call(&xvfb.display, None, Some(environment))).contains(environment));
    let root_result = call(&xvfb.display, None, None);
    assert!(
        message(&root_result).contains(&root_path),
        "unexpected root discovery result: {root_result}"
    );
    assert!(
        message(&call(&xvfb.display, Some("relative"), Some(environment))).contains("--socket")
    );
    assert!(message(&call(&xvfb.display, None, Some("relative"))).contains("AGENT_SEAT_SOCKET"));

    let mismatch = Advertisement::new("/tmp/agent-seat-mismatch.sock")
        .expect("bounded mismatch fixture")
        .encode();
    connection
        .change_property8(PropMode::REPLACE, root, property, utf8, mismatch.as_bytes())
        .expect("set mismatched root")
        .check()
        .expect("mismatched root accepted");
    connection.flush().expect("publish mismatch");
    assert!(message(&call(&xvfb.display, None, None)).contains("no live"));

    let incompatible = format!("agent-seat\x002\0{root_path}");
    for window in [owner, root] {
        connection
            .change_property8(
                PropMode::REPLACE,
                window,
                property,
                utf8,
                incompatible.as_bytes(),
            )
            .expect("set incompatible advertisement")
            .check()
            .expect("incompatible advertisement accepted");
    }
    connection.flush().expect("publish incompatible fixture");
    let result = call(&xvfb.display, None, None);
    assert_eq!(code(&result), "incompatible_revision");
    assert!(message(&result).contains("unsupported revision"));

    connection
        .change_property8(PropMode::REPLACE, root, property, utf8, encoded.as_bytes())
        .expect("restore root")
        .check()
        .expect("restored root accepted");
    connection
        .set_selection_owner(NONE, selection, CURRENT_TIME)
        .expect("release selection")
        .check()
        .expect("selection release accepted");
    connection.flush().expect("publish stale fixture");
    assert!(message(&call(&xvfb.display, None, None)).contains("no live"));
}
