use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Article, Corpus, Family};

static CODE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(\d+(?:\.\d+)*)(?:\s*(?:ч\.?|часть)\s*(\d+(?:\.\d+)*))?(?:\s*(ук|коап|ак|пдд|дк|упк|пк)\.?)?$",
    )
    .expect("code query")
});
static PARTIAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(\d+(?:\.\d+)*)(?:\s*(?:ч\.?|часть)\s*(\d+(?:\.\d+)*)?)?(?:\s*([a-zа-я]+))?\.?$",
    )
    .expect("partial query")
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    Empty,
    Code {
        code: String,
        part: Option<String>,
        family: Option<Family>,
    },
    /// Номер набран, суффикс кодекса ещё не дописан (`10.5у`).
    Prefix {
        code: String,
    },
    Words {
        words: Vec<String>,
    },
}

impl Query {
    /// Код уже закончен суффиксом или номером части — карточку можно открыть сразу.
    /// Голое число без точки (`17`, `17ук`) остаётся списком всех `17`, `17.1`, …
    pub fn immediate(&self) -> bool {
        match self {
            Self::Code {
                code,
                part: None,
                family: Some(_),
            } if !code.contains('.') => false,
            Self::Code {
                family: Some(_), ..
            } => true,
            Self::Code {
                part: Some(_),
                family: None,
                ..
            } => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub article: usize,
    pub part: usize,
    pub part_found: bool,
}

pub fn parse_query(input: &str) -> Query {
    let folded = fold(input);
    let collapsed = squash(&folded);
    let stripped = strip_statya(&collapsed);
    if stripped.is_empty() {
        return Query::Empty;
    }
    if let Some(query) = parse_code(stripped) {
        return query;
    }
    if let Some(code) = parse_prefix(stripped) {
        return Query::Prefix { code };
    }
    let words: Vec<String> = stripped
        .split_whitespace()
        .filter(|word| *word != "ст" && *word != "статья")
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        Query::Empty
    } else {
        Query::Words { words }
    }
}

pub fn lookup(corpus: &Corpus, input: &str) -> Vec<Hit> {
    match parse_query(input) {
        Query::Empty => Vec::new(),
        Query::Code { code, part, family } => code_hits(corpus, &code, part.as_deref(), family),
        Query::Prefix { code } => prefix_hits(corpus, &code),
        Query::Words { words } => word_hits(corpus, &words),
    }
}

fn parse_code(input: &str) -> Option<Query> {
    let captures = CODE_RE.captures(input)?;
    let code = captures.get(1)?.as_str().to_string();
    let part = captures.get(2).map(|item| item.as_str().to_string());
    let family = captures
        .get(3)
        .and_then(|item| family_suffix(item.as_str()));
    Some(Query::Code { code, part, family })
}

fn parse_prefix(input: &str) -> Option<String> {
    let captures = PARTIAL_RE.captures(input)?;
    let tail = captures.get(3).map(|item| item.as_str()).unwrap_or("");
    if tail.is_empty() || is_partial_suffix(tail) {
        Some(captures.get(1)?.as_str().to_string())
    } else {
        None
    }
}

fn family_suffix(suffix: &str) -> Option<Family> {
    match suffix {
        "ук" => Some(Family::Uk),
        "коап" | "ак" => Some(Family::Koap),
        "пдд" | "дк" => Some(Family::Pdd),
        "упк" | "пк" => Some(Family::Upk),
        _ => None,
    }
}

fn is_partial_suffix(tail: &str) -> bool {
    const FULL: &[&str] = &["ук", "коап", "ак", "пдд", "дк", "упк", "пк"];
    FULL.iter()
        .any(|name| *name != tail && name.starts_with(tail))
}

fn code_hits(corpus: &Corpus, code: &str, part: Option<&str>, family: Option<Family>) -> Vec<Hit> {
    if part.is_none() && !code.contains('.') {
        return number_prefix_hits(corpus, code, family);
    }
    if let Some(part_number) = part {
        let exact = articles_with_code(corpus, code, family);
        if !exact.is_empty() {
            return exact
                .into_iter()
                .map(|index| hit_for_part(&corpus.articles[index], index, part_number))
                .collect();
        }
    }

    let exact = articles_with_code(corpus, code, family);
    if !exact.is_empty() {
        return exact
            .into_iter()
            .map(|index| Hit {
                part: corpus.articles[index].default_part(),
                article: index,
                part_found: true,
            })
            .collect();
    }

    // `10.5.1` — часть 1 статьи 10.5, если отдельной статьи 10.5.1 нет,
    // а у родителя есть явный маркер `ч. 1`. На Portland 10.5.1 — своя статья
    // и сюда не попадает.
    if part.is_none() {
        if let Some((parent, last)) = split_parent(code) {
            let parents = articles_with_code(corpus, &parent, family);
            let peeled: Vec<Hit> = parents
                .into_iter()
                .filter_map(|index| {
                    let article = &corpus.articles[index];
                    let part_index = article
                        .parts
                        .iter()
                        .position(|item| item.explicit && item.number == last)?;
                    Some(Hit {
                        article: index,
                        part: part_index,
                        part_found: true,
                    })
                })
                .collect();
            if !peeled.is_empty() {
                return peeled;
            }
        }
    }
    Vec::new()
}

fn hit_for_part(article: &Article, index: usize, number: &str) -> Hit {
    if let Some(part) = article.part_index(number) {
        Hit {
            article: index,
            part,
            part_found: true,
        }
    } else {
        Hit {
            article: index,
            part: article.default_part(),
            part_found: false,
        }
    }
}

fn articles_with_code(corpus: &Corpus, code: &str, family: Option<Family>) -> Vec<usize> {
    let mut hits: Vec<usize> = corpus
        .articles
        .iter()
        .enumerate()
        .filter(|(_, article)| {
            article.code == code && family.is_none_or(|item| article.family == item)
        })
        .map(|(index, _)| index)
        .collect();
    hits.sort_by(|&left, &right| article_order(&corpus.articles[left], &corpus.articles[right]));
    hits
}

fn number_prefix_hits(corpus: &Corpus, code: &str, family: Option<Family>) -> Vec<Hit> {
    let mut hits: Vec<Hit> = corpus
        .articles
        .iter()
        .enumerate()
        .filter(|(_, article)| {
            let same_number = article.code == code || article.code.starts_with(&format!("{code}."));
            same_number && family.is_none_or(|item| article.family == item)
        })
        .map(|(index, article)| Hit {
            article: index,
            part: article.default_part(),
            part_found: true,
        })
        .collect();
    hits.sort_by(|left, right| {
        article_order(
            &corpus.articles[left.article],
            &corpus.articles[right.article],
        )
    });
    hits
}

fn prefix_hits(corpus: &Corpus, code: &str) -> Vec<Hit> {
    let mut hits: Vec<Hit> = corpus
        .articles
        .iter()
        .enumerate()
        .filter(|(_, article)| {
            article.code == code || article.code.starts_with(&format!("{code}."))
        })
        .map(|(index, article)| Hit {
            article: index,
            part: article.default_part(),
            part_found: true,
        })
        .collect();
    hits.sort_by(|left, right| {
        article_order(
            &corpus.articles[left.article],
            &corpus.articles[right.article],
        )
    });
    hits
}

fn word_hits(corpus: &Corpus, words: &[String]) -> Vec<Hit> {
    let mut hits: Vec<(bool, usize)> = corpus
        .articles
        .iter()
        .enumerate()
        .filter_map(|(index, article)| {
            let title = fold(&article.title);
            let hay = fold(&format!("{} {}", article.title, article.full_text));
            if words.iter().all(|word| word_in(&hay, word)) {
                let title_hit = words.iter().all(|word| word_in(&title, word));
                Some((title_hit, index))
            } else {
                None
            }
        })
        .collect();
    hits.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| article_order(&corpus.articles[left.1], &corpus.articles[right.1]))
    });
    hits.into_iter()
        .map(|(_, index)| Hit {
            part: corpus.articles[index].default_part(),
            article: index,
            part_found: true,
        })
        .collect()
}

fn article_order(left: &Article, right: &Article) -> std::cmp::Ordering {
    left.family
        .cmp(&right.family)
        .then_with(|| code_key(&left.code).cmp(&code_key(&right.code)))
}

fn code_key(code: &str) -> Vec<u32> {
    code.split('.')
        .map(|piece| piece.parse().unwrap_or(0))
        .collect()
}

fn split_parent(code: &str) -> Option<(String, String)> {
    let (parent, last) = code.rsplit_once('.')?;
    if parent.is_empty() || last.is_empty() {
        None
    } else {
        Some((parent.to_string(), last.to_string()))
    }
}

fn strip_statya(input: &str) -> &str {
    let input = input.trim();
    if let Some(rest) = input.strip_prefix("статья") {
        let rest = rest.trim_start_matches(['.', ' ']);
        if rest.starts_with(|ch: char| ch.is_ascii_digit()) {
            return rest;
        }
    }
    if let Some(rest) = input.strip_prefix("ст") {
        let rest = rest.trim_start_matches(['.', ' ']);
        if rest.starts_with(|ch: char| ch.is_ascii_digit()) {
            return rest;
        }
    }
    input
}

/// В тексте Portland УК угона нет: там «неправомерное завладение».
/// Слово из спеки ищем и так. Жаргон вроде «шатырка» сюда не входит.
fn word_in(hay: &str, word: &str) -> bool {
    stems(word).iter().any(|stem| hay.contains(stem))
}

fn stems(word: &str) -> Vec<&str> {
    match word {
        "угон" | "угона" | "угоне" | "угону" | "угоном" => {
            vec![word, "завладен"]
        }
        _ => vec![word],
    }
}

fn fold(text: &str) -> String {
    text.to_lowercase().replace('ё', "е")
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{laws_root, load_corpus};
    use crate::model::Server;
    use crate::report::report_line;

    fn corpus(server: Server) -> Corpus {
        let root = laws_root().expect("laws");
        load_corpus(&root, server).expect("corpus")
    }

    #[test]
    fn normalizes_code_forms() {
        for sample in ["10.5ук", "10.5 УК", "ст. 10.5 ук", "СТ. 10.5 УК"] {
            match parse_query(sample) {
                Query::Code { code, part, family } => {
                    assert_eq!(code, "10.5", "{sample}");
                    assert!(part.is_none(), "{sample}");
                    assert_eq!(family, Some(Family::Uk), "{sample}");
                }
                other => panic!("{sample} -> {other:?}"),
            }
        }
        match parse_query("10.5 ч.2") {
            Query::Code { code, part, family } => {
                assert_eq!(code, "10.5");
                assert_eq!(part.as_deref(), Some("2"));
                assert_eq!(family, None);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_query("угон"), Query::Words { .. }));
        assert!(!parse_query("угон").immediate());
        assert!(parse_query("10.5ук").immediate());
        assert!(parse_query("10.5 ч.2").immediate());
        assert!(!parse_query("10.5").immediate());
        assert!(!parse_query("17").immediate());
        assert!(!parse_query("17ук").immediate());
    }

    #[test]
    fn bare_number_lists_every_dotted_variant() {
        let corpus = corpus(Server::Portland);
        let hits = lookup(&corpus, "17");
        let codes: Vec<_> = hits
            .iter()
            .map(|hit| corpus.articles[hit.article].code.as_str())
            .collect();
        assert!(codes.contains(&"17.1"), "{codes:?}");
        assert!(codes.contains(&"17.2"), "{codes:?}");
        assert!(codes.len() > 2, "{codes:?}");
        let uk_only = lookup(&corpus, "17ук");
        assert!(uk_only
            .iter()
            .all(|hit| corpus.articles[hit.article].family == Family::Uk));
        assert!(uk_only
            .iter()
            .any(|hit| corpus.articles[hit.article].code == "17.1"));
        assert_eq!(Family::Pdd.label(), "ДК");
        assert_eq!(Family::Koap.label(), "АК");
        assert_eq!(Family::Upk.label(), "ПК");
        assert!(matches!(
            parse_query("10ак"),
            Query::Code {
                family: Some(Family::Koap),
                ..
            }
        ));
        assert!(matches!(
            parse_query("10пк"),
            Query::Code {
                family: Some(Family::Upk),
                ..
            }
        ));
    }

    #[test]
    fn memphis_part_code_matches_spec_line() {
        let corpus = corpus(Server::Memphis);
        let hits = lookup(&corpus, "10.5.1ук");
        assert_eq!(
            hits.len(),
            1,
            "хиты: {:?}",
            hits.iter()
                .map(|hit| &corpus.articles[hit.article].code)
                .collect::<Vec<_>>()
        );
        let article = &corpus.articles[hits[0].article];
        assert_eq!(article.code, "10.5");
        assert_eq!(article.parts[hits[0].part].number, "1");
        assert_eq!(
            report_line(article, hits[0].part),
            "ст. 10.5 ч. 1 УК \u{2014} Неправомерное завладение ТС | 3\u{2013}4 года / штраф 40\u{2013}80к | [F/R]",
            "title {:?} jur {:?} pun {:?}",
            article.title,
            article.jurisdiction,
            article.parts[hits[0].part].punishment
        );
        let second = lookup(&corpus, "10.5 ч.2");
        assert_eq!(second.len(), 1);
        assert_eq!(
            corpus.articles[second[0].article].parts[second[0].part].number,
            "2"
        );
    }

    #[test]
    fn portland_child_article_stays_its_own_code() {
        let corpus = corpus(Server::Portland);
        let hits = lookup(&corpus, "10.5.1ук");
        assert_eq!(hits.len(), 1);
        assert_eq!(corpus.articles[hits[0].article].code, "10.5.1");
        let parent = lookup(&corpus, "10.5ук");
        assert_eq!(parent.len(), 1);
        assert_eq!(corpus.articles[parent[0].article].code, "10.5");
    }

    #[test]
    fn portland_ugon_lists_theft_articles() {
        let corpus = corpus(Server::Portland);
        let hits = lookup(&corpus, "угон");
        let codes: Vec<_> = hits
            .iter()
            .map(|hit| corpus.articles[hit.article].code.as_str())
            .collect();
        assert!(codes.contains(&"10.5"), "{codes:?}");
        assert!(codes.len() >= 2, "{codes:?}");
        assert!(!parse_query("угон").immediate());
    }

    #[test]
    fn word_search_returns_every_hit() {
        let corpus = corpus(Server::Memphis);
        let hits = lookup(&corpus, "угон");
        assert!(hits.len() >= 2, "нашлось {}", hits.len());
        let codes: Vec<_> = hits
            .iter()
            .map(|hit| corpus.articles[hit.article].code.as_str())
            .collect();
        assert!(
            codes.iter().any(|code| *code != codes[0])
                || hits
                    .iter()
                    .map(|hit| corpus.articles[hit.article].family)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    > 1
        );
    }
}
