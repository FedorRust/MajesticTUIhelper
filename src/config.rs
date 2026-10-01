use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Server;

#[derive(Serialize, Deserialize)]
struct StateFile {
    server: String,
    #[serde(default)]
    theme: String,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mj")
}

pub fn session_path() -> PathBuf {
    config_dir().join("session")
}

pub fn state_path() -> PathBuf {
    config_dir().join("state.json")
}

fn load_state() -> Option<StateFile> {
    let text = fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn store_state(server: Server, theme: &str) -> io::Result<()> {
    ensure_config_dir()?;
    let body = serde_json::to_string_pretty(&StateFile {
        server: server.as_str().to_string(),
        theme: theme.to_string(),
    })
    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(state_path(), body + "\n")
}

pub fn load_server() -> Option<Server> {
    Server::parse(&load_state()?.server)
}

pub fn load_theme_id() -> Option<String> {
    let theme = load_state()?.theme;
    if theme.is_empty() {
        None
    } else {
        Some(theme)
    }
}

pub fn save_server(server: Server) -> io::Result<()> {
    let theme = load_state().map(|state| state.theme).unwrap_or_default();
    store_state(server, &theme)
}

pub fn save_theme(server: Server, theme: &str) -> io::Result<()> {
    store_state(server, theme)
}

pub fn load_session() -> Vec<(String, String)> {
    let text = fs::read_to_string(session_path()).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (name, value) = line.split_once('=')?;
            if name.is_empty() {
                None
            } else {
                Some((name.to_string(), value.to_string()))
            }
        })
        .collect()
}

pub fn session_ready() -> bool {
    load_session()
        .iter()
        .any(|(name, _)| name == "xf_user" || name == "xf_session")
}

pub fn save_session(cookies: &[(String, String)]) -> io::Result<()> {
    ensure_config_dir()?;
    let mut body = String::from("# mj session\n");
    for (name, value) in cookies {
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    write_private(&session_path(), &body)
}

fn ensure_config_dir() -> io::Result<PathBuf> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

pub fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(contents.as_bytes())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_file_is_mode_600() {
        let dir = std::env::temp_dir().join(format!("mj-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session");
        write_private(&path, "xf_session=abc\n").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            mode, 0o600,
            "кука сессии должна быть доступна только владельцу"
        );
    }
}
