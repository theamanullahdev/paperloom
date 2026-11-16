//! MCP over stdin/stdout, for agents that start a program and talk to it
//! (one JSON message per line each way). Lives only as long as the agent
//! keeps it open. Tool calls work while MCP is switched on in the app.

use std::io::{BufRead, Write};

use crate::tools::Gate;

pub fn run() -> i32 {
    paperloom_common::log::init("mcp");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = crate::protocol::handle_text(&line, Gate::NeedsSwitch) {
            if writeln!(stdout, "{answer}").and_then(|()| stdout.flush()).is_err() {
                break;
            }
        }
    }
    0
}
