//! Generic, authority-free MCP companion.

#![forbid(unsafe_code)]

mod discovery;
mod mcp;
mod seat;

use std::ffi::OsString;
use std::path::PathBuf;

/// Runs the command-line companion.
///
/// # Errors
///
/// Returns an error for invalid arguments or a failed stdio server.
pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let mut socket = None;
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
        } else if argument == "--help" || argument == "-h" {
            println!(
                "agent-seat-mcp [--socket PATH]\n\n\
                 Generic MCP companion for Agent Seat providers. Socket resolution order:\n\
                 --socket, AGENT_SEAT_SOCKET, then selection-bound X11 discovery.\n\
                 Initialization and tool listing do not resolve or connect to a seat."
            );
            return Ok(());
        } else {
            return Err(format!("unknown argument {:?}", argument));
        }
    }
    mcp::serve(socket)
}
