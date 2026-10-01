use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    Portland,
    Memphis,
}

impl Server {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "portland" => Some(Self::Portland),
            "memphis" => Some(Self::Memphis),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portland => "portland",
            Self::Memphis => "memphis",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Portland => "Portland",
            Self::Memphis => "Memphis",
        }
    }

    pub fn forum_node(self) -> u32 {
        match self {
            Self::Portland => 1338,
            Self::Memphis => 1471,
        }
    }
}

/// Четыре кодекса, которые индексирует v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    Uk,
    Koap,
    Pdd,
    Upk,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Self::Uk => "УК",
            Self::Koap => "КоАП",
            Self::Pdd => "ДК",
            Self::Upk => "УПК",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Uk, Self::Koap, Self::Pdd, Self::Upk]
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub number: String,
    /// Маркер `ч. N` был в тексте, а не подставлен для плоской статьи.
    pub explicit: bool,
    pub stars: u8,
    pub composition: String,
    pub punishment: String,
}

#[derive(Clone, Debug)]
pub struct Article {
    pub family: Family,
    pub code: String,
    pub title: String,
    /// Содержимое скобок без самих скобок: `F/R`, `Федеральная/Региональная`.
    pub jurisdiction: String,
    pub stars: u8,
    pub parts: Vec<Part>,
    pub full_text: String,
}

impl Article {
    pub fn part_index(&self, number: &str) -> Option<usize> {
        self.parts.iter().position(|part| part.number == number)
    }

    pub fn default_part(&self) -> usize {
        self.part_index("1").unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub struct CodeFile {
    pub family: Family,
    pub path: PathBuf,
    pub url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Corpus {
    pub server: Server,
    pub articles: Vec<Article>,
    pub files: Vec<CodeFile>,
}

impl Corpus {
    pub fn counts(&self) -> String {
        Family::all()
            .into_iter()
            .map(|family| {
                let count = self
                    .articles
                    .iter()
                    .filter(|article| article.family == family)
                    .count();
                format!("{} {count}", family.label())
            })
            .collect::<Vec<_>>()
            .join("  ")
    }
}
