//! Explicit hostile gate derived from the emitted private MCP configuration.

#![cfg(target_os = "linux")]

use std::env;
use std::fs::{self, DirBuilder, File};
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let runtime = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR"));
        assert!(runtime.is_absolute(), "XDG_RUNTIME_DIR is not absolute");
        let path = runtime.join(format!("agent-seat-mcp-confinement-{}", std::process::id()));
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(&path)
            .expect("create private runtime fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires an active systemd user manager and a test UID that can open host uinput"]
fn emitted_private_profile_exposes_only_the_provider_channel() {
    assert!(
        File::open("/dev/uinput").is_ok(),
        "test UID cannot open host uinput, so the negative-authority gate is inconclusive"
    );
    let fixture = Fixture::new();
    let provider_socket = fixture.0.join("provider.sock");
    let forbidden_socket = fixture.0.join("broker.sock");
    let provider = UnixListener::bind(&provider_socket).expect("provider socket fixture");
    let forbidden = UnixListener::bind(&forbidden_socket).expect("forbidden socket fixture");
    let compiled = fixture.0.join("compiled-probe");
    let inside_probe = "/usr/bin/false";
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is in the workspace crates directory");
    let home_secret = workspace
        .join("target")
        .join(format!("private-companion-secret-{}", std::process::id()));
    fs::write(&home_secret, "private\n").expect("home secret fixture");

    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/private_companion_probe.rs");
    let compile = Command::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&compiled)
        .arg(source)
        .output()
        .expect("rustc is required for the private companion gate");
    assert!(
        compile.status.success(),
        "cannot compile private companion probe: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let generated = Command::new(env!("CARGO_BIN_EXE_agent-seat-mcp"))
        .arg("--socket")
        .arg(&provider_socket)
        .arg("--print-private-mcp-config")
        .output()
        .expect("generate private MCP configuration");
    assert!(
        generated.status.success(),
        "private MCP configuration failed"
    );
    let document: Value = serde_json::from_slice(&generated.stdout).expect("private MCP JSON");
    let mut arguments = document["mcpServers"]["agent-seat"]["args"]
        .as_array()
        .expect("private argument array")
        .iter()
        .map(|argument| argument.as_str().expect("string argument").to_owned())
        .collect::<Vec<_>>();
    let executable_paths = arguments
        .iter_mut()
        .find(|argument| argument.starts_with("--property=ExecPaths="))
        .expect("execution allow-list property");
    *executable_paths =
        format!("--property=ExecPaths={inside_probe} -/usr/lib -/usr/lib64 -/lib -/lib64");
    let separator = arguments
        .iter()
        .position(|argument| argument == "--")
        .expect("command separator");
    arguments.truncate(separator);
    arguments.push(format!(
        "--property=BindReadOnlyPaths={}:{}",
        path(&compiled),
        inside_probe
    ));
    arguments.extend([
        format!(
            "--setenv=AGENT_SEAT_FORBIDDEN_SOCKET={}",
            path(&forbidden_socket)
        ),
        format!("--setenv=AGENT_SEAT_HOME_SECRET={}", path(&home_secret)),
        format!(
            "--setenv=AGENT_SEAT_PARENT_ENVIRON=/proc/{}/environ",
            std::process::id()
        ),
        "--".to_owned(),
        inside_probe.to_owned(),
    ]);

    let output = Command::new("/usr/bin/systemd-run")
        .args(arguments)
        .output()
        .expect("systemd-run is required for the private companion gate");
    let _ = fs::remove_file(&home_secret);
    assert!(
        output.status.success(),
        "private companion probe failed with {}: stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("agent-seat-private-companion-probe=pass"),
        "the private companion probe did not reach its success marker"
    );
    provider
        .set_nonblocking(true)
        .expect("make provider fixture nonblocking");
    assert!(
        provider.accept().is_ok(),
        "systemd did not connect the exact provider socket"
    );
    forbidden
        .set_nonblocking(true)
        .expect("make forbidden fixture nonblocking");
    assert!(
        forbidden.accept().is_err(),
        "the forbidden socket unexpectedly received a connection"
    );
}

fn path(path: &Path) -> &str {
    path.to_str().expect("fixture path is UTF-8")
}
