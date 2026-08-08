//! Agent Seat Settings command and GTK entry point.

#![forbid(unsafe_code)]

mod ui;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use agent_seat_settings::SettingsModel;
use agent_seat_x11::default_policy_path;

const HELP: &str = r#"agent-seat-settings - review and edit Agent Seat policy

USAGE:
  agent-seat-settings [--config PATH]
  agent-seat-settings [--config PATH] --check
  agent-seat-settings [--config PATH] --print
  agent-seat-settings [--config PATH] --restore-previous

OPTIONS:
  --config PATH       Use an existing policy at an absolute path
  --check             Validate and report the saved policy without a display
  --print             Print the exact validated saved policy without a display
  --restore-previous  Atomically exchange the saved and .previous policies
  -h, --help          Print this help

With no command, the GTK editor opens. A missing default policy is created as
the same documented, private, disabled template produced by agent-seat-x11.
Explicit paths are never created. CLI commands do not initialize GTK or X11.

Saving or restoring changes the file on disk only. A running provider keeps
its original policy until it is restarted."#;

fn main() {
    if let Err(error) = run(std::env::args_os()) {
        eprintln!("agent-seat-settings: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let options = Options::parse(arguments)?;
    match options.command {
        Command::Help => {
            println!("{HELP}");
            Ok(())
        }
        Command::Gui => ui::run(options.config),
        Command::Check => {
            let model = open_existing(options.config.as_deref())?;
            println!(
                "{}: valid and {}",
                model.path().display(),
                if model.draft().is_enabled() {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            Ok(())
        }
        Command::Print => {
            let model = open_existing(options.config.as_deref())?;
            print!("{}", model.saved_source());
            Ok(())
        }
        Command::RestorePrevious => {
            let mut model = open_existing(options.config.as_deref())?;
            model.restore_previous()?;
            println!(
                "Restored {} from its validated recovery policy.\n\
                 The displaced policy is now at {}.\n\
                 Restart a running provider to activate the restored policy.",
                model.path().display(),
                model.recovery_path().display()
            );
            Ok(())
        }
    }
}

fn open_existing(path: Option<&Path>) -> Result<SettingsModel, String> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => default_policy_path()?,
    };
    SettingsModel::open(&path)
}

#[derive(Clone, Copy)]
enum Command {
    Gui,
    Check,
    Print,
    RestorePrevious,
    Help,
}

struct Options {
    config: Option<PathBuf>,
    command: Command,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let mut config = None;
        let mut command = Command::Gui;
        let mut selected_command = false;
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--config") => {
                    if config.is_some() {
                        return Err("--config may be specified only once".to_owned());
                    }
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--config requires an absolute path".to_owned())?;
                    let path = PathBuf::from(value);
                    if !path.is_absolute() {
                        return Err("--config requires an absolute path".to_owned());
                    }
                    config = Some(path);
                }
                Some("--check") => select_command(
                    &mut command,
                    &mut selected_command,
                    Command::Check,
                    "--check",
                )?,
                Some("--print") => select_command(
                    &mut command,
                    &mut selected_command,
                    Command::Print,
                    "--print",
                )?,
                Some("--restore-previous") => select_command(
                    &mut command,
                    &mut selected_command,
                    Command::RestorePrevious,
                    "--restore-previous",
                )?,
                Some("-h" | "--help") => {
                    select_command(&mut command, &mut selected_command, Command::Help, "--help")?
                }
                Some(value) => return Err(format!("unknown argument: {value}")),
                None => return Err("arguments must be valid UTF-8".to_owned()),
            }
        }
        Ok(Self { config, command })
    }
}

fn select_command(
    command: &mut Command,
    selected: &mut bool,
    candidate: Command,
    name: &str,
) -> Result<(), String> {
    if *selected {
        return Err(format!("{name} cannot be combined with another command"));
    }
    *command = candidate;
    *selected = true;
    Ok(())
}
