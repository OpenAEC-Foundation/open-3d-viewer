//! View templates in the Library product: what a drawing of a kind shows in a phase
//! (`bim_library::template`).
//!
//! The page shows the view templates of the source as tiles, each with its phase. The right
//! pane shows the picked one: its phase, its kind of drawing and its scale, whether dimensions
//! and levels are written, the drawing set that annotates it, its rules in the order they are
//! tried, and how a few sample elements are drawn by it.
//!
//! Not yet: changing a view template here, a sample building drawn as the template would draw
//! it, and a view of the Modeller that follows one.

use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::annotate::Described;
use bim_library::template::{Detail, Look};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, text, Column};
use iced::Element;
use openaec_ui::Language;

#[derive(Debug, Clone)]
pub enum ViewTemplateMessage {
    /// Show this view template of the source shown.
    Pick(String),
}

/// Which view template the right pane shows.
#[derive(Debug, Clone, Default)]
pub struct ViewTemplateView {
    pub picked: Option<String>,
}

/// The classes a pane shows the look of: what a Dutch plan has, and what a phase adds.
const SAMPLES: [&str; 8] = ["IfcWall", "IfcSlab", "IfcDoor", "IfcSpace", "IfcMember", "IfcFurniture", "IfcDuctSegment", "IfcGridAxis"];

fn detail_name(detail: Detail, l: Language) -> &'static str {
    match detail {
        Detail::Outline => l.text("omtrek", "outline"),
        Detail::Coarse => l.text("één arcering", "one hatch"),
        Detail::Layers => l.text("lagen", "layers"),
        Detail::Full => l.text("volledig", "all of it"),
    }
}

/// A look in a line: hidden, or its detail, whether it is hatched, its pens and whether it is
/// faint; what it leaves open is not said.
pub fn look_line(look: &Look, l: Language) -> String {
    if look.hidden {
        return l.text("niet getoond", "not shown").to_string();
    }
    let mut parts: Vec<String> = look.detail.iter().map(|detail| detail_name(*detail, l).to_string()).collect();
    parts.extend(look.hatched.map(|hatched| if hatched { l.text("gearceerd", "hatched") } else { l.text("niet gearceerd", "not hatched") }.to_string()));
    parts.extend(look.cut.iter().map(|pen| format!("{} {pen}", l.text("pen gesneden", "cut pen"))));
    parts.extend(look.beyond.iter().map(|pen| format!("{} {pen}", l.text("pen aanzicht", "seen pen"))));
    if look.faint {
        parts.push(l.text("grijs", "faint").to_string());
    }
    if parts.is_empty() {
        l.text("zoals de tekeningsoort", "as the kind of drawing").to_string()
    } else {
        parts.join(" · ")
    }
}

impl App {
    pub(super) fn view_template_update(&mut self, message: ViewTemplateMessage) {
        match message {
            ViewTemplateMessage::Pick(key) => self.view_template_view.picked = Some(key),
        }
    }

    /// The view templates of the source shown as tiles: the phase as its look, the name, and
    /// its scale with how many rules it has.
    pub(super) fn view_template_tiles(&self, p: Palette, l: Language, library: &bim_library::Library) -> Vec<Element<'_, Message>> {
        let lang = super::library::lang(l);
        library
            .view_templates
            .iter()
            .map(|template| {
                let look = container(text(template.phase.code()).size(26).color(p.text)).width(iced::Fill).align_x(iced::Center);
                let about = format!("{} · 1:{}", template.phase.name(lang), template.scale);
                let on = self.view_template_view.picked.as_deref() == Some(template.key.as_str());
                super::libraries::tile(p, look.into(), template.name(lang).to_string(), about, on, Some(Message::ViewTemplate(ViewTemplateMessage::Pick(template.key.clone()))))
            })
            .collect()
    }

    /// The right pane for the view templates: the picked one.
    pub fn view_template_pane<'a>(&'a self, p: Palette, l: Language, library: &'a bim_library::Library) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let Some(template) = self.view_template_view.picked.as_deref().and_then(|key| library.view_template(key)) else {
            let hint = l.text("Klik een weergavesjabloon om te zien wat het toont.", "Click a view template to see what it shows.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let caption = |label: &'static str| text(label).size(11).color(p.text_muted);
        let line = |content: String| text(content).size(11).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph);
        let named = |content: String| text(content).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);
        let yes_no = |yes: bool| if yes { l.text("ja", "yes") } else { l.text("nee", "no") };
        let kind = self.standard.kinds.iter().find(|k| k.key == template.kind).map_or(template.kind.clone(), |k| k.names.get(lang).to_string());
        let mut pane: Column<'_, Message> = column![
            text(template.name(lang).to_string()).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            line(format!("{}: {} ({})", l.text("Fase", "Phase"), template.phase.name(lang), template.phase.code())),
            line(format!("{}: {kind} · 1:{}", l.text("Tekeningsoort", "Kind of drawing"), template.scale)),
            line(format!("{}: {} · {}: {}", l.text("Maten", "Dimensions"), yes_no(template.dimensions), l.text("Peilen", "Levels"), yes_no(template.levels))),
        ]
        .spacing(4);
        if let Some(set) = template.drawing_set.as_deref() {
            let name = library.drawing_set(set).map_or(set, |of| of.name(lang));
            pane = pane.push(line(format!("{}: {name}", l.text("Tekeningset", "Drawing set"))));
        }
        pane = pane.push(caption(l.text("Regels, in volgorde", "Rules, in order")));
        for (k, rule) in template.rules.iter().enumerate() {
            pane = pane.push(named(format!("{}. {}", k + 1, rule.names.get(lang)))).push(line(format!("{} → {}", rule.filter.classes.join(", "), look_line(&rule.look, l))));
        }
        pane = pane.push(named(l.text("Al het andere", "Everything else").to_string())).push(line(look_line(&template.otherwise, l)));
        pane = pane.push(caption(l.text("Voorbeeld: hoe elementen getekend worden", "Preview: how elements are drawn")));
        for class in SAMPLES {
            let look = template.look(&Described { class: class.into(), ..Described::default() });
            pane = pane.push(line(format!("{class}: {}", look_line(&look, l))));
        }
        container(pane).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_is_read_in_a_line() {
        let l = Language::English;
        let library = bim_library::Library::starter();
        let of = |key: &str, class: &str| look_line(&library.view_template(key).expect(key).look(&Described { class: class.into(), ..Described::default() }), l);
        assert_eq!(of("floor-plan-so-200", "IfcWall"), "outline · not hatched");
        assert_eq!(of("floor-plan-do-100", "IfcWall"), "one hatch · hatched");
        assert_eq!(of("floor-plan-uo-50", "IfcWall"), "layers · hatched");
        assert_eq!((of("floor-plan-vo-200", "IfcFurniture"), of("floor-plan-do-100", "IfcFurniture")), ("not shown".to_string(), "one hatch · hatched · faint".to_string()));
        assert_eq!(of("floor-plan-uo-50", "IfcDoor"), "all of it · not hatched");
        assert_eq!(look_line(&Look::default(), l), "as the kind of drawing");
        assert_eq!(look_line(&Look { cut: Some("0.7".into()), ..Look::default() }, Language::Dutch), "pen gesneden 0.7");
    }

    #[test]
    fn the_page_shows_the_view_templates_and_the_pane_the_picked_one() {
        let mut app = App::default();
        let p = app.theme.palette();
        assert_eq!(app.view_template_tiles(p, Language::English, &app.library).len(), 20);
        app.view_template_update(ViewTemplateMessage::Pick("floor-plan-ov-100".into()));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1400.0), app.view_template_pane(p, Language::English, &app.library));
        assert!(pane.find("floor plan OV 1:100").is_ok() && pane.find("Phase: building permit (OV)").is_ok() && pane.find("Dimensions: yes · Levels: yes").is_ok());
        assert!(pane.find("Drawing set: architectural floor plan 1:100").is_ok() && pane.find("Preview: how elements are drawn").is_ok());
        assert!(pane.find("IfcDuctSegment: not shown").is_ok() && pane.find("IfcWall: one hatch · hatched").is_ok());
    }
}
