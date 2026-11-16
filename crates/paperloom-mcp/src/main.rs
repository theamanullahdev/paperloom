//! `paperloom-mcp`: lets an AI agent on this computer do what Paperloom's
//! window does. One set of tools, reachable every way an agent might
//! come knocking:
//!
//!   paperloom-mcp                 MCP over stdin/stdout (agents that start a program)
//!   paperloom-mcp serve           the local server: MCP over HTTP, a plain JSON
//!                                 API and a web page, all on 127.0.0.1
//!   paperloom-mcp call <tool> ['{"json":"arguments"}']   one call from a shell
//!   paperloom-mcp tools | guide   what there is, and how to use it
//!
//! Nothing here runs unless the user switched MCP on in the app (which
//! starts `serve`); the other forms answer "MCP is off" until then.

mod args;
mod guide;
mod http;
mod jobs;
mod protocol;
mod stdio;
mod tools;
mod web;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None | Some("stdio") => stdio::run(),
        Some("serve") => {
            paperloom_common::log::init("mcp");
            let port = args.iter().skip_while(|arg| *arg != "--port").nth(1).and_then(|port| port.parse().ok());
            http::serve(port)
        }
        Some("call") => call(&args[1..]),
        Some("tools") => {
            for tool in tools::all() {
                println!("{:<22} {}", tool.name, tool.about);
            }
            0
        }
        Some("guide") => {
            print!("{}", guide::text(&paperloom_common::mcp::McpConfig::load()));
            0
        }
        Some("--version" | "-V") => {
            println!("paperloom-mcp {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            println!("Usage: paperloom-mcp [stdio | serve [--port N] | call <tool> ['{{\"json\":\"arguments\"}}'] | tools | guide]\nSwitch MCP on in Paperloom (the MCP page) first.");
            0
        }
    };
    std::process::exit(code);
}

/// `paperloom-mcp call <tool> [json]`: prints the result as JSON; exit 1 on an error.
fn call(args: &[String]) -> i32 {
    let Some(name) = args.first() else {
        eprintln!("Usage: paperloom-mcp call <tool> ['{{\"json\":\"arguments\"}}']   (see: paperloom-mcp tools)");
        return 2;
    };
    let arguments = match args.get(1) {
        Some(text) => match serde_json::from_str(text) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("The arguments are not valid JSON: {err}");
                return 2;
            }
        },
        None => serde_json::json!({}),
    };
    paperloom_common::log::init("mcp");
    match tools::call(name, &arguments, tools::Gate::NeedsSwitch) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}
