//! Standalone, policy-owning Tier 0 X11 Agent Seat provider.

#![forbid(unsafe_code)]

mod config;
mod launch;
mod observer;
mod ownership;
mod runtime;
mod session;

use std::ffi::OsString;
use std::io;
use std::num::NonZeroU64;
use std::path::PathBuf;
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
  Review its capabilities, set enabled = true, validate it with
  agent-seat-x11 --check-config, then run agent-seat-x11 again.

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
             The provider has not started. Review the documented policy, set enabled = true,\n\
             then run `agent-seat-x11 --check-config` and start the provider again.",
            config_path.display()
        );
        return Ok(());
    }
    let config = Arc::new(config::Config::load(&config_path)?);
    if options.check_config {
        println!("{}: valid and enabled", config_path.display());
        return Ok(());
    }

    let screen = ownership::selected_screen()?;
    let listener = runtime::ListenerGuard::bind(options.socket.as_deref(), screen)?;
    let ownership = ownership::Ownership::claim(listener.path())?;
    if ownership.screen() != screen {
        return Err("selected X11 screen changed during provider startup".to_owned());
    }

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
    let launcher = Arc::new(launch::LaunchSupervisor::new());
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
