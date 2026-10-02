use std::path::Path;
use std::time::Duration;

use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit};
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{COOKIE, LOCATION, REFERER, SET_COOKIE, USER_AGENT};
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
    let Some(form) = password_form(&page) else {
        return Err(UpdateError::Failed(
            "на странице входа нет формы. Пароль не сохранён.".into(),
        ));
    };
    let mut fields = hidden_fields(&form);
    fields.retain(|(name, _)| name != "login" && name != "password" && name != "remember");
    fields.push(("login".into(), user.to_string()));
    fields.push(("password".into(), password.to_string()));
    fields.push(("remember".into(), "1".into()));
    if !fields.iter().any(|(name, _)| name == "_xfToken") {
        return Err(UpdateError::Failed(
            "на странице входа нет токена XenForo. Пароль не сохранён.".into(),
        ));
    }
    let action = form_action(&form).unwrap_or_else(|| format!("{BASE}/login/login"));
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
    let node = match server.forum_node() {
        Some(node) => node,
        None => discover_law_node(&client, &mut session, server)?,
    };
    let index_url = format!("{BASE}/forums/zakonodatel-naya-baza.{node}/");
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
    let url = match (server, family) {
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
        _ => return None,
    };
    Some(url)
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

    fn store_cookie(&mut self, name: String, value: String) {
        self.cookies.retain(|(existing, _)| existing != &name);
        self.cookies.push((name, value));
    }

    fn roundtrip(
        &mut self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<Hop, String> {
        let cookie = self.header();
        let request = if cookie.is_empty() {
            request
        } else {
            request.header(COOKIE, cookie)
        };
        let response = request.send().map_err(|err| err.to_string())?;
        self.absorb(&response);
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(absolute)
                .ok_or_else(|| "форум прислал переход без адреса".to_string())?;
            let _ = response.bytes();
            return Ok(Hop::Next(location));
        }
        response.text().map(|text| Hop::Page(text)).map_err(|err| err.to_string())
    }

    fn get(&mut self, client: &Client, url: &str) -> Result<String, String> {
        let text = self.get_following(client, url)?;
        if let Some((name, value)) = antibot_cookie(&text) {
            self.store_cookie(name, value);
            let again = self.get_following(client, url)?;
            if antibot_cookie(&again).is_some() {
                return Err("форум закрыл запрос антиботом. Старая законка на месте.".into());
            }
            return Ok(again);
        }
        Ok(text)
    }

    fn get_following(&mut self, client: &Client, url: &str) -> Result<String, String> {
        let mut url = url.to_string();
        for _ in 0..6 {
            let request = client.get(&url).header(USER_AGENT, UA).header(REFERER, BASE);
            match self.roundtrip(request)? {
                Hop::Page(text) => return Ok(text),
                Hop::Next(next) => url = next,
            }
        }
        Err("форум зациклил переход".into())
    }

    fn post_form(
        &mut self,
        client: &Client,
        url: &str,
        fields: &[(String, String)],
    ) -> Result<String, String> {
        let text = self.post_following(client, url, fields)?;
        if let Some((name, value)) = antibot_cookie(&text) {
            self.store_cookie(name, value);
            let again = self.post_following(client, url, fields)?;
            if antibot_cookie(&again).is_some() {
                return Err("форум закрыл запрос антиботом. Старая законка на месте.".into());
            }
            return Ok(again);
        }
        Ok(text)
    }

    fn post_following(
        &mut self,
        client: &Client,
        url: &str,
        fields: &[(String, String)],
    ) -> Result<String, String> {
        let mut url = url.to_string();
        let mut posting = true;
        for _ in 0..6 {
            let request = if posting {
                posting = false;
                let pairs: Vec<(&str, &str)> = fields
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect();
                client
                    .post(&url)
                    .header(USER_AGENT, UA)
                    .header(REFERER, format!("{BASE}/login/"))
                    .form(&pairs)
            } else {
                client.get(&url).header(USER_AGENT, UA).header(REFERER, BASE)
            };
            match self.roundtrip(request)? {
                Hop::Page(text) => return Ok(text),
                Hop::Next(next) => url = next,
            }
        }
        Err("форум зациклил переход".into())
    }
}

enum Hop {
    Page(String),
    Next(String),
}

fn client() -> Result<Client, UpdateError> {
    Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::custom(|attempt| attempt.stop()))
        .build()
        .map_err(|err| UpdateError::Failed(err.to_string()))
}

fn discover_law_node(
    client: &Client,
    session: &mut Session,
    server: Server,
) -> Result<u32, UpdateError> {
    let home = session
        .get(client, &format!("{BASE}/"))
        .map_err(UpdateError::Failed)?;
    if let Some(node) = law_node_on_page(&home, server) {
        return Ok(node);
    }
    let mut seen = Vec::new();
    for href in server_forum_links(&home, server) {
        if seen.len() >= 6 || seen.iter().any(|item: &String| item == &href) {
            continue;
        }
        seen.push(href.clone());
        let page = session.get(client, &href).map_err(UpdateError::Failed)?;
        if let Some(node) = law_node_on_page(&page, server) {
            return Ok(node);
        }
    }
    if let Some(entry) = server.rules_forum() {
        let page = session.get(client, entry).map_err(UpdateError::Failed)?;
        if let Some(node) = law_node_on_page(&page, server) {
            return Ok(node);
        }
        for href in breadcrumb_forums(&page) {
            if seen.iter().any(|item| item == &href) {
                continue;
            }
            let parent = session.get(client, &href).map_err(UpdateError::Failed)?;
            if let Some(node) = law_node_on_page(&parent, server) {
                return Ok(node);
            }
        }
    }
    Err(UpdateError::Failed(format!(
        "не нашёл законодательную базу {} на форуме",
        server.title()
    )))
}

fn law_node_on_page(html: &str, server: Server) -> Option<u32> {
    let document = Html::parse_document(html);
    let block_sel = Selector::parse("div.block").ok()?;
    let header_sel = Selector::parse(".block-header").ok()?;
    let link_sel = Selector::parse("a[href*='/forums/']").ok()?;
    for block in document.select(&block_sel) {
        let header = block
            .select(&header_sel)
            .next()
            .map(|item| item.text().collect::<String>())
            .unwrap_or_default();
        if !mentions_server(&header, server) {
            continue;
        }
        for link in block.select(&link_sel) {
            let Some(href) = link.value().attr("href") else {
                continue;
            };
            let text = link.text().collect::<String>();
            if let Some(node) = law_node_from_anchor(href, &text) {
                return Some(node);
            }
        }
    }
    if !mentions_server(&document_heading(&document), server) {
        return None;
    }
    let mut found = Vec::new();
    for link in document.select(&link_sel) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Some(node) = law_node_from_anchor(href, &link.text().collect::<String>()) else {
            continue;
        };
        if !found.contains(&node) {
            found.push(node);
        }
    }
    if found.len() == 1 {
        Some(found[0])
    } else {
        None
    }
}

fn document_heading(document: &Html) -> String {
    let title = Selector::parse("title, .p-breadcrumbs").ok();
    let Some(title) = title else {
        return String::new();
    };
    document
        .select(&title)
        .map(|item| item.text().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

fn server_forum_links(html: &str, server: Server) -> Vec<String> {
    let document = Html::parse_document(html);
    let Ok(link_sel) = Selector::parse("a[href*='/forums/']") else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for link in document.select(&link_sel) {
        let text = link.text().collect::<String>();
        if !mentions_server(&text, server) {
            continue;
        }
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let href = absolute(href);
        if !links.contains(&href) {
            links.push(href);
        }
    }
    links
}

fn breadcrumb_forums(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let Ok(link_sel) = Selector::parse(".p-breadcrumbs a[href*='/forums/']") else {
        return Vec::new();
    };
    document
        .select(&link_sel)
        .filter_map(|link| link.value().attr("href").map(absolute))
        .collect()
}

fn law_node_from_anchor(href: &str, text: &str) -> Option<u32> {
    let folded = fold(text);
    let href_folded = href.to_lowercase();
    if !folded.contains("законодат") && !href_folded.contains("zakonodatel") {
        return None;
    }
    node_id(href)
}

fn node_id(href: &str) -> Option<u32> {
    let pattern = Regex::new(r"\.(\d+)/?$").ok()?;
    let trimmed = href.trim_end_matches('/');
    pattern
        .captures(trimmed)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
}

fn mentions_server(text: &str, server: Server) -> bool {
    let folded = fold(text);
    server.aliases().iter().any(|alias| folded.contains(alias))
}

fn fold(text: &str) -> String {
    text.to_lowercase().replace('ё', "е")
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

fn password_form(html: &str) -> Option<String> {
    let pattern = Regex::new(r#"(?is)<form\b[^>]*>.*?</form>"#).ok()?;
    for form in pattern.find_iter(html) {
        let form = form.as_str();
        let folded = form.to_lowercase();
        if folded.contains("name=\"password\"") || folded.contains("name='password'") {
            return Some(form.to_string());
        }
    }
    None
}

fn error_block(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("div.blockMessage--error").ok()?;
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
        || folded.contains("please turn javascript on")
        || html.contains("vddosw3data")
}

/// Страница vDDoS считает куку `R3ACTLB` через AES и только потом отдаёт XenForo.
fn antibot_cookie(html: &str) -> Option<(String, String)> {
    if !html.contains("slowAES") {
        return None;
    }
    let pattern = Regex::new(r#""([0-9a-fA-F]{32})""#).ok()?;
    let hexes: Vec<String> = pattern
        .captures_iter(html)
        .filter_map(|caps| caps.get(1).map(|item| item.as_str().to_string()))
        .collect();
    if hexes.len() < 3 {
        return None;
    }
    let key = hex_bytes(&hexes[0])?;
    let iv = hex_bytes(&hexes[1])?;
    let block = hex_bytes(&hexes[2])?;
    let plain = aes128_cbc_block(&key, &iv, &block);
    Some((antibot_cookie_name(html), hex_encode(&plain)))
}

fn antibot_cookie_name(html: &str) -> String {
    let Ok(pattern) = Regex::new(r#""((?:\\x[0-9a-fA-F]{2})+)""#) else {
        return "R3ACTLB".into();
    };
    for caps in pattern.captures_iter(html) {
        let Some(encoded) = caps.get(1) else {
            continue;
        };
        let Some(decoded) = decode_js_hex(encoded.as_str()) else {
            continue;
        };
        let Some(name) = decoded.strip_suffix('=') else {
            continue;
        };
        if !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return name.to_string();
        }
    }
    "R3ACTLB".into()
}

fn decode_js_hex(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' || bytes[index + 1] != b'x' {
            return None;
        }
        let hex = std::str::from_utf8(&bytes[index + 2..index + 4]).ok()?;
        out.push(u8::from_str_radix(hex, 16).ok()?);
        index += 4;
    }
    String::from_utf8(out).ok()
}

fn hex_bytes(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn aes128_cbc_block(key: &[u8; 16], iv: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let cipher = Aes128::new(key.into());
    let mut decrypted = (*block).into();
    cipher.decrypt_block(&mut decrypted);
    let mut plain = [0u8; 16];
    for index in 0..16 {
        plain[index] = decrypted[index] ^ iv[index];
    }
    plain
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

    #[test]
    fn antibot_page_sets_reactlb_cookie() {
        let html = r#"
            <script src="/vddosw3data.js"></script>
            <script>
            var _0xa3fe=["\x52\x33\x41\x43\x54\x4C\x42\x3D","1f996f678373ac2ca0de79699721e355","b19f46c2e6d8a52affe9db9069d9c5b4","efbe6010c38521ce411d9a8652cad8c0"];
            slowAES.decrypt(c,2,a,b);
            </script>
        "#;
        assert_eq!(
            antibot_cookie(html),
            Some((
                "R3ACTLB".to_string(),
                "0635ba1ca6b4293ec570c8aed8d7912c".to_string()
            ))
        );
    }

    #[test]
    fn password_form_skips_webauthn_fields() {
        let html = r#"
            <form action="/login/login" method="post">
                <input type="hidden" name="_xfToken" value="good" />
                <input type="hidden" name="_xfRedirect" value="https://forum.majestic-rp.ru/" />
                <input type="password" name="password" />
            </form>
            <form action="/login/login" method="post">
                <input type="hidden" name="_xfToken" value="other" />
                <input type="hidden" name="webauthn_challenge" value="secret" />
            </form>
        "#;
        let form = password_form(html).unwrap();
        let names: Vec<_> = hidden_fields(&form)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            vec!["_xfToken".to_string(), "_xfRedirect".to_string()]
        );
        let index = r#"
            <div class="block">
                <h2 class="block-header"><a href="/forums/seattle.1200/">Seattle</a></h2>
                <a href="/forums/zakonodatel-naya-baza.1149/">Законодательная база</a>
            </div>
            <div class="block">
                <h2 class="block-header">Phoenix</h2>
                <a href="/forums/zakonodatel-naya-baza.1211/">Законодательная база</a>
            </div>
        "#;
        assert_eq!(law_node_on_page(index, Server::Seattle), Some(1149));
        assert_eq!(law_node_on_page(index, Server::Phoenix), Some(1211));
        assert!(error_block(
            r#"<div class="blockMessage">cookie</div><div class="blockMessage blockMessage--error">Неверный пароль</div>"#
        )
        .as_deref()
            == Some("Неверный пароль"));
    }
}
