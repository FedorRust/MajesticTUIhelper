use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
struct Ink {
    bg: Color,
    fg: Color,
    muted: Color,
    accent: Color,
    select: Color,
    select_fg: Color,
    error: Color,
    border: Color,
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    ink: Option<Ink>,
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn ink(
    bg: u32,
    fg: u32,
    muted: u32,
    accent: u32,
    select: u32,
    select_fg: u32,
    error: u32,
    border: u32,
) -> Option<Ink> {
    Some(Ink {
        bg: rgb(bg),
        fg: rgb(fg),
        muted: rgb(muted),
        accent: rgb(accent),
        select: rgb(select),
        select_fg: rgb(select_fg),
        error: rgb(error),
        border: rgb(border),
    })
}

/// Схемы, которые чаще всего ставят в терминалах и окружениях Linux.
pub const ALL: &[Theme] = &[
    Theme {
        id: "terminal",
        name: "Терминал",
        ink: None,
    },
    Theme {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        ink: ink(
            0x1e1e2e, 0xcdd6f4, 0xa6adc8, 0xcba6f7, 0x313244, 0xcdd6f4, 0xf38ba8, 0x45475a,
        ),
    },
    Theme {
        id: "catppuccin-frappe",
        name: "Catppuccin Frappé",
        ink: ink(
            0x303446, 0xc6d0f5, 0xa5adce, 0xca9ee6, 0x414559, 0xc6d0f5, 0xe78284, 0x51576d,
        ),
    },
    Theme {
        id: "catppuccin-latte",
        name: "Catppuccin Latte",
        ink: ink(
            0xeff1f5, 0x4c4f69, 0x6c6f85, 0x8839ef, 0xccd0da, 0x4c4f69, 0xd20f39, 0xbcc0cc,
        ),
    },
    Theme {
        id: "gruvbox-dark",
        name: "Gruvbox Dark",
        ink: ink(
            0x282828, 0xebdbb2, 0xa89984, 0xfabd2f, 0x3c3836, 0xebdbb2, 0xfb4934, 0x504945,
        ),
    },
    Theme {
        id: "gruvbox-light",
        name: "Gruvbox Light",
        ink: ink(
            0xfbf1c7, 0x3c3836, 0x7c6f64, 0xb57614, 0xebdbb2, 0x3c3836, 0x9d0006, 0xd5c4a1,
        ),
    },
    Theme {
        id: "dracula",
        name: "Dracula",
        ink: ink(
            0x282a36, 0xf8f8f2, 0x6272a4, 0xbd93f9, 0x44475a, 0xf8f8f2, 0xff5555, 0x6272a4,
        ),
    },
    Theme {
        id: "nord",
        name: "Nord",
        ink: ink(
            0x2e3440, 0xeceff4, 0xd8dee9, 0x88c0d0, 0x3b4252, 0xeceff4, 0xbf616a, 0x434c5e,
        ),
    },
    Theme {
        id: "tokyo-night",
        name: "Tokyo Night",
        ink: ink(
            0x1a1b26, 0xc0caf5, 0xa9b1d6, 0x7aa2f7, 0x283457, 0xc0caf5, 0xf7768e, 0x3b4261,
        ),
    },
    Theme {
        id: "tokyo-night-storm",
        name: "Tokyo Night Storm",
        ink: ink(
            0x24283b, 0xc0caf5, 0xa9b1d6, 0x7aa2f7, 0x292e42, 0xc0caf5, 0xf7768e, 0x3b4261,
        ),
    },
    Theme {
        id: "solarized-dark",
        name: "Solarized Dark",
        ink: ink(
            0x002b36, 0x839496, 0x657b83, 0x268bd2, 0x073642, 0x93a1a1, 0xdc322f, 0x586e75,
        ),
    },
    Theme {
        id: "solarized-light",
        name: "Solarized Light",
        ink: ink(
            0xfdf6e3, 0x657b83, 0x93a1a1, 0x268bd2, 0xeee8d5, 0x586e75, 0xdc322f, 0x93a1a1,
        ),
    },
    Theme {
        id: "rose-pine",
        name: "Rosé Pine",
        ink: ink(
            0x191724, 0xe0def4, 0x908caa, 0xc4a7e7, 0x26233a, 0xe0def4, 0xeb6f92, 0x403d52,
        ),
    },
    Theme {
        id: "rose-pine-moon",
        name: "Rosé Pine Moon",
        ink: ink(
            0x232136, 0xe0def4, 0x908caa, 0xc4a7e7, 0x393552, 0xe0def4, 0xeb6f92, 0x44415a,
        ),
    },
    Theme {
        id: "everforest",
        name: "Everforest",
        ink: ink(
            0x2d353b, 0xd3c6aa, 0x859289, 0xa7c080, 0x3d484d, 0xd3c6aa, 0xe67e80, 0x475258,
        ),
    },
    Theme {
        id: "kanagawa",
        name: "Kanagawa",
        ink: ink(
            0x1f1f28, 0xdcd7ba, 0xc8c093, 0x7e9cd8, 0x2a2a37, 0xdcd7ba, 0xff5d62, 0x363646,
        ),
    },
    Theme {
        id: "adwaita-dark",
        name: "Adwaita Dark",
        ink: ink(
            0x242424, 0xffffff, 0x9a9996, 0x3584e4, 0x3584e4, 0xffffff, 0xe01b24, 0x3d3d3d,
        ),
    },
    Theme {
        id: "adwaita",
        name: "Adwaita",
        ink: ink(
            0xfafafa, 0x2e3436, 0x77767b, 0x3584e4, 0x3584e4, 0xffffff, 0xc01c28, 0xdeddda,
        ),
    },
    Theme {
        id: "breeze-dark",
        name: "Breeze Dark",
        ink: ink(
            0x232627, 0xeff0f1, 0x7f8c8d, 0x3daee9, 0x3daee9, 0xffffff, 0xda4453, 0x4d4d4d,
        ),
    },
    Theme {
        id: "breeze",
        name: "Breeze",
        ink: ink(
            0xeff0f1, 0x232627, 0x7f8c8d, 0x3daee9, 0x3daee9, 0xffffff, 0xed1515, 0xbdc3c7,
        ),
    },
    Theme {
        id: "oxocarbon",
        name: "Oxocarbon",
        ink: ink(
            0x161616, 0xf2f4f8, 0x878d96, 0x33b1ff, 0x262626, 0xf2f4f8, 0xff7eb6, 0x393939,
        ),
    },
    Theme {
        id: "one-dark",
        name: "One Dark",
        ink: ink(
            0x282c34, 0xabb2bf, 0x828997, 0x61afef, 0x3e4451, 0xabb2bf, 0xe06c75, 0x4b5263,
        ),
    },
    Theme {
        id: "ayu",
        name: "Ayu Dark",
        ink: ink(
            0x0b0e14, 0xbfbdb6, 0x8a9199, 0xe6b450, 0x11151c, 0xbfbdb6, 0xf07178, 0x1c212b,
        ),
    },
    Theme {
        id: "monokai",
        name: "Monokai",
        ink: ink(
            0x272822, 0xf8f8f2, 0x75715e, 0xa6e22e, 0x3e3d32, 0xf8f8f2, 0xf92672, 0x49483e,
        ),
    },
    Theme {
        id: "palenight",
        name: "Palenight",
        ink: ink(
            0x292d3e, 0xa6accd, 0x959dcb, 0x82aaff, 0x444267, 0xa6accd, 0xff5370, 0x3c435e,
        ),
    },
];

pub fn get(id: &str) -> Theme {
    ALL.iter()
        .copied()
        .find(|theme| theme.id == id)
        .unwrap_or(ALL[0])
}

pub fn index_of(id: &str) -> usize {
    ALL.iter().position(|theme| theme.id == id).unwrap_or(0)
}

impl Theme {
    pub fn text(self) -> Style {
        match self.ink {
            Some(ink) => Style::default().fg(ink.fg).bg(ink.bg),
            None => Style::default(),
        }
    }

    pub fn muted(self) -> Style {
        match self.ink {
            Some(ink) => Style::default().fg(ink.muted).bg(ink.bg),
            None => Style::default(),
        }
    }

    pub fn accent(self) -> Style {
        match self.ink {
            Some(ink) => Style::default().fg(ink.accent).bg(ink.bg),
            None => Style::default(),
        }
    }

    pub fn error(self) -> Style {
        match self.ink {
            Some(ink) => Style::default().fg(ink.error).bg(ink.bg),
            None => Style::default().fg(Color::Red),
        }
    }

    pub fn border(self) -> Style {
        match self.ink {
            Some(ink) => Style::default().fg(ink.border),
            None => Style::default(),
        }
    }

    pub fn selected(self) -> Style {
        match self.ink {
            Some(ink) => Style::default().fg(ink.select_fg).bg(ink.select),
            None => Style::default().add_modifier(Modifier::REVERSED),
        }
    }

    pub fn title(self) -> Style {
        self.accent().add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_named() {
        let mut ids: Vec<&str> = ALL.iter().map(|theme| theme.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert!(count >= 20);
        assert!(ALL.iter().all(|theme| !theme.name.is_empty()));
        assert_eq!(get("нет-такой").id, "terminal");
        assert_eq!(get("nord").name, "Nord");
    }
}
