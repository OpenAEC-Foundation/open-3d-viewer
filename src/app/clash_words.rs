//! What a group of clashes is, in the words of the building site, and how soon it asks to be
//! picked up: a duct through a beam before a pipe that grazes a wall. Both follow from the
//! classes of the elements and how far they go into each other; nothing leaves the computer.

use super::collab::{field, ClashRow, Reach};
use super::App;
use crate::collab::clash::Kind;
use crate::collab::rules::{matches, SERVICES, STRUCTURE};
use openaec_ui::Language;

/// How soon a group of clashes asks to be picked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High,
    Normal,
    Low,
}

impl Priority {
    /// As BCF writes a priority.
    pub fn bcf(self) -> &'static str {
        match self {
            Priority::High => "High",
            Priority::Normal => "Normal",
            Priority::Low => "Low",
        }
    }

    pub fn word(self, l: Language) -> &'static str {
        match self {
            Priority::High => l.text("eerst", "first"),
            Priority::Normal => l.text("daarna", "next"),
            Priority::Low => l.text("later", "later"),
        }
    }
}

/// What an element of `class` is called on the building site; `None` for a class without a
/// word of its own.
pub fn class_word(class: &str, l: Language) -> Option<&'static str> {
    let c = class.to_ascii_lowercase();
    let is = |start: &str| c.starts_with(start);
    Some(match () {
        _ if is("ifcbeam") => l.text("ligger", "beam"),
        _ if is("ifccolumn") => l.text("kolom", "column"),
        _ if is("ifcslab") => l.text("vloer", "floor"),
        _ if is("ifcwall") => l.text("wand", "wall"),
        _ if is("ifcfooting") => l.text("fundering", "footing"),
        _ if is("ifcpile") => l.text("paal", "pile"),
        _ if is("ifcmember") => l.text("staaf", "member"),
        _ if is("ifcplate") => l.text("plaat", "plate"),
        _ if is("ifcstair") => l.text("trap", "stair"),
        _ if is("ifcramp") => l.text("helling", "ramp"),
        _ if is("ifcroof") => l.text("dak", "roof"),
        _ if is("ifcdoor") => l.text("deur", "door"),
        _ if is("ifcwindow") => l.text("raam", "window"),
        _ if is("ifccurtainwall") => l.text("vliesgevel", "curtain wall"),
        _ if is("ifcrailing") => l.text("hekwerk", "railing"),
        _ if is("ifccovering") => l.text("afwerking", "covering"),
        _ if is("ifcduct") => l.text("luchtkanaal", "duct"),
        _ if is("ifcpipe") => l.text("leiding", "pipe"),
        _ if is("ifccable") => l.text("kabelbaan", "cable run"),
        _ if is("ifcairterminal") => l.text("rooster", "air terminal"),
        _ if is("ifcsanitary") => l.text("sanitair", "sanitary fitting"),
        _ if is("ifcflow") || is("ifcdistribution") || is("ifcvalve") || is("ifcpump") || is("ifcfan") || is("ifctank") || is("ifcenergy") => l.text("installatie", "service"),
        _ if is("ifcfurni") => l.text("inrichting", "furniture"),
        _ if is("ifcreinforc") => l.text("wapening", "reinforcement"),
        _ => return None,
    })
}

fn of_any(class: &str, classes: &[&str]) -> bool {
    classes.iter().any(|p| matches(p, class))
}

/// The words for a clash of an element of class `a` with one of `b`: what goes through or into
/// what. The service comes first (the duct goes through the beam, not the other way round).
fn words(a: &str, b: &str, kind: Kind, reach: Reach, copy: bool, l: Language) -> Option<String> {
    let (a, b) = if of_any(a, &SERVICES) || !of_any(b, &SERVICES) && of_any(b, &STRUCTURE) { (a, b) } else { (b, a) };
    let (first, second) = (class_word(a, l)?, class_word(b, l)?);
    let capital = |s: &str| s.chars().next().map(|c| c.to_uppercase().collect::<String>() + &s[c.len_utf8()..]).unwrap_or_default();
    Some(if copy {
        format!("{} {}", l.text("Dubbel:", "Double:"), first)
    } else {
        let how = match (kind, reach) {
            (Kind::Near, _) => l.text("te dicht bij", "too near"),
            (Kind::Hidden, _) => l.text("niet te zien achter", "out of sight behind"),
            (Kind::Inside, _) => l.text("in", "inside"),
            (_, Reach::Through) => l.text("door", "through"),
            (_, Reach::Graze) => l.text("schampt", "grazes"),
            _ => l.text("kruist", "crosses"),
        };
        format!("{} {how} {second}", capital(first))
    })
}

/// How soon clashes between classes `a` and `b` ask to be picked up, `count` of them, the worst
/// of them reaching `reach`.
fn weigh(a: &str, b: &str, kind: Kind, reach: Reach, copy: bool, count: usize) -> Priority {
    let structure = of_any(a, &STRUCTURE) || of_any(b, &STRUCTURE);
    let services = of_any(a, &SERVICES) || of_any(b, &SERVICES);
    match (kind, reach) {
        // a sign not seen is about getting out
        (Kind::Hidden, _) => Priority::High,
        _ if copy => Priority::Low,
        (Kind::Near, _) => if structure && count >= 10 { Priority::Normal } else { Priority::Low },
        (_, Reach::Graze) => Priority::Low,
        (_, Reach::Through) | (Kind::Inside, _) if structure => Priority::High,
        _ if structure && services => Priority::High,
        _ if structure && count >= 5 => Priority::High,
        _ => Priority::Normal,
    }
}

impl App {
    fn class_of(&self, id: u64) -> &str {
        self.model.as_ref().and_then(|m| m.props.get(&id)).and_then(|p| field(p, "IfcType")).unwrap_or("")
    }

    /// What a group of clashes is called and how soon it asks to be picked up: by the pair of
    /// classes most of its clashes are between, and the worst of those. `None` for the words
    /// when the classes have none.
    pub(super) fn clash_words(&self, rows: &[&ClashRow], l: Language) -> (Option<String>, Priority) {
        let mut pairs: Vec<((String, String), Vec<&ClashRow>)> = Vec::new();
        for r in rows {
            let (a, b) = (self.class_of(r.a).to_ascii_lowercase(), self.class_of(r.b).to_ascii_lowercase());
            let pair = if a <= b { (a, b) } else { (b, a) };
            match pairs.iter_mut().find(|(p, _)| *p == pair) {
                Some(found) => found.1.push(r),
                None => pairs.push((pair, vec![r])),
            }
        }
        if pairs.is_empty() {
            return (None, Priority::Normal);
        }
        // the words of the pair of classes most clashes are between, of those that have words
        pairs.sort_by_key(|(_, of)| std::cmp::Reverse(of.len()));
        let said = pairs.iter().find_map(|((a, b), of)| {
            let worst = of.iter().max_by(|x, y| x.reach.cmp(&y.reach).then(x.distance.total_cmp(&y.distance))).copied()?;
            words(a, b, worst.kind, worst.reach, of.iter().all(|r| r.copy), l)
        });
        // the group is as urgent as its most urgent pair of classes
        let priority = pairs
            .iter()
            .map(|((a, b), of)| {
                let worst = of.iter().map(|r| r.reach).max().unwrap_or_default();
                weigh(a, b, of[0].kind, worst, of.iter().all(|r| r.copy), of.len())
            })
            .min()
            .unwrap_or(Priority::Normal);
        (said, priority)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clash_is_said_in_the_words_of_the_site() {
        let nl = Language::Dutch;
        assert_eq!(words("IfcBeam", "IfcDuctSegment", Kind::Crossing, Reach::Through, false, nl).unwrap(), "Luchtkanaal door ligger");
        assert_eq!(words("IfcPipeSegment", "IfcWallStandardCase", Kind::Crossing, Reach::Graze, false, nl).unwrap(), "Leiding schampt wand");
        assert_eq!(words("IfcWall", "IfcWall", Kind::Crossing, Reach::Partly, true, nl).unwrap(), "Dubbel: wand");
        assert_eq!(words("IfcDoor", "IfcColumn", Kind::Crossing, Reach::Partly, false, Language::English).unwrap(), "Door crosses column");
        assert!(words("IfcBuildingElementProxy", "IfcWall", Kind::Crossing, Reach::Partly, false, nl).is_none());
    }

    #[test]
    fn structure_gone_through_comes_first_and_what_only_grazes_last() {
        assert_eq!(weigh("ifcbeam", "ifcductsegment", Kind::Crossing, Reach::Through, false, 1), Priority::High);
        assert_eq!(weigh("ifcwall", "ifcpipesegment", Kind::Crossing, Reach::Graze, false, 30), Priority::Low);
        assert_eq!(weigh("ifcdoor", "ifcfurniture", Kind::Crossing, Reach::Partly, false, 2), Priority::Normal);
        assert_eq!(weigh("ifcwall", "ifcwall", Kind::Crossing, Reach::Partly, true, 4), Priority::Low);
        assert_eq!(weigh("ifcwall", "ifcslab", Kind::Crossing, Reach::Partly, false, 8), Priority::High);
        assert!(Priority::High < Priority::Low);
    }
}
