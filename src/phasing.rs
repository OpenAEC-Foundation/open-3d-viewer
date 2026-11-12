//! Phasing (Revit): every element is made in a phase and maybe demolished in a later one; a view
//! stands in a phase and has a phase filter, which says for each status an element can have in
//! that phase (existing, demolished, new, temporary) whether it is left out, drawn as it is, or
//! drawn with the project's override for that status (existing work in halftone). Headless.

use std::collections::HashMap;

/// What an element is in the phase of a view. The numbers are those of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Demolished in an earlier phase.
    Past = 1,
    Existing = 2,
    Demolished = 3,
    New = 4,
    /// Made and demolished in the view's phase.
    Temporary = 5,
    /// Made in a later phase.
    Future = 6,
}

/// How a view draws an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Hidden,
    Normal,
    Halftone,
}

/// A filter draws a status not at all, as it is, or with the project's override.
const NOT_DRAWN: u8 = 0;
const OVERRIDDEN: u8 = 2;

/// The phase of a view (its place in the project's order of phases) and its filter: per status
/// (by its number) how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewPhase {
    pub phase: u16,
    pub show: [u8; 7],
}

/// The phasing of a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Phasing {
    /// Per element: the phase it is made in and the one it is demolished in, as places in the
    /// project's order of phases.
    pub elements: HashMap<u64, (u16, Option<u16>)>,
    /// Per status (by its number): whether the project's override for it is halftone.
    pub halftone: [bool; 7],
    /// Per view (its path in the document): its phase and filter.
    pub views: HashMap<String, ViewPhase>,
}

impl Phasing {
    /// What an element made in `created` and demolished in `demolished` is in phase `phase`.
    pub fn status(phase: u16, created: u16, demolished: Option<u16>) -> Status {
        match demolished {
            _ if created > phase => Status::Future,
            Some(d) if d < phase => Status::Past,
            Some(d) if d == phase && created == phase => Status::Temporary,
            Some(d) if d == phase => Status::Demolished,
            _ if created == phase => Status::New,
            _ => Status::Existing,
        }
    }

    /// How the view at `path` draws `element`; as it is where the view or the element has no
    /// phase.
    pub fn look(&self, path: &str, element: u64) -> Look {
        let (Some(view), Some(&(created, demolished))) = (self.views.get(path), self.elements.get(&element)) else { return Look::Normal };
        let status = Self::status(view.phase, created, demolished);
        match status {
            // never drawn, whatever the filter
            Status::Past | Status::Future => Look::Hidden,
            _ => match view.show[status as usize] {
                NOT_DRAWN => Look::Hidden,
                OVERRIDDEN if self.halftone[status as usize] => Look::Halftone,
                _ => Look::Normal,
            },
        }
    }

    /// Whether the view at `path` draws anything otherwise than as it is.
    pub fn applies(&self, path: &str) -> bool {
        self.views.contains_key(path) && !self.elements.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_has_its_status_in_a_phase() {
        assert_eq!(Phasing::status(3, 3, None), Status::New);
        assert_eq!(Phasing::status(3, 2, None), Status::Existing);
        assert_eq!(Phasing::status(3, 2, Some(3)), Status::Demolished);
        assert_eq!(Phasing::status(3, 3, Some(3)), Status::Temporary);
        assert_eq!(Phasing::status(3, 4, None), Status::Future);
        assert_eq!(Phasing::status(3, 1, Some(2)), Status::Past);
        assert_eq!(Phasing::status(3, 2, Some(4)), Status::Existing, "demolished later: still there");
    }

    #[test]
    fn a_view_draws_by_its_filter() {
        let mut p = Phasing { halftone: [false, false, true, false, false, false, false], ..Phasing::default() };
        p.elements.extend([(1, (2, None)), (2, (3, None)), (3, (2, Some(3))), (4, (4, None))]);
        // new as it is, existing overridden, demolished left out
        p.views.insert("new".into(), ViewPhase { phase: 3, show: [0, 0, 2, 0, 1, 2, 0] });
        // the phase before, showing all
        p.views.insert("before".into(), ViewPhase { phase: 2, show: [0, 0, 2, 2, 1, 2, 0] });
        assert_eq!([1, 2, 3, 4].map(|e| p.look("new", e)), [Look::Halftone, Look::Normal, Look::Hidden, Look::Hidden]);
        assert_eq!([1, 2, 3, 4].map(|e| p.look("before", e)), [Look::Normal, Look::Hidden, Look::Normal, Look::Hidden]);
        assert_eq!(p.look("new", 99), Look::Normal, "an element without a phase");
        assert_eq!(p.look("other", 1), Look::Normal, "a view without a phase");
        assert!(p.applies("new") && !p.applies("other"));
    }
}
