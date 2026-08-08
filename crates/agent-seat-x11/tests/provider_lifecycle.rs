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
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use agent_seat_proto::{
    BoundedList, BoundedText, Call, Capability, ClientDescriptor, ClientGeometryRequest,
    ClientMessage, ClientState, ClientStateRequest, ClientWorkspaceRequest, DesktopSnapshot, Empty,
    ErrorCode, Event, EventBatch, EventKind, Hello, MAX_REQUEST_FRAME_BYTES,
    MAX_RESPONSE_FRAME_BYTES, ManagementReply, Observation as ManagementObservation, Outcome,
    PROTOCOL_NAME, PROTOCOL_REVISION, PeerInfo, PollRequest, ReadFrame, Rect, Reply, Request,
    RequestId, Sequence, ServerMessage, StateAction, SubscribeRequest, TargetRequest,
    WorkspaceRequest, read_frame, write_frame,
};
use rustix::process::{Pid, Signal, geteuid, kill_process};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);
static X11_TEST_LOCK: Mutex<()> = Mutex::new(());

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
    _serial: MutexGuard<'static, ()>,
}

impl Xvfb {
    fn start() -> Self {
        // Xvfb's -displayfd selection is not atomic across simultaneous
        // server startups, so one process test owns the X11 fixture at a time.
        let serial = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            _serial: serial,
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

fn write_observer_config(directory: &Path, scope: &str, titles: bool) -> PathBuf {
    let path = write_config(
        directory,
        &[
            "observe_structure",
            "observe_titles",
            "observe_events",
            "manage_activate",
        ],
        4,
        1_000,
    );
    let source = fs::read_to_string(&path).expect("read base observer config");
    fs::write(
        &path,
        format!("{source}\n[observation]\nclients = \"{scope}\"\ntitles = {titles}\n"),
    )
    .expect("write observer config");
    path
}

fn write_management_config(directory: &Path) -> PathBuf {
    let path = write_config(
        directory,
        &[
            "observe_structure",
            "observe_titles",
            "manage_activate",
            "manage_close",
            "manage_workspace",
            "manage_state",
            "manage_geometry",
        ],
        4,
        2_000,
    );
    let source = fs::read_to_string(&path).expect("read base management config");
    fs::write(
        &path,
        format!("{source}\n[observation]\nclients = \"all_workspaces\"\ntitles = true\n"),
    )
    .expect("write management config");
    path
}

fn hello(stream: &mut UnixStream) -> ServerMessage {
    hello_with(
        stream,
        vec![Capability::ObserveStructure, Capability::ManageClose],
    )
}

fn hello_with(stream: &mut UnixStream, requested: Vec<Capability>) -> ServerMessage {
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
            requested: BoundedList::new(requested).expect("bounded fixture"),
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

fn wire_call(stream: &mut UnixStream, next_id: &mut u64, call: Call) -> Outcome {
    let id = NonZeroU64::new(*next_id).expect("nonzero request ID");
    *next_id = next_id.checked_add(1).expect("request ID space");
    write_frame(
        stream,
        &ClientMessage::Request(Request {
            id: RequestId::new(id),
            call,
        }),
        MAX_REQUEST_FRAME_BYTES,
    )
    .expect("write provider call");
    match read_frame(stream, MAX_RESPONSE_FRAME_BYTES).expect("read provider response") {
        ReadFrame::Message(ServerMessage::Response(response)) if response.id.get() == id.get() => {
            response.outcome
        }
        other => panic!("unexpected provider response: {other:?}"),
    }
}

fn snapshot(stream: &mut UnixStream, next_id: &mut u64) -> DesktopSnapshot {
    match wire_call(stream, next_id, Call::DesktopSnapshot(Empty {})) {
        Outcome::Ok(Reply::DesktopSnapshot(snapshot)) => snapshot,
        other => panic!("unexpected snapshot outcome: {other:?}"),
    }
}

struct TestClient {
    connection: RustConnection,
    root: u32,
    window: u32,
    utf8: u32,
    wm_name: u32,
    wm_protocols: u32,
    wm_delete: u32,
}

impl TestClient {
    fn create(display: &str, title: &str) -> Self {
        let (connection, screen) = x11rb::connect(Some(display)).expect("fixture X11 connection");
        let screen = &connection.setup().roots[screen];
        let root = screen.root;
        let window = connection.generate_id().expect("fixture window ID");
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root,
                40,
                50,
                320,
                180,
                0,
                WindowClass::INPUT_OUTPUT,
                screen.root_visual,
                &CreateWindowAux::new(),
            )
            .expect("fixture create request")
            .check()
            .expect("fixture create");
        let utf8 = intern(&connection, b"UTF8_STRING");
        let wm_name = intern(&connection, b"_NET_WM_NAME");
        let wm_protocols = intern(&connection, b"WM_PROTOCOLS");
        let wm_delete = intern(&connection, b"WM_DELETE_WINDOW");
        connection
            .change_property8(PropMode::REPLACE, window, wm_name, utf8, title.as_bytes())
            .expect("fixture title request")
            .check()
            .expect("fixture title");
        connection
            .change_property32(
                PropMode::REPLACE,
                window,
                wm_protocols,
                AtomEnum::ATOM,
                &[wm_delete],
            )
            .expect("fixture protocols request")
            .check()
            .expect("fixture protocols");
        connection
            .map_window(window)
            .expect("fixture map request")
            .check()
            .expect("fixture map");
        connection.flush().expect("fixture flush");
        Self {
            connection,
            root,
            window,
            utf8,
            wm_name,
            wm_protocols,
            wm_delete,
        }
    }

    fn rename(&self, title: &str) {
        self.connection
            .change_property8(
                PropMode::REPLACE,
                self.window,
                self.wm_name,
                self.utf8,
                title.as_bytes(),
            )
            .expect("rename request")
            .check()
            .expect("rename");
        self.connection.flush().expect("rename flush");
    }

    fn move_to_workspace(&self, workspace: u32) {
        let atom = intern(&self.connection, b"_NET_WM_DESKTOP");
        let event = ClientMessageEvent::new(32, self.window, atom, [workspace, 2, 0, 0, 0]);
        self.connection
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                event,
            )
            .expect("workspace event request")
            .check()
            .expect("workspace event");
        self.connection.flush().expect("workspace event flush");
    }

    fn minimize(&self) {
        let atom = intern(&self.connection, b"WM_CHANGE_STATE");
        let event = ClientMessageEvent::new(32, self.window, atom, [3, 0, 0, 0, 0]);
        self.connection
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                event,
            )
            .expect("minimize event request")
            .check()
            .expect("minimize event");
        self.connection.flush().expect("minimize event flush");
    }

    fn map(&self) {
        self.connection
            .map_window(self.window)
            .expect("remap request")
            .check()
            .expect("remap");
        self.connection.flush().expect("remap flush");
    }

    fn destroy(&self) {
        self.connection
            .destroy_window(self.window)
            .expect("destroy request")
            .check()
            .expect("destroy");
        self.connection.flush().expect("destroy flush");
    }

    fn respond_to_close(self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(x11rb::protocol::Event::ClientMessage(event)) = self
                    .connection
                    .poll_for_event()
                    .expect("close responder event")
                {
                    let data = event.data.as_data32();
                    if event.type_ == self.wm_protocols && data[0] == self.wm_delete {
                        self.destroy();
                        return;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "client did not receive WM_DELETE_WINDOW"
                );
                thread::sleep(Duration::from_millis(10));
            }
        })
    }

    fn destroy_after(self, delay: Duration) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            thread::sleep(delay);
            self.destroy();
        })
    }
}

fn intern(connection: &RustConnection, name: &[u8]) -> u32 {
    connection
        .intern_atom(false, name)
        .expect("atom request")
        .reply()
        .expect("atom reply")
        .atom
}

fn start_openbox(display: &str) -> Child {
    let mut child = Command::new("openbox")
        .env("DISPLAY", display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Openbox is required by the provider gate");
    let (connection, screen) = x11rb::connect(Some(display)).expect("Openbox readiness X11");
    let root = connection.setup().roots[screen].root;
    let check_atom = intern(&connection, b"_NET_SUPPORTING_WM_CHECK");
    let count_atom = intern(&connection, b"_NET_NUMBER_OF_DESKTOPS");
    let current_atom = intern(&connection, b"_NET_CURRENT_DESKTOP");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let owner = connection
            .get_property(false, root, check_atom, AtomEnum::WINDOW, 0, 1)
            .expect("Openbox check request")
            .reply()
            .expect("Openbox check reply")
            .value32()
            .and_then(|mut values| values.next());
        let wm_ready = owner.is_some_and(|owner| {
            connection
                .get_property(false, owner, check_atom, AtomEnum::WINDOW, 0, 1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
                == Some(owner)
        });
        let cardinal = |property| {
            connection
                .get_property(false, root, property, AtomEnum::CARDINAL, 0, 1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
        };
        let workspace_count = cardinal(count_atom);
        let current_workspace = cardinal(current_atom);
        let ready = wm_ready
            && workspace_count.is_some_and(|count| count > 0)
            && current_workspace
                .zip(workspace_count)
                .is_some_and(|(current, count)| current < count);
        if ready {
            return child;
        }
        if let Some(status) = child.try_wait().expect("Openbox startup status") {
            panic!("Openbox exited before EWMH readiness: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "Openbox did not publish a valid EWMH check"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_snapshot(
    stream: &mut UnixStream,
    next_id: &mut u64,
    predicate: impl Fn(&DesktopSnapshot) -> bool,
) -> DesktopSnapshot {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let snapshot = snapshot(stream, next_id);
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "snapshot did not converge");
        thread::sleep(Duration::from_millis(25));
    }
}

fn poll_events(stream: &mut UnixStream, next_id: &mut u64, after: Sequence) -> EventBatch {
    match wire_call(
        stream,
        next_id,
        Call::EventsPoll(PollRequest {
            after,
            limit: 64,
            wait_ms: 500,
        }),
    ) {
        Outcome::Ok(Reply::Events(events)) => events,
        other => panic!("unexpected event outcome: {other:?}"),
    }
}

fn management(stream: &mut UnixStream, next_id: &mut u64, call: Call) -> ManagementReply {
    match wire_call(stream, next_id, call) {
        Outcome::Ok(Reply::Management(reply)) => reply,
        other => panic!("unexpected management outcome: {other:?}"),
    }
}

fn client_named(snapshot: &DesktopSnapshot, title: &str) -> ClientDescriptor {
    snapshot
        .clients
        .iter()
        .find(|client| client.title.as_deref() == Some(title))
        .cloned()
        .unwrap_or_else(|| panic!("snapshot has no client titled {title:?}"))
}

const fn target(client: &ClientDescriptor) -> TargetRequest {
    TargetRequest {
        client: client.id,
        generation: client.generation,
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
                && welcome.features.as_slice() == [
                    agent_seat_proto::Feature::EwmhObservation,
                    agent_seat_proto::Feature::EwmhManagement,
                ]
    ));
    assert!(matches!(
        seat_status(&mut stream),
        ServerMessage::Response(response)
            if matches!(response.outcome, agent_seat_proto::Outcome::Ok(Reply::SeatStatus(_)))
    ));
    let mut next_id = 2;
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ClientClose(TargetRequest {
                client: agent_seat_proto::ClientId::new(NonZeroU64::MIN),
                generation: agent_seat_proto::Generation::new(0),
            }),
        ),
        Outcome::Error(error) if error.code == ErrorCode::Refused
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
    let mut openbox = start_openbox(&xvfb.display);
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

#[test]
fn openbox_snapshots_and_diffs_converge_across_client_lifecycle() {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);

    let directory = FixtureDir::new("observation");
    let config = write_observer_config(&directory.0, "all_workspaces", true);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(&socket).expect("observer peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::ObserveTitles,
                Capability::ObserveEvents,
            ],
        ),
        ServerMessage::Welcome(welcome)
            if welcome.features.as_slice() == [
                agent_seat_proto::Feature::EwmhObservation,
                agent_seat_proto::Feature::EwmhManagement,
            ]
                && welcome.granted.as_slice() == [
                    Capability::ObserveStructure,
                    Capability::ObserveTitles,
                    Capability::ObserveEvents,
                ]
    ));
    let mut next_id = 1;
    let initial = snapshot(&mut stream, &mut next_id);
    assert!(!initial.workspaces.is_empty());
    let mut cursor = match wire_call(
        &mut stream,
        &mut next_id,
        Call::EventsSubscribe(SubscribeRequest {
            kinds: BoundedList::new(vec![
                EventKind::ClientAdded,
                EventKind::ClientChanged,
                EventKind::ClientRemoved,
            ])
            .expect("event filter"),
        }),
    ) {
        Outcome::Ok(Reply::Subscribed(subscription)) => subscription.cursor,
        other => panic!("unexpected subscription outcome: {other:?}"),
    };
    let initial_cursor = cursor;

    let client = TestClient::create(&xvfb.display, "agent-seat-alpha");
    let events = poll_events(&mut stream, &mut next_id, cursor);
    cursor = events.cursor;
    let added = events.events.iter().find_map(|event| match &event.event {
        Event::ClientAdded(client) if client.title.as_deref() == Some("agent-seat-alpha") => {
            Some(client.clone())
        }
        _ => None,
    });
    let added = added.expect("client-added event with granted title");
    assert!(added.frame.is_some());

    client.rename("agent-seat-beta");
    let events = poll_events(&mut stream, &mut next_id, cursor);
    let renamed = events.events.iter().find_map(|event| match &event.event {
        Event::ClientChanged(client) if client.id == added.id => Some(client.clone()),
        _ => None,
    });
    let renamed = renamed.expect("renamed client event");
    assert_eq!(renamed.title.as_deref(), Some("agent-seat-beta"));
    assert!(renamed.generation > added.generation);
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::EventsPoll(PollRequest {
                after: initial_cursor,
                limit: 64,
                wait_ms: 0,
            }),
        ),
        Outcome::Error(error)
            if error.code == ErrorCode::ResyncRequired
                && error.current_sequence.is_some()
    ));

    client.minimize();
    let hidden = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.iter().any(|candidate| {
            candidate.id == added.id
                && candidate
                    .states
                    .contains(&agent_seat_proto::ClientState::Hidden)
        })
    });

    client.map();
    let visible = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.iter().any(|candidate| {
            candidate.id == added.id
                && !candidate
                    .states
                    .contains(&agent_seat_proto::ClientState::Hidden)
        })
    });
    cursor = visible.sequence;

    client.move_to_workspace(1);
    let events = poll_events(&mut stream, &mut next_id, cursor);
    cursor = events.cursor;
    assert!(events.events.iter().any(|event| matches!(
        &event.event,
        Event::ClientChanged(client)
            if client.id == added.id && client.workspace == Some(agent_seat_proto::WorkspaceId::new(1))
    )));
    assert!(hidden.sequence < cursor);

    client.destroy();
    let events = poll_events(&mut stream, &mut next_id, cursor);
    assert!(events.events.iter().any(|event| matches!(
        event.event,
        Event::ClientRemoved(id) if id == added.id
    )));

    let _ = openbox.kill();
    let _ = openbox.wait();
}

#[test]
fn current_workspace_scope_hides_titles_and_rekeys_returning_clients() {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);

    let directory = FixtureDir::new("scope");
    let config = write_observer_config(&directory.0, "current_workspace", false);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(&socket).expect("scope peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::ObserveTitles,
                Capability::ManageActivate,
            ],
        ),
        ServerMessage::Welcome(_)
    ));
    let mut next_id = 1;
    let client = TestClient::create(&xvfb.display, "private-title");
    let visible = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.len() == 1
    });
    let first = visible.clients[0].clone();
    assert_eq!(visible.clients[0].title, None);

    client.move_to_workspace(1);
    wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.is_empty()
    });
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ClientActivate(target(&first)),
        ),
        Outcome::Error(error) if error.code == ErrorCode::NoSuchClient
    ));
    client.move_to_workspace(0);
    let returned = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.len() == 1
    });
    assert_ne!(returned.clients[0].id, first.id);
    assert_eq!(returned.clients[0].title, None);

    client.destroy();
    let _ = openbox.kill();
    let _ = openbox.wait();
}

#[test]
fn openbox_management_distinguishes_terminal_and_no_send_outcomes() {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);
    let directory = FixtureDir::new("management");
    let config = write_management_config(&directory.0);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(&socket).expect("management peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::ObserveTitles,
                Capability::ManageActivate,
                Capability::ManageClose,
                Capability::ManageWorkspace,
                Capability::ManageState,
                Capability::ManageGeometry,
            ],
        ),
        ServerMessage::Welcome(welcome)
            if welcome.features.as_slice() == [
                agent_seat_proto::Feature::EwmhObservation,
                agent_seat_proto::Feature::EwmhManagement,
            ]
    ));
    let mut next_id = 1;
    let alpha_client = TestClient::create(&xvfb.display, "manage-alpha");
    let beta_client = TestClient::create(&xvfb.display, "manage-beta");
    let initial = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.len() == 2
    });
    let alpha = client_named(&initial, "manage-alpha");

    let activated = management(
        &mut stream,
        &mut next_id,
        Call::ClientActivate(target(&alpha)),
    );
    assert_eq!(activated.observation, ManagementObservation::Observed);

    let before_switch = snapshot(&mut stream, &mut next_id);
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::WorkspaceSwitch(WorkspaceRequest {
                workspace: agent_seat_proto::WorkspaceId::new(u16::MAX),
                sequence: before_switch.sequence,
            }),
        ),
        Outcome::Error(error) if error.code == ErrorCode::InvalidArgument
    ));
    let switched = management(
        &mut stream,
        &mut next_id,
        Call::WorkspaceSwitch(WorkspaceRequest {
            workspace: agent_seat_proto::WorkspaceId::new(1),
            sequence: before_switch.sequence,
        }),
    );
    assert_eq!(switched.observation, ManagementObservation::Observed);

    let alpha = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha");
    let moved = management(
        &mut stream,
        &mut next_id,
        Call::ClientWorkspace(ClientWorkspaceRequest {
            target: target(&alpha),
            workspace: agent_seat_proto::WorkspaceId::new(1),
        }),
    );
    assert_eq!(moved.observation, ManagementObservation::Observed);

    let alpha = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha");
    let fullscreen = management(
        &mut stream,
        &mut next_id,
        Call::ClientState(ClientStateRequest {
            target: target(&alpha),
            state: ClientState::Fullscreen,
            action: StateAction::Add,
        }),
    );
    assert_eq!(fullscreen.observation, ManagementObservation::Observed);
    let alpha = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha");
    let restored = management(
        &mut stream,
        &mut next_id,
        Call::ClientState(ClientStateRequest {
            target: target(&alpha),
            state: ClientState::Fullscreen,
            action: StateAction::Remove,
        }),
    );
    assert_eq!(restored.observation, ManagementObservation::Observed);

    let alpha = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha");
    let frame = alpha.frame.expect("managed frame");
    let requested_frame = Rect {
        x: frame.x + 20,
        y: frame.y + 15,
        width: frame.width + 40,
        height: frame.height + 30,
    };
    let geometry = management(
        &mut stream,
        &mut next_id,
        Call::ClientGeometry(ClientGeometryRequest {
            target: target(&alpha),
            frame: requested_frame,
        }),
    );
    assert_eq!(geometry.observation, ManagementObservation::Observed);

    let stale = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha");
    alpha_client.rename("manage-alpha-renamed");
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ClientActivate(target(&stale)),
        ),
        Outcome::Error(error)
            if error.code == ErrorCode::Stale
                && error.current_generation.is_some()
                && error.current_sequence.is_some()
    ));

    let alpha = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha-renamed");
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ClientState(ClientStateRequest {
                target: target(&alpha),
                state: ClientState::Hidden,
                action: StateAction::Toggle,
            }),
        ),
        Outcome::Error(error) if error.code == ErrorCode::Unsupported
    ));

    let beta = client_named(&snapshot(&mut stream, &mut next_id), "manage-beta");
    let ignored = management(&mut stream, &mut next_id, Call::ClientClose(target(&beta)));
    assert_eq!(ignored.observation, ManagementObservation::TimedOut);

    let close_client = TestClient::create(&xvfb.display, "manage-close");
    let close = client_named(
        &wait_snapshot(&mut stream, &mut next_id, |snapshot| {
            snapshot
                .clients
                .iter()
                .any(|client| client.title.as_deref() == Some("manage-close"))
        }),
        "manage-close",
    );
    let close_responder = close_client.respond_to_close();
    let closed = management(&mut stream, &mut next_id, Call::ClientClose(target(&close)));
    assert_eq!(closed.observation, ManagementObservation::Observed);
    close_responder.join().expect("close responder");

    let alpha = client_named(&snapshot(&mut stream, &mut next_id), "manage-alpha-renamed");
    let destroyer = alpha_client.destroy_after(Duration::from_millis(50));
    let disappeared = management(
        &mut stream,
        &mut next_id,
        Call::ClientGeometry(ClientGeometryRequest {
            target: target(&alpha),
            frame: Rect {
                x: 0,
                y: 0,
                width: u32::MAX,
                height: u32::MAX,
            },
        }),
    );
    assert_eq!(disappeared.observation, ManagementObservation::TargetGone);
    destroyer.join().expect("target destroyer");
    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ClientActivate(target(&alpha)),
        ),
        Outcome::Error(error) if error.code == ErrorCode::NoSuchClient
    ));

    beta_client.destroy();
    let _ = openbox.kill();
    let _ = openbox.wait();
}
