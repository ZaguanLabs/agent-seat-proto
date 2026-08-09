//! Standalone, policy-owning Tier 0 X11 Agent Seat provider.

#![forbid(unsafe_code)]

mod active;
mod config;
mod launch;
mod observer;
mod ownership;
mod runtime;
mod session;

pub use active::{ActivePolicyStatus, active_policy_status};
pub use config::{
    ClientScope, LaunchMode, MAX_POLICY_CAPABILITIES, MAX_POLICY_IO_TIMEOUT_MS,
    MAX_POLICY_REQUESTS, MAX_POLICY_SESSIONS, MIN_POLICY_IO_TIMEOUT_MS, MIN_POLICY_REQUESTS,
    MIN_POLICY_SESSIONS, PolicyDraft, PolicySnapshot, default_path as default_policy_path,
    ensure_default_policy, read_policy, recovery_policy_path, replace_policy,
};
pub use launch::{MAX_INSTALLED_APPLICATIONS, installed_applications};

use std::ffi::OsString;
use std::fs;
use std::io;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use signal_hook::consts::signal::{SIGINT, SIGTERM};

const HELP: &str = r#"agent-seat-x11 - local Agent Seat authority for X11

USAGE:
  agent-seat-x11 [OPTIONS]

OPTIONS:
  --config PATH   Read an existing configuration at an absolute path
  --socket PATH   Use an absolute socket path instead of XDG runtime discovery
  --check-config  Validate policy without connecting to X11 or creating a socket
  -h, --help      Print this help

FIRST RUN:
  When run with no options, a missing default configuration is created at
  $XDG_CONFIG_HOME/agent-seat/config.toml, or
  $HOME/.config/agent-seat/config.toml when XDG_CONFIG_HOME is unset.

  The generated file is mode 0600, extensively commented, and disabled.
  Review its capabilities and validate it with agent-seat-x11 --check-config.
  When ready, set enabled = true, validate again, then run agent-seat-x11.

  The provider runs in the foreground inside the X11 desktop session.
  Explicit --config paths are never created or overwritten."#;

/// Runs the foreground provider until SIGINT, SIGTERM, or selection loss.
///
/// # Errors
///
/// Returns an error for invalid arguments/configuration, unsafe paths, X11
/// ownership failure, listener failure, or provider event-loop failure.
pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let options = match Options::parse(arguments)? {
        Command::Run(options) => options,
        Command::Help => {
            println!("{HELP}");
            return Ok(());
        }
    };
    let creates_first_run_config =
        options.config.is_none() && options.socket.is_none() && !options.check_config;
    let config_path = options.config.unwrap_or(config::default_path()?);
    if creates_first_run_config && config::create_first_run_config(&config_path)? {
        println!(
            "Created first-run configuration at {}.\n\
             The provider has not started. Review the documented policy and run\n\
             `agent-seat-x11 --check-config`. When ready, set enabled = true, validate again,\n\
             then start the provider.",
            config_path.display()
        );
        return Ok(());
    }
    if options.check_config {
        let state = if config::Config::check(&config_path)? {
            "enabled"
        } else {
            "disabled"
        };
        println!("{}: valid and {state}", config_path.display());
        return Ok(());
    }
    let (policy_snapshot, config) = config::Config::load(&config_path)?;
    if config.provider_private_devices() {
        require_private_input_devices(Path::new("/dev"))?;
    }
    let config = Arc::new(config);

    let screen = ownership::selected_screen()?;
    let listener = runtime::ListenerGuard::bind(options.socket.as_deref(), screen)?;
    let ownership = ownership::Ownership::claim(listener.path())?;
    if ownership.screen() != screen {
        return Err("selected X11 screen changed during provider startup".to_owned());
    }
    let _active_policy = match active::ActivePolicyGuard::publish(&policy_snapshot) {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("agent-seat-x11: active-policy status unavailable: {error}");
            None
        }
    };

    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&stopping))
        .and_then(|_| signal_hook::flag::register(SIGTERM, Arc::clone(&stopping)))
        .map_err(|error| format!("cannot install shutdown handlers: {error}"))?;

    eprintln!(
        "agent-seat-x11: ready on screen {screen} at {}",
        listener.path().display()
    );
    serve(listener, ownership, config, stopping)
}

fn serve(
    listener: runtime::ListenerGuard,
    mut ownership: ownership::Ownership,
    config: Arc<config::Config>,
    stopping: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut sessions: Vec<JoinHandle<Result<(), String>>> =
        Vec::with_capacity(config.max_sessions());
    let launcher = Arc::new(launch::LaunchSupervisor::new(
        config.provider_private_devices(),
    )?);
    let mut next_session = 1_u64;
    let mut ownership_lost = false;

    while !stopping.load(Ordering::Relaxed) {
        reap(&mut sessions);
        launcher.reap();
        if ownership.lost()? {
            ownership_lost = true;
            break;
        }
        match listener.listener().accept() {
            Ok((stream, _)) if sessions.len() >= config.max_sessions() => {
                session::reject_capacity(stream);
            }
            Ok((stream, _)) => {
                let session_number = NonZeroU64::new(next_session)
                    .ok_or_else(|| "session identity space is exhausted".to_owned())?;
                next_session = next_session
                    .checked_add(1)
                    .ok_or_else(|| "session identity space is exhausted".to_owned())?;
                let session_config = Arc::clone(&config);
                let session_launcher = Arc::clone(&launcher);
                let handle = thread::Builder::new()
                    .name(format!("agent-seat-{session_number}"))
                    .spawn(move || {
                        session::run(stream, session_config, session_launcher, session_number)
                    })
                    .map_err(|error| format!("cannot start bounded session worker: {error}"))?;
                sessions.push(handle);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(format!("provider listener failed: {error}")),
        }
    }
    for handle in sessions {
        report_session(handle);
    }
    if ownership_lost {
        Err("Agent Seat selection ownership was lost".to_owned())
    } else {
        ownership.withdraw()?;
        Ok(())
    }
}

fn require_private_input_devices(device_root: &Path) -> Result<(), String> {
    for relative in ["input", "uinput"] {
        let path = device_root.join(relative);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(format!(
                    "input.provider_private_devices requires {path:?} to be hidden; start the documented private-device user service"
                ));
            }
            Err(_) => {
                return Err(format!(
                    "input.provider_private_devices cannot verify that {path:?} is hidden"
                ));
            }
        }
    }
    Ok(())
}

fn reap(sessions: &mut Vec<JoinHandle<Result<(), String>>>) {
    let mut index = 0;
    while index < sessions.len() {
        if sessions[index].is_finished() {
            report_session(sessions.swap_remove(index));
        } else {
            index += 1;
        }
    }
}

fn report_session(handle: JoinHandle<Result<(), String>>) {
    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("agent-seat-x11: session ended: {error}"),
        Err(_) => eprintln!("agent-seat-x11: session worker panicked"),
    }
}

#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    socket: Option<PathBuf>,
    check_config: bool,
}

enum Command {
    Run(Options),
    Help,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            if argument == "--config" {
                if options.config.is_some() {
                    return Err("--config may be specified only once".to_owned());
                }
                options.config =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        "--config requires an absolute path".to_owned()
                    })?));
            } else if argument == "--socket" {
                if options.socket.is_some() {
                    return Err("--socket may be specified only once".to_owned());
                }
                options.socket =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        "--socket requires an absolute path".to_owned()
                    })?));
            } else if argument == "--check-config" {
                if options.check_config {
                    return Err("--check-config may be specified only once".to_owned());
                }
                options.check_config = true;
            } else if argument == "--help" || argument == "-h" {
                return Ok(Command::Help);
            } else {
                return Err(format!("unknown argument {argument:?}"));
            }
        }
        if options
            .config
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err("--config requires an absolute path".to_owned());
        }
        if options
            .socket
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err("--socket requires an absolute path".to_owned());
        }
        Ok(Command::Run(options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, DirBuilder};
    use std::os::unix::fs::DirBuilderExt as _;

    struct DeviceRoot(PathBuf);

    impl DeviceRoot {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("agent-seat-private-devices-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            let mut builder = DirBuilder::new();
            builder.mode(0o700).create(&path).expect("device fixture");
            Self(path)
        }
    }

    impl Drop for DeviceRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn private_device_gate_requires_input_nodes_to_be_absent() {
        let root = DeviceRoot::new();
        assert!(require_private_input_devices(&root.0).is_ok());

        fs::write(root.0.join("uinput"), []).expect("uinput fixture");
        assert!(require_private_input_devices(&root.0).is_err());
        fs::remove_file(root.0.join("uinput")).expect("remove uinput fixture");

        fs::create_dir(root.0.join("input")).expect("input fixture");
        assert!(require_private_input_devices(&root.0).is_err());
    }
}
