//! The cut faces of the section box and the section plane in the 3D view, in the hatch of what
//! each element is made of (`crate::caps`): made again when the section, the plane or what is
//! shown changes.

use super::App;
use crate::caps::{cut_faces, CutFaces};
use crate::drawing::Shown;
use crate::section::{SectionBox, SectionPlane};
use glam::Mat4;
use std::sync::Arc;

/// A model with more triangles than this keeps the plain cut fill: cutting it again at every
/// move of the section would hold the view up.
const MOST_TRIANGLES: usize = 3_000_000;

/// What the cut faces were made from: the section box, the plane, the layers shown, and which
/// model and hidden elements (by their addresses).
pub(super) type CutFacesKey = (Option<SectionBox>, Option<SectionPlane>, Vec<bool>, usize, usize);

impl App {
    /// What the cut faces are made from now; `None` where none are drawn: without a section, with
    /// the cut fill off, in a view's own tab (it draws its cut itself), or on a model turned up.
    fn cut_faces_key(&self) -> Option<CutFacesKey> {
        let model = self.model.as_ref()?;
        let (section, plane) = (self.active_section(), self.plane);
        let drawn = self.section_fill && (section.is_some() || plane.is_some()) && self.shown_view().is_none() && self.model_matrix() == Mat4::IDENTITY;
        (drawn && model.triangle_count() <= MOST_TRIANGLES).then(|| (section, plane, self.visible.clone(), Arc::as_ptr(model) as usize, Arc::as_ptr(&self.hidden) as usize))
    }

    /// After any change: the cut faces made again when what they are made from changed.
    pub(super) fn keep_cut_faces(&mut self) {
        let key = self.cut_faces_key();
        if key == self.cut_faces_key {
            return;
        }
        let faces = match (&key, self.model.clone()) {
            (Some((section, plane, layers, ..)), Some(model)) => {
                // the hidden elements, by the unit of a triangle's first vertex
                let units = self.hidden_units(&self.hidden);
                let index = self.explode_index.clone().filter(|_| units.iter().any(|u| *u));
                let hidden = |l: usize, v: u32| index.as_ref().is_some_and(|ix| units.get(ix.unit(l, v)).copied().unwrap_or(false));
                cut_faces(&model, Shown { layers, hidden: &hidden }, section.as_ref(), plane.as_ref(), bim_library::DrawingStandard::Nen47)
            }
            _ => CutFaces::default(),
        };
        if !(faces.is_empty() && self.cut_faces.is_empty()) {
            self.cut_faces = Arc::new(faces);
        }
        self.cut_faces_key = key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model};
    use glam::Vec3;
    use std::path::PathBuf;

    /// A closed box from (0, 0, 0) to (4, 2, 3).
    fn solid() -> Layer {
        let positions = vec![0., 0., 0., 4., 0., 0., 4., 2., 0., 0., 2., 0., 0., 0., 3., 4., 0., 3., 4., 2., 3., 0., 2., 3.];
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        Layer { name: "Beton".into(), color: [0.5; 3], normals: vec![0.0; positions.len()], positions, colors: None, indices, elements: Vec::new(), props: Vec::new(), hidden: false, points: false }
    }

    #[test]
    fn the_section_plane_gives_cut_faces_until_the_fill_is_off() {
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("box.obj"), "OBJ", vec![solid()])));
        app.keep_cut_faces();
        assert!(app.cut_faces.is_empty(), "nothing is cut yet");
        app.plane = Some(SectionPlane { normal: Vec3::Z, offset: 1.5 });
        app.keep_cut_faces();
        assert!(!app.cut_faces.triangles.is_empty(), "the plane cuts the solid");
        let before = Arc::as_ptr(&app.cut_faces);
        app.keep_cut_faces();
        assert_eq!(before, Arc::as_ptr(&app.cut_faces), "not made again while nothing changed");
        app.section_fill = false;
        app.keep_cut_faces();
        assert!(app.cut_faces.is_empty());
    }
}
