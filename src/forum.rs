use std::path::Path;
use std::time::Duration;

use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{COOKIE, REFERER, SET_COOKIE, USER_AGENT};
use scraper::{Html, Selector};

use crate::config;
use crate::index::family_from_title;
use crate::model::{CodeFile, Family, Server};
use crate::parse::clean;

const BASE: &str = "https://forum.majestic-rp.ru";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

pub struct UpdateReport {
    pub updated: Vec<Family>,
    pub problems: Vec<String>,
}

pub enum UpdateError {
    NeedLogin(String),
    Failed(String),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedLogin(text) | Self::Failed(text) => formatter.write_str(text),
        }
    }
}

/// Логин XenForo. Пароль остаётся у вызывающего и на диск не пишется.
pub fn login(user: &str, password: &str) -> Result<(), UpdateError> {
    let client = client()?;
    let mut session = Session::default();
    let page = session
        .get(&client, &format!("{BASE}/login/"))
        .map_err(UpdateError::Failed)?;
    if is_challenge(&page) {
        return Err(UpdateError::Failed(
            "форум закрыл запрос антиботом. Старая законка на месте.".into(),
        ));
    }
    let mut fields = hidden_fields(&page);
    fields.retain(|(name, _)| name != "login" && name != "password" && name != "remember");
    fields.push(("login".into(), user.to_string()));
    fields.push(("password".into(), password.to_string()));
    fields.push(("remember".into(), "1".into()));
    if !fields.iter().any(|(name, _)| name == "_xfToken") {
        return Err(UpdateError::Failed(
            "на странице входа нет токена XenForo. Пароль не сохранён.".into(),
        ));
    }
    let action = form_action(&page).unwrap_or_else(|| format!("{BASE}/login/login"));
    let response = session
        .post_form(&client, &action, &fields)
        .map_err(|err| UpdateError::Failed(scrub_password(&err, password)))?;
    let response = scrub_password(&response, password);
    let logged_in = session.cookies.iter().any(|(name, _)| name == "xf_user");
    if !logged_in {
        if response.to_lowercase().contains("two-step")
            || response.to_lowercase().contains("двухфактор")
        {
            return Err(UpdateError::Failed(
                "форум просит второй шаг входа. Куку не сохранил, законка на месте.".into(),
            ));
        }
        let message =
            error_block(&response).unwrap_or_else(|| "форум не пустил. Пароль не сохранён.".into());
        return Err(UpdateError::Failed(scrub_password(&message, password)));
    }
    config::save_session(&session.cookies)
        .map_err(|err| UpdateError::Failed(format!("не записал сессию: {err}")))?;
    Ok(())
}

pub fn update(
    root: &Path,
    server: Server,
    existing: &[CodeFile],
) -> Result<UpdateReport, UpdateError> {
    if !config::session_ready() {
        return Err(UpdateError::NeedLogin(
            "Нужен вход на форум. Пароль на диск не пишется.".into(),
        ));
    }
    let client = client()?;
    let mut session = Session {
        cookies: config::load_session(),
    };
    let index_url = format!(
        "{BASE}/forums/zakonodatel-naya-baza.{}/",
        server.forum_node()
    );
    let index_html = session
        .get(&client, &index_url)
        .map_err(UpdateError::Failed)?;
    if is_challenge(&index_html) {
        return Err(UpdateError::Failed(
            "форум закрыл запрос антиботом. Старая законка на месте.".into(),
        ));
    }
    if is_login_wall(&index_html) {
        return Err(UpdateError::NeedLogin(
            "Сессия форума умерла. Старая законка на месте.".into(),
        ));
    }

    let discovered = discover_threads(&index_html);
    let mut report = UpdateReport {
        updated: Vec::new(),
        problems: Vec::new(),
    };
    for family in Family::all() {
        let url = discovered
            .iter()
            .find(|(item, _)| *item == family)
            .map(|(_, url)| url.clone())
            .or_else(|| {
                existing
                    .iter()
                    .find(|file| file.family == family)
                    .and_then(|file| file.url.clone())
            })
            .or_else(|| fallback_url(server, family).map(str::to_string));
        let Some(url) = url else {
            report
                .problems
                .push(format!("{}: нет ссылки на тему", family.label()));
            continue;
        };
        match fetch_code(&client, &mut session, &url) {
            Ok(body) => {
                let path = existing
                    .iter()
                    .find(|file| file.family == family)
                    .map(|file| file.path.clone())
                    .unwrap_or_else(|| {
                        root.join(server.as_str())
                            .join(format!("{}.md", family_slug(family)))
                    });
                let title = family.label();
                let markdown = format!(
                    "# {title}\n\nИсточник: {url}\nДлина: {} символов\n\n{body}\n",
                    body.chars().count()
                );
                if let Some(parent) = path.parent() {
                    if let Err(err) = std::fs::create_dir_all(parent) {
                        report.problems.push(format!("{}: {err}", family.label()));
                        continue;
                    }
                }
                let tmp = path.with_extension("md.part");
                if std::fs::write(&tmp, &markdown)
                    .and_then(|_| std::fs::rename(&tmp, &path))
                    .is_err()
                {
                    let _ = std::fs::remove_file(&tmp);
                    report.problems.push(format!(
                        "{}: не записал файл, старый текст на месте",
                        family.label()
                    ));
                    continue;
                }
                report.updated.push(family);
            }
            Err(err) => report.problems.push(format!("{}: {err}", family.label())),
        }
    }
    if report.updated.is_empty()
        && report
            .problems
            .iter()
            .any(|item| item.contains("Сессия") || item.contains("вход"))
    {
        return Err(UpdateError::NeedLogin(
            "Сессия форума умерла. Старая законка на месте.".into(),
        ));
    }
    Ok(report)
}

fn fetch_code(client: &Client, session: &mut Session, url: &str) -> Result<String, String> {
    let html = session.get(client, url)?;
    if is_login_wall(&html) {
        return Err("сессия умерла, файл не трогал".into());
    }
    if is_challenge(&html) {
        return Err("антибот, файл не трогал".into());
    }
    let text = extract_post(&html).ok_or("в теме нет текста статьи, файл не трогал")?;
    let count = text.matches("Статья").count();
    if count < 5 {
        return Err(format!("в ответе мало статей ({count}), файл не трогал"));
    }
    Ok(text)
}

fn fallback_url(server: Server, family: Family) -> Option<&'static str> {
    Some(match (server, family) {
        (Server::Portland, Family::Uk) => "https://forum.majestic-rp.ru/threads/ugolovnyi-kodeks-shtata-san-andreas.3263639/",
        (Server::Portland, Family::Koap) => {
            "https://forum.majestic-rp.ru/threads/administrativnyi-kodeks-shtata-san-andreas.3263619/"
        }
        (Server::Portland, Family::Pdd) => "https://forum.majestic-rp.ru/threads/dorozhnyi-kodeks-shtata-san-andreas.3569154/",
        (Server::Portland, Family::Upk) => "https://forum.majestic-rp.ru/threads/protsessual-nyi-kodeks-shtata-san-andreas.3263593/",
        (Server::Memphis, Family::Uk) => "https://forum.majestic-rp.ru/threads/ugolovnyi-kodeks-shtata-san-andreas.3607892/",
        (Server::Memphis, Family::Koap) => {
            "https://forum.majestic-rp.ru/threads/kodeks-ob-administrativnykh-pravonarusheniyakh-shtata-san-andreas.3505180/"
        }
        (Server::Memphis, Family::Pdd) => "https://forum.majestic-rp.ru/threads/dorozhnyi-kodeks-shtata-san-andreas.3505137/",
        (Server::Memphis, Family::Upk) => "https://forum.majestic-rp.ru/threads/protsessual-nyi-kodeks-shtata-san-andreas.3607902/",
    })
}

fn family_slug(family: Family) -> &'static str {
    match family {
        Family::Uk => "ugolovnyi-kodeks",
        Family::Koap => "administrativnyi-kodeks",
        Family::Pdd => "dorozhnyi-kodeks",
        Family::Upk => "protsessualnyi-kodeks",
    }
}

#[derive(Default)]
struct Session {
    cookies: Vec<(String, String)>,
}

impl Session {
    fn header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn absorb(&mut self, response: &reqwest::blocking::Response) {
        for value in response.headers().get_all(SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some(pair) = text.split(';').next() else {
                continue;
            };
            let Some((name, cookie_value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let expired = text.to_lowercase().contains("max-age=0");
            self.cookies.retain(|(existing, _)| existing != name);
            if !expired {
                self.cookies
                    .push((name.to_string(), cookie_value.trim().to_string()));
            }
        }
    }

    fn get(&mut self, client: &Client, url: &str) -> Result<String, String> {
        let mut request = client.get(url).header(USER_AGENT, UA).header(REFERER, BASE);
        let cookie = self.header();
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie);
        }
        let response = request.send().map_err(|err| err.to_string())?;
        self.absorb(&response);
        response.text().map_err(|err| err.to_string())
    }

    fn post_form(
        &mut self,
        client: &Client,
        url: &str,
        fields: &[(String, String)],
    ) -> Result<String, String> {
        let pairs: Vec<(&str, &str)> = fields
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let mut request = client
            .post(url)
            .header(USER_AGENT, UA)
            .header(REFERER, format!("{BASE}/login/"))
            .form(&pairs);
        let cookie = self.header();
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie);
        }
        let response = request.send().map_err(|err| err.to_string())?;
        self.absorb(&response);
        response.text().map_err(|err| err.to_string())
    }
}

fn client() -> Result<Client, UpdateError> {
    Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|err| UpdateError::Failed(err.to_string()))
}

pub fn discover_threads(html: &str) -> Vec<(Family, String)> {
    let document = Html::parse_document(html);
    let Ok(selector) = Selector::parse("a[href*='/threads/']") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for link in document.select(&selector) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let title = link.text().collect::<String>();
        let Some(family) = family_from_title(&title) else {
            continue;
        };
        if found.iter().any(|(item, _)| *item == family) {
            continue;
        }
        found.push((family, absolute(href)));
    }
    found
}

fn absolute(href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with('/') {
        format!("{BASE}{href}")
    } else {
        format!("{BASE}/{href}")
    }
}

pub fn extract_post(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("div.bbWrapper, article.message-body").ok()?;
    let mut best = String::new();
    for node in document.select(&selector) {
        let text = html_to_text(&node.inner_html());
        if text.len() > best.len() {
            best = text;
        }
    }
    if best.trim().is_empty() {
        None
    } else {
        Some(best)
    }
}

pub fn html_to_text(html: &str) -> String {
    let broken = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</p>", "\n")
        .replace("</div>", "\n")
        .replace("</h1>", "\n")
        .replace("</h2>", "\n")
        .replace("</h3>", "\n")
        .replace("</li>", "\n");
    let document = Html::parse_fragment(&broken);
    let text = document.root_element().text().collect::<String>();
    clean(&text)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .lines()
        .fold(String::new(), |mut acc, line| {
            if line.is_empty() && acc.ends_with("\n\n") {
                return acc;
            }
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(line);
            acc
        })
}

fn hidden_fields(html: &str) -> Vec<(String, String)> {
    let Ok(pattern) = Regex::new(r#"(?is)<input\b[^>]*>"#) else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    for tag in pattern.find_iter(html) {
        let tag = tag.as_str();
        let folded = tag.to_lowercase();
        if !folded.contains("hidden") {
            continue;
        }
        let Some(name) = attr(tag, "name") else {
            continue;
        };
        let value = attr(tag, "value").unwrap_or_default();
        fields.push((name, unescape(&value)));
    }
    fields
}

fn form_action(html: &str) -> Option<String> {
    let pattern = Regex::new(r#"(?is)<form\b[^>]*>"#).ok()?;
    let tag = pattern.find(html)?.as_str();
    let action = attr(tag, "action")?;
    Some(absolute(&unescape(&action)))
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r#"(?i)\b{name}\s*=\s*(?:"([^"]*)"|'([^']*)')"#)).ok()?;
    let caps = pattern.captures(tag)?;
    Some(caps.get(1).or_else(|| caps.get(2))?.as_str().to_string())
}

fn unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn error_block(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("div.blockMessage--error, div.blockMessage").ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(truncate(&text, 240))
    }
}

fn is_login_wall(html: &str) -> bool {
    let folded = html.to_lowercase();
    let login = folded.contains("name=\"password\"") || folded.contains("name='password'");
    login && !folded.contains("/threads/")
}

fn is_challenge(html: &str) -> bool {
    let folded = html.to_lowercase();
    folded.contains("cf-browser-verification")
        || folded.contains("just a moment")
        || folded.contains("attention required")
}

fn scrub_password(text: &str, password: &str) -> String {
    if password.is_empty() {
        truncate(text, 240)
    } else {
        truncate(&text.replace(password, "••••"), 240)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_string()
    } else {
        let head: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_token_and_post_text() {
        let html = r#"
            <form action="/login/login" method="post">
                <input type="hidden" name="_xfToken" value="abc,123" />
                <input type="text" name="login" />
                <input type="password" name="password" />
            </form>
        "#;
        let fields = hidden_fields(html);
        assert_eq!(
            fields,
            vec![("_xfToken".to_string(), "abc,123".to_string())]
        );
        assert_eq!(
            form_action(html).as_deref(),
            Some("https://forum.majestic-rp.ru/login/login")
        );

        let thread = r#"
            <html><body>
            <a href="/threads/ugolovnyi-kodeks.1/">Уголовный кодекс штата</a>
            <a href="/threads/dorozhnyi.2/">Дорожный кодекс</a>
            <a href="/threads/prilozhenie.3/">Приложение к уголовному кодексу</a>
            <div class="bbWrapper"><p>Статья 1. Первый</p><p>Статья 2. Второй</p></div>
            </body></html>
        "#;
        let found = discover_threads(thread);
        assert!(found.iter().any(|(family, _)| *family == Family::Uk));
        assert!(found.iter().any(|(family, _)| *family == Family::Pdd));
        assert_eq!(found.len(), 2);
        let text = extract_post(thread).unwrap();
        assert!(text.contains("Статья 1"));
        assert!(text.contains("Статья 2"));
    }
}
