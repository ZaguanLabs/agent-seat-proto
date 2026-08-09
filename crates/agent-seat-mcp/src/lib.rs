//! Generic, authority-free MCP companion.

#![forbid(unsafe_code)]

mod discovery;
mod mcp;
mod seat;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use agent_seat_proto::Advertisement;
use serde_json::json;

const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const INSTALLED_COMPANION: &str = "/usr/bin/agent-seat-mcp";
const PRIVATE_SOCKET_NAME: &str = "x11-input.sock";
const PROVIDER_FD_NAME: &str = "agent-seat-provider";

/// Runs the command-line companion.
///
/// # Errors
///
/// Returns an error for invalid arguments or a failed stdio server.
pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let mut socket = None;
    let mut print_private_config = false;
    let mut provider_fd = false;
    while let Some(argument) = arguments.next() {
        if argument == "--socket" {
            if socket.is_some() {
                return Err("--socket may be specified only once".to_owned());
            }
            let path = arguments
                .next()
                .ok_or_else(|| "--socket requires an absolute path".to_owned())?;
            socket = Some(PathBuf::from(path));
        } else if argument == "--print-mcp-config" {
            println!("{{\"mcpServers\":{{\"agent-seat\":{{\"command\":\"agent-seat-mcp\"}}}}}}");
            return Ok(());
        } else if argument == "--print-private-mcp-config" {
            if print_private_config {
                return Err("--print-private-mcp-config may be specified only once".to_owned());
            }
            print_private_config = true;
        } else if argument == "--provider-fd" {
            if provider_fd {
                return Err("--provider-fd may be specified only once".to_owned());
            }
            provider_fd = true;
        } else if argument == "--help" || argument == "-h" {
            println!(
                "agent-seat-mcp [--socket PATH]\n\
                 agent-seat-mcp [--socket PATH] --print-private-mcp-config\n\n\
                 Generic MCP companion for Agent Seat providers. Socket resolution order:\n\
                 --socket, AGENT_SEAT_SOCKET, then selection-bound X11 discovery.\n\
                 Initialization and tool listing do not resolve or connect to a seat.\n\n\
                 --print-private-mcp-config emits a copyable systemd user-service\n\
                 registration for the optional input deployment. It uses an explicit\n\
                 provider socket and gives the companion no X11, input-device, broker,\n\
                 home, network, or arbitrary-executable authority."
            );
            return Ok(());
        } else {
            return Err(format!("unknown argument {:?}", argument));
        }
    }
    if print_private_config {
        if provider_fd {
            return Err("--provider-fd cannot print an MCP configuration".to_owned());
        }
        return print_private_mcp_config(socket.as_deref());
    }
    if provider_fd {
        if socket.is_some() {
            return Err("--provider-fd cannot be combined with --socket".to_owned());
        }
        return mcp::serve(None, Some(inherited_seat()?));
    }
    mcp::serve(socket, None)
}

fn print_private_mcp_config(explicit_socket: Option<&Path>) -> Result<(), String> {
    let socket = private_socket(explicit_socket)?;
    let socket = socket
        .to_str()
        .ok_or_else(|| "private-profile socket path must be UTF-8".to_owned())?;
    Advertisement::new(socket)
        .map_err(|error| format!("invalid private-profile socket path: {error}"))?;
    let arguments = private_systemd_arguments(socket);
    println!(
        "{}",
        json!({
            "mcpServers": {
                "agent-seat": {
                    "command": SYSTEMD_RUN,
                    "args": arguments,
                }
            }
        })
    );
    Ok(())
}

fn private_socket(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        "XDG_RUNTIME_DIR or an explicit --socket is required for the private MCP profile".to_owned()
    })?;
    let runtime = PathBuf::from(runtime);
    if !runtime.is_absolute() {
        return Err("XDG_RUNTIME_DIR must be absolute".to_owned());
    }
    Ok(runtime.join("agent-seat").join(PRIVATE_SOCKET_NAME))
}

fn private_systemd_arguments(socket: &str) -> Vec<String> {
    let mut arguments = [
        "--user",
        "--pipe",
        "--wait",
        "--collect",
        "--quiet",
        "--service-type=exec",
        "--property=PrivateDevices=yes",
        "--property=DevicePolicy=strict",
        "--property=PrivateNetwork=yes",
        "--property=RestrictAddressFamilies=AF_UNIX",
        "--property=SystemCallArchitectures=native",
        "--property=SystemCallFilter=@system-service",
        "--property=SystemCallErrorNumber=EPERM",
        "--property=NoNewPrivileges=yes",
        "--property=CapabilityBoundingSet=",
        "--property=AmbientCapabilities=",
        "--property=LockPersonality=yes",
        "--property=MemoryDenyWriteExecute=yes",
        "--property=RestrictNamespaces=yes",
        "--property=RestrictRealtime=yes",
        "--property=RestrictSUIDSGID=yes",
        "--property=RemoveIPC=yes",
        "--property=ProtectSystem=strict",
        "--property=ProtectHome=yes",
        "--property=PrivateTmp=yes",
        "--property=ProtectProc=invisible",
        "--property=ProcSubset=pid",
        "--property=ProtectClock=yes",
        "--property=ProtectControlGroups=yes",
        "--property=ProtectHostname=yes",
        "--property=ProtectKernelLogs=yes",
        "--property=ProtectKernelModules=yes",
        "--property=ProtectKernelTunables=yes",
        "--property=InaccessiblePaths=-/dev/input -/dev/uinput",
        "--property=TemporaryFileSystem=/run:ro",
        "--property=NoExecPaths=/",
        "--property=ExecPaths=/usr/bin/agent-seat-mcp -/usr/lib -/usr/lib64 -/lib -/lib64",
        "--property=UnsetEnvironment=DISPLAY XAUTHORITY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS SSH_AUTH_SOCK AGENT_SEAT_SOCKET HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_DATA_DIRS XDG_RUNTIME_DIR PATH",
        "--property=UMask=0077",
        "--property=TasksMax=2",
        "--property=LimitNOFILE=32",
        "--property=LimitCORE=0",
        "--property=MemoryMax=64M",
        "--property=CPUQuota=25%",
        "--property=KillMode=mixed",
        "--property=Restart=no",
    ]
    .map(str::to_owned)
    .to_vec();
    arguments.push(format!("--property=OpenFile={socket}:{PROVIDER_FD_NAME}"));
    arguments.extend([
        "--".to_owned(),
        INSTALLED_COMPANION.to_owned(),
        "--provider-fd".to_owned(),
    ]);
    arguments
}

fn inherited_seat() -> Result<seat::Seat, String> {
    let process_id = std::process::id().to_string();
    if std::env::var("LISTEN_PID").ok().as_deref() != Some(process_id.as_str())
        || std::env::var("LISTEN_FDS").ok().as_deref() != Some("1")
        || std::env::var("LISTEN_FDNAMES").ok().as_deref() != Some(PROVIDER_FD_NAME)
    {
        return Err("inherited provider descriptor environment is not exact".to_owned());
    }
    let mut descriptors = sd_listen_fds::get()
        .map_err(|_| "inherited provider descriptor environment is malformed".to_owned())?;
    if descriptors.len() != 1 {
        return Err("exactly one inherited provider descriptor is required".to_owned());
    }
    let (name, descriptor) = descriptors
        .pop()
        .ok_or_else(|| "inherited provider descriptor is missing".to_owned())?;
    if name.as_deref() != Some(PROVIDER_FD_NAME) {
        return Err("inherited provider descriptor name is invalid".to_owned());
    }
    seat::Seat::from_stream(std::os::unix::net::UnixStream::from(descriptor.into_std()))
        .map_err(|error| error.to_string())
}
