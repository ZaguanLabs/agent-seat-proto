//! Process-boundary T0 lifecycle, policy, ownership, and isolation tests.

use std::fs::{self, DirBuilder};
use std::io::{BufRead as _, Read as _, Write as _};
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

use agent_seat_activity_broker::{BrokerState, BrokerStatus, StopReason, read_status_request};
use agent_seat_proto::{
    ApplicationId, ApplicationLaunchRequest, ApplicationListRequest, BoundedList, BoundedText,
    Call, Capability, ClientDescriptor, ClientGeometryRequest, ClientMessage, ClientState,
    ClientStateRequest, ClientWorkspaceRequest, DesktopSnapshot, Empty, ErrorCode, Event,
    EventBatch, EventKind, Hello, InputTerminal, MAX_REQUEST_FRAME_BYTES, MAX_RESPONSE_FRAME_BYTES,
    ManagementReply, Observation as ManagementObservation, Outcome, PROTOCOL_NAME,
    PROTOCOL_REVISION, PeerInfo, PointerMoveRequest, PollRequest, ReadFrame, Rect, Reply, Request,
    RequestId, Sequence, ServerMessage, StateAction, SubscribeRequest, TargetRequest,
    WorkspaceRequest, read_frame, write_frame,
};
use agent_seat_x11::{ActivePolicyStatus, active_policy_status, read_policy, replace_policy};
use rustix::process::{Pid, Signal, geteuid, kill_process};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux,
    EventMask, PropMode, StackMode, WindowClass,
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
        Self::wait_until_ready(display, socket, spawn_provider(display, config, socket))
    }

    fn start_with_data(
        display: &str,
        config: &Path,
        socket: &Path,
        user_data: &Path,
        system_data: &Path,
    ) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
            .args(["--config", config.to_str().expect("config UTF-8")])
            .args(["--socket", socket.to_str().expect("socket UTF-8")])
            .env("DISPLAY", display)
            .env(
                "XDG_RUNTIME_DIR",
                socket.parent().expect("socket runtime directory"),
            )
            .env("XDG_DATA_HOME", user_data)
            .env("XDG_DATA_DIRS", system_data)
            .env("XDG_CURRENT_DESKTOP", "Openbox")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn provider with XDG fixtures");
        Self::wait_until_ready(display, socket, child)
    }

    fn wait_until_ready(display: &str, socket: &Path, mut child: Child) -> Self {
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
        .env(
            "XDG_RUNTIME_DIR",
            socket.parent().expect("socket runtime directory"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn provider")
}

#[test]
fn active_policy_evidence_distinguishes_matching_changed_and_stopped_provider() {
    let directory = FixtureDir::new("active-policy");
    let xvfb = Xvfb::start();
    let config = write_config(&directory.0, &["observe_structure"], 4, 500);
    let socket = directory.0.join("active.sock");
    let mut provider = Provider::start(&xvfb.display, &config, &socket);
    let original = read_policy(&config).expect("read active policy");

    assert_eq!(
        query_active_policy(&directory.0, &config),
        format!(
            "{:?}",
            ActivePolicyStatus::Matching {
                pid: provider.child.id()
            }
        )
    );

    let changed_source = original
        .source()
        .replace("max_sessions = 4", "max_sessions = 5");
    let _changed = replace_policy(&original, &changed_source).expect("replace saved policy");
    assert_eq!(
        query_active_policy(&directory.0, &config),
        format!(
            "{:?}",
            ActivePolicyStatus::Different {
                pid: provider.child.id()
            }
        )
    );

    kill_process(Pid::from_child(&provider.child), Signal::KILL).expect("crash provider");
    assert!(
        !provider
            .child
            .wait()
            .expect("crashed provider exit")
            .success(),
        "killed provider unexpectedly exited successfully"
    );
    assert_eq!(
        query_active_policy(&directory.0, &config),
        format!("{:?}", ActivePolicyStatus::NotReported)
    );
}

#[test]
fn active_policy_status_helper() {
    let Some(config) = std::env::var_os("AGENT_SEAT_ACTIVE_CONFIG") else {
        return;
    };
    let result = std::env::var_os("AGENT_SEAT_ACTIVE_RESULT").expect("active result path");
    let snapshot = read_policy(Path::new(&config)).expect("read active helper policy");
    let status = active_policy_status(&snapshot).expect("read active helper status");
    fs::write(result, format!("{status:?}")).expect("write active status result");
}

fn query_active_policy(runtime: &Path, config: &Path) -> String {
    let result = runtime.join("active-status.txt");
    let output = Command::new(std::env::current_exe().expect("provider test executable"))
        .args(["--exact", "active_policy_status_helper", "--nocapture"])
        .env("XDG_RUNTIME_DIR", runtime)
        .env("AGENT_SEAT_ACTIVE_CONFIG", config)
        .env("AGENT_SEAT_ACTIVE_RESULT", &result)
        .output()
        .expect("run active status helper");
    assert!(
        output.status.success(),
        "active status helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::read_to_string(result).expect("read active status result")
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

fn write_pointer_config(directory: &Path, broker_socket: &Path) -> PathBuf {
    let path = write_config(
        directory,
        &["observe_structure", "observe_titles", "input_pointer"],
        4,
        2_000,
    );
    let source = fs::read_to_string(&path).expect("read base pointer config");
    fs::write(
        &path,
        format!(
            "{source}\n[observation]\nclients = \"all_workspaces\"\ntitles = true\n\
             [input]\nbroker_socket = {:?}\nbroker_peer_uid = {}\n",
            broker_socket,
            geteuid().as_raw()
        ),
    )
    .expect("write pointer config");
    path
}

fn serve_broker(socket: &Path, statuses: Vec<BrokerStatus>) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(socket).expect("bind broker fixture");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept broker fixture");
        for status in statuses {
            read_status_request(&mut stream).expect("read broker status request");
            stream
                .write_all(&status.encode())
                .expect("write broker status response");
        }
    })
}

fn write_launch_config(directory: &Path) -> PathBuf {
    let path = write_config(
        directory,
        &["observe_structure", "launch_list", "launch_execute"],
        4,
        2_000,
    );
    let source = fs::read_to_string(&path).expect("read base launch config");
    fs::write(
        &path,
        format!(
            "{source}\n[observation]\nclients = \"all_workspaces\"\n\
             [launch]\nmode = \"allow_listed\"\n\
             allow = [\"allowed.desktop\", \"failure.desktop\", \"hostile.desktop\", \
             \"user.desktop\"]\n"
        ),
    )
    .expect("write launch config");
    path
}

fn write_desktop(directory: &Path, id: &str, name: &str, exec: &str) {
    fs::create_dir_all(directory).expect("desktop fixture directory");
    fs::write(
        directory.join(id),
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n"),
    )
    .expect("desktop fixture");
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

struct OverrideWindow {
    connection: RustConnection,
    window: u32,
}

impl OverrideWindow {
    fn create(display: &str, x: i16, y: i16, width: u16, height: u16) -> Self {
        let (connection, screen) = x11rb::connect(Some(display)).expect("overlay X11 connection");
        let screen = &connection.setup().roots[screen];
        let window = connection.generate_id().expect("overlay window ID");
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                screen.root,
                x,
                y,
                width,
                height,
                0,
                WindowClass::INPUT_OUTPUT,
                screen.root_visual,
                &CreateWindowAux::new().override_redirect(1),
            )
            .expect("overlay create request")
            .check()
            .expect("overlay create");
        connection
            .map_window(window)
            .expect("overlay map request")
            .check()
            .expect("overlay map");
        connection.sync().expect("overlay map sync");
        Self { connection, window }
    }

    fn lower(&self) {
        self.connection
            .configure_window(
                self.window,
                &ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
            )
            .expect("lower overlay request")
            .check()
            .expect("lower overlay");
        self.connection.sync().expect("lower overlay sync");
    }

    fn raise(&self) {
        self.connection
            .configure_window(
                self.window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )
            .expect("raise overlay request")
            .check()
            .expect("raise overlay");
        self.connection.sync().expect("raise overlay sync");
    }
}

impl Drop for OverrideWindow {
    fn drop(&mut self) {
        let _ = self.connection.destroy_window(self.window);
        let _ = self.connection.flush();
    }
}

impl TestClient {
    fn create(display: &str, title: &str) -> Self {
        Self::create_with_startup_id(display, title, None)
    }

    fn create_with_startup_id(display: &str, title: &str, startup_id: Option<&str>) -> Self {
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
        if let Some(startup_id) = startup_id {
            let atom = intern(&connection, b"_NET_STARTUP_ID");
            connection
                .change_property8(PropMode::REPLACE, window, atom, utf8, startup_id.as_bytes())
                .expect("fixture startup ID request")
                .check()
                .expect("fixture startup ID");
        }
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
    assert!(disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stdout).contains("valid and disabled"));

    fs::write(&config, "enabled = false\nmax_sessions = 0\n").expect("invalid disabled config");
    let invalid_disabled = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config UTF-8")])
        .arg("--check-config")
        .env_remove("DISPLAY")
        .output()
        .expect("check invalid disabled config");
    assert!(!invalid_disabled.status.success());
    assert!(String::from_utf8_lossy(&invalid_disabled.stderr).contains("max_sessions"));

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
fn help_explains_options_first_run_and_configuration_safety() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .arg("--help")
        .output()
        .expect("provider help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "--config PATH",
        "--socket PATH",
        "--check-config",
        "FIRST RUN",
        "$XDG_CONFIG_HOME/agent-seat/config.toml",
        "mode 0600",
        "enabled = true",
        "Explicit --config paths are never created or overwritten",
    ] {
        assert!(stdout.contains(expected), "help omitted {expected:?}");
    }
}

#[test]
fn first_run_creates_documented_disabled_private_config_without_x11() {
    let directory = FixtureDir::new("first-run");
    let first = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .env("XDG_CONFIG_HOME", &directory.0)
        .env_remove("DISPLAY")
        .output()
        .expect("first provider run");
    assert!(first.status.success());
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("Created first-run configuration"));
    assert!(stdout.contains("provider has not started"));

    let config = directory.0.join("agent-seat/config.toml");
    let source = fs::read_to_string(&config).expect("read generated config");
    assert!(source.contains("enabled = false"));
    assert!(source.contains(&format!("uid = {}", geteuid().as_raw())));
    assert!(source.contains("every capability below permits"));
    assert!(source.contains("\"observe_structure\""));
    assert!(source.contains("# \"manage_close\""));
    assert!(source.contains("mode = \"deny\""));
    let mode = fs::metadata(&config)
        .expect("generated config metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let directory_mode = fs::metadata(config.parent().expect("config parent"))
        .expect("generated config directory metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(directory_mode, 0o700);

    let second = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .env("XDG_CONFIG_HOME", &directory.0)
        .env_remove("DISPLAY")
        .output()
        .expect("second provider run");
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("provider is disabled"));
    assert_eq!(
        fs::read_to_string(&config).expect("reread generated config"),
        source
    );

    fs::write(
        &config,
        source.replacen("enabled = false", "enabled = true", 1),
    )
    .expect("enable generated config");
    let checked = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .arg("--check-config")
        .env("XDG_CONFIG_HOME", &directory.0)
        .env_remove("DISPLAY")
        .output()
        .expect("validate generated config");
    assert!(checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stdout).contains("valid and enabled"));
}

#[test]
fn explicit_missing_config_is_not_created() {
    let directory = FixtureDir::new("explicit-missing-config");
    let config = directory.0.join("custom.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .args(["--config", config.to_str().expect("config path UTF-8")])
        .env_remove("DISPLAY")
        .output()
        .expect("provider with explicit missing config");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot inspect"));
    assert!(!config.exists());
}

#[test]
fn check_config_does_not_create_a_missing_default() {
    let directory = FixtureDir::new("check-missing-config");
    let output = Command::new(env!("CARGO_BIN_EXE_agent-seat-x11"))
        .arg("--check-config")
        .env("XDG_CONFIG_HOME", &directory.0)
        .env_remove("DISPLAY")
        .output()
        .expect("check missing default config");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot inspect"));
    assert!(!directory.0.join("agent-seat/config.toml").exists());
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
                    agent_seat_proto::Feature::DesktopLaunch,
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
                agent_seat_proto::Feature::DesktopLaunch,
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
fn pointer_move_is_broker_gated_target_relative_and_observed_on_x11() {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);
    let directory = FixtureDir::new("pointer-move");
    let broker_socket = directory.0.join("activity.sock");
    let ready = BrokerStatus {
        instance: 41,
        epoch: 7,
        state: BrokerState::Ready,
        reason: StopReason::None,
    };
    let broker = serve_broker(&broker_socket, vec![ready, ready, ready]);
    let config = write_pointer_config(&directory.0, &broker_socket);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(&socket).expect("pointer peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::ObserveTitles,
                Capability::InputPointer,
            ],
        ),
        ServerMessage::Welcome(welcome)
            if welcome.features.contains(&agent_seat_proto::Feature::InputInjection)
                && welcome.features.contains(&agent_seat_proto::Feature::HumanActivity)
    ));

    let client = TestClient::create(&xvfb.display, "pointer-target");
    let lower_cover = OverrideWindow::create(&xvfb.display, 0, 0, 1_024, 768);
    lower_cover.lower();
    let mut next_id = 1;
    let observed = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.iter().any(|candidate| {
            candidate
                .title
                .as_ref()
                .is_some_and(|title| title.as_str() == "pointer-target")
        })
    });
    let observed_target = client_named(&observed, "pointer-target");
    let expected = client
        .connection
        .translate_coordinates(client.window, client.root, 25, 30)
        .expect("expected pointer coordinates request")
        .reply()
        .expect("expected pointer coordinates reply");

    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::PointerMove(PointerMoveRequest {
                target: target(&observed_target),
                x: 25,
                y: 30,
            }),
        ),
        Outcome::Ok(Reply::Input(reply))
            if reply.completed == 1
                && reply.requested == 1
                && reply.terminal == InputTerminal::Queued
    ));
    let pointer = client
        .connection
        .query_pointer(client.root)
        .expect("query moved pointer request")
        .reply()
        .expect("query moved pointer reply");
    assert_eq!(
        (pointer.root_x, pointer.root_y),
        (expected.dst_x, expected.dst_y)
    );

    broker.join().expect("broker fixture");
    client.destroy();
    let _ = openbox.kill();
    let _ = openbox.wait();
}

#[test]
fn pointer_move_is_refused_when_an_override_redirect_window_covers_the_target() {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);
    let directory = FixtureDir::new("pointer-cover");
    let broker_socket = directory.0.join("activity.sock");
    let ready = BrokerStatus {
        instance: 47,
        epoch: 9,
        state: BrokerState::Ready,
        reason: StopReason::None,
    };
    let broker = serve_broker(&broker_socket, vec![ready]);
    let config = write_pointer_config(&directory.0, &broker_socket);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(&socket).expect("covered pointer peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::ObserveTitles,
                Capability::InputPointer,
            ],
        ),
        ServerMessage::Welcome(_)
    ));

    let client = TestClient::create(&xvfb.display, "covered-pointer-target");
    let mut next_id = 1;
    let observed = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.iter().any(|candidate| {
            candidate
                .title
                .as_ref()
                .is_some_and(|title| title.as_str() == "covered-pointer-target")
        })
    });
    let observed_target = client_named(&observed, "covered-pointer-target");
    let cover = OverrideWindow::create(&xvfb.display, 0, 0, 1_024, 768);
    cover.raise();

    match wire_call(
        &mut stream,
        &mut next_id,
        Call::PointerMove(PointerMoveRequest {
            target: target(&observed_target),
            x: 25,
            y: 30,
        }),
    ) {
        Outcome::Error(error) if error.code == ErrorCode::InvalidArgument => {}
        other => panic!("covered pointer outcome: {other:?}"),
    }

    broker.join().expect("covered broker fixture");
    client.destroy();
    let _ = openbox.kill();
    let _ = openbox.wait();
}

#[test]
fn pointer_move_is_not_sent_when_activity_changes_under_the_x11_grab() {
    assert_pointer_move_interrupted(StopReason::Activity, "pointer-activity");
}

#[test]
fn pointer_move_is_not_sent_when_session_eligibility_changes() {
    assert_pointer_move_interrupted(StopReason::EligibilityChanged, "pointer-eligibility");
}

fn assert_pointer_move_interrupted(reason: StopReason, label: &str) {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);
    let directory = FixtureDir::new(label);
    let broker_socket = directory.0.join("activity.sock");
    let ready = BrokerStatus {
        instance: 53,
        epoch: 11,
        state: BrokerState::Ready,
        reason: StopReason::None,
    };
    let stopped = BrokerStatus {
        instance: 53,
        epoch: 12,
        state: BrokerState::Stopped,
        reason,
    };
    let broker = serve_broker(&broker_socket, vec![ready, stopped, stopped]);
    let config = write_pointer_config(&directory.0, &broker_socket);
    let socket = directory.0.join("seat.sock");
    let _provider = Provider::start(&xvfb.display, &config, &socket);
    let mut stream = UnixStream::connect(&socket).expect("interrupted pointer peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::ObserveTitles,
                Capability::InputPointer,
            ],
        ),
        ServerMessage::Welcome(_)
    ));

    let client = TestClient::create(&xvfb.display, "interrupted-pointer-target");
    let mut next_id = 1;
    let observed = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.iter().any(|candidate| {
            candidate
                .title
                .as_ref()
                .is_some_and(|title| title.as_str() == "interrupted-pointer-target")
        })
    });
    let observed_target = client_named(&observed, "interrupted-pointer-target");
    client
        .connection
        .warp_pointer(NONE, client.root, 0, 0, 0, 0, 3, 4)
        .expect("initial pointer request")
        .check()
        .expect("initial pointer position");
    client.connection.sync().expect("initial pointer sync");

    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::PointerMove(PointerMoveRequest {
                target: target(&observed_target),
                x: 25,
                y: 30,
            }),
        ),
        Outcome::Ok(Reply::Input(reply))
            if reply.completed == 0
                && reply.requested == 1
                && reply.terminal == InputTerminal::Interrupted
    ));
    let pointer = client
        .connection
        .query_pointer(client.root)
        .expect("query interrupted pointer request")
        .reply()
        .expect("query interrupted pointer reply");
    assert_eq!((pointer.root_x, pointer.root_y), (3, 4));

    broker.join().expect("interrupted broker fixture");
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
                agent_seat_proto::Feature::DesktopLaunch,
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
    wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.iter().any(|client| {
            client.id == stale.id
                && client.generation > stale.generation
                && client.title.as_deref() == Some("manage-alpha-renamed")
        })
    });
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

#[test]
fn openbox_launch_is_bounded_policy_controlled_and_shell_free() {
    let xvfb = Xvfb::start();
    let mut openbox = start_openbox(&xvfb.display);
    let directory = FixtureDir::new("launch");
    let user_data = directory.0.join("user-data");
    let system_data = directory.0.join("system-data");
    let user_applications = user_data.join("applications");
    let system_applications = system_data.join("applications");
    let launched = directory.0.join("allowed-launched");
    let unlisted = directory.0.join("unlisted-launched");
    let user = directory.0.join("user-launched");
    let injected = directory.0.join("shell-injected");
    let invalid_executable = directory.0.join("invalid-executable");
    fs::write(&invalid_executable, b"not an executable image\n")
        .expect("invalid executable fixture");
    fs::set_permissions(&invalid_executable, fs::Permissions::from_mode(0o700))
        .expect("invalid executable permissions");

    write_desktop(
        &system_applications,
        "allowed.desktop",
        "Allowed fixture",
        &format!("/usr/bin/touch {}", launched.display()),
    );
    write_desktop(
        &system_applications,
        "unlisted.desktop",
        "Unlisted fixture",
        &format!("/usr/bin/touch {}", unlisted.display()),
    );
    write_desktop(
        &user_applications,
        "user.desktop",
        "User fixture",
        &format!("/usr/bin/touch {}", user.display()),
    );
    write_desktop(
        &system_applications,
        "hostile.desktop",
        "Hostile fixture",
        &format!(
            "/usr/bin/printf \"literal;touch\" /usr/bin/touch {}",
            injected.display()
        ),
    );
    write_desktop(
        &system_applications,
        "failure.desktop",
        "Failure fixture",
        invalid_executable
            .to_str()
            .expect("invalid executable UTF-8"),
    );

    let config = write_launch_config(&directory.0);
    let socket = directory.0.join("seat.sock");
    let provider =
        Provider::start_with_data(&xvfb.display, &config, &socket, &user_data, &system_data);
    let mut stream = UnixStream::connect(&socket).expect("launch peer");
    assert!(matches!(
        hello_with(
            &mut stream,
            vec![
                Capability::ObserveStructure,
                Capability::LaunchList,
                Capability::LaunchExecute,
            ],
        ),
        ServerMessage::Welcome(welcome)
            if welcome.features.contains(&agent_seat_proto::Feature::DesktopLaunch)
                && welcome.granted.as_slice() == [
                    Capability::ObserveStructure,
                    Capability::LaunchList,
                    Capability::LaunchExecute,
                ]
    ));
    let mut next_id = 1;
    let page = match wire_call(
        &mut stream,
        &mut next_id,
        Call::ApplicationsList(ApplicationListRequest {
            cursor: 0,
            limit: 16,
        }),
    ) {
        Outcome::Ok(Reply::Applications(page)) => page,
        other => panic!("unexpected application list outcome: {other:?}"),
    };
    let listed = page
        .applications
        .iter()
        .map(|application| application.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        listed,
        ["allowed.desktop", "failure.desktop", "hostile.desktop"]
    );
    assert_eq!(page.next_cursor, None);

    let startup_id = format!("agent-seat-x11-{}-1", provider.child.id());
    let display = xvfb.display.clone();
    let launch_marker = launched.clone();
    let correlating_client = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !launch_marker.exists() {
            assert!(Instant::now() < deadline, "allowed application did not run");
            thread::sleep(Duration::from_millis(5));
        }
        let client =
            TestClient::create_with_startup_id(&display, "launch-correlated", Some(&startup_id));
        thread::sleep(Duration::from_millis(1_100));
        client.destroy();
    });
    let allowed = match wire_call(
        &mut stream,
        &mut next_id,
        Call::ApplicationLaunch(ApplicationLaunchRequest {
            application: ApplicationId::new("allowed.desktop").expect("application ID"),
        }),
    ) {
        Outcome::Ok(Reply::Launched(reply)) => reply,
        other => panic!("unexpected allowed launch outcome: {other:?}"),
    };
    assert_eq!(allowed.token.get(), 1);
    assert!(
        allowed.client.is_some(),
        "exact startup ID was not correlated"
    );
    correlating_client.join().expect("correlating client");

    for (application, marker) in [("unlisted.desktop", &unlisted), ("user.desktop", &user)] {
        assert!(matches!(
            wire_call(
                &mut stream,
                &mut next_id,
                Call::ApplicationLaunch(ApplicationLaunchRequest {
                    application: ApplicationId::new(application).expect("application ID"),
                }),
            ),
            Outcome::Error(error) if error.code == ErrorCode::Refused
        ));
        assert!(!marker.exists(), "refused application was executed");
    }

    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ApplicationLaunch(ApplicationLaunchRequest {
                application: ApplicationId::new("hostile.desktop").expect("application ID"),
            }),
        ),
        Outcome::Ok(Reply::Launched(reply)) if reply.client.is_none()
    ));
    thread::sleep(Duration::from_millis(50));
    assert!(!injected.exists(), "desktop Exec metadata reached a shell");

    assert!(matches!(
        wire_call(
            &mut stream,
            &mut next_id,
            Call::ApplicationLaunch(ApplicationLaunchRequest {
                application: ApplicationId::new("failure.desktop").expect("application ID"),
            }),
        ),
        Outcome::Error(error) if error.code == ErrorCode::Unavailable
    ));
    assert!(
        openbox
            .try_wait()
            .expect("Openbox after launch failure")
            .is_none()
    );
    let responsive = TestClient::create(&xvfb.display, "openbox-still-responsive");
    let snapshot = wait_snapshot(&mut stream, &mut next_id, |snapshot| {
        snapshot.clients.len() == 1 && snapshot.clients[0].frame.is_some()
    });
    assert!(!snapshot.clients.is_empty());
    responsive.destroy();

    let _ = openbox.kill();
    let _ = openbox.wait();
}
