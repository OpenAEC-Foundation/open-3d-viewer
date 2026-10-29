//! Viewer styles using the canonical OpenAEC Blueprint UI 0.4 colour roles.

use iced::widget::{button, container};
use iced::{border, Background, Border, Color, Shadow, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    #[default]
    Light,
    Forge,
    Night,
    Blueprint,
    Contrast,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 5] = [ThemeKind::Light, ThemeKind::Forge, ThemeKind::Night, ThemeKind::Blueprint, ThemeKind::Contrast];

    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Light => "Light",
            ThemeKind::Forge => "Forge",
            ThemeKind::Night => "Night",
            ThemeKind::Blueprint => "Blueprint",
            ThemeKind::Contrast => "Contrast",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.label().eq_ignore_ascii_case(s)).or(match s {
            "dark" | "openaec" => Some(ThemeKind::Night),
            _ => None,
        })
    }

    pub fn palette(self) -> Palette {
        use openaec_ui::design::tokens::ThemeId;
        let theme = match self {
            Self::Light => ThemeId::Light,
            Self::Forge | Self::Night => ThemeId::Dark,
            Self::Blueprint => ThemeId::Blueprint,
            Self::Contrast => ThemeId::HighContrastDark,
        }.tokens();
        let c = theme.color;
        let mut grid_minor = c.workspace_text;
        grid_minor.a = if theme.high_contrast { 0.18 } else { 0.07 };
        let mut grid_major = c.workspace_text;
        grid_major.a = if theme.high_contrast { 0.36 } else { 0.18 };
        Palette {
            bg: c.frame, surface: c.panel, docbar: c.docbar, border: c.border,
            text: c.text, text_secondary: c.text_secondary, text_muted: c.text_disabled,
            accent: c.accent, accent_hover: c.accent_hover, accent_text: c.text_on_accent,
            hover: c.hover, active: c.selected, group_label: c.text_secondary,
            status_bg: c.statusbar, status_text: c.statusbar_text,
            viewport: c.workspace, grid_minor, grid_major, dark: theme.dark,
        }
    }

    /// The iced theme (built-in widgets: scrollbars, checkboxes, ...).
    pub fn theme(self) -> Theme {
        let p = self.palette();
        Theme::custom(
            format!("OpenAEC {}", self.label()),
            iced::theme::Palette {
                background: p.bg,
                text: p.text,
                primary: p.accent,
                success: hex(0x16A34A),
                warning: hex(0xF59E0B),
                danger: hex(0xDC2626),
            },
        )
    }
}

/// Colours used by the custom styles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub docbar: Color,
    pub border: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_text: Color,
    pub hover: Color,
    pub active: Color,
    pub group_label: Color,
    pub status_bg: Color,
    pub status_text: Color,
    pub viewport: Color,
    pub grid_minor: Color,
    pub grid_major: Color,
    pub dark: bool,
}

pub const fn hex(v: u32) -> Color {
    Color {
        r: ((v >> 16) & 0xFF) as f32 / 255.0,
        g: ((v >> 8) & 0xFF) as f32 / 255.0,
        b: (v & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

impl Palette {
    pub fn ribbon(&self) -> container::Style {
        container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.surface)),
            border: Border { color: self.border, width: 1.0, radius: 0.0.into() },
            ..Default::default()
        }
    }

    pub fn title_bar(&self) -> container::Style {
        container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.docbar)),
            ..Default::default()
        }
    }

    pub fn panel(&self) -> container::Style {
        container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.surface)),
            border: Border { color: self.border, width: 1.0, radius: 0.0.into() },
            ..Default::default()
        }
    }

    pub fn panel_header(&self) -> container::Style {
        container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(self.docbar)),
            ..Default::default()
        }
    }

    pub fn status_bar(&self) -> container::Style {
        container::Style {
            text_color: Some(self.status_text),
            background: Some(Background::Color(self.status_bg)),
            ..Default::default()
        }
    }

    pub fn swatch(color: Color) -> container::Style {
        container::Style {
            background: Some(Background::Color(color)),
            border: Border { color: Color { a: 0.35, ..Color::BLACK }, width: 1.0, radius: 2.0.into() },
            ..Default::default()
        }
    }

    pub fn overlay(&self) -> container::Style {
        container::Style {
            text_color: Some(self.text),
            background: Some(Background::Color(Color { a: 0.85, ..self.surface })),
            border: Border { color: self.border, width: 1.0, radius: 6.0.into() },
            shadow: Shadow { color: Color { a: 0.15, ..Color::BLACK }, offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
            ..Default::default()
        }
    }

    /// Ribbon button; `active` = toggled on.
    pub fn ribbon_button(&self, active: bool, status: button::Status) -> button::Style {
        let (bg, border_color, text) = match (active, status) {
            (_, button::Status::Disabled) => (Color::TRANSPARENT, Color::TRANSPARENT, self.text_muted),
            (true, button::Status::Hovered | button::Status::Pressed) => (self.active, self.accent, self.accent),
            (true, _) => (self.active, Color { a: 0.35, ..self.accent }, self.accent),
            (false, button::Status::Hovered) => (self.hover, Color { a: 0.25, ..self.accent }, self.accent),
            (false, button::Status::Pressed) => (self.active, Color { a: 0.35, ..self.accent }, self.accent),
            (false, button::Status::Active) => (Color::TRANSPARENT, Color::TRANSPARENT, self.text),
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border { color: border_color, width: 1.0, radius: 4.0.into() },
            shadow: Shadow::default(),
            snap: true,
        }
    }

    /// Primary (filled accent) button.
    pub fn primary_button(&self, status: button::Status) -> button::Style {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => self.accent_hover,
            _ => self.accent,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: self.accent_text,
            border: border::rounded(4),
            shadow: Shadow::default(),
            snap: true,
        }
    }

    /// Flat list-row / icon button.
    pub fn row_button(&self, status: button::Status) -> button::Style {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => self.hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: self.text,
            border: border::rounded(3),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn srgb3(c: Color) -> [f32; 3] {
    [c.r, c.g, c.b]
}

pub fn srgb4(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}
