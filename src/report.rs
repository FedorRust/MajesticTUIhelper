use std::sync::LazyLock;

use regex::Regex;

use crate::model::Article;

static MONEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\$?\s*(\d{1,3}(?:[ .]\d{3})+|\d+)\s*\$?").expect("money regex")
});
static YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)от\s+(\d+)\s+до\s+(\d+)\s*(?:лет|года|год)").expect("year range")
});
static YEAR_ONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|\D)(\d+)\s*(год|года|лет)\b").expect("year one"));

pub fn report_line(article: &Article, part_index: usize) -> String {
    let part = article
        .parts
        .get(part_index)
        .or_else(|| article.parts.first());
    let number = part.map(|item| item.number.as_str()).unwrap_or("1");
    let sanction = part
        .map(|item| summarize_punishment(&item.punishment))
        .unwrap_or_default();
    let jurisdiction = jur_tag(&article.jurisdiction);
    let mut pieces = vec![format!(
        "ст. {} ч. {} {} \u{2014} {}",
        article.code,
        number,
        article.family.label(),
        short_title(&article.title)
    )];
    if !sanction.is_empty() {
        pieces.push(sanction);
    }
    if !jurisdiction.is_empty() {
        pieces.push(jurisdiction);
    }
    pieces.join(" | ")
}

pub fn short_title(title: &str) -> String {
    let mut text = squash(title)
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':'))
        .trim()
        .to_string();
    const SWAPS: &[(&str, &str)] = &[
        ("автомобилем или иным транспортным средством", "ТС"),
        ("автомобиля или иного транспортного средства", "ТС"),
    ];
    for (from, to) in SWAPS {
        let folded = text.to_lowercase();
        if let Some(index) = folded.find(from) {
            text.replace_range(index..index + from.len(), to);
        }
    }
    squash(&text)
}

pub fn jur_tag(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = raw
        .split('/')
        .map(|piece| abbreviate_jur(piece.trim()))
        .filter(|piece| !piece.is_empty())
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("[{}]", parts.join("/"))
    }
}

fn abbreviate_jur(piece: &str) -> String {
    let folded = piece.to_lowercase().replace('ё', "е");
    if folded == "f" || folded.contains("федерал") {
        "F".to_string()
    } else if folded == "r" || folded.contains("регион") {
        "R".to_string()
    } else if folded == "ф" || folded.contains("финанс") {
        "Ф".to_string()
    } else {
        piece.to_string()
    }
}

pub fn summarize_punishment(text: &str) -> String {
    let text = squash(text);
    if text.is_empty() {
        return String::new();
    }
    let prison = prison_phrase(&text);
    let fine = fine_phrase(&text);
    match (prison, fine) {
        (Some(prison), Some(fine)) => format!("{prison} / {fine}"),
        (Some(prison), None) => prison,
        (None, Some(fine)) => fine,
        (None, None) => truncate(&text, 80),
    }
}

fn prison_phrase(text: &str) -> Option<String> {
    if let Some(caps) = YEAR_RANGE.captures(text) {
        let from: u32 = caps.get(1)?.as_str().parse().ok()?;
        let to: u32 = caps.get(2)?.as_str().parse().ok()?;
        return Some(format!("{from}\u{2013}{to} {}", years_word(to)));
    }
    let caps = YEAR_ONE.captures(text)?;
    let years: u32 = caps.get(1)?.as_str().parse().ok()?;
    if years > 30 {
        return None;
    }
    Some(format!("{years} {}", years_word(years)))
}

fn fine_phrase(text: &str) -> Option<String> {
    let folded = text.to_lowercase();
    let window = folded
        .find("штраф")
        .map(|index| &text[index + "штраф".len()..])
        .unwrap_or("");
    if window.is_empty() && !text.contains('$') {
        return None;
    }
    let source = if window.is_empty() { text } else { window };
    let amounts = money_amounts(source);
    if amounts.is_empty() {
        return None;
    }
    let capped = source.to_lowercase().contains("до")
        && !source.to_lowercase().contains("от")
        && amounts.len() == 1;
    let body = if amounts.len() >= 2 {
        format!(
            "{}\u{2013}{}",
            money_k(amounts[0]),
            money_k_suffix(amounts[amounts.len() - 1])
        )
    } else if capped {
        format!("до {}", money_k_suffix(amounts[0]))
    } else {
        money_k_suffix(amounts[0])
    };
    Some(format!("штраф {body}"))
}

fn money_amounts(text: &str) -> Vec<u64> {
    let mut found = Vec::new();
    for caps in MONEY.captures_iter(text) {
        let Some(raw) = caps.get(0) else { continue };
        let Some(digits) = caps.get(1) else { continue };
        let Some(value) = parse_money(digits.as_str()) else {
            continue;
        };
        let marked = raw.as_str().contains('$');
        if value >= 1000 || marked {
            found.push(value);
        }
    }
    found
}

fn parse_money(raw: &str) -> Option<u64> {
    let compact: String = raw.chars().filter(|ch| ch.is_ascii_digit()).collect();
    compact.parse().ok()
}

fn money_k(value: u64) -> String {
    if value >= 1000 && value % 1000 == 0 {
        format!("{}", value / 1000)
    } else if value >= 1000 {
        let whole = value / 1000;
        let frac = (value % 1000) / 100;
        if frac == 0 {
            format!("{whole}")
        } else {
            format!("{whole}.{frac}")
        }
    } else {
        format!("${value}")
    }
}

fn money_k_suffix(value: u64) -> String {
    if value >= 1000 {
        format!("{}к", money_k(value))
    } else {
        format!("${value}")
    }
}

fn years_word(value: u32) -> &'static str {
    let tail = value % 100;
    if (11..15).contains(&tail) {
        return "лет";
    }
    match tail % 10 {
        1 => "год",
        2..=4 => "года",
        _ => "лет",
    }
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memphis_sanction_matches_spec_shape() {
        let text = "от 3 до 4 лет лишения свободы, либо штраф от 40.000 до 80.000$";
        assert_eq!(
            summarize_punishment(text),
            "3\u{2013}4 года / штраф 40\u{2013}80к"
        );
    }

    #[test]
    fn portland_sanction_and_fine_cap() {
        assert_eq!(
            summarize_punishment(
                "3 года лишения свободы или уголовный штраф в размере от 10.000$ до 30.000$."
            ),
            "3 года / штраф 10\u{2013}30к"
        );
        assert_eq!(summarize_punishment("штраф до $5.000."), "штраф до 5к");
    }

    #[test]
    fn jurisdiction_and_title() {
        assert_eq!(jur_tag("F/R"), "[F/R]");
        assert_eq!(jur_tag("Федеральная/Региональная"), "[F/R]");
        assert_eq!(jur_tag("Финансовая"), "[Ф]");
        assert_eq!(
            short_title("Неправомерное завладение автомобилем или иным транспортным средством."),
            "Неправомерное завладение ТС"
        );
    }
}
