//! The website style of open-aec.com, for the OpenAEC Installer (issue #13).
//!
//! The installer is the hub app (`design/ontwerp-0.4/05-layouts.md`, proposal E):
//! a dark top band like the website, and below it cards, filter pills and
//! buttons in the website's style. Everything here uses the tokens of 0.4 and
//! keeps its contrast in every theme; in High contrast gradients, glows and
//! glass give way to plain surfaces and 2 px borders.
//!
//! The primary button keeps dark text on amber, as decided for contrast; the
//! website's white text on amber does not reach 4.5 : 1.
use super::a11y::{self, Node, Role};
use super::focus;
use super::widgets::{self as w, Tone};
use super::{font, mix, Look, ThemeId};
use crate::fonts;
use iced::widget::{self, column, container, row, text, tooltip, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Shadow, Vector};

/// Space Grotesk bold, as the headings and tile names on open-aec.com.
pub const HEADING_BOLD: Font = Font { weight: iced::font::Weight::Bold, ..fonts::HEADING };

/// A linear gradient with stops, top to bottom.
pub fn gradient(stops: &[(f32, Color)]) -> Background {
    let mut g = iced::gradient::Linear::new(iced::Degrees(180.0));
    for &(offset, color) in stops {
        g = g.add_stop(offset, color);
    }
    Background::Gradient(iced::Gradient::Linear(g))
}

fn vertical(top: Color, bottom: Color) -> Background {
    gradient(&[(0.0, top), (1.0, bottom)])
}

/// The colour with another opacity.
pub fn alpha(color: Color, a: f32) -> Color {
    Color { a, ..color }
}

/// Shadow colour of the website's deep soft shadows.
fn soft_shadow(look: Look, a: f32) -> Color {
    if look.tokens.dark { Color::from_rgba(0.0, 0.0, 0.0, (a * 2.2).min(0.8)) } else { Color::from_rgba8(42, 42, 50, a) }
}

/// The look for the dark top band: Donker in every light theme, the theme
/// itself when it is dark or High contrast.
pub fn band_look(look: Look) -> Look {
    if look.tokens.dark || look.high_contrast() { look } else { Look::from(ThemeId::Dark) }
}

/// The website's dark top band: from the panel colour with a soft light at the
/// top to the frame colour. Use it with [`band_look`].
pub fn dark_band_style(dark: Look) -> container::Style {
    let c = dark.color;
    if dark.high_contrast() {
        return container::Style { text_color: Some(c.text), background: Some(Background::Color(c.panel)), ..Default::default() };
    }
    container::Style { text_color: Some(c.text), background: Some(gradient(&[(0.0, mix(Color::WHITE, 0.06, c.panel)), (0.7, c.frame), (1.0, c.frame)])), ..Default::default() }
}

/// The light tile of open-aec.com: a light gradient with a light line on top,
/// 12 px corners, a fine border (2 px amber when featured) and a deep soft shadow.
pub fn card_style(look: Look, featured: bool) -> container::Style {
    let c = look.color;
    let hc = look.high_contrast();
    let shine = mix(Color::WHITE, 0.5, c.raised);
    container::Style {
        snap: false,
        text_color: Some(c.text),
        background: Some(if hc { Background::Color(c.raised) } else { gradient(&[(0.0, shine), (0.006, c.raised), (1.0, mix(c.frame, 0.3, c.raised))]) }),
        border: Border { color: if featured { c.accent } else { c.border }, width: if featured { 2.0 } else { 1.0 }, radius: 12.0.into() },
        shadow: if hc { Shadow::default() } else { Shadow { color: soft_shadow(look, 0.2), offset: Vector::new(0.0, 14.0), blur_radius: 30.0 } },
    }
}

/// The glass tile in the dark band: white 7 % to 2.4 % with a light line on
/// top, a white border and a deep shadow.
pub fn glass_card_style(look: Look, featured: bool) -> container::Style {
    let c = look.color;
    if look.high_contrast() {
        return container::Style { text_color: Some(c.text), background: Some(Background::Color(c.raised)), border: Border { color: if featured { c.accent } else { c.border }, width: 2.0, radius: 12.0.into() }, ..Default::default() };
    }
    container::Style {
        snap: false,
        text_color: Some(c.text),
        background: Some(gradient(&[(0.0, alpha(Color::WHITE, 0.16)), (0.006, alpha(Color::WHITE, 0.07)), (1.0, alpha(Color::WHITE, 0.024))])),
        border: Border { color: if featured { c.accent } else { alpha(Color::WHITE, 0.09) }, width: if featured { 2.0 } else { 1.0 }, radius: 12.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.5), offset: Vector::new(0.0, 16.0), blur_radius: 34.0 },
    }
}

/// A smaller glass surface, such as the figures in the band.
pub fn glass_style(look: Look) -> container::Style {
    let mut s = glass_card_style(look, false);
    s.shadow = Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.3), offset: Vector::new(0.0, 8.0), blur_radius: 20.0 };
    s
}

/// The kind of website button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Amber gradient with a light line and an amber glow.
    Primary,
    /// The light button: raised, fine border, small shadow.
    Light,
    /// The glass button in the dark band: white 6 %, a white 14 % border.
    Glass,
}

/// A button as on open-aec.com: 8 px corners, 14 px semibold, an icon on the left.
pub fn button<'a, M: Clone + 'a>(look: Look, label: impl Into<String>, kind: Kind, icon: Option<&'static str>, on_press: Option<M>) -> Element<'a, M> {
    site_button(look, label.into(), kind, icon, on_press, false)
}

/// The button in a tile: 7 × 14 px padding and 13 px text.
pub fn small_button<'a, M: Clone + 'a>(look: Look, label: impl Into<String>, kind: Kind, icon: Option<&'static str>, on_press: Option<M>) -> Element<'a, M> {
    site_button(look, label.into(), kind, icon, on_press, true)
}

fn site_button<'a, M: Clone + 'a>(look: Look, label: String, kind: Kind, icon: Option<&'static str>, on_press: Option<M>, small: bool) -> Element<'a, M> {
    let c = look.color;
    let hc = look.high_contrast();
    let disabled = on_press.is_none();
    let fg = if disabled { c.text_disabled } else if kind == Kind::Primary { c.text_on_accent } else { c.text };
    let mut content = row![].spacing(8).align_y(Alignment::Center);
    if let Some(source) = icon {
        content = content.push(w::glyph(source, 16.0, fg, if kind == Kind::Primary || disabled { fg } else { c.accent_text }));
    }
    let node = Node::new(Role::Button, label.clone()).disabled(disabled);
    content = content.push(text(label).size(if small { 13 } else { 14 }).font(fonts::STRONG).color(fg));
    let control = widget::button(content).padding(if small { [7, 14] } else { [10, 20] }).on_press_maybe(on_press).style(move |_, status| {
        use widget::button::Status as S;
        let hovered = status == S::Hovered;
        let pressed = status == S::Pressed;
        let radius: iced::border::Radius = 8.0.into();
        match kind {
            Kind::Primary if !disabled => {
                let shine = mix(Color::WHITE, 0.35, c.accent_hover);
                let (top, bottom) = if pressed { (c.accent_pressed, c.accent_pressed) } else if hovered { (c.accent_hover, c.accent) } else { (mix(c.accent_hover, 0.6, c.accent), c.accent) };
                widget::button::Style {
                    snap: false,
                    background: Some(if hc { Background::Color(c.accent) } else { gradient(&[(0.0, shine), (0.04, top), (1.0, bottom)]) }),
                    text_color: fg,
                    border: Border { color: if hc && hovered { c.text } else { Color::TRANSPARENT }, width: if hc { 2.0 } else { 0.0 }, radius },
                    shadow: if hc || pressed { Shadow::default() } else { Shadow { color: alpha(c.accent, if hovered { 0.6 } else { 0.45 }), offset: Vector::new(0.0, 7.0), blur_radius: 20.0 } },
                }
            }
            Kind::Glass if !hc => widget::button::Style {
                snap: false,
                background: Some(Background::Color(alpha(Color::WHITE, if pressed { 0.03 } else if hovered { 0.11 } else { 0.06 }))),
                text_color: fg,
                border: Border { color: if hovered { c.accent } else { alpha(Color::WHITE, 0.14) }, width: 1.0, radius },
                shadow: Shadow::default(),
            },
            _ => widget::button::Style {
                snap: false,
                background: Some(if disabled { Background::Color(c.panel) } else if pressed { Background::Color(c.pressed) } else if hovered && !hc { Background::Color(c.hover) } else { vertical(c.raised, mix(c.frame, 0.25, c.raised)) }),
                text_color: fg,
                border: Border { color: if hovered { c.accent } else { c.border }, width: if hc && hovered { 2.0 } else { 1.0 }, radius },
                shadow: if hc || disabled { Shadow::default() } else { Shadow { color: soft_shadow(look, 0.08), offset: Vector::new(0.0, 2.0), blur_radius: 4.0 } },
            },
        }
    });
    a11y::node(focus::focusable(look, control, !disabled, 8.0), node)
}

/// A round icon button of 34 px with a tooltip, as the source and browser
/// buttons on the website tiles. The tooltip also shows on keyboard focus.
pub fn round_icon<'a, M: Clone + 'a>(look: Look, icon: &'static str, tip: impl Into<String>, on_press: M) -> Element<'a, M> {
    let c = look.color;
    let tip = tip.into();
    let control = widget::button(container(w::glyph(icon, 16.0, c.text, c.accent_text)).center(Length::Fill)).width(34).height(34).padding(0).on_press(on_press).style(move |_, status| {
        let hovered = status == widget::button::Status::Hovered;
        widget::button::Style {
            snap: false,
            background: Some(Background::Color(if hovered && !look.high_contrast() { c.hover } else { alpha(c.raised, if look.tokens.dark { 0.5 } else { 1.0 }) })),
            text_color: c.text,
            border: Border { color: if hovered { c.accent } else { c.border }, width: if look.high_contrast() && hovered { 2.0 } else { 1.0 }, radius: 17.0.into() },
            shadow: Shadow::default(),
        }
    });
    let control = a11y::node(focus::focusable(look, control, true, 17.0), Node::new(Role::Button, tip.clone()));
    let bubble = |tip: String| container(text(tip).size(font::SIZE_LABEL).font(fonts::BODY).color(c.text)).padding([4, 8]).style(move |_| w::surface(look, look.tokens.shadow.popup, 3.0));
    tooltip(focus::tip(control, bubble(tip.clone())), bubble(tip), tooltip::Position::Bottom).gap(4).into()
}

/// The website's icon tile: an amber gradient at 145°, from 22 % to 6 %.
pub fn icon_tile<'a, M: 'a>(look: Look, icon: &'static str, size: f32) -> Element<'a, M> {
    let c = look.color;
    let hc = look.high_contrast();
    container(w::glyph(icon, (size * 0.55).round(), c.accent_text, c.accent_text))
        .center(size)
        .style(move |_| container::Style {
            background: Some(if hc {
                Background::Color(c.panel)
            } else {
                Background::Gradient(iced::Gradient::Linear(iced::gradient::Linear::new(iced::Degrees(145.0)).add_stop(0.0, alpha(c.accent, 0.22)).add_stop(1.0, alpha(c.accent, 0.06))))
            }),
            border: Border { color: if hc { c.accent } else { alpha(c.accent, 0.25) }, width: 1.0, radius: (size * 0.25).round().into() },
            ..Default::default()
        })
        .into()
}

/// The section label of open-aec.com: mono uppercase on an amber tint, round,
/// with an amber dot.
pub fn section_label<'a, M: 'a>(look: Look, label: &str) -> Element<'a, M> {
    let c = look.color;
    let hc = look.high_contrast();
    let dot = container(Space::new().width(6).height(6)).style(move |_| container::Style { background: Some(Background::Color(c.accent)), border: Border { radius: 3.0.into(), ..Default::default() }, ..Default::default() });
    container(row![dot, text(label.to_uppercase()).size(12).font(fonts::MONO).color(c.accent_text)].spacing(8).align_y(Alignment::Center))
        .padding([7, 14])
        .style(move |_| container::Style {
            background: Some(Background::Color(if hc { c.panel } else { alpha(c.accent, 0.15) })),
            border: Border { color: if hc { c.accent } else { Color::TRANSPARENT }, width: 1.0, radius: 9999.0.into() },
            ..Default::default()
        })
        .into()
}

/// A section head: label, a 26 px heading and an explanation.
pub fn section_head<'a, M: 'a>(look: Look, label: &str, heading: impl text::IntoFragment<'a>, explanation: impl text::IntoFragment<'a>) -> Element<'a, M> {
    let c = look.color;
    column![section_label(look, label), text(heading).size(26).font(HEADING_BOLD).color(c.text), text(explanation).size(15).font(fonts::BODY).color(c.text_secondary)].spacing(10).into()
}

/// A small mono chip, as the discipline letters on the website tiles. With a
/// tone it is a status chip in the status colours.
pub fn chip<'a, M: 'a>(look: Look, label: &str, tone: Option<Tone>) -> Element<'a, M> {
    let c = look.color;
    let (fg, bg, edge) = match tone {
        Some(t) => (t.color(look), if look.high_contrast() { c.panel } else { t.tint(look) }, t.color(look)),
        None => (c.text_secondary, if look.tokens.dark { alpha(Color::WHITE, 0.06) } else { c.hover }, c.border),
    };
    container(text(label.to_uppercase()).size(11).font(fonts::MONO).color(fg))
        .padding([1, 7])
        .style(move |_| container::Style { background: Some(Background::Color(bg)), border: Border { color: edge, width: 1.0, radius: 4.0.into() }, ..Default::default() })
        .into()
}

/// A filter pill with a count, as on open-aec.com: round, 14 px semibold; the
/// active one in the amber gradient with a glow. It reads as a toggle.
pub fn pill<'a, M: Clone + 'a>(look: Look, letter: Option<char>, name: &'a str, count: usize, active: bool, on_press: M) -> Element<'a, M> {
    let c = look.color;
    let hc = look.high_contrast();
    let fg = if active { c.text_on_accent } else { c.text };
    let badge = if active { alpha(c.text_on_accent, 0.12) } else { c.hover };
    let mut content = row![].spacing(8).align_y(Alignment::Center);
    if let Some(l) = letter {
        let edge = if active { alpha(c.text_on_accent, 0.2) } else { c.border };
        content = content.push(
            container(text(l.to_string()).size(10).font(fonts::MONO).color(fg))
                .padding([0, 5])
                .style(move |_| container::Style { background: Some(Background::Color(alpha(fg, 0.06))), border: Border { color: edge, width: 1.0, radius: 4.0.into() }, ..Default::default() }),
        );
    }
    let content = content.push(
        row![
            text(name).size(14).font(fonts::STRONG).color(fg),
            container(text(count.to_string()).size(11).font(fonts::MONO).color(fg))
                .padding([1, 7])
                .style(move |_| container::Style { background: Some(Background::Color(badge)), border: Border { radius: 9999.0.into(), ..Default::default() }, ..Default::default() }),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    let control = widget::button(content).padding([9, 16]).on_press(on_press).style(move |_, status| {
        let hovered = status == widget::button::Status::Hovered;
        widget::button::Style {
            snap: false,
            background: Some(if active && !hc {
                gradient(&[(0.0, mix(Color::WHITE, 0.35, c.accent_hover)), (0.05, mix(c.accent_hover, 0.6, c.accent)), (1.0, c.accent)])
            } else if active {
                Background::Color(c.accent)
            } else if hovered && !hc {
                Background::Color(c.hover)
            } else {
                vertical(c.raised, mix(c.frame, 0.25, c.raised))
            }),
            text_color: fg,
            border: Border { color: if active || hovered { c.accent } else { c.border }, width: if hc && hovered { 2.0 } else { 1.0 }, radius: 9999.0.into() },
            shadow: if hc {
                Shadow::default()
            } else if active {
                Shadow { color: alpha(c.accent, 0.5), offset: Vector::new(0.0, 7.0), blur_radius: 20.0 }
            } else {
                Shadow { color: soft_shadow(look, 0.08), offset: Vector::new(0.0, 2.0), blur_radius: 4.0 }
            },
        }
    });
    a11y::node(focus::focusable(look, control, true, 9999.0), Node::new(Role::ToggleButton, format!("{name}, {count}")).checked(active))
}
