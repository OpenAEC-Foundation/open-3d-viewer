//! MCP server for Open 3D Viewer Studio: lets an AI agent (Claude Code, Claude Desktop or any MCP
//! client) drive the viewer as a person would: open files, look around, show and hide layers,
//! find and select elements, cut with a section box or plane, measure and take screenshots.
//!
//! It speaks MCP over stdio (JSON-RPC, one message per line) and passes every tool call on to the
//! running viewer through its local control port (`remote_link`). When no viewer runs it starts
//! one: `O3V_VIEWER`, else the viewer next to this program. The tools and their handling are in
//! `mcp.rs`, which the viewer also serves over HTTP for the OpenAEC SuperCloud.
//!
//! Usage:
//!   open-3d-viewer-mcp                  (started by the MCP client)
//!   open-3d-viewer-mcp --prefixed       (tool names with the prefix viewer3d_, as in the SuperCloud;
//!                                        also with OPENAEC_BEHEERD=1 or O3V_MCP_PREFIX=1)
//!   open-3d-viewer-mcp --print-config   (the JSON to add it to an MCP client)
//!   open-3d-viewer-mcp --list-tools     (the tool definitions as JSON, with the prefix)

#[path = "../remote_link.rs"]
mod remote_link;

#[path = "../mcp.rs"]
#[allow(dead_code)] // the HTTP endpoint is the viewer's
mod mcp;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ---- the viewer -------------------------------------------------------------------------------

struct Viewer {
    conn: Option<(BufReader<TcpStream>, TcpStream, String)>,
}

impl Viewer {
    /// Connects to the announced viewer.
    fn connect_announced() -> Option<(BufReader<TcpStream>, TcpStream, String)> {
        let a = remote_link::read_announcement()?;
        let stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, a.port)), Duration::from_secs(2)).ok()?;
        let _ = stream.set_nodelay(true);
        Some((BufReader::new(stream.try_clone().ok()?), stream, a.token))
    }

    /// Connects, starting the viewer first when none runs.
    fn connect(&mut self) -> Result<(), String> {
        if self.conn.is_some() {
            return Ok(());
        }
        if let Some(c) = Self::connect_announced() {
            self.conn = Some(c);
            return Ok(());
        }
        let exe = viewer_exe().ok_or(
            "Open 3D Viewer Studio is not running and was not found next to this MCP server. Start the viewer, or set O3V_VIEWER to its .exe.",
        )?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // its own process group, so it stays open when the MCP client stops this server
            cmd.creation_flags(0x0000_0200 | 0x0000_0008); // CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS
        }
        cmd.spawn().map_err(|e| format!("could not start {}: {e}", exe.display()))?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(c) = Self::connect_announced() {
                self.conn = Some(c);
                return Ok(());
            }
        }
        Err(format!("started {} but it did not answer within 30 s", exe.display()))
    }

    /// Sends a command and waits for its answer; reconnects once when the viewer was restarted.
    fn call(&mut self, cmd: &str, args: &Value, timeout: Duration) -> Result<Value, String> {
        for attempt in 0..2 {
            self.connect()?;
            let (reader, writer, token) = self.conn.as_mut().expect("connected");
            let request = json!({ "token": token, "cmd": cmd, "args": args });
            let _ = writer.set_read_timeout(Some(timeout));
            let mut line = String::new();
            let sent = writeln!(writer, "{request}").and_then(|_| writer.flush());
            let read = sent.and_then(|_| reader.read_line(&mut line));
            match read {
                Ok(n) if n > 0 => {
                    let answer: Value = serde_json::from_str(&line).map_err(|e| format!("bad answer from the viewer: {e}"))?;
                    if answer["ok"] == true {
                        return Ok(answer["result"].clone());
                    }
                    let error = answer["error"].as_str().unwrap_or("unknown error").to_string();
                    if error == "wrong token" && attempt == 0 {
                        self.conn = None; // another viewer took over the announcement
                        continue;
                    }
                    return Err(error);
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    self.conn = None;
                    return Err(format!("the viewer did not answer within {} s", timeout.as_secs()));
                }
                _ => self.conn = None, // closed: the viewer was restarted
            }
        }
        Err("lost the connection to the viewer".into())
    }
}

/// The viewer program: `O3V_VIEWER`, else `open-3d-viewer` or the newest
/// `open-3d-viewer-*` build next to this program.
fn viewer_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("O3V_VIEWER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let plain = dir.join(format!("open-3d-viewer{}", std::env::consts::EXE_SUFFIX));
    if plain.is_file() {
        return Some(plain);
    }
    std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            name.starts_with("open-3d-viewer-") && name.ends_with(std::env::consts::EXE_SUFFIX) && !name.contains("mcp")
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Tool names with the SuperCloud prefix: asked for, or in managed mode (OpenAEC Linux).
fn prefixed() -> bool {
    let on = |name| matches!(std::env::var(name).as_deref(), Ok("1"));
    std::env::args().any(|a| a == "--prefixed") || on("O3V_MCP_PREFIX") || on("OPENAEC_BEHEERD")
}

fn main() {
    if std::env::args().any(|a| a == "--print-config") {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        let config = json!({ "mcpServers": { "open-3d-viewer": { "command": exe, "args": [] } } });
        println!("{}", serde_json::to_string_pretty(&config).unwrap_or_default());
        println!("\nClaude Code:  claude mcp add open-3d-viewer -- \"{exe}\"");
        return;
    }
    if std::env::args().any(|a| a == "--list-tools") {
        println!("{}", serde_json::to_string_pretty(&json!({ "tools": mcp::tool_list(true) })).unwrap_or_default());
        return;
    }
    let prefixed = prefixed();
    let mut viewer = Viewer { conn: None };
    let mut call = |cmd: &str, args: &Value, timeout: Duration| viewer.call(cmd, args, timeout);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = mcp::handle_text(&line, prefixed, &mut call) {
            if writeln!(stdout, "{answer}").and_then(|_| stdout.flush()).is_err() {
                break;
            }
        }
    }
}
