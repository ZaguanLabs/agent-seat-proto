//! Standalone Tier 0 X11 Agent Seat provider process.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    match agent_seat_x11::run(std::env::args_os().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-seat-x11: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
