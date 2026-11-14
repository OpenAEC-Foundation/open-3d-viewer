//! The Select tool of the Modeller: selecting is a tool of its own, as in the reference modeller.
//! It is on when no other tool is.

use crate::app::App;

impl App {
    /// Switches every tool off: what is clicked is then selected. The selection stays.
    pub(in crate::app) fn select_tool(&mut self) {
        self.set_model_tool(None);
        self.modeller.grid_tool = false;
        self.modeller.points.clear();
        self.modeller.cursor = None;
        self.modeller.transform = None;
        self.measure.tool = None;
        self.dimension_tool = false;
        self.note_tool = false;
        self.section_tool = false;
    }
}
