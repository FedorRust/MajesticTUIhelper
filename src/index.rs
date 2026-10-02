use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{CodeFile, Corpus, Family, Server};
use crate::parse::{self, parse_document};

pub fn laws_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("MJ_LAWS") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("laws"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("laws"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("laws"));
            candidates.push(dir.join("../../laws"));
        }
    }
    candidates
        .into_iter()
        .find(|path| path.join("portland").is_dir() && path.join("memphis").is_dir())
}

pub fn load_corpus(root: &Path, server: Server) -> Result<Corpus, String> {
    let dir = root.join(server.as_str());
    if !dir.is_dir() {
        return Err(format!("законка {} ещё не скачана", server.title()));
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|err| format!("не читается {}: {err}", dir.display()))?
        .filter_map(|item| item.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect();
    entries.sort();

    let mut articles = Vec::new();
    let mut files = Vec::new();
    for path in entries {
        let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
            continue;
        };
        if name.eq_ignore_ascii_case("INDEX.md") {
            continue;
        }
        let Some(family) = family_from_filename(name) else {
            continue;
        };
        let text = fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let url = parse::source_url(&text);
        articles.extend(parse_document(&text, family));
        files.push(CodeFile { family, path, url });
    }
    files.sort_by_key(|file| file.family);
    Ok(Corpus {
        server,
        articles,
        files,
    })
}

pub fn family_from_filename(name: &str) -> Option<Family> {
    let name = name.to_lowercase();
    if name.contains("ugolovnyi-kodeks") {
        Some(Family::Uk)
    } else if name.contains("protsessual") {
        Some(Family::Upk)
    } else if name.contains("dorozhnyi-kodeks") {
        Some(Family::Pdd)
    } else if name.contains("administrativnyi-kodeks")
        || name.contains("administrativnykh-pravonarusheniy")
    {
        Some(Family::Koap)
    } else {
        None
    }
}

pub fn family_from_title(title: &str) -> Option<Family> {
    let title = title.to_lowercase().replace('ё', "е");
    if title.contains("приложен") {
        return None;
    }
    if title.contains("уголовн") {
        Some(Family::Uk)
    } else if title.contains("процессуальн") {
        Some(Family::Upk)
    } else if title.contains("дорожн") {
        Some(Family::Pdd)
    } else if title.contains("административ") {
        Some(Family::Koap)
    } else {
        None
    }
}
