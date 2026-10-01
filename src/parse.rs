use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Article, Family, Part};

static ARTICLE_AT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Статья\s+\d").expect("article regex"));
static PART_AT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:часть|ч\.)\s*\d+(?:\.\d+)*").expect("part regex"));
static PUNISH_AT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)Наказание\s*:").expect("punishment regex"));
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*Статья\s+(\d+(?:\.\d+)*)\s*(.*)$").expect("header regex")
});
static BRACKET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\[\]]+)\]").expect("bracket regex"));
static PART_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:часть|ч\.)\s*(\d+(?:\.\d+)*)\s*(★*)\s*(.*)$").expect("part line")
});

pub fn parse_document(text: &str, family: Family) -> Vec<Article> {
    let text = clean(text);
    let starts: Vec<usize> = ARTICLE_AT.find_iter(&text).map(|m| m.start()).collect();
    let mut articles = Vec::new();
    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        if let Some(article) = parse_article(&text[start..end], family) {
            articles.push(article);
        }
    }
    articles
}

pub fn source_url(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Источник:")
            .map(|rest| rest.trim().to_string())
            .filter(|url| !url.is_empty())
    })
}

fn parse_article(chunk: &str, family: Family) -> Option<Article> {
    let pretty = break_markers(chunk);
    let mut lines = pretty.lines();
    let header = lines.next()?.trim();
    let captures = HEADER.captures(header)?;
    let code = captures.get(1)?.as_str().to_string();
    let rest = captures.get(2)?.as_str();
    let stars = rest.chars().filter(|ch| *ch == '★').count() as u8;
    let jurisdiction = BRACKET
        .captures(rest)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    let mut title = BRACKET.replace_all(rest, " ").to_string();
    title.retain(|ch| ch != '★');
    let title = squash(&title)
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':'))
        .trim()
        .to_string();
    if title.is_empty() && code.is_empty() {
        return None;
    }

    let body: Vec<&str> = lines.collect();
    let parts = split_parts(&body, stars);
    let full_text = pretty
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    Some(Article {
        family,
        code,
        title,
        jurisdiction,
        stars,
        parts,
        full_text,
    })
}

fn split_parts(lines: &[&str], article_stars: u8) -> Vec<Part> {
    let mut parts: Vec<PartDraft> = Vec::new();
    let mut prelude: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((number, stars, rest)) = part_header(trimmed) {
            if is_reference(&rest) {
                push_loose(&mut parts, &mut prelude, trimmed);
                continue;
            }
            parts.push(PartDraft {
                number,
                stars,
                lines: if rest.is_empty() {
                    Vec::new()
                } else {
                    vec![rest]
                },
            });
            continue;
        }
        push_loose(&mut parts, &mut prelude, trimmed);
    }

    if parts.is_empty() {
        let (composition, punishment) = consume(&prelude);
        return vec![Part {
            number: "1".to_string(),
            explicit: false,
            stars: article_stars,
            composition,
            punishment,
        }];
    }

    parts
        .into_iter()
        .map(|draft| {
            let (composition, punishment) = consume(&draft.lines);
            Part {
                number: draft.number,
                explicit: true,
                stars: draft.stars,
                composition,
                punishment,
            }
        })
        .collect()
}

struct PartDraft {
    number: String,
    stars: u8,
    lines: Vec<String>,
}

fn push_loose(parts: &mut [PartDraft], prelude: &mut Vec<String>, line: &str) {
    if let Some(part) = parts.last_mut() {
        part.lines.push(line.to_string());
    } else {
        prelude.push(line.to_string());
    }
}

fn part_header(line: &str) -> Option<(String, u8, String)> {
    let captures = PART_LINE.captures(line)?;
    let number = captures.get(1)?.as_str().to_string();
    let stars = captures
        .get(2)
        .map(|m| m.as_str().chars().count() as u8)
        .unwrap_or(0);
    let rest = captures
        .get(3)
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    Some((number, stars, rest))
}

fn is_reference(rest: &str) -> bool {
    let tail = rest
        .trim_start_matches('★')
        .trim()
        .to_lowercase()
        .replace('ё', "е");
    tail.starts_with("ст")
        || tail.starts_with("настоящ")
        || tail.starts_with("данн")
        || tail.starts_with("указан")
        || tail.starts_with("этой")
        || tail.starts_with("этого")
}

fn consume(lines: &[String]) -> (String, String) {
    let mut composition = String::new();
    let mut punishment = String::new();
    let mut mode = 0;
    for line in lines {
        let trimmed = line
            .trim()
            .trim_start_matches(|ch: char| matches!(ch, '.' | ',' | ';'))
            .trim();
        if trimmed.is_empty() {
            continue;
        }
        let folded = trimmed.to_lowercase().replace('ё', "е");
        if folded.starts_with("примечан")
            || folded.starts_with("исключен")
            || folded.starts_with("пояснен")
        {
            mode = 2;
            continue;
        }
        if folded.starts_with("наказание") {
            mode = 1;
            let rest = trimmed
                .split_once(':')
                .map(|(_, right)| right.trim())
                .unwrap_or("");
            push_sentence(&mut punishment, rest);
            continue;
        }
        if mode == 0 {
            push_sentence(&mut composition, trimmed);
        } else if mode == 1 {
            push_sentence(&mut punishment, trimmed);
        }
    }
    (squash(&composition), squash(&punishment))
}

fn push_sentence(buf: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !buf.is_empty() {
        buf.push(' ');
    }
    buf.push_str(text);
}

fn break_markers(text: &str) -> String {
    let with_parts = break_before(text, &PART_AT);
    break_before(&with_parts, &PUNISH_AT)
}

fn break_before(text: &str, pattern: &Regex) -> String {
    let mut out = String::new();
    let mut last = 0;
    for found in pattern.find_iter(text) {
        let start = found.start();
        out.push_str(&text[last..start]);
        if start > 0 && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(found.as_str());
        last = found.end();
    }
    out.push_str(&text[last..]);
    out
}

pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{2060}' | '\u{00ad}' => {}
            '\u{00a0}' | '\t' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn law(server: &str, needle: &str) -> String {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("laws")
            .join(server);
        let entry = std::fs::read_dir(&root)
            .unwrap()
            .find_map(|item| {
                let path = item.ok()?.path();
                let name = path.file_name()?.to_string_lossy().to_string();
                if name.contains(needle) {
                    Some(path)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("нет файла {needle} в {server}"));
        std::fs::read_to_string(entry).unwrap()
    }

    #[test]
    fn memphis_theft_has_two_parts() {
        let articles = parse_document(&law("memphis", "ugolovnyi-kodeks"), Family::Uk);
        let article = articles
            .iter()
            .find(|article| article.code == "10.5")
            .expect("10.5");
        assert_eq!(article.jurisdiction, "F/R");
        assert!(article.title.contains("Неправомерное завладение"));
        assert_eq!(
            article
                .parts
                .iter()
                .map(|part| part.number.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2"]
        );
        assert!(article.parts[0].explicit);
        assert!(
            article.parts[0].punishment.contains("40.000")
                || article.parts[0].punishment.contains("40")
        );
        assert_eq!(article.parts[0].stars, 4);
    }

    #[test]
    fn portland_flat_article_and_child_code() {
        let articles = parse_document(&law("portland", "ugolovnyi-kodeks"), Family::Uk);
        let parent = articles
            .iter()
            .find(|article| article.code == "10.5")
            .expect("10.5");
        assert_eq!(parent.parts.len(), 1);
        assert!(!parent.parts[0].explicit);
        assert!(parent.parts[0].punishment.to_lowercase().contains("штраф"));
        assert!(articles.iter().any(|article| article.code == "10.5.1"));
        let harm = articles
            .iter()
            .find(|article| article.code == "6.1")
            .expect("6.1");
        assert_eq!(
            harm.parts
                .iter()
                .map(|part| part.number.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2", "3"]
        );
    }
}
