use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    Portland,
    Memphis,
    Orlando,
    Denver,
    Phoenix,
    Seattle,
}

impl Server {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "portland" => Some(Self::Portland),
            "memphis" => Some(Self::Memphis),
            "orlando" => Some(Self::Orlando),
            "denver" => Some(Self::Denver),
            "phoenix" => Some(Self::Phoenix),
            "seattle" => Some(Self::Seattle),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portland => "portland",
            Self::Memphis => "memphis",
            Self::Orlando => "orlando",
            Self::Denver => "denver",
            Self::Phoenix => "phoenix",
            Self::Seattle => "seattle",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Portland => "Portland",
            Self::Memphis => "Memphis",
            Self::Orlando => "Orlando",
            Self::Denver => "Denver",
            Self::Phoenix => "Phoenix",
            Self::Seattle => "Seattle",
        }
    }

    /// Раздел «Законодательная база», если номер уже известен.
    /// У Seattle и Phoenix раздел ищется по форуму во время обновления.
    pub fn forum_node(self) -> Option<u32> {
        match self {
            Self::Portland => Some(1338),
            Self::Memphis => Some(1471),
            Self::Denver => Some(1276),
            Self::Orlando => Some(1405),
            Self::Phoenix | Self::Seattle => None,
        }
    }

    /// Раздел доп. правил сервера: от него можно подняться к категории и найти законку.
    pub fn rules_forum(self) -> Option<&'static str> {
        match self {
            Self::Seattle => Some("https://forum.majestic-rp.ru/forums/seattle.1200/"),
            Self::Phoenix => Some("https://forum.majestic-rp.ru/forums/phoenix.1262/"),
            _ => None,
        }
    }

    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Portland => &["portland", "портленд"],
            Self::Memphis => &["memphis", "мемфис"],
            Self::Orlando => &["orlando", "орландо"],
            Self::Denver => &["denver", "денвер"],
            Self::Phoenix => &["phoenix", "финикс", "феникс"],
            Self::Seattle => &["seattle", "сиэтл", "сиетл"],
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::Portland,
            Self::Memphis,
            Self::Orlando,
            Self::Denver,
            Self::Phoenix,
            Self::Seattle,
        ]
    }

    pub fn index(self) -> usize {
        Self::all()
            .iter()
            .position(|server| *server == self)
            .unwrap_or(0)
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
            Self::Koap => "АК",
            Self::Pdd => "ДК",
            Self::Upk => "ПК",
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
