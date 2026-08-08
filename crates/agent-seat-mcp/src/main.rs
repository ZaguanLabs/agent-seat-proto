//! Authority-free MCP companion process.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    match agent_seat_mcp::run(std::env::args_os().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agent-seat-mcp: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
