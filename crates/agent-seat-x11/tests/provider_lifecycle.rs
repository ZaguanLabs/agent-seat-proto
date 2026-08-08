//! Process-boundary T0 lifecycle, policy, ownership, and isolation tests.

use std::fs::{self, DirBuilder};
use std::io::{BufRead as _, Read as _};
use std::num::NonZeroU64;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use agent_seat_proto::{
    BoundedList, BoundedText, Call, Capability, ClientMessage, Empty, ErrorCode, Hello,
    MAX_REQUEST_FRAME_BYTES, MAX_RESPONSE_FRAME_BYTES, PROTOCOL_NAME, PROTOCOL_REVISION, PeerInfo,
    ReadFrame, Reply, Request, RequestId, ServerMessage, read_frame, write_frame,
};
use rustix::process::{Pid, Signal, geteuid, kill_process};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-seat-t0-{label}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(&path)
            .expect("fixture directory");
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb is required by the T0 gate");
        let mut display_number = String::new();
        std::io::BufReader::new(child.stdout.take().expect("display pipe"))
            .read_line(&mut display_number)
            .expect("display number");
        assert!(!display_number.trim().is_empty(), "Xvfb did not start");
        Self {
            child,
            display: format!(":{}", display_number.trim()),
        }
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Provider {
    child: Child,
}

impl Provider {
    fn start(display: &str, config: &Path, socket: &Path) -> Self {
        let mut child = spawn_provider(display, config, socket);
        let deadline = Instant::now() + Duration::from_secs(3);
        let (connection, screen) = x11rb::connect(Some(display)).expect("startup X11 connection");
        let selection = connection
            .intern_atom(false, format!("_AGENT_SEAT_S{screen}").as_bytes())
            .expect("startup selection request")
            .reply()
            .expect("startup selection reply")
            .atom;
        let mut socket_ready = false;
        loop {
            if !socket_ready {
                if let Ok(stream) = UnixStream::connect(socket) {
                    drop(stream);
                    socket_ready = true;
                }
            }
            let owner = connection
                .get_selection_owner(selection)
                .expect("startup owner request")
                .reply()
                .expect("startup owner reply")
                .owner;
            if socket_ready && owner != NONE {
                break;
            }
            if let Some(status) = child.try_wait().expect("provider startup status") {
                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .expect("provider stderr")
                    .read_to_string(&mut stderr)
                    .expect("read provider error");
                panic!("provider exited during startup ({status}): {stderr}");
            }
            assert!(
                Instant::now() < deadline,
                "provider did not listen at {}",
                socket.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
        // The readiness connection closes without a hello. Let the provider
        // reap that worker before tests make capacity assertions.
        thread::sleep(Duration::from_millis(30));
        Self { child }
    }

    fn terminate(&mut self) {
        if self.child.try_wait().expect("provider status").is_none() {
            kill_process(Pid::from_child(&self.child), Signal::TERM).expect("signal provider");
            assert!(
                self.child.wait().expect("provider clean exit").success(),
                "provider did not stop cleanly"
            );
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = kill_process(Pid::from_child(&self.child), Signal::TERM);
            let _ = self.child.wait();
        }
    }
}

fn spawn_provider(display: &str, config: &Path, socket: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config UTF-8")])
        .args(["--socket", socket.to_str().expect("socket UTF-8")])
        .env("DISPLAY", display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn provider")
}

fn write_config(
    directory: &Path,
    capabilities: &[&str],
    max_sessions: u8,
    timeout_ms: u32,
) -> PathBuf {
    let path = directory.join("config.toml");
    let capabilities = capabilities
        .iter()
        .map(|capability| format!("\"{capability}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        &path,
        format!(
            "enabled = true\nmax_sessions = {max_sessions}\nio_timeout_ms = {timeout_ms}\n\
             [grant]\nuid = {}\ncapabilities = [{capabilities}]\n",
            geteuid().as_raw()
        ),
    )
    .expect("write provider config");
    path
}

fn hello(stream: &mut UnixStream) -> ServerMessage {
    let requested = BoundedList::new(vec![Capability::ObserveStructure, Capability::ManageClose])
        .expect("bounded fixture");
    write_frame(
        stream,
        &ClientMessage::Hello(Hello {
            protocol: BoundedText::new(PROTOCOL_NAME).expect("protocol fixture"),
            revision: PROTOCOL_REVISION,
            peer: PeerInfo {
                name: BoundedText::new("test-peer").expect("name fixture"),
                version: BoundedText::new("1").expect("version fixture"),
                purpose: BoundedText::new("T0 process test").expect("purpose fixture"),
            },
            requested,
        }),
        MAX_REQUEST_FRAME_BYTES,
    )
    .expect("write hello");
    match read_frame(stream, MAX_RESPONSE_FRAME_BYTES).expect("read opening response") {
        ReadFrame::Message(message) => message,
        ReadFrame::CleanEof => panic!("provider closed before opening response"),
    }
}

fn seat_status(stream: &mut UnixStream) -> ServerMessage {
    write_frame(
        stream,
        &ClientMessage::Request(Request {
            id: RequestId::new(NonZeroU64::MIN),
            call: Call::SeatStatus(Empty {}),
        }),
        MAX_REQUEST_FRAME_BYTES,
    )
    .expect("write status call");
    match read_frame(stream, MAX_RESPONSE_FRAME_BYTES).expect("read status response") {
        ReadFrame::Message(message) => message,
        ReadFrame::CleanEof => panic!("provider closed before status response"),
    }
}

#[test]
fn configuration_check_is_strict_explicit_and_desktop_free() {
    let directory = FixtureDir::new("config");
    let config = directory.0.join("config.toml");
    fs::write(&config, "enabled = false\n").expect("disabled config");
    let disabled = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config UTF-8")])
        .arg("--check-config")
        .env_remove("DISPLAY")
        .output()
        .expect("check disabled config");
    assert!(!disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("disabled"));

    fs::write(&config, "enabled = true\nunknown = 1\n").expect("unknown config");
    let unknown = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config UTF-8")])
        .arg("--check-config")
        .env_remove("DISPLAY")
        .output()
        .expect("check unknown config");
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown field"));

    fs::write(&config, "enabled = true\n").expect("valid config");
    let valid = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config UTF-8")])
        .arg("--check-config")
        .env_remove("DISPLAY")
        .output()
        .expect("check valid config");
    assert!(valid.status.success());
    assert!(String::from_utf8_lossy(&valid.stdout).contains("valid and enabled"));

    fs::set_permissions(&config, fs::Permissions::from_mode(0o666)).expect("loosen config mode");
    let writable = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config UTF-8")])
        .arg("--check-config")
        .env_remove("DISPLAY")
        .output()
        .expect("check writable config");
    assert!(!writable.status.success());
    assert!(String::from_utf8_lossy(&writable.stderr).contains("not writable"));
}

#[test]
fn no_wm_lifecycle_policy_ownership_and_stale_recovery() {
    let xvfb = Xvfb::start();
    let directory = FixtureDir::new("foundation");
    let config = write_config(&directory.0, &["observe_structure"], 4, 500);
    let socket = directory.0.join("seat.sock");
    drop(UnixListener::bind(&socket).expect("stale socket fixture"));

    let mut provider = Provider::start(&xvfb.display, &config, &socket);
    let (connection, screen) = x11rb::connect(Some(&xvfb.display)).expect("inspect X11");
    let selection = connection
        .intern_atom(false, format!("_AGENT_SEAT_S{screen}").as_bytes())
        .expect("selection request")
        .reply()
        .expect("selection reply")
        .atom;
    let owner = connection
        .get_selection_owner(selection)
        .expect("owner request")
        .reply()
        .expect("owner reply")
        .owner;
    assert_ne!(owner, NONE);
    let root = connection.setup().roots[screen].root;
    let property = connection
        .intern_atom(false, agent_seat_proto::ADVERTISEMENT_PROPERTY.as_bytes())
        .expect("property request")
        .reply()
        .expect("property reply")
        .atom;
    let utf8 = connection
        .intern_atom(false, b"UTF8_STRING")
        .expect("UTF8 atom request")
        .reply()
        .expect("UTF8 atom reply")
        .atom;
    for window in [owner, root] {
        let advertisement = connection
            .get_property(false, window, property, AtomEnum::ANY, 0, 65)
            .expect("advertisement request")
            .reply()
            .expect("advertisement reply");
        assert_eq!(advertisement.type_, utf8);
        assert_eq!(advertisement.format, 8);
        let encoded = std::str::from_utf8(&advertisement.value).expect("advertisement UTF-8");
        assert_eq!(
            agent_seat_proto::Advertisement::parse(encoded)
                .expect("canonical advertisement")
                .socket(),
            socket.to_str().expect("socket UTF-8")
        );
    }

    let mut stream = UnixStream::connect(&socket).expect("connect provider");
    assert!(matches!(
        hello(&mut stream),
        ServerMessage::Welcome(welcome)
            if welcome.assurance == agent_seat_proto::Assurance::Tier0
                && welcome.backend == agent_seat_proto::Backend::X11Ewmh
                && welcome.granted.as_slice() == [Capability::ObserveStructure]
                && welcome.features.is_empty()
    ));
    assert!(matches!(
        seat_status(&mut stream),
        ServerMessage::Response(response)
            if matches!(response.outcome, agent_seat_proto::Outcome::Ok(Reply::SeatStatus(_)))
    ));

    let second_socket = directory.0.join("second.sock");
    let duplicate = spawn_provider(&xvfb.display, &config, &second_socket);
    let output = duplicate.wait_with_output().expect("duplicate exit");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already owns"),
        "unexpected duplicate error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!second_socket.exists());
    assert!(
        provider
            .child
            .try_wait()
            .expect("original status")
            .is_none()
    );

    drop(stream);
    provider.terminate();
    assert!(!socket.exists());
    let current = connection
        .get_selection_owner(selection)
        .expect("released owner request")
        .reply()
        .expect("released owner reply")
        .owner;
    assert_eq!(current, NONE);
    let root_property = connection
        .get_property(false, root, property, AtomEnum::ANY, 0, 65)
        .expect("root property request")
        .reply()
        .expect("root property reply");
    assert_eq!(root_property.type_, NONE);
}

#[test]
fn missing_grant_is_a_typed_peer_denial() {
    let xvfb = Xvfb::start();
    let directory = FixtureDir::new("denial");
    let config = directory.0.join("config.toml");
    fs::write(&config, "enabled = true\nio_timeout_ms = 500\n").expect("deny config");
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(socket).expect("connect denied peer");
    assert!(matches!(
        hello(&mut stream),
        ServerMessage::Goodbye(goodbye) if goodbye.code == ErrorCode::Refused
    ));
}

#[test]
fn slow_peer_is_evicted_and_capacity_recovers() {
    let xvfb = Xvfb::start();
    let directory = FixtureDir::new("capacity");
    let config = write_config(&directory.0, &["observe_structure"], 1, 100);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);

    let slow = UnixStream::connect(&socket).expect("slow peer");
    thread::sleep(Duration::from_millis(30));
    let mut refused = UnixStream::connect(&socket).expect("capacity peer");
    assert!(matches!(
        read_frame(&mut refused, MAX_RESPONSE_FRAME_BYTES).expect("capacity response"),
        ReadFrame::Message(ServerMessage::Goodbye(goodbye))
            if goodbye.code == ErrorCode::Unavailable
    ));
    drop(slow);
    thread::sleep(Duration::from_millis(150));

    let mut recovered = UnixStream::connect(&socket).expect("recovered peer");
    assert!(matches!(hello(&mut recovered), ServerMessage::Welcome(_)));
}

#[test]
fn selection_loss_stops_the_provider_and_removes_its_socket() {
    let xvfb = Xvfb::start();
    let directory = FixtureDir::new("selection-loss");
    let config = write_config(&directory.0, &["observe_structure"], 2, 500);
    let socket = directory.0.join("seat.sock");
    let mut provider = Provider::start(&xvfb.display, &config, &socket);

    let (connection, screen) = x11rb::connect(Some(&xvfb.display)).expect("stealing X11 client");
    let root = connection.setup().roots[screen].root;
    let replacement = connection.generate_id().expect("replacement window ID");
    connection
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            replacement,
            root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .expect("replacement window request")
        .check()
        .expect("replacement window");
    let selection = connection
        .intern_atom(false, format!("_AGENT_SEAT_S{screen}").as_bytes())
        .expect("selection request")
        .reply()
        .expect("selection reply")
        .atom;
    connection
        .set_selection_owner(replacement, selection, CURRENT_TIME)
        .expect("selection replacement request")
        .check()
        .expect("selection replacement");
    connection.flush().expect("flush selection replacement");

    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = provider.child.try_wait().expect("provider status") {
            break status;
        }
        assert!(Instant::now() < deadline, "provider ignored selection loss");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success());
    assert!(!socket.exists());
}

#[test]
fn provider_crash_does_not_take_down_openbox() {
    let xvfb = Xvfb::start();
    let mut openbox = Command::new("openbox")
        .env("DISPLAY", &xvfb.display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Openbox is required by the T0 gate");
    thread::sleep(Duration::from_millis(200));
    assert!(openbox.try_wait().expect("Openbox status").is_none());

    let directory = FixtureDir::new("openbox");
    let config = write_config(&directory.0, &["observe_structure"], 2, 500);
    let socket = directory.0.join("seat.sock");
    let mut provider = Provider::start(&xvfb.display, &config, &socket);
    provider.child.kill().expect("crash provider");
    let _ = provider.child.wait().expect("provider crash exit");
    assert!(openbox.try_wait().expect("Openbox after crash").is_none());

    let mut client = Command::new("xterm")
        .env("DISPLAY", &xvfb.display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("xterm is required by the T0 gate");
    thread::sleep(Duration::from_millis(150));
    assert!(client.try_wait().expect("xterm status").is_none());
    let _ = client.kill();
    let _ = client.wait();
    let _ = openbox.kill();
    let _ = openbox.wait();
}
