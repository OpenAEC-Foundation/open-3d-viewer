use crate::tokens::{self, Tokens};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Forge,
    OpenAec,
    Blueprint,
    Contrast,
}
impl Theme {
    pub const ALL: [Self; 5] = [Self::Light, Self::Forge, Self::OpenAec, Self::Blueprint, Self::Contrast];
    pub fn design(self) -> crate::design::tokens::ThemeId {
        use crate::design::tokens::ThemeId;
        match self {
            Self::Light => ThemeId::Light,
            Self::Forge | Self::OpenAec => ThemeId::Dark,
            Self::Blueprint => ThemeId::Blueprint,
            Self::Contrast => ThemeId::HighContrastDark,
        }
    }
    pub fn swatches(self) -> [iced::Color; 4] {
        let c = self.design().tokens().color;
        [c.frame, c.panel, c.accent, c.text]
    }
    pub fn tokens(self) -> Tokens {
        let mut t = match self { Self::Light => tokens::LIGHT, Self::Forge => tokens::FORGE, Self::OpenAec => tokens::OPENAEC, Self::Blueprint => tokens::BLUEPRINT, Self::Contrast => tokens::CONTRAST };
        let c = self.design().tokens().color;
        t.bg = c.frame;
        t.bg_lighter = c.panel;
        t.surface = c.panel;
        t.content_bg = c.workspace;
        t.docbar_bg = c.docbar;
        t.text = c.text;
        t.text_secondary = c.text_secondary;
        t.text_muted = c.text_secondary;
        t.text_faint = c.text_disabled;
        t.border = c.border;
        t.border_subtle = c.border;
        t.accent = c.accent;
        t.accent_hover = c.accent_hover;
        t.accent_soft = c.selected;
        t.accent_tint = c.accent_text;
        t.accent_text = c.text_on_accent;
        t.active = c.selected;
        t.hover = c.hover;
        t.hover_strong = c.pressed;
        t.focus_color = c.focus;
        t.danger_color = c.error;
        t.danger_hover = c.error;
        t.file_tab_bg = c.accent;
        t.file_tab_hover = c.accent_hover;
        t.file_tab_text = c.text_on_accent;
        t.ribbon_btn_active_bg = c.selected;
        t.ribbon_btn_active_border = c.selected_bar;
        t.ribbon_btn_active_text = c.accent_text;
        t.ribbon_btn_hover = c.hover;
        t.ribbon_btn_hover_border = c.border;
        t.ribbon_icon_active = c.accent_text;
        t.ribbon_text_hover = c.text;
        t.ribbon_group_label = c.text_secondary;
        t.ribbon_group_separator = c.border;
        t.status_bg = c.statusbar;
        t.status_border = c.border;
        t.status_text = c.statusbar_text;
        t.status_text_label = c.statusbar_text;
        t.status_hover = c.hover;
        t.status_separator = c.border;
        t.btn_primary_bg = c.accent;
        t.btn_primary_border = c.accent;
        t.btn_primary_text = c.text_on_accent;
        t.btn_primary_hover_bg = c.accent_hover;
        t.btn_primary_hover_text = c.text_on_accent;
        t.btn_secondary_bg = c.raised;
        t.btn_secondary_text = c.text;
        t.btn_secondary_hover_bg = c.hover;
        t.btn_secondary_hover_border = c.border_strong;
        t.dialog_bg = c.panel;
        t.dialog_border = c.border;
        t.dialog_btn_bg = c.accent;
        t.dialog_btn_hover = c.accent_hover;
        t.dialog_btn_text = c.text_on_accent;
        t.dialog_content_bg = c.panel;
        t.dialog_content_text = c.text;
        t.dialog_content_secondary = c.text_secondary;
        t.dialog_header_bg = c.frame;
        t.dialog_header_text = c.text;
        t.dialog_input_bg = c.raised;
        t.dialog_input_border = c.border;
        t.dialog_input_text = c.text;
        t.dialog_sidebar_bg = c.frame;
        t.dialog_sidebar_border = c.border;
        t.dialog_tab_active_accent = c.selected_bar;
        t.dialog_tab_active_bg = c.selected;
        t.dialog_tab_active_text = c.accent_text;
        t.dialog_tab_hover = c.hover;
        t.dialog_tab_hover_text = c.text;
        t.dialog_tab_text = c.text;
        t.dialog_section_border = c.border;
        t.backstage_item_shortcut = c.text_secondary;
        t.backstage_item_shortcut_hover = c.accent_text;
        t.placeholder_heading = c.text_secondary;
        t.placeholder_text = c.text_secondary;
        t.placeholder_icon = c.accent;
        t
    }
    /// Brand strong-border token from the pinned Style Book colors.json (light/dark).
    pub fn border_strong(self) -> iced::Color { self.design().tokens().color.border_strong }
    /// Modal.css and themes.css at dfdcd41: 0 4px 16px with a theme-specific color.
    pub fn dialog_shadow(self) -> iced::Shadow {
        let color = match self {
            Self::Light => iced::Color::from_rgba8(0, 0, 0, 0.12),
            Self::Forge => iced::Color::from_rgba8(0, 0, 0, 0.35),
            Self::OpenAec => iced::Color::from_rgba8(0, 0, 0, 0.40),
            Self::Blueprint => iced::Color::from_rgba8(0, 0, 0, 0.45),
            Self::Contrast => iced::Color::from_rgba8(255, 215, 0, 0.40),
        };
        iced::Shadow { color, offset: iced::Vector::new(0.0, 4.0), blur_radius: 16.0 }
    }
    pub fn iced(self) -> iced::Theme {
        let t = self.tokens();
        iced::Theme::custom(self.to_string(), iced::theme::Palette {
            background: t.bg, text: t.text, primary: t.accent,
            success: iced::Color::from_rgb8(22, 163, 74), warning: iced::Color::from_rgb8(217, 119, 6), danger: t.danger_color,
        })
    }
}
impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Light => "Light", Self::Forge => "Forge", Self::OpenAec => "OpenAEC", Self::Blueprint => "Blueprint", Self::Contrast => "High contrast" })
    }
}
