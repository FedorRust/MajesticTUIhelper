use std::io::{self, IsTerminal, Stdout, Write};
use std::path::PathBuf;
use std::time::Duration;

use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::config;
use crate::forum::{self, UpdateError};
use crate::index::{self, load_corpus};
use crate::model::{Article, Corpus, Server};
use crate::query::{self, Hit, Query};
use crate::report::report_line;
use crate::theme::{self, Theme};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Picker,
    Search,
    Card,
    Login,
    Themes,
}

enum Click {
    Pick(Server),
    Open(usize),
    CopyShort,
    CopyFull,
    Update,
    Github,
    Theme(usize),
}

const GITHUB_URL: &str = "https://github.com/FedorRust";
const GITHUB_LOGO: &[&str] = &[
    "           ▄▄▄████▄▄▄",
    "        ▄██████████████▄",
    "      ▄███▀██████████▀███▄",
    "     ████              ████",
    "    ▄████▀             ████▄",
    "    ████▀               ████",
    "    ████▄              ▄████",
    "    █████              █████",
    "     ██▀▀█▄▄▄      ▄▄▄█████",
    "      ██▄ ▀█▀      ███████",
    "       ▀██▄▄▄      █████▀",
    "          ▀██      ██▀",
];

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
    }
}

pub struct App {
    laws: PathBuf,
    server: Server,
    restored: bool,
    corpus: Corpus,
    screen: Screen,
    pick: usize,
    pick_offset: usize,
    query: String,
    suppress: Option<String>,
    results: Vec<Hit>,
    sel: usize,
    list_offset: usize,
    card: Option<Hit>,
    full_text: bool,
    scroll: u16,
    login_user: String,
    login_pass: String,
    login_field: usize,
    status: String,
    status_err: bool,
    hot: Vec<(Rect, Click)>,
    /// Сетевой шаг после того, как кадр со статусом уже нарисован.
    job: Option<Job>,
    /// Живёт вместе с приложением: на X11 буфер пропадает, если владельца сразу уничтожить.
    clipboard: Option<arboard::Clipboard>,
    theme: Theme,
    theme_sel: usize,
    theme_offset: usize,
    theme_back: Screen,
}

enum Job {
    Update,
    Login,
}

impl App {
    pub fn new() -> Result<Self, String> {
        let laws = index::laws_root().ok_or_else(|| {
            "не найден каталог laws с portland и memphis. Запусти mj из репозитория или задай MJ_LAWS".to_string()
        })?;
        let restored = config::load_server();
        let server = restored.unwrap_or(Server::Portland);
        Self::open(laws, server, restored.is_some())
    }

    fn open(laws: PathBuf, server: Server, restored: bool) -> Result<Self, String> {
        let (corpus, status, status_err) = match load_corpus(&laws, server) {
            Ok(corpus) => (corpus, String::new(), false),
            Err(err) => (
                Corpus {
                    server,
                    articles: Vec::new(),
                    files: Vec::new(),
                },
                err,
                true,
            ),
        };
        let pick = server.index();
        let theme = theme::get(&config::load_theme_id().unwrap_or_default());
        Ok(Self {
            laws,
            server,
            restored,
            corpus,
            screen: Screen::Picker,
            pick,
            pick_offset: 0,
            query: String::new(),
            suppress: None,
            results: Vec::new(),
            sel: 0,
            list_offset: 0,
            card: None,
            full_text: false,
            scroll: 0,
            login_user: String::new(),
            login_pass: String::new(),
            login_field: 0,
            status,
            status_err,
            hot: Vec::new(),
            job: None,
            clipboard: None,
            theme_sel: theme::index_of(theme.id),
            theme_offset: 0,
            theme_back: Screen::Picker,
            theme,
        })
    }

    fn confirm_server(&mut self) {
        self.server = Server::all()
            .get(self.pick)
            .copied()
            .unwrap_or(Server::Portland);
        let _ = config::save_server(self.server);
        self.restored = true;
        match load_corpus(&self.laws, self.server) {
            Ok(corpus) => {
                self.corpus = corpus;
                self.query.clear();
                self.suppress = None;
                self.results.clear();
                self.sel = 0;
                self.card = None;
                self.full_text = false;
                self.screen = Screen::Search;
                self.set_status("", false);
            }
            Err(err) => self.set_status(&err, true),
        }
    }

    fn reload(&mut self) {
        if let Ok(corpus) = load_corpus(&self.laws, self.server) {
            self.corpus = corpus;
        }
        self.card = None;
        self.full_text = false;
        self.screen = Screen::Search;
        self.refresh();
    }

    fn refresh(&mut self) {
        let parsed = query::parse_query(&self.query);
        let hits = query::lookup(&self.corpus, &self.query);
        let suppressed = self.suppress.as_deref() == Some(self.query.as_str());
        self.results = hits;
        self.sel = 0;
        self.list_offset = 0;
        if self.results.is_empty() {
            if !self.query.is_empty() {
                let message = match parsed {
                    Query::Code { .. } => "Нет такой статьи",
                    Query::Words { .. } => "Ничего не найдено",
                    _ => "",
                };
                if !message.is_empty() {
                    self.set_status(message, true);
                }
            }
            return;
        }
        if !suppressed && parsed.immediate() && self.results.len() == 1 {
            let hit = self.results[0].clone();
            self.open_hit(hit);
        }
    }

    fn open_hit(&mut self, hit: Hit) {
        if !hit.part_found {
            self.set_status("Такой части нет, показана первая", true);
        }
        self.card = Some(hit);
        self.screen = Screen::Card;
        self.full_text = false;
        self.scroll = 0;
    }

    fn set_status(&mut self, text: &str, err: bool) {
        self.status = text.to_string();
        self.status_err = err;
    }

    fn article(&self, hit: &Hit) -> &Article {
        &self.corpus.articles[hit.article]
    }
}

pub fn run() -> Result<(), String> {
    let mut app = App::new()?;
    if !io::stdin().is_terminal() {
        return Err("нужен обычный терминал".into());
    }
    enable_raw_mode().map_err(|err| err.to_string())?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)
        .map_err(|err| err.to_string())?;
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend).map_err(|err| err.to_string())?;
    event_loop(&mut terminal, &mut app)
}

fn event_loop(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<(), String> {
    let mut dirty = true;
    loop {
        if dirty {
            terminal
                .draw(|frame| draw(frame, app))
                .map_err(|err| err.to_string())?;
            dirty = false;
        }
        if !event::poll(Duration::from_millis(200)).map_err(|err| err.to_string())? {
            continue;
        }
        match event::read().map_err(|err| err.to_string())? {
            Event::Key(key) => {
                if matches!(key.kind, KeyEventKind::Release) {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'))
                {
                    return Ok(());
                }
                app.status.clear();
                app.status_err = false;
                if on_key(app, key) {
                    return Ok(());
                }
                if app.job.is_some() {
                    terminal
                        .draw(|frame| draw(frame, app))
                        .map_err(|err| err.to_string())?;
                    match app.job.take() {
                        Some(Job::Update) => run_update(app),
                        Some(Job::Login) => perform_login(app),
                        None => {}
                    }
                }
                dirty = true;
            }
            Event::Mouse(mouse) => {
                if matches!(mouse.kind, MouseEventKind::Moved) {
                    continue;
                }
                app.status.clear();
                app.status_err = false;
                on_mouse(app, mouse.kind, mouse.column, mouse.row);
                dirty = true;
            }
            Event::Resize(_, _) => dirty = true,
            _ => {}
        }
    }
}

fn on_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::F(5) && !matches!(app.screen, Screen::Login | Screen::Themes) {
        begin_update(app);
        return false;
    }
    if key.code == KeyCode::F(6) && !matches!(app.screen, Screen::Login | Screen::Themes) {
        open_themes(app);
        return false;
    }
    match app.screen {
        Screen::Picker => match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Up | KeyCode::Char('k') => move_pick(app, -1),
            KeyCode::Down | KeyCode::Char('j') => move_pick(app, 1),
            KeyCode::Enter => app.confirm_server(),
            KeyCode::Char('t') => open_themes(app),
            KeyCode::Esc => return true,
            _ => {}
        },
        Screen::Themes => match key.code {
            KeyCode::Up | KeyCode::Char('k') => move_theme(app, -1),
            KeyCode::Down | KeyCode::Char('j') => move_theme(app, 1),
            KeyCode::PageUp => move_theme(app, -8),
            KeyCode::PageDown => move_theme(app, 8),
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') => {
                app.screen = app.theme_back;
            }
            _ => {}
        },
        Screen::Search => match key.code {
            KeyCode::Esc => {
                if app.query.is_empty() {
                    app.screen = Screen::Picker;
                } else {
                    app.query.clear();
                    app.suppress = None;
                    app.results.clear();
                }
            }
            KeyCode::Up => move_sel(app, -1),
            KeyCode::Down => move_sel(app, 1),
            KeyCode::PageUp => move_sel(app, -8),
            KeyCode::PageDown => move_sel(app, 8),
            KeyCode::Enter => {
                if let Some(hit) = app.results.get(app.sel).cloned() {
                    app.open_hit(hit);
                }
            }
            KeyCode::Backspace => {
                app.query.pop();
                app.suppress = None;
                app.refresh();
            }
            KeyCode::Char(ch) => {
                app.query.push(ch);
                app.suppress = None;
                app.refresh();
            }
            _ => {}
        },
        Screen::Card => match key.code {
            KeyCode::Esc => {
                if app.full_text {
                    app.full_text = false;
                    app.scroll = 0;
                } else {
                    app.screen = Screen::Search;
                    app.suppress = Some(app.query.clone());
                    app.refresh();
                }
            }
            KeyCode::Char('f') | KeyCode::Enter => {
                app.full_text = !app.full_text;
                app.scroll = 0;
            }
            KeyCode::Char('c') => copy_short(app),
            KeyCode::Char('C') => copy_full(app),
            KeyCode::Up | KeyCode::Char('k') => {
                if app.full_text {
                    app.scroll = app.scroll.saturating_sub(1);
                } else {
                    shift_part(app, -1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.full_text {
                    app.scroll = app.scroll.saturating_add(1);
                } else {
                    shift_part(app, 1);
                }
            }
            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(8),
            KeyCode::PageDown => app.scroll = app.scroll.saturating_add(8),
            KeyCode::Backspace if !app.full_text => {
                app.screen = Screen::Search;
                app.query.pop();
                app.suppress = None;
                app.refresh();
            }
            _ => {}
        },
        Screen::Login => match key.code {
            KeyCode::Esc => {
                app.login_pass.clear();
                app.screen = Screen::Search;
            }
            KeyCode::Tab | KeyCode::Up | KeyCode::Down => {
                app.login_field = if app.login_field == 0 { 1 } else { 0 };
            }
            KeyCode::Backspace => {
                if app.login_field == 0 {
                    app.login_user.pop();
                } else {
                    app.login_pass.pop();
                }
            }
            KeyCode::Enter => submit_login(app),
            KeyCode::Char(ch) => {
                if app.login_field == 0 {
                    app.login_user.push(ch);
                } else {
                    app.login_pass.push(ch);
                }
            }
            _ => {}
        },
    }
    false
}

fn on_mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    match kind {
        MouseEventKind::ScrollUp => match app.screen {
            Screen::Card => app.scroll = app.scroll.saturating_sub(3),
            Screen::Picker => move_pick(app, -1),
            Screen::Themes => move_theme(app, -1),
            _ => move_sel(app, -1),
        },
        MouseEventKind::ScrollDown => match app.screen {
            Screen::Card => app.scroll = app.scroll.saturating_add(3),
            Screen::Picker => move_pick(app, 1),
            Screen::Themes => move_theme(app, 1),
            _ => move_sel(app, 1),
        },
        MouseEventKind::Down(MouseButton::Left) => {
            let action = app
                .hot
                .iter()
                .rev()
                .find(|(rect, _)| contains(*rect, column, row))
                .map(|(_, action)| match action {
                    Click::Pick(server) => Click::Pick(*server),
                    Click::Open(index) => Click::Open(*index),
                    Click::CopyShort => Click::CopyShort,
                    Click::CopyFull => Click::CopyFull,
                    Click::Update => Click::Update,
                    Click::Github => Click::Github,
                    Click::Theme(index) => Click::Theme(*index),
                });
            match action {
                Some(Click::Pick(server)) => {
                    app.pick = server.index();
                    app.confirm_server();
                }
                Some(Click::Open(index)) => {
                    if let Some(hit) = app.results.get(index).cloned() {
                        app.sel = index;
                        app.open_hit(hit);
                    }
                }
                Some(Click::CopyShort) => copy_short(app),
                Some(Click::CopyFull) => copy_full(app),
                Some(Click::Update) => begin_update(app),
                Some(Click::Github) => open_github(app),
                Some(Click::Theme(index)) => set_theme(app, index),
                None => {}
            }
        }
        _ => {}
    }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

fn move_sel(app: &mut App, delta: isize) {
    if app.results.is_empty() {
        return;
    }
    let len = app.results.len() as isize;
    let next = (app.sel as isize + delta).clamp(0, len - 1) as usize;
    app.sel = next;
}

fn shift_part(app: &mut App, delta: isize) {
    let Some(hit) = app.card.clone() else { return };
    let len = app.article(&hit).parts.len() as isize;
    if len == 0 {
        return;
    }
    let next = (hit.part as isize + delta).clamp(0, len - 1) as usize;
    if let Some(card) = app.card.as_mut() {
        card.part = next;
        card.part_found = true;
    }
    app.scroll = 0;
}

fn copy_short(app: &mut App) {
    let Some(hit) = app.card.clone() else { return };
    let line = report_line(app.article(&hit), hit.part);
    copy_text(app, &line);
}

fn copy_full(app: &mut App) {
    let Some(hit) = app.card.clone() else { return };
    let text = app.article(&hit).full_text.clone();
    copy_text(app, &text);
}

fn copy_text(app: &mut App, text: &str) {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && copy_with_wl_copy(text).is_ok() {
        app.set_status("Скопировано", false);
        return;
    }
    if app.clipboard.is_none() {
        match arboard::Clipboard::new() {
            Ok(clipboard) => app.clipboard = Some(clipboard),
            Err(err) => {
                app.set_status(&format!("Буфер недоступен: {err}"), true);
                return;
            }
        }
    }
    let Some(clipboard) = app.clipboard.as_mut() else {
        return;
    };
    match clipboard.set_text(text.to_string()) {
        Ok(()) => app.set_status("Скопировано", false),
        Err(err) => app.set_status(&format!("Буфер: {err}"), true),
    }
}

fn copy_with_wl_copy(text: &str) -> Result<(), String> {
    // Аргументом wl-copy сразу выходит и оставляет фоновый процесс.
    // Через stdin он остаётся на переднем плане и вешает интерфейс, если его ждать.
    match std::process::Command::new("wl-copy")
        .arg(text)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("wl-copy завершился с кодом {status}")),
        Err(err) if err.kind() == std::io::ErrorKind::ArgumentListTooLong => {
            copy_wl_copy_stdin(text)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err("не найден wl-copy".into()),
        Err(err) => Err(err.to_string()),
    }
}

fn copy_wl_copy_stdin(text: &str) -> Result<(), String> {
    let mut child = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    let mut stdin = child.stdin.take().ok_or("wl-copy не принял текст")?;
    stdin
        .write_all(text.as_bytes())
        .map_err(|err| err.to_string())?;
    drop(stdin);
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn begin_update(app: &mut App) {
    if matches!(app.screen, Screen::Picker) {
        app.server = Server::all()
            .get(app.pick)
            .copied()
            .unwrap_or(Server::Portland);
        if let Ok(corpus) = load_corpus(&app.laws, app.server) {
            app.corpus = corpus;
        }
    }
    if !config::session_ready() {
        app.login_field = 0;
        app.screen = Screen::Login;
        app.set_status(
            "Пароль на диск не пишется. Кука сессии ляжет в ~/.config/mj/",
            false,
        );
        return;
    }
    app.set_status("Обновляю УК, АК, ДК и ПК…", false);
    app.job = Some(Job::Update);
}

fn submit_login(app: &mut App) {
    let user = app.login_user.trim();
    if user.is_empty() || app.login_pass.is_empty() {
        app.set_status("Нужны логин и пароль", true);
        return;
    }
    app.set_status("Вхожу на форум…", false);
    app.job = Some(Job::Login);
}

fn perform_login(app: &mut App) {
    let user = app.login_user.trim().to_string();
    let mut password = std::mem::take(&mut app.login_pass);
    let login = forum::login(&user, &password);
    password.clear();
    match login {
        Ok(()) => {
            app.set_status("Сессия сохранена, обновляю кодексы…", false);
            run_update(app);
        }
        Err(UpdateError::NeedLogin(message)) | Err(UpdateError::Failed(message)) => {
            app.set_status(&message, true);
        }
    }
}

fn run_update(app: &mut App) {
    let existing = app.corpus.files.clone();
    match forum::update(&app.laws, app.server, &existing) {
        Ok(report) => {
            app.reload();
            let names = if report.updated.is_empty() {
                "ни один кодекс не обновился".to_string()
            } else {
                report
                    .updated
                    .iter()
                    .map(|family| family.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let mut message = format!("Обновлено: {names}.");
            if !report.problems.is_empty() {
                message.push(' ');
                message.push_str(&report.problems.join(" "));
                message.push_str(" Старые файлы этих кодексов на месте.");
            }
            app.set_status(
                &message,
                !report.problems.is_empty() && report.updated.is_empty(),
            );
            app.screen = Screen::Search;
        }
        Err(UpdateError::NeedLogin(message)) => {
            app.screen = Screen::Login;
            app.login_field = 0;
            app.set_status(&message, true);
        }
        Err(UpdateError::Failed(message)) => {
            app.set_status(&message, true);
            if !matches!(app.screen, Screen::Login | Screen::Picker) {
                app.screen = Screen::Search;
            }
        }
    }
}

fn open_github(app: &mut App) {
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", GITHUB_URL]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(GITHUB_URL);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(GITHUB_URL);
        command
    };
    match command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            app.set_status("Открываю GitHub", false);
        }
        Err(_) => app.set_status("Не удалось открыть браузер", true),
    }
}

fn open_themes(app: &mut App) {
    app.theme_back = app.screen;
    app.theme_sel = theme::index_of(app.theme.id);
    app.screen = Screen::Themes;
}

fn set_theme(app: &mut App, index: usize) {
    let Some(theme) = theme::ALL.get(index).copied() else {
        return;
    };
    app.theme_sel = index;
    app.theme = theme;
    let _ = config::save_theme(app.server, theme.id);
}

fn move_theme(app: &mut App, delta: isize) {
    let len = theme::ALL.len() as isize;
    if len == 0 {
        return;
    }
    let next = (app.theme_sel as isize + delta).clamp(0, len - 1) as usize;
    set_theme(app, next);
}

fn draw(frame: &mut Frame, app: &mut App) {
    app.hot.clear();
    frame.render_widget(Block::default().style(app.theme.text()), frame.area());
    match app.screen {
        Screen::Picker => {
            draw_picker(frame, app);
            draw_github(frame, app);
        }
        Screen::Search => draw_search(frame, app),
        Screen::Card => draw_card(frame, app),
        Screen::Login => draw_login(frame, app),
        Screen::Themes => draw_themes(frame, app),
    }
}

fn draw_github(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let height = GITHUB_LOGO.len() as u16;
    let width = GITHUB_LOGO
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as u16;
    if area.width <= width || area.height <= height {
        return;
    }
    let rect = Rect {
        x: area.right().saturating_sub(width),
        y: area.bottom().saturating_sub(height),
        width,
        height,
    };
    let lines: Vec<Line> = GITHUB_LOGO.iter().copied().map(Line::raw).collect();
    frame.render_widget(Paragraph::new(lines).style(app.theme.accent()), rect);
    app.hot.push((rect, Click::Github));
}

fn move_pick(app: &mut App, delta: isize) {
    let len = Server::all().len() as isize;
    if len == 0 {
        return;
    }
    app.pick = (app.pick as isize + delta).clamp(0, len - 1) as usize;
}

fn draw_picker(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    let hint = if app.restored {
        "Прошлый сервер выделен. Enter — поиск."
    } else {
        "Enter — поиск по УК, АК, ДК и ПК."
    };
    let title = Paragraph::new(hint).block(framed(app.theme, " mj "));
    frame.render_widget(title, chunks[0]);
    let servers = Server::all();
    let height = chunks[1].height as usize;
    keep_visible(app.pick, servers.len(), height, &mut app.pick_offset);
    for (index, server) in servers
        .iter()
        .enumerate()
        .skip(app.pick_offset)
        .take(height.max(1))
    {
        let rect = Rect {
            x: chunks[1].x,
            y: chunks[1].y + (index - app.pick_offset) as u16,
            width: chunks[1].width,
            height: 1,
        };
        let style = if app.pick == index {
            app.theme.selected()
        } else {
            app.theme.text()
        };
        let row = Paragraph::new(fit(
            &format!("  {} ", server.title()),
            rect.width as usize,
        ))
        .style(style);
        frame.render_widget(row, rect);
        app.hot.push((rect, Click::Pick(*server)));
    }
    draw_footer(
        frame,
        chunks[2],
        app,
        "↑↓ сервер    Enter поиск    t тема    q выход",
    );
}

fn draw_themes(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(area);
    let intro = Paragraph::new("Схема применяется сразу и запоминается.")
        .block(framed(app.theme, " Тема "));
    frame.render_widget(intro, chunks[0]);
    let list_area = chunks[1];
    let height = list_area.height as usize;
    keep_visible(
        app.theme_sel,
        theme::ALL.len(),
        height,
        &mut app.theme_offset,
    );
    let width = list_area.width as usize;
    let offset = app.theme_offset;
    let selected = app.theme_sel;
    let mut lines = Vec::new();
    for (index, theme) in theme::ALL.iter().enumerate().skip(offset).take(height) {
        let style = if index == selected {
            app.theme.selected()
        } else {
            app.theme.text()
        };
        lines.push(Line::styled(
            fit(&format!("  {} ", theme.name), width.max(1)),
            style,
        ));
        let row = Rect {
            x: list_area.x,
            y: list_area.y + (index - offset) as u16,
            width: list_area.width,
            height: 1,
        };
        app.hot.push((row, Click::Theme(index)));
    }
    frame.render_widget(Paragraph::new(lines).style(app.theme.text()), list_area);
    draw_footer(
        frame,
        chunks[2],
        app,
        "↑↓ схема    Enter назад    Esc назад",
    );
}

fn draw_search(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(area);
    let field = if app.query.is_empty() {
        "номер статьи, название статьи".to_string()
    } else {
        format!("> {}█", app.query)
    };
    let input = Paragraph::new(field)
        .style(if app.query.is_empty() {
            app.theme.muted()
        } else {
            app.theme.text()
        })
        .block(framed(
            app.theme,
            format!(" Поиск · {} ", app.server.title()),
        ));
    frame.render_widget(input, chunks[0]);

    let list_area = chunks[1];
    let inner = Rect {
        x: list_area.x,
        y: list_area.y,
        width: list_area.width,
        height: list_area.height,
    };
    let height = inner.height as usize;
    keep_visible(app.sel, app.results.len(), height, &mut app.list_offset);
    let width = inner.width.saturating_sub(2) as usize;
    let offset = app.list_offset;
    let selected = app.sel;
    let rows: Vec<(usize, String)> = app
        .results
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(index, hit)| {
            let article = &app.corpus.articles[hit.article];
            let label = format!(
                "{} {}  {}",
                article.family.label(),
                article.code,
                article.title
            );
            (index, clip(&label, width.max(1)))
        })
        .collect();
    let mut lines = Vec::new();
    if app.results.is_empty() {
        lines.push(Line::raw(if app.query.is_empty() {
            "Индекс готов."
        } else {
            "Пусто."
        }));
    }
    for (slot, (index, label)) in rows.into_iter().enumerate() {
        let style = if index == selected {
            app.theme.selected()
        } else {
            app.theme.text()
        };
        lines.push(Line::styled(fit(&label, width.max(1)), style));
        let row = Rect {
            x: inner.x,
            y: inner.y + slot as u16,
            width: inner.width,
            height: 1,
        };
        app.hot.push((row, Click::Open(index)));
    }
    frame.render_widget(Paragraph::new(lines).style(app.theme.text()), list_area);

    let button = Paragraph::new("Обновить с форума")
        .style(app.theme.text())
        .alignment(ratatui::layout::Alignment::Center)
        .block(framed(app.theme, ""));
    frame.render_widget(button, chunks[2]);
    app.hot.push((chunks[2], Click::Update));
    draw_footer(
        frame,
        chunks[3],
        app,
        "↑↓ список    Enter карточка    Esc сервер    F5 обновить    F6 тема",
    );
}

fn draw_card(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(area);
    let Some(hit) = app.card.clone() else {
        app.screen = Screen::Search;
        return;
    };
    let article = app.article(&hit).clone();
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(
            "{} {}  {}",
            article.code,
            article.family.label(),
            article.title
        ),
        app.theme.title(),
    )));
    let jur = crate::report::jur_tag(&article.jurisdiction);
    if !jur.is_empty() {
        lines.push(Line::raw(jur));
    }
    lines.push(Line::raw(""));
    if app.full_text {
        for line in article.full_text.lines() {
            lines.push(Line::raw(line.to_string()));
        }
    } else {
        let order = std::iter::once(hit.part)
            .chain((0..article.parts.len()).filter(|index| *index != hit.part));
        for index in order {
            let Some(part) = article.parts.get(index) else {
                continue;
            };
            let selected = index == hit.part;
            let mut header = format!("{}ч. {}", if selected { "▸ " } else { "  " }, part.number);
            if part.stars > 0 {
                header.push(' ');
                header.push_str(&"★".repeat(part.stars as usize));
            }
            let style = if selected {
                app.theme.selected()
            } else {
                app.theme.text()
            };
            lines.push(Line::styled(header, style));
            if !part.composition.is_empty() {
                lines.push(Line::raw(format!("  {}", part.composition)));
            }
            if !part.punishment.is_empty() {
                lines.push(Line::raw(format!("  Наказание: {}", part.punishment)));
            }
            lines.push(Line::raw(""));
        }
    }
    let body = Paragraph::new(lines)
        .style(app.theme.text())
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0))
        .block(framed(
            app.theme,
            if app.full_text {
                " Полный текст "
            } else {
                " Карточка "
            },
        ));
    frame.render_widget(body, chunks[0]);

    let buttons = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(chunks[1]);
    render_button(frame, buttons[0], "Копировать", app.theme);
    render_button(frame, buttons[1], "Копировать всё", app.theme);
    render_button(frame, buttons[2], "Обновить", app.theme);
    app.hot.push((buttons[0], Click::CopyShort));
    app.hot.push((buttons[1], Click::CopyFull));
    app.hot.push((buttons[2], Click::Update));
    let help = if app.full_text {
        "↑↓ лист    f или Enter карточка    c копировать    C всё    Esc назад"
    } else {
        "↑↓ часть    f или Enter текст    c копировать    C всё    Esc назад"
    };
    draw_footer(frame, chunks[2], app, help);
}

fn draw_login(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let box_width = area.width.min(64);
    let box_height = 12.min(area.height);
    let left = area.x + area.width.saturating_sub(box_width) / 2;
    let top = area.y + area.height.saturating_sub(box_height) / 2;
    let popup = Rect {
        x: left,
        y: top,
        width: box_width,
        height: box_height,
    };
    frame.render_widget(framed(app.theme, " Вход на форум "), popup);
    let inner = Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Пароль только в этом запросе, на диск не пишется.")
            .style(app.theme.muted()),
        rows[0],
    );
    let user_style = field_style(app.theme, app.login_field == 0);
    let pass_style = field_style(app.theme, app.login_field == 1);
    frame.render_widget(Paragraph::new("Логин").style(user_style), rows[1]);
    let user = if app.login_field == 0 {
        format!("> {}█", app.login_user)
    } else {
        format!("> {}", app.login_user)
    };
    frame.render_widget(Paragraph::new(user).style(user_style), rows[2]);
    frame.render_widget(Paragraph::new("Пароль").style(pass_style), rows[3]);
    let mask = "•".repeat(app.login_pass.chars().count());
    let pass = if app.login_field == 1 {
        format!("> {mask}█")
    } else {
        format!("> {mask}")
    };
    frame.render_widget(Paragraph::new(pass).style(pass_style), rows[4]);
    draw_footer(frame, rows[5], app, "Tab поле    Enter войти    Esc отмена");
}

fn framed(theme: Theme, title: impl Into<String>) -> Block<'static> {
    let title = title.into();
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border())
        .style(theme.text());
    if !title.is_empty() {
        block = block.title(Span::styled(title, theme.title()));
    }
    block
}

fn render_button(frame: &mut Frame, area: Rect, label: &str, theme: Theme) {
    let widget = Paragraph::new(label)
        .style(theme.text())
        .alignment(ratatui::layout::Alignment::Center)
        .block(framed(theme, ""));
    frame.render_widget(widget, area);
}

fn field_style(theme: Theme, active: bool) -> Style {
    if active {
        theme.selected()
    } else {
        theme.text()
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App, help: &str) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let status_style = if app.status_err {
        app.theme.error()
    } else {
        app.theme.muted()
    };
    let status = if app.status.is_empty() {
        app.corpus.counts()
    } else {
        app.status.clone()
    };
    frame.render_widget(Paragraph::new(status).style(status_style), rows[0]);
    frame.render_widget(Paragraph::new(help).style(app.theme.muted()), rows[1]);
}

fn keep_visible(sel: usize, len: usize, height: usize, offset: &mut usize) {
    if len == 0 || height == 0 {
        *offset = 0;
        return;
    }
    if sel < *offset {
        *offset = sel;
    }
    if sel >= *offset + height {
        *offset = sel + 1 - height;
    }
}

fn fit(text: &str, width: usize) -> String {
    let clipped = clip(text, width);
    let pad = width.saturating_sub(clipped.chars().count());
    format!("{clipped}{}", " ".repeat(pad))
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let head: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn test_app(server: Server) -> App {
        let laws = index::laws_root().expect("laws");
        App::open(laws, server, true).expect("app")
    }

    #[test]
    fn suffixed_code_opens_card() {
        let mut app = test_app(Server::Memphis);
        app.screen = Screen::Search;
        app.query = "10.5.1ук".into();
        app.refresh();
        assert!(matches!(app.screen, Screen::Card));
        let hit = app.card.as_ref().unwrap();
        assert_eq!(app.article(hit).code, "10.5");
        assert_eq!(app.article(hit).parts[hit.part].number, "1");
    }

    #[test]
    fn word_query_stays_a_list() {
        let mut app = test_app(Server::Memphis);
        app.screen = Screen::Search;
        app.query = "угон".into();
        app.refresh();
        assert!(matches!(app.screen, Screen::Search));
        assert!(app.results.len() >= 2);
    }

    #[test]
    fn missing_laws_still_opens_picker() {
        let laws = std::env::temp_dir().join(format!("mj-empty-{}", std::process::id()));
        let app = App::open(laws, Server::Seattle, true).expect("picker");
        assert!(matches!(app.screen, Screen::Picker));
        assert!(app.status.contains("Seattle"));
        assert!(app.status_err);
    }

    #[test]
    fn picker_renders_both_servers() {
        let mut app = test_app(Server::Portland);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let view: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(view.contains("Portland"));
        assert!(view.contains("Memphis"));
        assert!(view.contains("Orlando"));
        assert!(view.contains("Denver"));
        assert!(view.contains("Phoenix"));
        assert!(view.contains("Seattle"));
        assert!(view.contains("▄▄▄████▄▄▄"));
    }

    #[test]
    fn theme_screen_lists_linux_schemes() {
        let mut app = test_app(Server::Portland);
        app.screen = Screen::Themes;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let view: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(view.contains("Catppuccin Mocha"));
        assert!(view.contains("Gruvbox"));
        assert!(view.contains("Nord"));
    }
}
