//! Element drawings from the app: the drawings of the prefab units of the framed systems (see
//! [`crate::element_drawing`]) written into a folder, for the properties panel's button, the
//! remote control and the command line.

use super::{App, Message, Status};
use crate::export::drawing::{file_name, write, DrawingFormat};
use bim_core::ElementId;
use iced::Task;
use openaec_ui::Language;
use std::path::{Path, PathBuf};

/// What [`App::write_element_drawings`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// How many systems had drawings, and how many drawings they had.
    pub systems: usize,
    pub drawings: usize,
    pub files: Vec<PathBuf>,
}

impl App {
    /// Writes the element drawings of `only` (of every framed system when none are named) into
    /// `folder`, made if it is not there, in `format`, the materials named in language `l`.
    pub(crate) fn write_element_drawings(&self, folder: &Path, format: DrawingFormat, only: &[ElementId], l: Language) -> Result<Written, String> {
        let model = &self.modeller.model;
        let lang = super::library::lang(l);
        let named = |key: &str| self.library.material(key).map_or_else(|| key.to_string(), |m| m.name(lang).to_string());
        let mut made = Vec::new();
        let mut systems = 0;
        for element in model.iter().filter(|e| only.is_empty() || only.contains(&e.id)) {
            let drawings = crate::element_drawing::element_drawings(model, element, &named);
            systems += usize::from(!drawings.is_empty());
            made.extend(drawings);
        }
        if made.is_empty() {
            return Err("no framed system to draw: element drawings are of timber frames, steel frames and metal stud walls".to_string());
        }
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let mut files = Vec::with_capacity(made.len());
        for d in &made {
            let path = folder.join(file_name(d, format));
            std::fs::write(&path, write(d, format)?).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push(path);
        }
        Ok(Written { systems, drawings: files.len(), files })
    }

    /// The properties panel's button: asks for a folder for the drawings of the selected system.
    pub(super) fn element_drawings_dialog(&mut self, l: Language) -> Task<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        return Task::perform(async { rfd::AsyncFileDialog::new().set_title("Element drawings").pick_folder().await.map(|f| f.path().to_path_buf()) }, move |folder| Message::ElementDrawingsTo(folder, l));
        #[cfg(target_arch = "wasm32")]
        {
            let _ = l;
            Task::none()
        }
    }

    /// Writes the drawings of the selected systems into `folder` as PDF and says what was written.
    pub(super) fn element_drawings_to(&mut self, folder: &Path, l: Language) {
        let only = self.selected_modelled();
        self.status = match self.write_element_drawings(folder, DrawingFormat::Pdf, &only, l) {
            Ok(w) => Status::Loaded(format!("{} {} → {}", w.drawings, l.text("elementtekeningen", "element drawings"), folder.display())),
            Err(e) => Status::Error(e),
        };
    }
}
