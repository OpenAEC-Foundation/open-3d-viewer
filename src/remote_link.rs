//! The link between a running viewer and its MCP server (`open-3d-viewer-mcp`), shared by both
//! binaries: where the viewer announces its control port, and the line protocol.
//!
//! The viewer listens on 127.0.0.1 (a free port) and writes the port and a random token to the
//! announcement file in the user's settings folder. A client sends one JSON object per line,
//! `{"token": "…", "cmd": "get_state", "args": {…}}`, and gets one line back,
//! `{"ok": true, "result": …}` or `{"ok": false, "error": "…"}`. Only someone who can read the
//! user's settings folder knows the token, so other users on the machine cannot drive the viewer.

use std::path::PathBuf;

/// What a running viewer announces.
#[derive(Debug, Clone, PartialEq)]
pub struct Announcement {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// The user's settings folder: `%APPDATA%\OpenAEC\Open 3D Viewer Studio`, or the same under
/// `$XDG_CONFIG_HOME` / `~/.config` elsewhere.
pub fn settings_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("OpenAEC").join("Open 3D Viewer Studio"))
}

/// `remote.json` in the settings folder, next to the preferences.
pub fn announcement_path() -> Option<PathBuf> {
    Some(settings_dir()?.join("remote.json"))
}

#[allow(dead_code)] // the viewer writes, the MCP server reads
pub fn announce(a: &Announcement) -> std::io::Result<()> {
    let path = announcement_path().ok_or_else(|| std::io::Error::other("no settings folder"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::json!({ "port": a.port, "token": a.token, "pid": a.pid });
    std::fs::write(path, json.to_string())
}

#[allow(dead_code)]
pub fn read_announcement() -> Option<Announcement> {
    let text = std::fs::read_to_string(announcement_path()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Announcement {
        port: u16::try_from(v["port"].as_u64()?).ok()?,
        token: v["token"].as_str()?.to_string(),
        pid: u32::try_from(v["pid"].as_u64().unwrap_or(0)).unwrap_or(0),
    })
}
