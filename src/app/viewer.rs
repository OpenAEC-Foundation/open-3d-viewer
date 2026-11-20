//! Access to the open project from the read-only Viewer.
use super::{App, Message};

impl App {
    pub(crate) fn set_viewer_mode(&mut self, viewer: bool) {
        if viewer && self.annotates {
            self.drop_grab();
            self.select_tool();
            self.modeller.slab_drag = None;
            if let Some(mut sketch) = self.modeller.sketch.take() {
                sketch.held = None;
                self.modeller.parked_sketch = Some(sketch);
            }
            self.modeller.offset = None;
            self.modeller.pair_edit = None;
            self.moving_model = None;
            self.draw_tool = None;
            self.dragging_view = None;
            self.reference_target = None;
        }
        self.annotates = !viewer;
    }
}

impl Message {
    /// Allow inspection and presentation. New authoring commands stay out of the Viewer by default.
    pub(crate) fn allowed_in_viewer(&self) -> bool {
        use Message::*;
        matches!(self,
            OpenDialog | OpenFolderDialog | AddDialog | Open(_) | Picked(_) | AddPicked(_) | AddSeveral(_) | Loaded(_) |
            ModelVisible(..) | PickModel(_) | ToggleFolder(_) | ViewOrder(_) | ProjectView(_) | Origin(super::origin::Message::Map(..)) |
            BuildFilmDialog | BuildFilmPicked(_) | BuildFilmDone(_) | RenderDialog | RenderPicked(_) | Saved(_) |
            Mode(_) | ToggleGrid | ToggleFlipUp | Theme(_) | Tab(_) | Preset(_) | Ortho(_) | Fit | NavigateSheet { .. } |
            ToggleLayersPanel | TogglePropertiesPanel | Layer(..) | AllLayers(_) | ToggleLayerGroup(_) | LayerGroup(..) |
            LayerFilter(_) | SideTab(_) | LayersQuery(_) | Navigate(_) | Cube(_) | ApplyView(_) | ToggleGrids |
            ToggleExplorer(_) | ToggleViewGroup(_) | ExplorerTab(_) | OpenPlan(_) | OpenDocSheet(_) | ReleaseView(_) |
            Docs(super::docs::DocsMessage::Close) | Walk { .. } | DrawingOnPaper(_) | PickOnSheet(_) | PickMark(_) |
            ToggleViewsIn3d | ToggleAmbient | CloudColor(_) | Field(_) | ToggleCloudDepth | Analysis(_) | AnalysisScale(_) |
            AnalysisScaleDone | ToggleGridHeadsHidden | ToggleGridDims | ToggleLevels3d | ExportMenu(_) | ExportDrawing(_) |
            Exported(_) | SelectTab(_) | CloseTab(_) | OpenSheet(_) | OpenDraft(_) | PickDraft(_) | ShowSheet(_) | CloseSheet(_) |
            SheetView(_) | OpenSchedule(_) | ShowSchedule(_) | CloseSchedule(_) | ExportSchedules | ScheduleRow(..) |
            ScheduleScroll(_) | Schedules(..) | Drawings(..) | CutAhead(..) | CutFaces(..) | OpenIfc | CloseIfc | IfcView(_) |
            IfcScroll(..) | IfcPick(_) | IfcToggle(_) | IfcRelation(_) | IfcQuery(_) | IfcNext(_) | ToggleSection | ResetSection |
            SectionToSelection | ToggleSectionFill | Section(_) | SectionSlider(..) | SectionTurn(_) | TogglePlane | FlipPlane |
            PlaneMove(_) | PlaneSlider(_) | ToggleExplode | ExplodeMode(_) | Explode(..) | ResetExplode | Measure(_) |
            MeasureFinish | MeasureEdges(_) | StructuralGrid(_) | MeasureDelete(_) | MeasureNote(_) | MeasureClear |
            MeasureLength(_) | MeasureArea(_) | MeasureVolume(_) | MeasureDecimals(..) | Undo | Redo | SelectWhere(..) |
            SelectLayerOfSelection | HideSelection | IsolateSelection | ShowAllElements | ContextMenu(_) | CloseContextMenu |
            Escape | Pressed(_) | Tick | TakeShot | Shot(_) | Remote(_) | RemoteShot(_) | ClearMeasurements
        )
    }
}
