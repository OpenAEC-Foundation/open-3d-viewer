//! Drawing sets in the Library product: the rules by which a drawing of a kind is annotated
//! (`bim_library::annotate`).
//!
//! The page shows the drawing sets of the source as tiles. The right pane shows the picked one:
//! its label types, each with what it reads, its rules in the order they are tried, and a
//! preview: what a few sample elements (an external and an internal wall, a room, a door, a
//! beam, a column, a floor) get from it, so the rules are read without a view.
//!
//! Not yet: changing a drawing set here, and the Modeller applying one to a view.

use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::annotate::{Described, DrawingSet, Frame, Gives, LabelField, LabelType, Measure, Rule};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, text, Column};
use iced::Element;
use openaec_ui::Language;

#[derive(Debug, Clone)]
pub enum DrawingSetMessage {
    /// Show this drawing set of the source shown.
    Pick(String),
}

/// Which drawing set the right pane shows.
#[derive(Debug, Clone, Default)]
pub struct DrawingSetView {
    pub picked: Option<String>,
}

/// The elements a preview annotates: one of each kind a Dutch plan has.
pub fn samples() -> Vec<Described> {
    let of = |class: &str, name: &str, type_name: &str, code: &str, props: &[(&str, &str)]| Described { class: class.into(), name: name.into(), type_name: type_name.into(), code: code.into(), props: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() };
    vec![
        of("IfcWall", "", "Metselwerk-spouw-isolatie-kalkzandsteen 380", "21", &[("Thickness", "0.380 m"), ("Length", "5.400 m"), ("LoadBearing", "true")]),
        of("IfcWall", "", "Kalkzandsteen 100", "22", &[("Thickness", "0.100 m"), ("Length", "3.600 m")]),
        of("IfcSpace", "Woonkamer", "", "", &[("Area", "32.5 m²")]),
        of("IfcDoor", "D01", "Binnendeur 930x2315", "32", &[]),
        of("IfcBeam", "L1", "HEA 200", "28", &[("Profile", "HEA 200"), ("Length", "5.400 m")]),
        of("IfcColumn", "K1", "HEB 200", "28", &[("Profile", "HEB 200")]),
        of("IfcSlab", "", "Kanaalplaat 200", "23", &[("Thickness", "0.200 m")]),
    ]
}

fn field_name(field: &LabelField, l: Language) -> String {
    match field {
        LabelField::Name => l.text("naam", "name").into(),
        LabelField::Type => l.text("type", "type").into(),
        LabelField::Code => "NL-SfB".into(),
        LabelField::Material => l.text("materiaal", "material").into(),
        LabelField::Thickness => l.text("dikte", "thickness").into(),
        LabelField::Profile => l.text("profiel", "profile").into(),
        LabelField::Length => l.text("lengte", "length").into(),
        LabelField::Area => l.text("oppervlakte", "area").into(),
        LabelField::Level => l.text("peil", "level").into(),
        LabelField::Property(key) => key.clone(),
        LabelField::Text(fixed) => format!("\"{fixed}\""),
    }
}

fn frame_name(frame: Frame, l: Language) -> &'static str {
    match frame {
        Frame::None => l.text("geen kader", "no frame"),
        Frame::Rectangle => l.text("rechthoek", "rectangle"),
        Frame::Rounded => l.text("afgeronde rechthoek", "rounded rectangle"),
        Frame::Circle => l.text("cirkel", "circle"),
        Frame::Hexagon => l.text("zeshoek", "hexagon"),
    }
}

fn measure_name(measure: Measure, l: Language) -> &'static str {
    match measure {
        Measure::Thickness => l.text("dikte", "thickness"),
        Measure::Length => l.text("lengte", "length"),
        Measure::Openings => l.text("openingen", "openings"),
        Measure::Grids => l.text("maten op stramien", "dimensions to the grids"),
    }
}

/// A label type in a line: what it reads, its text style and its frame.
pub fn label_line(label: &LabelType, l: Language) -> String {
    let fields: Vec<String> = label.fields.iter().map(|f| field_name(f, l)).collect();
    let leader = if label.leader { l.text(" · aanhaallijn", " · leader") } else { "" };
    format!("{} · {} mm · {}{leader}", fields.join(" + "), label.text_style, frame_name(label.frame, l))
}

/// A rule in a line: which elements it takes and what they get.
pub fn rule_line(set: &DrawingSet, rule: &Rule, lang: bim_library::Lang, l: Language) -> String {
    let mut takes: Vec<String> = rule.filter.classes.clone();
    takes.extend(rule.filter.codes.iter().map(|code| format!("NL-SfB {code}")));
    takes.extend(rule.filter.type_contains.iter().map(|part| format!("{} \"{part}\"", l.text("type met", "type with"))));
    takes.extend(rule.filter.property.iter().map(|(key, value)| if value.is_empty() { key.clone() } else { format!("{key} = {value}") }));
    let takes = if takes.is_empty() { l.text("elk element", "every element").to_string() } else { takes.join(", ") };
    let gives = match &rule.gives {
        Gives::Label { label } => format!("{}: {}", l.text("label", "label"), set.label(label).map_or(label.as_str(), |of| of.names.get(lang))),
        Gives::Dimension { measure, chain } => format!("{}: {}{}", l.text("maat", "dimension"), measure_name(*measure, l), if *chain { l.text(", als keten", ", as a chain") } else { "" }),
    };
    format!("{takes} → {gives}")
}

/// What a sample element gets from `set`, in a line; none when it gets nothing.
pub fn sample_line(set: &DrawingSet, element: &Described, l: Language) -> Option<String> {
    let got = set.annotate(element);
    let mut parts: Vec<String> = got.label.iter().map(|(_, text)| format!("\"{text}\"")).collect();
    parts.extend(got.dimensions.iter().map(|(_, measure, _)| measure_name(*measure, l).to_string()));
    let what = if element.name.is_empty() { element.type_name.as_str() } else { element.name.as_str() };
    (!parts.is_empty()).then(|| format!("{} {what}: {}", element.class, parts.join(" · ")))
}

impl App {
    pub(super) fn drawing_set_update(&mut self, message: DrawingSetMessage) {
        match message {
            DrawingSetMessage::Pick(key) => self.drawing_set_view.picked = Some(key),
        }
    }

    /// The drawing sets of the source shown as tiles: its name, and how many label types and
    /// rules it has.
    pub(super) fn drawing_set_tiles(&self, p: Palette, l: Language, library: &bim_library::Library) -> Vec<Element<'_, Message>> {
        let lang = super::library::lang(l);
        library
            .drawing_sets
            .iter()
            .map(|set| {
                let scale = set.scale.map_or(String::new(), |scale| format!("1:{scale}"));
                let look = container(text(scale).size(22).color(p.text)).width(iced::Fill).align_x(iced::Center);
                let about = format!("{}: {} · {}: {}", l.text("labels", "labels"), set.labels.len(), l.text("regels", "rules"), set.rules.len());
                let on = self.drawing_set_view.picked.as_deref() == Some(set.key.as_str());
                super::libraries::tile(p, look.into(), set.name(lang).to_string(), about, on, Some(Message::DrawingSet(DrawingSetMessage::Pick(set.key.clone()))))
            })
            .collect()
    }

    /// The right pane for the drawing sets: the picked one's label types, its rules and a
    /// preview on sample elements.
    pub fn drawing_set_pane<'a>(&'a self, p: Palette, l: Language, library: &'a bim_library::Library) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let Some(set) = self.drawing_set_view.picked.as_deref().and_then(|key| library.drawing_set(key)) else {
            let hint = l.text("Klik een tekeningset om zijn labels en regels te zien.", "Click a drawing set to see its labels and rules.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let caption = |label: &'static str| text(label).size(11).color(p.text_muted);
        let line = |content: String| text(content).size(11).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph);
        let named = |content: String| text(content).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);
        let kind = self.standard.kinds.iter().find(|k| k.key == set.kind).map_or(set.kind.clone(), |k| k.names.get(lang).to_string());
        let mut pane: Column<'_, Message> = column![
            text(set.name(lang).to_string()).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            line(format!("{}: {kind}{}", l.text("Tekeningsoort", "Kind of drawing"), set.scale.map_or(String::new(), |scale| format!(" · 1:{scale}")))),
        ]
        .spacing(4);
        pane = pane.push(caption(l.text("Labeltypen", "Label types")));
        for label in &set.labels {
            pane = pane.push(named(label.names.get(lang).to_string())).push(line(label_line(label, l)));
        }
        pane = pane.push(caption(l.text("Regels, in volgorde", "Rules, in order")));
        for (k, rule) in set.rules.iter().enumerate() {
            pane = pane.push(named(format!("{}. {}", k + 1, rule.names.get(lang)))).push(line(rule_line(set, rule, lang, l)));
        }
        pane = pane.push(caption(l.text("Voorbeeld: wat elementen krijgen", "Preview: what elements get")));
        let samples = samples();
        let got: Vec<String> = samples.iter().filter_map(|element| sample_line(set, element, l)).collect();
        for sample in got {
            pane = pane.push(line(sample));
        }
        container(pane).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::Lang;

    #[test]
    fn a_drawing_set_is_read_in_lines() {
        let library = bim_library::Library::starter();
        let l = Language::English;
        let plan = library.drawing_set("architectural-plan-50").expect("the plan at 1:50");
        assert_eq!(label_line(plan.label("wall-type").unwrap(), l), "type · 2.5 mm · no frame · leader");
        assert_eq!(label_line(plan.label("opening").unwrap(), l), "name · 2.5 mm · circle");
        let rule = |key: &str| rule_line(plan, plan.rules.iter().find(|r| r.key == key).unwrap(), Lang::En, l);
        assert_eq!(rule("wall-types"), "IfcWall → label: wall type");
        assert_eq!(rule("external-walls"), "IfcWall, NL-SfB 21 → dimension: openings, as a chain");
        assert_eq!(rule("wall-lengths"), "IfcWall → dimension: length");
        // a preview: what a sample element gets
        let samples = samples();
        let lines: Vec<String> = samples.iter().filter_map(|e| sample_line(plan, e, l)).collect();
        assert_eq!(lines[0], "IfcWall Metselwerk-spouw-isolatie-kalkzandsteen 380: \"Metselwerk-spouw-isolatie-kalkzandsteen 380\" · thickness · openings · length");
        assert!(lines.contains(&"IfcSpace Woonkamer: \"Woonkamer 32.5 m²\"".to_string()) && lines.contains(&"IfcDoor D01: \"D01\"".to_string()));
        // a beam gets nothing on an architectural plan, and its mark and profile on a structural one
        assert!(lines.iter().all(|line| !line.starts_with("IfcBeam")));
        let structural = library.drawing_set("structural-plan-100").unwrap();
        assert_eq!(sample_line(structural, &samples[4], l).as_deref(), Some("IfcBeam L1: \"L1 HEA 200\""));
        let structural_rule = rule_line(structural, structural.rules.iter().find(|r| r.key == "bearing-walls").unwrap(), Lang::En, l);
        assert_eq!(structural_rule, "IfcWall, LoadBearing = true → dimension: thickness");
    }

    #[test]
    fn the_page_shows_the_drawing_sets_and_the_pane_the_picked_one() {
        let mut app = App::default();
        let p = app.theme.palette();
        assert_eq!(app.drawing_set_tiles(p, Language::English, &app.library).len(), 4);
        app.drawing_set_update(DrawingSetMessage::Pick("structural-plan-100".into()));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1200.0), app.drawing_set_pane(p, Language::English, &app.library));
        assert!(pane.find("structural plan 1:100").is_ok() && pane.find("Label types").is_ok() && pane.find("Rules, in order").is_ok() && pane.find("Preview: what elements get").is_ok());
        assert!(pane.find("IfcBeam L1: \"L1 HEA 200\"").is_ok());
    }
}
