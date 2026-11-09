//! Application state, messages and the main window layout:
//! ribbon on top, explorer left, 3D viewport centre, properties and layers right, status bar
//! bottom.

mod export_drawing;
mod grids;
mod ifc;
mod layers;
mod levels;
mod marks;
pub mod modeller;
mod paper;
mod schedules;
mod view_props;
mod views;
mod sheets;
mod structural;
mod underlays;
mod views_in_3d;

pub use ifc::{IfcTab, IfcView};
pub use layers::SideTab;
pub use view_props::ViewField;
pub use levels::{ExplorerTab, LevelForm};
pub use views::{DocTab, OpenPlan, ViewSlot};

pub use schedules::{tab_id, tab_schedule, VIEW_3D};

use crate::document::Document;
use crate::explode::{ExplodeIndex, ExplodeMode, UnitOwner};
use crate::export;
use crate::icons;
use crate::loaders;
use crate::measure::format::{AreaUnit, Category, LengthUnit, VolumeUnit, MAX_DECIMALS};
use crate::measure::hover::{Hover, Pointer, Screen};
use crate::measure::{MeasureState, Tool};
use crate::model::Model;
use crate::theme::{srgb3, srgb4, Palette, ThemeKind};
use crate::viewport::camera::{Camera, ViewPreset};
use crate::viewport::render::{transform_bounds, RenderMode, RenderSettings};
use crate::model::{Bounds, ViewKind};
use crate::section::{PlaneMove, PlaneOverlay, SectionBox, SectionEvent, SectionOverlay, SectionPlane};
use crate::history::History;
use crate::selection::{Pickable, Selection};
use crate::units::{unit_state, UnitKey, UnitKeySet, UNIT_HIDDEN, UNIT_SELECTED};
use crate::view_cube::{view_from, CubeEvent, CubeOverlay, CubeTarget};
use crate::viewport::{Navigate, Viewport3D};
use glam::{Mat4, Vec2, Vec3};
use iced::widget::text::Wrapping;
use iced::{Background, Border, Color, Padding, Shadow};
use openaec_ui::Language;
use iced::alignment::Vertical;
use iced::widget::{
    button, canvas, center, column, container, row, rule, scrollable, shader, slider, stack, svg, text, text_input, Space,
};
use iced::{event, keyboard, window, Center, Element, Fill, Font, Length, Subscription, Task, Theme};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use web_time::Instant;

pub const FONT: Font = Font::with_name("Inter");
pub const FONT_BOLD: Font = Font { weight: iced::font::Weight::Semibold, ..FONT };

pub struct ModelEntry {
    pub model: Arc<Model>,
    pub visible: bool,
    pub layers: std::ops::Range<usize>,
    layer_visibility: Vec<bool>,
}

impl ModelEntry {
    fn new(model: Arc<Model>, first_layer: usize) -> Self {
        let layers = first_layer..first_layer + model.layers.len();
        let layer_visibility = model.layers.iter().map(|layer| !layer.hidden).collect();
        Self { model, visible: true, layers, layer_visibility }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RibbonTab {
    #[default]
    Home,
    View,
}

#[derive(Debug, Clone)]
pub enum Status {
    Ready,
    Loading { file: String, since: Instant, progress: Arc<loaders::Progress> },
    Saving { file: String, since: Instant },
    Loaded(String),
    Error(String),
}

pub struct App {
    pub theme: ThemeKind,
    pub model: Option<Arc<Model>>,
    pub models: Vec<ModelEntry>,
    adding_model: bool,
    /// The files of the last model when they layer over each other (IFCX) and that model: a
    /// file of the same kind added next composes with them into that model.
    layer_stack: (Vec<loaders::Source>, Option<u64>),
    /// The files of the load running now, and whether it replaces the last model (a restack).
    loading_stack: (Vec<loaders::Source>, bool),
    pub visible: Vec<bool>,
    pub camera: Camera,
    pub mode: RenderMode,
    pub grid: bool,
    pub flip_up: bool,
    pub tab: RibbonTab,
    pub show_layers: bool,
    pub show_properties: bool,
    /// What the explorer's search field holds.
    pub layer_filter: String,
    /// The tab of the right pane shown.
    pub side_tab: SideTab,
    /// What the Layers tab's search field holds.
    pub layers_query: String,
    /// What is being typed in a field of the right pane's View section: the view's path, the
    /// field and the text.
    pub view_typing: Option<(String, ViewField, String)>,
    /// The 3D view's layer switches while another view shows: every layer's, and per model its
    /// own.
    parked_layers: Option<(Vec<bool>, Vec<Vec<bool>>)>,
    /// The undo histories of the views not shown, by view path (the 3D view: "").
    parked_histories: std::collections::HashMap<String, History<ViewState>>,
    pub status: Status,
    pub viewport_size: (f32, f32),
    /// The last press was a finger's, not the mouse's: the hint shows the touch gestures.
    pub fingers: bool,
    /// View preset to apply when the next model arrives (command line).
    pub initial_view: Option<ViewPreset>,
    /// `--window-screenshot`: capture the real window once the model is shown, then exit.
    pub window_shot: Option<PathBuf>,
    /// A file to open once the window runs (command line; the OpenAEC runtime has no boot task).
    pub pending_open: Option<PathBuf>,
    /// The picked layer and, for BIM formats, the element in it.
    pub selected: Option<(usize, Option<u64>)>,
    /// What is selected, in pick order; `selected` and `selection` follow it (see
    /// [`App::selection_changed`]).
    pub sel: Selection,
    /// Index ranges of the selection: (layer, first index, index count).
    pub selection: Vec<(usize, u32, u32)>,
    /// A running camera transition: from, to, start.
    pub anim: Option<(Camera, Camera, Instant)>,
    /// The section box (world); kept while switched off, so switching on restores it.
    pub section: Option<SectionBox>,
    pub section_on: bool,
    /// Fill the cut faces of solids.
    pub section_fill: bool,
    /// The section plane (world), when placed.
    pub plane: Option<SectionPlane>,
    /// The section plane as it was placed, before moves and flips: what Reset brings back.
    pub plane_placed: Option<SectionPlane>,
    /// Waiting for a click on a face to place the section plane.
    pub plane_picking: bool,
    /// The saved view last shown.
    pub active_view: Option<usize>,
    /// Show the model's grid lines with their bubbles.
    pub show_grids: bool,
    /// The grid line picked with a click (index into the model's grids), shown in the properties.
    pub picked_grid: Option<usize>,
    /// Explorer groups opened: views, rooms, layers, schedules, sheets.
    pub explorer_open: [bool; 5],
    /// The explorer's tab shown.
    pub explorer_tab: ExplorerTab,
    /// The project document (IfcX): the levels of the model and those added.
    pub document: Document,
    /// The form of a new level, while it is open.
    pub level_form: Option<LevelForm>,
    /// The floor plan whose tab shows (its path); `None`: the 3D view.
    pub active_plan: Option<String>,
    /// Floor plans open in a document tab, in tab order, each with its own camera and section.
    pub open_plans: Vec<views::OpenPlan>,
    /// The 3D view's camera and section box while a floor plan's tab shows.
    model_view: Option<views::ViewSlot>,
    /// Sheets of the document open in a document tab (paths), and the one shown.
    pub open_doc_sheets: Vec<String>,
    pub active_doc_sheet: Option<String>,
    /// The cut of every floor plan placed on a sheet, by the plan's path.
    pub cuts: std::collections::HashMap<String, Arc<crate::drawing::Cut>>,
    /// The drawings laid on the levels.
    pub underlays: Vec<crate::underlay::Underlay>,
    /// The lines the 3D view draws over the model, with their colours: the underlays' and the
    /// views' in 3D (world), made again when they change.
    overlay_lines: Arc<Vec<crate::views3d::Colored>>,
    /// The views stand in the 3D view on their planes; what they were drawn from, and their names.
    pub views_in_3d: bool,
    views_3d_key: Vec<u64>,
    views_3d_labels: Vec<(String, [f32; 3])>,
    /// The bar's Export button is open: the formats to export the drawing shown in follow it.
    pub export_menu: bool,
    /// The dimension tool of the sheets is on.
    pub dimension_tool: bool,
    /// The section tool of the sheets is on: two clicks on a floor plan draw a section.
    pub section_tool: bool,
    /// What is picked on the sheet shown: a placed view or a dimension.
    pub sheet_pick: Option<paper::SheetPick>,
    /// The cut height being typed for a floor plan: its path and the text.
    pub cut_edit: Option<(String, String)>,
    /// The view taken from the Views list while the mouse button is down, to drop on a sheet.
    pub dragging_view: Option<Arc<str>>,
    /// The text tool is on: a click on a view (its tab, or placed on a sheet) puts a text there.
    pub note_tool: bool,
    /// The wording of the next text placed, or of the picked text.
    pub note_text: String,
    /// What is picked on the view shown in its tab: a dimension or a text.
    pub mark_pick: Option<marks::MarkPick>,
    /// Groups of layers opened in the explorer (closed by default).
    pub open_layer_groups: HashSet<String>,
    /// Sheets open in a document tab, in tab order, and the one shown in the workspace.
    pub open_sheets: Vec<usize>,
    pub active_sheet: Option<usize>,
    /// Schedules open in a document tab, in tab order, and the one shown under the viewport.
    pub open_schedules: Vec<usize>,
    pub active_schedule: Option<usize>,
    /// The table of the shown schedule: scrolled offset and height (logical pixels).
    pub schedule_scroll: (f32, f32),
    /// The IFC tab: how the model's IFCX file is built up.
    pub ifc: IfcTab,
    /// The schedules of the model, once computed (they follow the model), and whether they are
    /// being computed.
    pub schedules: Vec<crate::model::Schedule>,
    pub schedules_pending: bool,
    /// Measurements shown in the viewport (made through remote control).
    pub measurements: Vec<crate::annotations::Measurement>,
    /// Where the viewport sits in the window (logical pixels), to cut it out of screenshots.
    pub viewport_origin: (f32, f32),
    /// Remote control (the MCP server): listening, and the calls waiting for an answer.
    pub remote: crate::remote::RemoteState,
    /// The exploded view: on or off (the amounts are kept while off, as the section box), the
    /// mode asked for and the amount per axis (percent).
    pub explode_on: bool,
    pub explode_mode: ExplodeMode,
    pub explode: [f32; 3],
    /// The units of the model for the exploded view, built when it is loaded.
    pub explode_index: Option<Arc<ExplodeIndex>>,
    /// The offset (world) of every unit as drawn, and the world bounds while apart; shared with
    /// the renderer, replaced when they change.
    pub explode_offsets: Arc<Vec<[f32; 4]>>,
    /// What the renderer gets per unit: the explode offset and the flags (`crate::units`).
    pub units: Arc<Vec<[f32; 4]>>,
    /// Elements (and the rest of layers) hidden with Hide or Isolate, by keys that stay when a
    /// model is added.
    pub hidden: Arc<UnitKeySet>,
    /// Undo and redo of what is shown: hidden elements and layer and model switches.
    pub history: History<ViewState>,
    /// The selection box while it is dragged: from, to (viewport pixels).
    pub box_select: Option<(Vec2, Vec2)>,
    /// The right-click menu, open at this viewport point (logical pixels).
    pub context_menu: Option<Vec2>,
    pub explode_bounds: Option<Bounds>,
    /// The measure tools and their results.
    pub measure: MeasureState,
    /// Where the cursor rests in the viewport (x, y, width, height), waiting to be looked up for
    /// the measure tool; when the last lookup ended and how long it took.
    hover_request: Option<(f32, f32, f32, f32)>,
    hover_last: Option<(Instant, std::time::Duration)>,
    /// What the hover was found in besides the view: the section box and plane and the shown
    /// layers. When they change, it is looked up again.
    hover_scene: Option<HoverScene>,
    /// Boxes around blocks of triangles of the model, for the ray of a click or hover.
    pick_index: Option<PickIndex>,
    /// The selection and hover of structural nodes and members.
    pub(crate) structural: structural::StructuralView,
    /// The modelled elements and the tool placing them.
    pub modeller: modeller::Modeller,
}

/// Width of the right-click menu (logical pixels), and about its height, to keep it inside the
/// viewport.
const MENU_WIDTH: f32 = 230.0;
const MENU_HEIGHT: f32 = 200.0;

type HoverScene = (Option<SectionBox>, Option<SectionPlane>, Vec<bool>, Arc<UnitKeySet>);

/// The items of the right-click menu in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Hide,
    Isolate,
    ShowAll,
    SelectLayer,
    ClearSelection,
    Undo,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenDialog,
    AddDialog,
    Open(PathBuf),
    Picked(Option<loaders::Source>),
    AddPicked(Option<loaders::Source>),
    ModelVisible(usize, bool),
    Loaded(Result<Arc<Model>, String>),
    SaveDialog,
    SavePicked(Option<PathBuf>),
    /// A save finished: the file written and how long it took, or the error.
    Saved(Result<(PathBuf, f32), String>),
    Mode(RenderMode),
    ToggleGrid,
    ToggleFlipUp,
    Theme(ThemeKind),
    Tab(RibbonTab),
    Preset(ViewPreset),
    Ortho(bool),
    Fit,
    ToggleLayersPanel,
    TogglePropertiesPanel,
    Layer(usize, bool),
    AllLayers(bool),
    /// Open or close a group of layers in the explorer (see [`LAYER_GROUP`]).
    ToggleLayerGroup(String),
    /// Show or hide every layer of a group.
    LayerGroup(String, bool),
    LayerFilter(String),
    /// Show a tab of the right pane.
    SideTab(SideTab),
    /// What the Layers tab's search field holds.
    LayersQuery(String),
    /// Typed in a field of the right pane's View section.
    ViewField(ViewField, String),
    /// Enter in a field of the View section: it shows the value in use again.
    ViewFieldDone,
    Navigate(Navigate),
    Cube(CubeEvent),
    /// Show a view saved in the file.
    ApplyView(usize),
    ToggleGrids,
    /// Open or close an explorer group (0 views, 1 rooms, 2 layers, 3 schedules, 4 sheets).
    ToggleExplorer(usize),
    /// Show a tab of the explorer.
    ExplorerTab(ExplorerTab),
    /// Open the form of a new level, named after this word ("Level") and a number.
    NewLevel(&'static str),
    LevelName(String),
    LevelElevation(String),
    /// Add the level of the form.
    AddLevel,
    CancelLevel,
    /// Remove an added level (its IfcX path).
    RemoveLevel(Arc<str>),
    /// Make a floor plan of a level (its path) and show it.
    NewPlan(Arc<str>),
    /// Show a floor plan (its path) in its tab.
    OpenPlan(Arc<str>),
    RemovePlan(Arc<str>),
    /// Make a sheet of the document, named this, and show it.
    NewSheet(&'static str),
    /// Lay the views of a sheet (its path) out: one scale, in rows, centred.
    ArrangeSheet(Arc<str>),
    /// Put the views on no sheet on new sheets, named for floor plans and for sections.
    FillSheets([&'static str; 2]),
    /// Show a sheet of the document (its path) in its tab.
    OpenDocSheet(Arc<str>),
    RemoveSheet(Arc<str>),
    /// Place a floor plan on a sheet: sheet, plan.
    PlaceOnSheet(Arc<str>, Arc<str>),
    /// Move a placed view: sheet, its index on the sheet, its new middle (paper metres).
    MoveOnSheet(Arc<str>, usize, [f64; 2]),
    /// Switch the dimension tool of the sheets on or off.
    DimensionTool(bool),
    /// Crop a floor plan (its path) to a rectangle in plan, or show all of it (`None`).
    CropPlan(Arc<str>, Option<([f64; 2], [f64; 2])>),
    /// Pick a placed view or a dimension on the sheet shown (`None`: nothing).
    PickOnSheet(Option<paper::SheetPick>),
    /// Pressed on a view in the Views list: it can be dragged onto the sheet shown.
    GrabView(Arc<str>),
    /// Let go over a view in the Views list: it opens (a click), unless another was carried.
    ReleaseView(Arc<str>),
    /// A view let go over the sheet shown, at a paper point: placed there, its middle on it.
    DropOnSheet(Arc<str>, [f64; 2]),
    /// The mouse button went up: the view carried, if any, is let go.
    LetGoView,
    /// Switch the text tool on or off.
    NoteTool(bool),
    /// The wording typed for the next text, or for the picked one.
    NoteText(String),
    /// Put a text on a view: the view, the point of its drawing, the wording.
    AddNote(Arc<str>, [f64; 2], String),
    /// Move a view's text (its index) to a point of its drawing.
    MoveNote(Arc<str>, usize, [f64; 2]),
    /// Pick a dimension or text on the view shown in its tab (`None`: nothing).
    PickMark(Option<marks::MarkPick>),
    /// Remove what is picked on the view shown in its tab.
    RemoveMark,
    /// Put the picked dimension on the other side of what it measures.
    FlipDimension,
    /// Give a view (its path) its standard dimensions.
    StandardDimensions(Arc<str>),
    /// Lay a drawing on a level (its path): asks for the file.
    AddUnderlay(Arc<str>),
    /// A drawing (its name) read to lay on a level (its path): its lines and how many entities it
    /// left out, or why not.
    UnderlayRead(Arc<str>, String, underlays::UnderlayRead),
    /// Switch an underlay (its index) on or off.
    ToggleUnderlay(usize),
    /// Take an underlay (its index) away.
    RemoveUnderlay(usize),
    /// Show the views in the 3D view on their planes, or not.
    ToggleViewsIn3d,
    /// Lay a PDF underlay (its index) at the next scale.
    UnderlayScale(usize),
    /// Open or close the bar's Export button.
    ExportMenu(bool),
    /// Export the drawing of the view or sheet shown in this format.
    ExportDrawing(crate::export::drawing::DrawingFormat),
    /// The drawing was exported (where, in how many seconds), or not; `None`: the dialog was
    /// cancelled.
    Exported(Option<Result<(PathBuf, f32), String>>),
    /// Delete was pressed (not in a text field): remove what is picked, on a sheet or a view.
    DeletePicked,
    /// Remove what is picked on the sheet shown.
    RemovePicked,
    /// Set the scale of a placed view: sheet, index, 1 : scale.
    SetScale(Arc<str>, usize, f64),
    /// Give a sheet another paper (width, height in metres).
    SetPaper(Arc<str>, [f64; 2]),
    /// The cut height being typed, and its use.
    CutText(String),
    ApplyCut,
    /// Add a dimension to a floor plan: plan, from, to (model x, y).
    AddDimension(Arc<str>, [f64; 2], [f64; 2]),
    /// Show a document tab ([`App::doc_tabs`]).
    SelectTab(usize),
    /// Close a document tab; the 3D view's stays.
    CloseTab(usize),
    /// Remove a section or elevation (its path).
    RemoveSection(Arc<str>),
    /// Make the four elevations of the model, named so (north, east, south, west).
    MakeElevations([&'static str; 4]),
    /// Make a section along a line drawn on a floor plan (plan x, y), named after this word.
    NewSection([f64; 2], [f64; 2], &'static str),
    /// Switch the section tool of the sheets on or off.
    SectionTool(bool),
    /// Open a sheet in its document tab.
    OpenSheet(usize),
    /// Show a sheet's tab (`None`: not a sheet).
    ShowSheet(Option<usize>),
    CloseSheet(usize),
    /// A view on a sheet clicked: show it (index in `Model::views`).
    SheetView(usize),
    /// Open a schedule in its document tab.
    OpenSchedule(usize),
    /// Show a schedule's tab (`None`: the 3D view alone).
    ShowSchedule(Option<usize>),
    CloseSchedule(usize),
    /// Select the elements of a schedule row: schedule, row.
    ScheduleRow(usize, usize),
    ScheduleScroll(scrollable::Viewport),
    /// The schedules of a model (by id), computed.
    Schedules(u64, Vec<crate::model::Schedule>),
    /// Open the IFC tab (the IFCX file of the model, read again), close it, pick what it shows.
    OpenIfc,
    CloseIfc,
    IfcView(IfcView),
    IfcScroll(IfcView, scrollable::Viewport),
    /// Pick a node of the IFC tab (its index): its elements are selected.
    IfcPick(u32),
    /// Open or close a row of the IFC tab's tree (its key).
    IfcToggle(String),
    /// The relation the IFC tab's Relations view shows (all: `None`).
    IfcRelation(Option<crate::ifcx_structure::Relation>),
    /// Search the IFC tab; jump to the next (or the previous) line found in its text.
    IfcQuery(String),
    IfcNext(bool),
    ToggleSection,
    ResetSection,
    SectionToSelection,
    ToggleSectionFill,
    Section(SectionEvent),
    /// A face set from the properties pane: axis, min face, percent of the model.
    SectionSlider(usize, bool, f32),
    /// The turn from the properties pane, degrees.
    SectionTurn(f32),
    /// Switch the section plane on (wait for a click on a face) or off.
    TogglePlane,
    /// Cut away the other side of the section plane.
    FlipPlane,
    /// Move the section plane along its normal (world units).
    PlaneMove(f32),
    /// The section plane from the properties pane: percent between its extremes.
    PlaneSlider(f32),
    /// Switch the exploded view on or off.
    ToggleExplode,
    ExplodeMode(ExplodeMode),
    /// An axis of the exploded view: axis, percent.
    Explode(usize, f32),
    /// All axes of the exploded view back to 0 %.
    ResetExplode,
    /// Switch a measure tool on, or off when it is on.
    Measure(Tool),
    /// Enter: close the polygon being measured.
    MeasureFinish,
    /// The edges of layers, built in the background for snapping.
    MeasureEdges(crate::measure::Edges),
    /// A structural hover grid built off the UI thread (`None`: the build was dropped).
    StructuralGrid(Option<structural::BuiltGrid>),
    MeasureDelete(usize),
    MeasureClear,
    MeasureLength(LengthUnit),
    MeasureArea(AreaUnit),
    MeasureVolume(VolumeUnit),
    /// Decimals shown for a category.
    MeasureDecimals(Category, u8),
    /// Go back to the visibility before the last change (layers, models, hidden elements).
    Undo,
    Redo,
    /// Select every element whose property (key, value) has this value.
    SelectWhere(String, String),
    /// Select everything on the layers the selection touches.
    SelectLayerOfSelection,
    /// Hide the selected elements; the selection empties.
    HideSelection,
    /// Hide every element but the selected ones.
    IsolateSelection,
    /// Show every element that Hide or Isolate hid.
    ShowAllElements,
    /// An item of the right-click menu; the menu closes.
    ContextMenu(MenuItem),
    /// A click beside the right-click menu: it closes.
    CloseContextMenu,
    /// Esc: stop placing the section plane, else stop modelling or measuring, else clear the
    /// selection.
    Escape,
    /// Start an empty model to model in, its first level named so.
    NewModel(&'static str),
    /// Switch a modelling tool on (what it places named so), or off (`None`).
    ModelTool(Option<(modeller::ModelTool, &'static str)>),
    /// Join the two linear elements selected (the first is the primary), the joint named so.
    Connect(bim_core::ConnectionKind, &'static str),
    /// A finger (`true`) or a mouse button (`false`) went down.
    Pressed(bool),
    /// A frame while loading (animates the loading card).
    Tick,
    TakeShot,
    Shot(window::Screenshot),
    /// A command from remote control (the MCP server).
    Remote(crate::remote::RemoteCall),
    /// The window captured for a remote `screenshot`.
    RemoteShot(window::Screenshot),
    ClearMeasurements,
}

impl From<Navigate> for Message {
    fn from(n: Navigate) -> Self {
        Message::Navigate(n)
    }
}

impl From<SectionEvent> for Message {
    fn from(e: SectionEvent) -> Self {
        Message::Section(e)
    }
}

impl From<PlaneMove> for Message {
    fn from(m: PlaneMove) -> Self {
        Message::PlaneMove(m.0)
    }
}

impl From<CubeEvent> for Message {
    fn from(e: CubeEvent) -> Self {
        Message::Cube(e)
    }
}

/// Length of a camera transition (view cube, presets).
const ANIM_SECONDS: f32 = 0.45;

impl Default for App {
    fn default() -> Self {
        crate::i18n::install();
        Self {
            theme: ThemeKind::Light,
            model: None,
            models: Vec::new(),
            adding_model: false,
            layer_stack: (Vec::new(), None),
            loading_stack: (Vec::new(), false),
            visible: Vec::new(),
            camera: Camera::default(),
            mode: RenderMode::Solid,
            grid: true,
            flip_up: false,
            tab: RibbonTab::Home,
            show_layers: true,
            show_properties: true,
            layer_filter: String::new(),
            side_tab: SideTab::default(),
            layers_query: String::new(),
            view_typing: None,
            parked_layers: None,
            parked_histories: Default::default(),
            status: Status::Ready,
            viewport_size: (1000.0, 700.0),
            fingers: false,
            initial_view: None,
            window_shot: None,
            pending_open: None,
            selected: None,
            selection: Vec::new(),
            sel: Selection::default(),
            anim: None,
            section: None,
            section_on: false,
            section_fill: true,
            plane: None,
            plane_placed: None,
            plane_picking: false,
            active_view: None,
            show_grids: true,
            picked_grid: None,
            explorer_open: [false, false, true, false, false],
            explorer_tab: ExplorerTab::Model,
            document: Document::default(),
            level_form: None,
            active_plan: None,
            open_plans: Vec::new(),
            model_view: None,
            open_doc_sheets: Vec::new(),
            active_doc_sheet: None,
            cuts: Default::default(),
            underlays: Vec::new(),
            overlay_lines: Arc::new(Vec::new()),
            views_in_3d: false,
            views_3d_key: Vec::new(),
            views_3d_labels: Vec::new(),
            export_menu: false,
            dimension_tool: false,
            section_tool: false,
            sheet_pick: None,
            cut_edit: None,
            dragging_view: None,
            note_tool: false,
            note_text: String::new(),
            mark_pick: None,
            open_layer_groups: HashSet::new(),
            open_sheets: Vec::new(),
            active_sheet: None,
            open_schedules: Vec::new(),
            active_schedule: None,
            schedule_scroll: (0.0, 0.0),
            ifc: IfcTab::default(),
            schedules: Vec::new(),
            schedules_pending: false,
            measurements: Vec::new(),
            viewport_origin: (0.0, 0.0),
            remote: Default::default(),
            explode_on: false,
            explode_mode: ExplodeMode::Storeys,
            explode: [0.0; 3],
            explode_index: None,
            explode_offsets: Arc::default(),
            units: Arc::default(),
            hidden: Arc::default(),
            history: History::new(),
            box_select: None,
            context_menu: None,
            explode_bounds: None,
            measure: MeasureState::default(),
            hover_request: None,
            hover_last: None,
            hover_scene: None,
            pick_index: None,
            structural: Default::default(),
            modeller: Default::default(),
        }
    }
}

/// Loads a file (or files that layer into one model) on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_task(sources: Vec<loaders::Source>, progress: Arc<loaders::Progress>) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(loaders::load_stack(&sources, &progress).map(Arc::new));
            });
            rx.await.unwrap_or_else(|_| Err("loader thread stopped".to_string()))
        },
        Message::Loaded,
    )
}

/// Builds the edges of `layers` for snapping on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn edges_task(model: Arc<Model>, layers: Vec<usize>) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            let id = model.id;
            std::thread::spawn(move || {
                let _ = tx.send(crate::measure::Edges::build(&model, &layers));
            });
            rx.await.unwrap_or(crate::measure::Edges(id, Vec::new()))
        },
        Message::MeasureEdges,
    )
}

/// Builds the edges of `layers` in the browser (no threads there): after the next frame, so the
/// tool shows as chosen first.
#[cfg(target_arch = "wasm32")]
fn edges_task(model: Arc<Model>, layers: Vec<usize>) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            crate::measure::Edges::build(&model, &layers)
        },
        Message::MeasureEdges,
    )
}

/// Loads a file in the browser (no threads there): after the next frame, so the loading card
/// shows first.
#[cfg(target_arch = "wasm32")]
pub fn load_task(sources: Vec<loaders::Source>, progress: Arc<loaders::Progress>) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            loaders::load_stack(&sources, &progress).map(Arc::new)
        },
        Message::Loaded,
    )
}

/// Asks for a model file: the native dialog on the desktop, the browser's file picker on the web
/// (which hands over the bytes, not a path).
fn pick_model_task(add: bool) -> Task<Message> {
    let mut exts = loaders::supported_extensions();
    exts.sort_unstable();
    Task::perform(
        async move {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title(if add { "Add 3D model" } else { "Open 3D model" })
                .add_filter("3D models", &exts)
                .add_filter("All files", &["*"]);
            #[cfg(not(target_arch = "wasm32"))]
            let dialog = match crate::managed::work_folder() {
                Some(work) => dialog.set_directory(work),
                None => dialog,
            };
            let file = dialog.pick_file().await?;
            #[cfg(not(target_arch = "wasm32"))]
            return Some(loaders::Source::File(file.path().to_path_buf()));
            #[cfg(target_arch = "wasm32")]
            return Some(loaders::Source::Bytes { name: file.file_name(), data: Arc::new(file.read().await) });
        },
        move |source| if add { Message::AddPicked(source) } else { Message::Picked(source) },
    )
}

/// The file name offered for "Save as": the model's name in the first writer's format.
fn save_name(model: &Model) -> String {
    let stem = model.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    format!("{stem}.{}", export::registry()[0].extensions()[0])
}

/// Asks where to save the model, one filter per writer.
#[cfg(not(target_arch = "wasm32"))]
fn save_dialog_task(model: &Model) -> Task<Message> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Save as").set_file_name(save_name(model));
    if let Some(dir) = crate::managed::dialog_folder(model.path.parent().map(PathBuf::from), crate::managed::work_folder()) {
        dialog = dialog.set_directory(dir);
    }
    for w in &export::registry() {
        dialog = dialog.add_filter(w.name(), w.extensions());
    }
    Task::perform(async move { dialog.save_file().await.map(|f| f.path().to_path_buf()) }, Message::SavePicked)
}

/// The browser has no save dialog: the file goes to the downloads.
#[cfg(target_arch = "wasm32")]
fn save_dialog_task(model: &Model) -> Task<Message> {
    Task::done(Message::SavePicked(Some(PathBuf::from(save_name(model)))))
}

/// Saves the model with the document on a worker thread; an existing file is only replaced when
/// `replace`.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_task(model: Arc<Model>, document: Document, path: PathBuf, replace: bool) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let start = Instant::now();
                let _ = tx.send(export::save(&model, &document, &path, replace).map(|()| (path, start.elapsed().as_secs_f32())));
            });
            rx.await.unwrap_or_else(|_| Err("save thread stopped".to_string()))
        },
        Message::Saved,
    )
}

/// Saves the model in the browser: written in memory and handed over as a download.
#[cfg(target_arch = "wasm32")]
pub fn save_task(model: Arc<Model>, document: Document, path: PathBuf, _replace: bool) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            let start = Instant::now();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = export::to_bytes(&model, &document, &name)?;
            crate::web::download(&name, &bytes, export::writer_for(&name)?.mime())?;
            Ok((path, start.elapsed().as_secs_f32()))
        },
        Message::Saved,
    )
}

impl App {
    pub fn title(&self) -> String {
        match &self.model {
            Some(m) => format!("{} - Open 3D Viewer Studio", m.file_name()),
            None => "Open 3D Viewer Studio".to_string(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme.theme()
    }

    /// Starts saving the model to `path` (Save as, or `save_file`). One save at a time: a dialog
    /// that was open before another save started gets nothing, so the `Saved` that follows is
    /// always the answer to the save that is running.
    pub(crate) fn start_save(&mut self, mut path: PathBuf, replace: bool) -> Task<Message> {
        let Some(model) = self.model.clone() else { return Task::none() };
        if matches!(self.status, Status::Loading { .. } | Status::Saving { .. }) {
            return Task::none();
        }
        if path.extension().is_none() {
            path.set_extension(export::registry()[0].extensions()[0]);
        }
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.status = Status::Saving { file, since: Instant::now() };
        save_task(model, self.document.clone(), path, replace)
    }

    pub(crate) fn model_matrix(&self) -> Mat4 {
        match (&self.model, self.flip_up) {
            (Some(m), true) if m.source_y_up => Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            (Some(_), true) => Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            _ => Mat4::IDENTITY,
        }
    }

    pub(crate) fn aspect(&self) -> f32 {
        self.viewport_size.0 / self.viewport_size.1.max(1.0)
    }

    pub fn fit(&mut self) {
        self.anim = None;
        self.camera = self.fitted(self.camera);
    }

    /// `camera` framing the whole model.
    fn fitted(&self, mut camera: Camera) -> Camera {
        if let Some(b) = self.world_bounds() {
            camera.fit(&b, self.aspect());
        }
        camera
    }

    /// The model's bounds in world space (after the up-axis override), as drawn: pulled apart
    /// by the exploded view.
    pub(crate) fn world_bounds(&self) -> Option<Bounds> {
        let model = self.model.as_ref()?;
        // an empty model (one to model in) has no bounds yet
        self.explode_bounds.or_else(|| model.bounds.is_valid().then(|| transform_bounds(&model.bounds, &self.model_matrix())))
    }

    /// The offset (world) of vertex `i` of layer `l` in the exploded view.
    pub(crate) fn explode_offset(&self, l: usize, i: u32) -> Vec3 {
        let unit = self.explode_index.as_ref().map_or(0, |e| e.unit(l, i));
        self.explode_offsets.get(unit).map_or(Vec3::ZERO, |o| Vec3::new(o[0], o[1], o[2]))
    }

    /// Recomputes where the units of the exploded view are drawn, after its switch, mode or
    /// amounts, or the up axis changed; the far plane makes room for the model pulled apart.
    fn update_explode(&mut self) {
        let Some(index) = self.explode_index.clone() else { return };
        let amount = if self.explode_on { self.explode.map(|v| v / 100.0) } else { [0.0; 3] };
        let mm = self.model_matrix();
        self.explode_offsets = Arc::new(index.offsets(self.explode_mode, amount, &mm));
        self.explode_bounds = (amount != [0.0; 3]).then(|| index.bounds(&self.explode_offsets, &mm)).filter(Bounds::is_valid);
        if let Some(b) = self.world_bounds() {
            self.camera.scene_radius = self.camera.scene_radius.max(b.radius());
        }
        self.update_units();
    }

    /// World bounds of the selected triangles.
    pub(crate) fn selection_bounds(&self) -> Option<Bounds> {
        let model = self.model.as_ref()?;
        let mm = self.model_matrix();
        let mut b = Bounds::empty();
        for &(l, first, count) in &self.selection {
            let layer = model.layers.get(l)?;
            for &i in layer.indices.get(first as usize..(first + count) as usize)? {
                let at = 3 * i as usize;
                if let Some(p) = layer.positions.get(at..at + 3) {
                    b.extend((mm.transform_point3(Vec3::new(p[0], p[1], p[2])) + self.explode_offset(l, i)).to_array());
                }
            }
        }
        // structural objects: their point or segment box (a load's: its member's), moved with
        // their unit
        for &id in &self.structural.selected {
            let Some(item) = model.structural_item(id) else { continue };
            let Some(at_rest) = item.bounds(&model.origins) else { continue };
            let unit = item.unit_key().and_then(|u| self.explode_index.as_ref()?.structural_unit(u));
            let o = unit.and_then(|u| self.explode_offsets.get(u as usize)).map_or(Vec3::ZERO, |o| Vec3::new(o[0], o[1], o[2]));
            let w = transform_bounds(&at_rest, &mm);
            b.extend((Vec3::from(w.min) + o).to_array());
            b.extend((Vec3::from(w.max) + o).to_array());
        }
        b.is_valid().then_some(b)
    }

    /// The section box when it is on.
    pub fn active_section(&self) -> Option<SectionBox> {
        self.section.filter(|_| self.section_on)
    }

    /// Shows a saved view, animated: its camera (a 3D view's eye and lens, the direction of a
    /// section or elevation, from above for a plan) and what it shows (a 3D view's section box,
    /// a section's crop, a plan's range from the view depth up to the cut plane) as the section
    /// box.
    pub fn apply_view(&mut self, i: usize) {
        let Some(view) = self.model.as_ref().and_then(|m| m.views.get(i).cloned()) else { return };
        // a saved view of the model shows in the 3D view's tab
        self.load_view(None);
        if self.show_view(&view) {
            self.active_view = Some(i);
        }
    }

    /// Shows a view: its section box or plan range and its camera, animated; false without a
    /// model to show.
    fn show_view(&mut self, view: &crate::model::SavedView) -> bool {
        // views are in the file's coordinates, of the model at rest
        if self.flip_up {
            self.flip_up = false;
            self.section = None;
        }
        if self.explode_on {
            self.explode_on = false;
            self.update_explode();
        }
        let Some(world) = self.world_bounds() else { return false };
        let clip = match (view.plan, view.clip) {
            (Some(plan), _) => {
                let mut b = SectionBox::around(&world);
                b.min.z = plan.low;
                b.max.z = plan.high.max(plan.low + 0.01);
                Some(b)
            }
            (None, Some(c)) => Some(c.clamped_to(&world)),
            _ => None,
        };
        if clip.is_some() {
            self.section = clip;
        }
        self.section_on = clip.is_some();
        let frame = clip.map(|c| c.world_bounds()).unwrap_or(world);

        let max_pitch = 89.9f32.to_radians();
        let (yaw, pitch) = match (view.plan, view.camera) {
            (Some(plan), _) => if plan.from_below { ViewPreset::Bottom } else { ViewPreset::Top }.orientation(),
            (None, Some(c)) => {
                let f = Vec3::from_array(c.forward);
                ((-f.y).atan2(-f.x), (-f.z).clamp(-1.0, 1.0).asin())
            }
            _ => ViewPreset::Iso.orientation(),
        };
        let mut to = self.camera;
        to.yaw = yaw;
        to.pitch = pitch.clamp(-max_pitch, max_pitch);
        to.fov_y = 45f32.to_radians();
        match view.camera.filter(|c| c.perspective) {
            // a camera: its own eye and lens
            Some(c) => {
                let (eye, f) = (Vec3::from_array(c.eye), Vec3::from_array(c.forward));
                let d = (Vec3::from_array(frame.center()) - eye).dot(f).max(1.0);
                to.ortho = false;
                to.target = eye + f * d;
                to.distance = d;
                to.scene_radius = world.radius();
                if let Some(fov) = c.fov_y {
                    to.fov_y = fov.clamp(10f32.to_radians(), 120f32.to_radians());
                }
            }
            // Revit's other views are orthographic
            None => {
                to.ortho = true;
                to.fit(&frame, self.aspect());
            }
        }
        self.anim = Some((self.camera, to, Instant::now()));
        true
    }

    /// Turns the camera to (yaw, pitch) and frames the model, animated.
    fn turn_to(&mut self, yaw: f32, pitch: f32) {
        let mut to = self.camera;
        to.yaw = yaw;
        to.pitch = pitch.clamp(-89.9f32.to_radians(), 89.9f32.to_radians());
        let to = self.fitted(to);
        self.anim = Some((self.camera, to, Instant::now()));
    }

    /// Installs a freshly loaded model (also used by the headless screenshot path).
    /// A new, empty project for another tab, with this one's window settings, ribbon tab,
    /// explorer groups and measuring units.
    pub fn new_project(&self) -> App {
        let mut app = App::default();
        app.take_window_settings(self);
        app.tab = self.tab;
        app.show_layers = self.show_layers;
        app.show_properties = self.show_properties;
        app.explorer_open = self.explorer_open;
        app.measure.units = self.measure.units.clone();
        app
    }

    /// What belongs to the window, not to a project, from the project shown before: the theme,
    /// the viewport's size and place, touch input and the remote control.
    pub fn take_window_settings(&mut self, from: &App) {
        self.theme = from.theme;
        self.viewport_size = from.viewport_size;
        self.viewport_origin = from.viewport_origin;
        self.fingers = from.fingers;
        self.remote.enabled = from.remote.enabled;
    }

    /// Whether this project has a model or is loading one: a new file then opens in a tab of its
    /// own.
    pub fn busy(&self) -> bool {
        self.model.is_some() || matches!(self.status, Status::Loading { .. })
    }

    pub fn set_model(&mut self, model: Arc<Model>) {
        self.hidden = Arc::default();
        self.history.clear();
        self.parked_layers = None;
        self.parked_histories.clear();
        self.new_document();
        self.models = vec![ModelEntry::new(model.clone(), 0)];
        self.modeller = modeller::Modeller::default();
        self.set_scene(model.clone());
        self.adopt_elements(&model);
    }

    pub fn add_model(&mut self, model: Arc<Model>) {
        if self.model.is_none() { self.set_model(model); return; }
        if self.models.is_empty() {
            self.models.push(ModelEntry::new(self.model.as_ref().unwrap().clone(), 0));
        }
        let first = self.models.last().map_or(0, |entry| entry.layers.end);
        self.models.push(ModelEntry::new(model, first));
        let sources: Vec<_> = self.models.iter().map(|entry| entry.model.clone()).collect();
        self.set_scene(Arc::new(Model::combine(&sources)));
        self.restore_model_visibility();
    }

    fn restore_model_visibility(&mut self) {
        for entry in &self.models {
            for (layer, shown) in entry.layers.clone().zip(&entry.layer_visibility) {
                self.visible[layer] = entry.visible && *shown;
            }
        }
        self.refresh_structural_visibility();
    }

    fn set_layer_visible(&mut self, layer: usize, shown: bool) {
        for entry in &mut self.models {
            if entry.layers.contains(&layer) {
                if shown && !entry.visible {
                    entry.visible = true;
                    for (i, value) in entry.layers.clone().zip(&entry.layer_visibility) { self.visible[i] = *value; }
                }
                entry.layer_visibility[layer - entry.layers.start] = shown;
                break;
            }
        }
        if let Some(value) = self.visible.get_mut(layer) { *value = shown; }
        self.refresh_structural_visibility();
    }

    fn set_scene(&mut self, model: Arc<Model>) {
        // the views' tabs close: the 3D view's layers and undo come back first
        self.load_view(None);
        self.parked_histories.clear();
        self.context_menu = None;
        self.visible = model.layers.iter().map(|l| !l.hidden).collect();
        self.status = Status::Loaded(format!(
            "Loaded {} in {:.2} s",
            model.file_name(),
            model.load_time.as_secs_f32()
        ));
        self.document.recognise(&model);
        self.model = Some(model);
        self.section = None;
        self.section_on = false;
        self.plane = None;
        self.plane_picking = false;
        self.active_view = None;
        self.active_plan = None;
        self.open_plans.clear();
        self.model_view = None;
        self.open_doc_sheets.clear();
        self.active_doc_sheet = None;
        self.cuts.clear();
        self.sheet_pick = None;
        self.cut_edit = None;
        self.view_typing = None;
        self.mark_pick = None;
        self.open_schedules.clear();
        self.active_schedule = None;
        self.close_ifc();
        self.schedules.clear();
        self.schedules_pending = false;
        self.open_sheets.clear();
        self.active_sheet = None;
        self.explorer_open[0] = false;
        self.sel.clear();
        self.selected = None;
        self.selection.clear();
        self.picked_grid = None;
        self.structural.reset();
        self.measure.reset();
        self.measurements.clear();
        self.flip_up = false;
        self.layer_filter.clear();
        self.explode_index = self.model.as_deref().map(|m| Arc::new(ExplodeIndex::new(m)));
        self.explode_on = false;
        self.explode = [0.0; 3];
        self.update_explode();
        self.camera.set_preset(self.initial_view.take().unwrap_or(ViewPreset::Iso));
        self.fit();
    }

    /// For `--window-screenshot`: give the window time to render, then capture it.
    pub fn shot_after_delay(&self) -> Task<Message> {
        if self.window_shot.is_none() {
            return Task::none();
        }
        Task::perform(async { std::thread::sleep(std::time::Duration::from_millis(1500)) }, |_| Message::TakeShot)
    }

    /// Starts loading a model, unless one is loading already.
    fn open(&mut self, source: loaders::Source, add: bool) -> Task<Message> {
        self.pending_open = None;
        if matches!(self.status, Status::Loading { .. }) {
            return Task::none();
        }
        // an IFCX file added to an IFCX model layers over it: the files load again as one model
        let last = self.models.last().map(|entry| entry.model.id).or(self.model.as_ref().map(|m| m.id));
        let composes = loaders::composes(&source);
        let restack = add && composes && !self.layer_stack.0.is_empty() && last.is_some() && last == self.layer_stack.1;
        let mut sources = if restack { self.layer_stack.0.clone() } else { Vec::new() };
        sources.push(source.clone());
        self.loading_stack = (sources.clone(), restack);
        self.adding_model = add;
        let progress = Arc::new(loaders::Progress::default());
        self.status = Status::Loading { file: source.name(), since: Instant::now(), progress: progress.clone() };
        load_task(sources, progress)
    }

    /// Puts a model in the place of the last one (an IFCX stack with a file added).
    fn replace_last_model(&mut self, model: Arc<Model>) {
        if self.models.len() <= 1 {
            // the same project: the levels added stay
            let document = std::mem::take(&mut self.document);
            self.set_model(model.clone());
            self.document = document;
            self.document.recognise(&model);
        } else {
            self.models.pop();
            self.add_model(model);
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let before = changes_visibility(&message).then(|| self.view_state());
        let shown = before.is_some() || matches!(message, Message::Undo | Message::Redo);
        let task = self.handle(message);
        if let Some(before) = before.filter(|b| *b != self.view_state()) {
            self.history.record(before);
        }
        if shown {
            self.keep_view_layers();
        }
        self.keep_views_in_3d();
        self.refresh_hover();
        // the structural hover goes when the camera moves (it no longer lies under the cursor) and
        // while measuring or placing a section plane
        if self.structural.note_camera(self.camera) || self.measure.tool.is_some() || self.plane_picking {
            self.set_structural_hover(None);
        }
        // the IFC tab follows the element selected in the model
        let task = if self.ifc.shown { Task::batch([task, self.ifc_follow()]) } else { task };
        match self.build_edges() {
            Some(edges) => Task::batch([task, edges]),
            None => task,
        }
    }

    /// A measure tool is on: builds the edges of the shown layers that have none yet, in the
    /// background, so the cursor does not wait for them.
    fn build_edges(&mut self) -> Option<Task<Message>> {
        if self.measure.tool.is_none() || self.measure.building_edges {
            return None;
        }
        let model = self.model.clone()?;
        let layers = self.measure.edges_to_build((0..model.layers.len()).filter(|&l| self.visible.get(l) == Some(&true)));
        if layers.is_empty() {
            return None;
        }
        self.measure.building_edges = true;
        Some(edges_task(model, layers))
    }

    fn hover_scene(&self) -> HoverScene {
        (self.active_section(), self.plane, self.visible.clone(), self.hidden.clone())
    }

    fn hover_scene_holds(&self) -> bool {
        self.hover_scene.as_ref().is_some_and(|(section, plane, visible, hidden)| {
            *section == self.active_section() && *plane == self.plane && *visible == self.visible && Arc::ptr_eq(hidden, &self.hidden)
        })
    }

    /// The section or the shown layers changed under the hover: look again where the cursor rests.
    fn refresh_hover(&mut self) {
        if self.measure.hover.is_none() || self.hover_scene_holds() {
            return;
        }
        if let Some(h) = self.measure.hover.take() {
            self.hover_request = Some((h.cursor.x, h.cursor.y, h.screen.size.0, h.screen.size.1));
            self.hover_last = None;
            self.update_hover();
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenDialog => return pick_model_task(false),
            Message::AddDialog => return pick_model_task(true),
            Message::Picked(None) => {}
            Message::AddPicked(None) => {}
            Message::Picked(Some(source)) => return self.open(source, false),
            Message::AddPicked(Some(source)) => return self.open(source, true),
            Message::Open(path) => return self.open(loaders::Source::File(path), false),
            Message::Loaded(Ok(model)) => {
                let (mut stack, restack) = std::mem::take(&mut self.loading_stack);
                if !stack.last().is_some_and(loaders::composes) {
                    stack.clear();
                }
                let id = model.id;
                if restack {
                    self.replace_last_model(model);
                } else if self.adding_model {
                    self.add_model(model);
                } else {
                    self.set_model(model);
                }
                self.layer_stack = (stack, Some(id));
                self.adding_model = false;
                self.remote_loaded();
                return Task::batch([self.compute_schedules(), self.shot_after_delay()]);
            }
            Message::Loaded(Err(e)) => {
                self.adding_model = false;
                self.loading_stack = (Vec::new(), false);
                self.status = Status::Error(e);
                self.remote_loaded();
                return self.shot_after_delay();
            }
            Message::Remote(call) => return self.remote(call),
            Message::RemoteShot(shot) => self.remote_shot(shot),
            Message::ClearMeasurements => self.measurements.clear(),
            Message::NewModel(level) => self.new_model(level),
            Message::ModelTool(tool) => self.set_model_tool(tool),
            Message::Connect(kind, name) => self.connect_selected(kind, name),
            Message::SaveDialog => {
                let Some(model) = &self.model else { return Task::none() };
                if matches!(self.status, Status::Loading { .. } | Status::Saving { .. }) {
                    return Task::none();
                }
                return save_dialog_task(model);
            }
            Message::SavePicked(None) => {}
            // the dialog asked before replacing a file
            Message::SavePicked(Some(path)) => return self.start_save(path, true),
            Message::Saved(result) => {
                self.remote_saved(&result);
                self.status = match result {
                    Ok((path, secs)) => {
                        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        Status::Loaded(format!("Saved {file} in {secs:.2} s"))
                    }
                    Err(e) => Status::Error(e),
                };
            }
            Message::TakeShot => {
                return window::latest().and_then(window::screenshot).map(Message::Shot);
            }
            Message::Shot(shot) => {
                if let Some(path) = self.window_shot.take() {
                    match crate::write_png(&path, shot.size.width, shot.size.height, &shot.rgba) {
                        Ok(()) => eprintln!("wrote {}", path.display()),
                        Err(e) => eprintln!("error: {e}"),
                    }
                    return iced::exit();
                }
            }
            Message::Mode(m) => self.mode = m,
            Message::ToggleGrid => self.grid = !self.grid,
            Message::ToggleFlipUp => {
                self.flip_up = !self.flip_up;
                self.update_explode();
                // the box, the plane and the measurements are in world space: start again
                self.section = None;
                self.plane = None;
                self.measure.reset();
                if self.section_on {
                    self.section = self.world_bounds().map(|b| SectionBox::around(&b));
                }
                self.fit();
            }
            Message::Theme(t) => self.theme = t,
            Message::Tab(t) => self.tab = t,
            Message::Preset(p) => {
                let (yaw, pitch) = p.orientation();
                self.turn_to(yaw, pitch);
            }
            Message::Cube(CubeEvent::Orbit { dx, dy }) => {
                self.anim = None;
                self.camera.orbit(dx, dy);
            }
            Message::Cube(CubeEvent::Snap(target)) => match target {
                CubeTarget::Face(p) => {
                    let (yaw, pitch) = p.orientation();
                    self.turn_to(yaw, pitch);
                }
                CubeTarget::Edge(d) | CubeTarget::Corner(d) => {
                    let (yaw, pitch) = view_from(d);
                    self.turn_to(yaw, pitch);
                }
                CubeTarget::Home => {
                    let (yaw, pitch) = ViewPreset::Iso.orientation();
                    self.turn_to(yaw, pitch);
                }
                CubeTarget::Lens => self.camera.ortho = !self.camera.ortho,
            },
            Message::Ortho(o) => self.camera.ortho = o,
            Message::Fit => self.fit(),
            Message::ToggleLayersPanel => self.show_layers = !self.show_layers,
            Message::TogglePropertiesPanel => self.show_properties = !self.show_properties,
            Message::Layer(i, v) => self.switch_layers(&[i], v),
            Message::ModelVisible(i, shown) => {
                if let Some(entry) = self.models.get_mut(i) {
                    entry.visible = shown;
                    self.sel.clear();
                    self.selection_changed();
                    self.set_structural_selection(Vec::new());
                    self.restore_model_visibility();
                    self.cut_all_again();
                }
            }
            Message::AllLayers(v) => {
                let layers = self.layers_found();
                self.switch_layers(&layers, v);
                // unfiltered in the viewport, it shows or hides the structural objects too (a
                // model of only those has no layers: its entry follows)
                if self.layers_query.trim().is_empty() && !matches!(self.layers_of(), Some(layers::LayersOf::Placed(_))) {
                    self.structural.all_hidden = !v;
                    for entry in self.models.iter_mut().filter(|e| e.layers.is_empty()) {
                        entry.visible = v;
                    }
                    self.refresh_structural_visibility();
                }
            }
            Message::ToggleLayerGroup(group) => {
                if !self.open_layer_groups.remove(&group) {
                    self.open_layer_groups.insert(group);
                }
            }
            Message::LayerGroup(group, v) => {
                if let Some(m) = &self.model {
                    let layers: Vec<usize> = (0..m.layers.len()).filter(|&i| layer_group(&m.layers[i].name).0 == Some(group.as_str())).collect();
                    self.switch_layers(&layers, v);
                }
            }
            Message::LayerFilter(f) => self.layer_filter = f,
            Message::SideTab(tab) => self.side_tab = tab,
            Message::LayersQuery(q) => self.layers_query = q,
            Message::ViewField(field, typed) => self.type_view_field(field, typed),
            Message::ViewFieldDone => self.view_typing = None,
            Message::Navigate(n) => {
                if !matches!(n, Navigate::Hover { .. } | Navigate::HoverOff | Navigate::Resized { .. } | Navigate::Context { .. }) {
                    self.context_menu = None;
                }
                match n {
                // a floor plan is not turned: dragging moves it
                Navigate::Orbit { dx, dy } if self.active_plan.is_some() => {
                    self.anim = None;
                    self.measure.hover = None;
                    self.camera.pan(dx, dy, self.viewport_size.1)
                }
                Navigate::Orbit { dx, dy } => {
                    self.anim = None;
                    self.measure.hover = None;
                    self.camera.orbit(dx, dy)
                }
                Navigate::Pan { dx, dy, height } => {
                    self.anim = None;
                    self.measure.hover = None;
                    self.camera.pan(dx, dy, height)
                }
                Navigate::Zoom { steps, x, y, width, height } => {
                    self.anim = None;
                    // the viewport looks again on the next frame
                    self.measure.hover = None;
                    let ndc = (2.0 * x / width.max(1.0) - 1.0, 1.0 - 2.0 * y / height.max(1.0));
                    self.camera.zoom_at(steps, ndc, width / height.max(1.0));
                }
                Navigate::Resized { x, y, width, height } => {
                    self.viewport_origin = (x, y);
                    self.viewport_size = (width, height);
                }
                Navigate::Pick { x, y, width, height, add } => self.pick(x, y, width, height, add),
                Navigate::Context { x, y, width, height } => self.context_click(x, y, width, height),
                Navigate::Box { x0, y0, x1, y1, width, height, done } => {
                    let rect = (Vec2::new(x0, y0), Vec2::new(x1, y1));
                    self.box_select = (!done).then_some(rect);
                    if done {
                        self.select_box(rect, width, height);
                    }
                }
                Navigate::Hover { x, y, width, height } => {
                    self.hover_request = Some((x, y, width, height));
                    self.update_hover();
                    if self.measure.tool.is_none() && !self.plane_picking {
                        return self.hover_structural(x, y, width, height);
                    }
                }
                Navigate::HoverOff => {
                    self.hover_request = None;
                    self.measure.hover = None;
                    self.structural_hover_off();
                }
                Navigate::Pinch { dx, dy, steps, x, y, width, height } => {
                    self.anim = None;
                    self.measure.hover = None;
                    self.camera.pan(dx, dy, height);
                    let ndc = (2.0 * x / width.max(1.0) - 1.0, 1.0 - 2.0 * y / height.max(1.0));
                    self.camera.zoom_at(steps, ndc, width / height.max(1.0));
                }
                }
            }
            Message::ContextMenu(item) => {
                self.context_menu = None;
                let message = match item {
                    MenuItem::Hide => Message::HideSelection,
                    MenuItem::Isolate => Message::IsolateSelection,
                    MenuItem::ShowAll => Message::ShowAllElements,
                    MenuItem::SelectLayer => Message::SelectLayerOfSelection,
                    MenuItem::Undo => Message::Undo,
                    MenuItem::ClearSelection => {
                        self.sel.clear();
                        self.selection_changed();
                        self.set_structural_selection(Vec::new());
                        return Task::none();
                    }
                };
                return self.update(message);
            }
            Message::CloseContextMenu => self.context_menu = None,
            Message::ApplyView(i) => self.apply_view(i),
            Message::ToggleGrids => {
                self.show_grids = !self.show_grids;
                if !self.show_grids {
                    self.picked_grid = None;
                }
            }
            Message::ToggleExplorer(i) => {
                if let Some(open) = self.explorer_open.get_mut(i) {
                    *open = !*open;
                }
            }
            Message::ExplorerTab(tab) => self.explorer_tab = tab,
            Message::NewLevel(word) => self.new_level(word),
            Message::LevelName(name) => {
                if let Some(form) = &mut self.level_form {
                    form.name = name;
                }
            }
            Message::LevelElevation(elevation) => {
                if let Some(form) = &mut self.level_form {
                    form.elevation = elevation;
                }
            }
            Message::AddLevel => self.add_level(),
            Message::CancelLevel => self.level_form = None,
            Message::RemoveLevel(path) => {
                self.document.remove_level(&path);
            }
            Message::NewPlan(level) => self.new_plan(&level),
            Message::OpenPlan(plan) => self.open_plan(&plan),
            Message::RemovePlan(plan) => {
                self.close_plan(&plan);
                self.document.remove_plan(&plan);
                self.cuts.remove(&*plan);
            }
            Message::NewSheet(name) => self.new_sheet(name),
            Message::ArrangeSheet(sheet) => self.arrange_sheet(&sheet),
            Message::FillSheets(names) => self.fill_sheets(names),
            Message::OpenDocSheet(sheet) => self.open_doc_sheet(&sheet),
            Message::RemoveSheet(sheet) => {
                self.close_doc_sheet(&sheet);
                self.document.remove_sheet(&sheet);
            }
            Message::PlaceOnSheet(sheet, plan) => self.place_on_sheet(&sheet, &plan),
            Message::MoveOnSheet(sheet, i, at) => {
                self.document.move_placed(&sheet, i, at);
            }
            Message::DimensionTool(on) => {
                self.dimension_tool = on;
                if on {
                    self.section_tool = false;
                    self.note_tool = false;
                    // the view in its tab is cut, to snap to its lines
                    if let Some(view) = self.shown_view().map(str::to_string) {
                        self.cut_view(&view);
                    }
                }
                self.pick_on_sheet(None);
                self.mark_pick = None;
            }
            Message::NoteTool(on) => {
                self.note_tool = on;
                if on {
                    self.section_tool = false;
                    self.dimension_tool = false;
                    if self.mark_pick.is_some() || self.sheet_pick.is_some() {
                        self.note_text.clear();
                    }
                }
                self.pick_on_sheet(None);
                self.mark_pick = None;
            }
            Message::NoteText(wording) => self.note_text_changed(wording),
            Message::AddNote(view, at, wording) => {
                self.document.add_note(&view, at, &wording);
            }
            Message::MoveNote(view, k, at) => {
                self.document.move_note(&view, k, at);
            }
            Message::PickMark(pick) => self.pick_mark(pick),
            Message::RemoveMark => self.remove_mark(),
            Message::FlipDimension => self.flip_dimension(),
            Message::StandardDimensions(view) => {
                self.standard_dimensions(&view);
            }
            Message::AddUnderlay(level) => return self.add_underlay(level),
            Message::UnderlayRead(level, name, read) => self.underlay_read(&level, &name, read),
            Message::ToggleUnderlay(i) => self.toggle_underlay(i),
            Message::RemoveUnderlay(i) => self.remove_underlay(i),
            Message::ToggleViewsIn3d => self.views_in_3d = !self.views_in_3d,
            Message::UnderlayScale(i) => self.next_underlay_scale(i),
            Message::ExportMenu(open) => self.export_menu = open,
            Message::ExportDrawing(format) => {
                self.export_menu = false;
                return self.export_drawing(format);
            }
            Message::Exported(None) => {}
            Message::Exported(Some(result)) => {
                self.status = match result {
                    Ok((path, secs)) => {
                        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        Status::Loaded(format!("Exported {file} in {secs:.2} s"))
                    }
                    Err(e) => Status::Error(e),
                };
            }
            Message::DeletePicked => {
                if self.delete_modelled() {
                    // modelled elements selected: they go
                } else if self.active_doc_sheet.is_some() {
                    if self.sheet_pick.is_some() {
                        self.remove_picked();
                    }
                } else if self.mark_pick.is_some() {
                    self.remove_mark();
                }
            }
            Message::PickOnSheet(pick) => self.pick_on_sheet(pick),
            Message::GrabView(view) => self.grab_view(view),
            Message::ReleaseView(view) => {
                let carried = self.dragging_view.take();
                if carried.is_none_or(|c| c == view) {
                    self.open_plan(&view);
                }
            }
            Message::DropOnSheet(view, at) => self.drop_on_sheet(&view, at),
            Message::LetGoView => self.dragging_view = None,
            Message::CropPlan(plan, crop) => {
                if self.document.set_crop(&plan, crop) {
                    self.fit_plan_section(&plan);
                }
            }
            Message::RemovePicked => self.remove_picked(),
            Message::SetScale(sheet, i, scale) => {
                self.document.set_scale(&sheet, i, scale);
            }
            Message::SetPaper(sheet, paper) => {
                self.document.set_paper(&sheet, paper);
            }
            Message::CutText(text) => {
                if let Some((_, t)) = &mut self.cut_edit {
                    *t = text;
                }
            }
            Message::ApplyCut => self.apply_cut(),
            Message::AddDimension(plan, from, to) => {
                self.document.add_dimension(&plan, from, to);
            }
            Message::SelectTab(i) => return self.select_tab(i),
            Message::RemoveSection(path) => {
                self.close_plan(&path);
                self.document.remove_section(&path);
                self.cuts.remove(&*path);
            }
            Message::MakeElevations(names) => self.make_elevations(names),
            Message::NewSection(from, to, word) => {
                self.section_tool = false;
                self.new_section(from, to, word);
            }
            Message::SectionTool(on) => {
                self.section_tool = on;
                if on {
                    self.dimension_tool = false;
                    self.note_tool = false;
                    // a floor plan in its tab is cut, to snap to its lines
                    if let Some(view) = self.shown_view().map(str::to_string) {
                        self.cut_view(&view);
                    }
                }
                self.pick_on_sheet(None);
            }
            Message::CloseTab(i) => return self.close_tab(i),
            Message::OpenSheet(i) => return self.open_sheet(i),
            Message::ShowSheet(i) => return self.show_sheet(i),
            Message::CloseSheet(i) => return self.close_sheet(i),
            Message::SheetView(view) => return self.sheet_view(view),
            Message::OpenSchedule(i) => return self.open_schedule(i),
            Message::ShowSchedule(i) => return self.show_schedule(i),
            Message::CloseSchedule(i) => return self.close_schedule(i),
            Message::ScheduleRow(s, r) => self.select_row(s, r),
            Message::ScheduleScroll(v) => return self.scrolled(v),
            Message::OpenIfc => return self.open_ifc(),
            Message::CloseIfc => self.close_ifc(),
            Message::IfcView(view) => return self.ifc_show_view(view),
            Message::IfcScroll(view, v) => return self.ifc_scrolled(view, v),
            Message::IfcPick(node) => self.ifc_pick(node),
            Message::IfcToggle(key) => self.ifc_toggle(key),
            Message::IfcRelation(relation) => return self.ifc_relation(relation),
            Message::IfcQuery(query) => return self.ifc_query(query),
            Message::IfcNext(forward) => return self.ifc_next(forward),
            Message::Schedules(id, schedules) => {
                if self.model.as_ref().is_some_and(|m| m.id == id) {
                    self.schedules = schedules;
                    self.schedules_pending = false;
                }
            }
            Message::ToggleSection => {
                let world = self.world_bounds();
                // switched on over a level or upright section plane: the plane becomes a face of
                // a box round the model, instead of a second cut
                if !self.section_on {
                    if let Some(b) = self.plane.zip(world).and_then(|(plane, world)| SectionBox::from_plane(&plane, &world)) {
                        self.section = Some(b);
                        self.plane = None;
                    }
                }
                if self.section.is_none() {
                    self.section = world.map(|b| SectionBox::around(&b));
                }
                self.section_on = !self.section_on && self.section.is_some();
            }
            Message::ResetSection => {
                if self.section_on {
                    self.section = self.world_bounds().map(|b| SectionBox::around(&b));
                }
                // the section plane back where it was placed: moves and flips undone
                if self.plane.is_some() {
                    self.plane = self.plane_placed;
                }
            }
            Message::SectionToSelection => {
                if let Some(b) = self.selection_bounds() {
                    self.section = Some(SectionBox::around(&b));
                    self.section_on = true;
                }
            }
            Message::ToggleSectionFill => self.section_fill = !self.section_fill,
            Message::Section(event) => {
                if let Some(s) = self.section.as_mut() {
                    match event {
                        SectionEvent::Move { axis, is_min, delta } => s.move_face(axis, is_min, delta),
                        SectionEvent::Turn(radians) => s.turn_by(radians),
                    }
                }
            }
            Message::SectionSlider(axis, is_min, percent) => {
                if let (Some(world), Some(s)) = (self.world_bounds(), self.section.as_mut()) {
                    let r = s.local_bounds(&world);
                    let to = r.min[axis] + (r.max[axis] - r.min[axis]) * percent / 100.0;
                    let now = if is_min { s.min[axis] } else { s.max[axis] };
                    s.move_face(axis, is_min, to - now);
                }
            }
            Message::SectionTurn(degrees) => {
                if let Some(s) = self.section.as_mut() {
                    s.turn_by(degrees.to_radians() - s.turn);
                }
            }
            Message::Pressed(fingers) => self.fingers = fingers,
            Message::Tick => {
                if let Some((from, to, start)) = self.anim {
                    let t = (start.elapsed().as_secs_f32() / ANIM_SECONDS).min(1.0);
                    let eased = t * t * (3.0 - 2.0 * t);
                    self.camera = from.lerp(&to, eased);
                    if t >= 1.0 {
                        self.anim = None;
                    }
                }
                self.update_hover();
            }
            Message::TogglePlane => {
                if self.plane.is_some() || self.plane_picking {
                    self.plane = None;
                    self.plane_picking = false;
                } else {
                    self.plane_picking = self.model.is_some();
                    self.measure.tool = None;
                    self.measure.picks.clear();
                    self.measure.hover = None;
                }
            }
            Message::FlipPlane => {
                if let Some(plane) = self.plane.as_mut() {
                    plane.flip();
                }
            }
            Message::PlaneMove(delta) => {
                if let (Some(world), Some(plane)) = (self.world_bounds(), self.plane.as_mut()) {
                    plane.move_by(delta, &world);
                }
            }
            Message::PlaneSlider(percent) => {
                if let (Some(world), Some(plane)) = (self.world_bounds(), self.plane.as_mut()) {
                    let (lo, hi) = plane.range(&world);
                    plane.move_by(lo + (hi - lo) * percent / 100.0 - plane.offset, &world);
                }
            }
            Message::ToggleExplode => {
                self.explode_on = !self.explode_on && self.explode_index.is_some();
                // measurements are on the model as it is, not taken apart
                if self.explode_on {
                    self.measure.tool = None;
                    self.measure.picks.clear();
                    self.measure.hover = None;
                }
                self.update_explode();
            }
            Message::ExplodeMode(mode) => {
                self.explode_mode = mode;
                self.update_explode();
            }
            Message::Explode(axis, percent) => {
                if let Some(a) = self.explode.get_mut(axis) {
                    *a = percent.clamp(0.0, 100.0);
                }
                self.update_explode();
            }
            Message::ResetExplode => {
                self.explode = [0.0; 3];
                self.update_explode();
            }
            Message::Measure(tool) => {
                self.measure.toggle(tool);
                if self.measure.tool.is_some() {
                    self.plane_picking = false;
                    if self.explode_on {
                        self.explode_on = false;
                        self.update_explode();
                    }
                }
            }
            Message::MeasureFinish => self.measure.finish(),
            Message::StructuralGrid(built) => return self.structural_grid_built(built),
            Message::MeasureEdges(edges) => {
                if self.model.as_ref().is_some_and(|m| m.id == edges.0) {
                    self.measure.add_edges(edges.1);
                    // snap to them where the cursor rests
                    if let Some(h) = self.measure.hover.take() {
                        self.hover_request = Some((h.cursor.x, h.cursor.y, h.screen.size.0, h.screen.size.1));
                        self.hover_last = None;
                        self.update_hover();
                    }
                }
            }
            Message::MeasureDelete(i) => {
                if i < self.measure.results.len() {
                    self.measure.results.remove(i);
                }
            }
            Message::MeasureClear => {
                self.measure.results.clear();
                self.measure.picks.clear();
                self.measurements.clear();
            }
            Message::MeasureLength(u) => self.measure.units.length = u,
            Message::MeasureArea(u) => self.measure.units.area = u,
            Message::MeasureVolume(u) => self.measure.units.volume = u,
            Message::MeasureDecimals(category, n) => {
                let n = n.min(MAX_DECIMALS);
                let u = &mut self.measure.units;
                match category {
                    Category::Length => u.length_decimals = n,
                    Category::Area => u.area_decimals = n,
                    Category::Volume => u.volume_decimals = n,
                    Category::Angle => u.angle_decimals = n,
                }
            }
            Message::Undo => {
                if let Some(state) = self.history.undo(self.view_state()) {
                    self.restore_view_state(state);
                }
            }
            Message::Redo => {
                if let Some(state) = self.history.redo(self.view_state()) {
                    self.restore_view_state(state);
                }
            }
            Message::SelectWhere(key, value) => {
                if let Some(model) = self.model.clone() {
                    self.sel.clear();
                    self.sel.extend(model.elements_with(&key, &value).into_iter().map(Pickable::Element));
                    self.selection_changed();
                }
            }
            Message::SelectLayerOfSelection => self.select_layer_of_selection(),
            Message::HideSelection => self.hide_selection(),
            Message::IsolateSelection => self.isolate_selection(),
            Message::ShowAllElements => self.show_all_elements(),
            Message::Escape if self.plane_picking => self.plane_picking = false,
            Message::Escape if self.modeller.tool.is_some() || self.modeller.refusal.is_some() => {
                if !self.escape_model_tool() {
                    self.modeller.refusal = None;
                }
            }
            Message::Escape => {
                if self.context_menu.take().is_some() {
                    return Task::none();
                }
                if !self.measure.escape() {
                    self.sel.clear();
                    self.selection_changed();
                    self.set_structural_selection(Vec::new());
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let ticks = if matches!(self.status, Status::Loading { .. } | Status::Saving { .. })
            || self.anim.is_some()
            || self.hover_request.is_some()
        {
            window::frames().map(|_| Message::Tick)
        } else {
            Subscription::none()
        };
        let events = event::listen_with(|event, status, _window| match event {
            iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::Open(path)),
            iced::Event::Touch(iced::touch::Event::FingerPressed { .. }) => Some(Message::Pressed(true)),
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Message::Pressed(false)),
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if status == event::Status::Ignored =>
            {
                use keyboard::key::Named;
                use keyboard::Key;
                // the other keys are shortcuts of the shell's commands (`shell::Studio::commands`),
                // and Escape goes through the shell, which closes a dialog first
                let _ = modifiers;
                match key.as_ref() {
                    Key::Named(Named::Home) => Some(Message::Preset(ViewPreset::Iso)),
                    Key::Named(Named::Enter) => Some(Message::MeasureFinish),
                    // what is picked on a sheet or a view; not while a text field takes the key
                    Key::Named(Named::Delete | Named::Backspace) => Some(Message::DeletePicked),
                    _ => None,
                }
            }
            _ => None,
        });
        let remote = if self.remote.enabled { Subscription::run(crate::remote::serve) } else { Subscription::none() };
        // files from the page: `?file=URL` and drag and drop
        #[cfg(target_arch = "wasm32")]
        let remote = Subscription::batch([remote, crate::web::subscription()]);
        // a view carried from the Views list is let go wherever the button goes up
        let letting_go = if self.dragging_view.is_some() {
            event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => Some(Message::LetGoView),
                _ => None,
            })
        } else {
            Subscription::none()
        };
        Subscription::batch([events, ticks, remote, letting_go])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let p = self.theme.palette();
        let body = row![]
            .push(self.show_layers.then(|| self.layers_panel(p)))
            .push(self.workspace_view(p, standalone_language()))
            .push(self.show_properties.then(|| self.properties_panel(p)))
            .height(Fill);
        column![self.ribbon(p), body, self.status_bar(p)].into()
    }

    /// Selects what is under the cursor: the nearest visible triangle, then its element (or the
    /// whole layer when the format has no elements); empty space clears the selection. While
    /// placing the section plane, puts it on the face under the cursor instead; while measuring,
    /// hands the click to the measure tool, with what is highlighted under the cursor.
    /// A right click (or a long press) at (x, y): what lies there is selected, unless it already
    /// is; empty space keeps the selection. The menu opens there.
    fn context_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let Some(model) = self.model.clone() else { return };
        if self.plane_picking {
            return;
        }
        let (width, height) = (width.max(1.0), height.max(1.0));
        let (origin, dir, hit) = self.cast(&model, x, y, width, height);
        let mm = self.model_matrix();
        let triangle = hit.and_then(|(l, t, s)| face_at(&model.layers[l], t, origin - s, dir).map(|(p, _)| mm.transform_point3(p + s)));
        if let Some(h) = self.pick_structural(x, y, width, height, triangle) {
            if !self.structural.selected.contains(&h.element) {
                self.click_structural(h.element, false);
            }
        } else if let Some(item) = hit.map(|(l, t, _)| pickable_at(&model, l, t)) {
            if !self.sel.contains(item) {
                self.click(Some(item), false);
            }
        }
        self.context_menu = Some(Vec2::new(x, y));
    }

    /// Something is selected: elements or layers, or structural objects picked by a click.
    pub(crate) fn has_selection(&self) -> bool {
        !self.sel.is_empty() || !self.structural.selected.is_empty()
    }

    /// The selection as unit keys, the structural objects picked by a click included.
    fn selection_keys(&self) -> impl Iterator<Item = UnitKey> + '_ {
        let structural = self.structural.selected.iter().map(|&id| Pickable::Element(id));
        self.sel.items().iter().copied().chain(structural).map(|i| self.item_key(i))
    }

    pub(crate) fn pick(&mut self, x: f32, y: f32, width: f32, height: f32, add: bool) {
        let Some(model) = self.model.clone() else { return };
        let (width, height) = (width.max(1.0), height.max(1.0));
        if self.modeller.tool.is_some() {
            return self.model_click(x, y, width, height);
        }
        if let Some(tool) = self.measure.tool.filter(|_| !self.plane_picking) {
            let screen = self.screen(width, height);
            let holds = self.hover_scene_holds() && self.measure.hover.as_ref().is_some_and(|h| h.holds_for(&screen, Vec2::new(x, y)));
            if !holds {
                self.measure.hover = self.measure_hover(&model, x, y, width, height);
            }
            let Some(hover) = self.measure.hover.clone() else { return };
            self.measure.click(&model, &hover, &self.model_matrix());
            // the measured element is also selected (shows its properties)
            if tool == Tool::Volume {
                self.click(Some(pickable_at(&model, hover.layer, hover.triangle)), false);
            }
            return;
        }
        let (origin, dir, hit) = self.cast(&model, x, y, width, height);
        if self.plane_picking {
            let mm = self.model_matrix();
            if let (Some((l, t, s)), Some(world)) = (hit, self.world_bounds()) {
                if let Some((point, normal)) = face_at(&model.layers[l], t, origin - s, dir) {
                    let (point, mut normal) = (mm.transform_point3(point + s), mm.transform_vector3(normal));
                    // the face as seen: its normal towards the viewer
                    if normal.dot(mm.transform_vector3(dir)) > 0.0 {
                        normal = -normal;
                    }
                    if let Some(p) = SectionPlane::on_face(point, normal, &world) {
                        self.plane = Some(p);
                        self.plane_placed = Some(p);
                        self.plane_picking = false;
                    }
                }
            }
            return;
        }
        // a grid line, unless a triangle lies clearly in front of it (its bubbles always); then a
        // structural node or member, the same; Ctrl adds or removes it, keeping the rest of the
        // selection
        let mm = self.model_matrix();
        let triangle = hit.and_then(|(l, t, s)| face_at(&model.layers[l], t, origin - s, dir).map(|(p, _)| mm.transform_point3(p + s)));
        if let Some(g) = self.grid_under(x, y, width, height, triangle) {
            return self.pick_grid(Some(g));
        }
        if let Some(h) = self.pick_structural(x, y, width, height, triangle) {
            self.click_structural(h.element, add);
            return;
        }
        self.click(hit.map(|(l, t, _)| pickable_at(&model, l, t)), add);
    }

    /// Selects the element of triangle `t` of layer `l` (the whole layer without elements), or
    /// nothing.
    /// A click on `hit` (None: empty space). Without `add` it selects only `hit`, so empty space
    /// clears; with `add` (Ctrl) it adds or removes `hit` and keeps the selection on empty space.
    pub(crate) fn click(&mut self, hit: Option<Pickable>, add: bool) {
        // structural objects are selected apart (`app::structural`); a plain click replaces them too
        if !add {
            self.set_structural_selection(Vec::new());
        }
        match (hit, add) {
            (Some(item), true) => self.sel.toggle(item),
            (None, true) => return,
            (hit, false) => self.sel.set(hit),
        }
        self.selection_changed();
    }

    /// After `sel` changed: the shown element (the first picked), the index ranges and the flags
    /// of the units.
    pub(crate) fn selection_changed(&mut self) {
        // a grid is picked alone
        self.picked_grid = None;
        let model = self.model.clone();
        self.selected = model.as_deref().and_then(|m| match self.sel.primary()? {
            Pickable::Element(id) => Some((m.layers.iter().position(|l| l.elements.iter().any(|e| e.0 == id))?, Some(id))),
            Pickable::Layer(l) => (l < m.layers.len()).then_some((l, None)),
        });
        self.selection = model.as_deref().map(|m| self.sel.ranges(m)).unwrap_or_default();
        self.update_units();
    }

    /// The stable key of a unit of the shown model.
    pub(crate) fn unit_key(&self, owner: UnitOwner) -> UnitKey {
        match owner {
            UnitOwner::Element(id) => UnitKey::Element(self.model.as_ref().map_or(crate::model::ElementKey { model: 0, source: id }, |m| m.element_key(id))),
            UnitOwner::LayerRest(l) => {
                let (model, entry) = self.models.iter().enumerate().find(|(_, e)| e.layers.contains(&l)).map_or((0, 0..0), |(m, e)| (m, e.layers.clone()));
                UnitKey::LayerRest { model: model as u32, layer: (l - entry.start) as u32 }
            }
        }
    }

    /// The unit of the shown model with `key`, if it is there.
    #[cfg(test)]
    pub(crate) fn unit_of_key(&self, key: UnitKey) -> Option<usize> {
        let index = self.explode_index.as_ref()?;
        match key {
            UnitKey::Element(k) => index.element_unit(self.model.as_ref()?.element_with_key(k)?),
            UnitKey::LayerRest { model, layer } => {
                let start = self.models.get(model as usize).map_or(0, |e| e.layers.start);
                index.layer_rest_unit(start + layer as usize)
            }
        }
    }

    /// The state the renderer gets per unit: the explode offsets and the selected flags.
    fn update_units(&mut self) {
        let Some(index) = self.explode_index.clone() else {
            self.units = Arc::default();
            return;
        };
        let mut flags = vec![0u8; index.len()];
        for &item in self.sel.items() {
            let unit = match item {
                Pickable::Element(id) => index.element_unit(id),
                Pickable::Layer(l) => index.layer_rest_unit(l),
            };
            if let Some(f) = unit.and_then(|u| flags.get_mut(u)) {
                *f |= UNIT_SELECTED;
            }
        }
        if !self.hidden.is_empty() {
            for (u, f) in flags.iter_mut().enumerate().skip(1) {
                if index.owner(u).is_some_and(|o| self.hidden.contains(&self.unit_key(o))) {
                    *f |= UNIT_HIDDEN;
                }
            }
        }
        self.units = Arc::new(unit_state(&self.explode_offsets, &flags));
    }

    /// Select layer: adds every element (and every layer without elements) of the layers the
    /// selection touches.
    fn select_layer_of_selection(&mut self) {
        let Some(model) = self.model.clone().filter(|_| !self.sel.is_empty()) else { return };
        let layers: std::collections::BTreeSet<usize> = self.selection.iter().map(|r| r.0).collect();
        let items: Vec<Pickable> = layers
            .into_iter()
            .filter_map(|l| model.layers.get(l).map(|layer| (l, layer)))
            .flat_map(|(l, layer)| {
                if layer.elements.is_empty() {
                    vec![Pickable::Layer(l)]
                } else {
                    layer.elements.iter().map(|e| Pickable::Element(e.0)).collect()
                }
            })
            .collect();
        self.sel.extend(items);
        self.selection_changed();
    }

    /// Adds what the box `rect` (viewport pixels of a viewport of width × height) holds to the
    /// selection: elements, and layers without elements; nothing hidden or switched off.
    fn select_box(&mut self, rect: (Vec2, Vec2), width: f32, height: f32) {
        let (Some(model), Some(index)) = (self.model.clone(), self.explode_index.clone()) else { return };
        // the elements on layers that are switched on; with every layer on, all of them
        let all_on = self.visible.iter().all(|&v| v);
        let shown_elements: crate::units::FastHashSet<u64> = if all_on {
            Default::default()
        } else {
            model.layers.iter().enumerate().filter(|(l, _)| self.visible.get(*l).copied().unwrap_or(true)).flat_map(|(_, layer)| layer.elements.iter().map(|e| e.0)).collect()
        };
        let view_proj = self.camera.view_proj(width.max(1.0) / height.max(1.0)) * self.model_matrix();
        // what the section box or plane cuts away wholly is not boxed either
        let (section, plane, mm) = (self.active_section(), self.plane, self.model_matrix());
        let cut_away = |u: usize| {
            if section.is_none() && plane.is_none() {
                return false;
            }
            let Some(b) = index.unit_bounds(u as u32) else {
                return false;
            };
            let o = self.units.get(u).map_or(Vec3::ZERO, |s| Vec3::new(s[0], s[1], s[2]));
            let corners = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| {
                let pick = |a: usize| if i >> a & 1 == 0 { b.min[a] } else { b.max[a] };
                mm.transform_point3(Vec3::new(pick(0), pick(1), pick(2))) + o
            });
            let mut world = Bounds::empty();
            corners.iter().for_each(|c| world.extend(c.to_array()));
            let outside_box = section.is_some_and(|s| {
                let local = s.local_bounds(&world);
                (0..3).any(|a| local.max[a] < s.min[a] || local.min[a] > s.max[a])
            });
            outside_box || plane.is_some_and(|p| corners.iter().all(|&c| p.cuts(c)))
        };
        let hidden = |u: usize| {
            let off = self.units.get(u).is_some_and(|s| s[3] as u8 & UNIT_HIDDEN != 0) || cut_away(u);
            off || match index.owner(u) {
                Some(UnitOwner::Element(id)) => !all_on && !shown_elements.contains(&id),
                Some(UnitOwner::LayerRest(l)) => !self.visible.get(l).copied().unwrap_or(true) || !model.layers[l].elements.is_empty(),
                None => true,
            }
        };
        let units = crate::viewport::box_select::units_in_box(&index, &self.units, view_proj, (width, height), rect, &hidden);
        let items: Vec<Pickable> = units
            .into_iter()
            .filter_map(|u| match index.owner(u)? {
                UnitOwner::Element(id) => Some(Pickable::Element(id)),
                UnitOwner::LayerRest(l) => Some(Pickable::Layer(l)),
            })
            .collect();
        self.sel.extend(items);
        self.selection_changed();
    }

    /// What is shown now, for undo.
    fn view_state(&self) -> ViewState {
        ViewState {
            hidden: self.hidden.clone(),
            layers: self.visible.clone(),
            models: self.models.iter().map(|e| (e.visible, e.layer_visibility.clone())).collect(),
        }
    }

    /// Shows what `s` says. A state from before an Add has fewer layers and models: those it has
    /// are restored, the newer ones stay as they are.
    fn restore_view_state(&mut self, s: ViewState) {
        self.hidden = s.hidden;
        for (shown, value) in self.visible.iter_mut().zip(s.layers) {
            *shown = value;
        }
        for (entry, (visible, layers)) in self.models.iter_mut().zip(s.models) {
            entry.visible = visible;
            for (shown, value) in entry.layer_visibility.iter_mut().zip(layers) {
                *shown = value;
            }
        }
        self.update_units();
    }

    /// The stable key of a selected item.
    fn item_key(&self, item: Pickable) -> UnitKey {
        self.unit_key(match item {
            Pickable::Element(id) => UnitOwner::Element(id),
            Pickable::Layer(l) => UnitOwner::LayerRest(l),
        })
    }

    /// Hide: the selection joins the hidden elements and empties.
    fn hide_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let mut hidden = (*self.hidden).clone();
        hidden.extend(self.selection_keys());
        self.hidden = Arc::new(hidden);
        self.sel.clear();
        self.selection_changed();
        self.set_structural_selection(Vec::new());
    }

    /// Isolate: every unit of the shown models is hidden but the selected ones.
    fn isolate_selection(&mut self) {
        let Some(index) = self.explode_index.clone().filter(|_| self.has_selection()) else { return };
        let keep: UnitKeySet = self.selection_keys().collect();
        let hidden = (1..index.len()).filter_map(|u| index.owner(u)).map(|o| self.unit_key(o)).filter(|k| !keep.contains(k)).collect();
        self.hidden = Arc::new(hidden);
        self.update_units();
    }

    /// Show all: nothing stays hidden by Hide or Isolate; layer and model switches stay as they are.
    fn show_all_elements(&mut self) {
        if !self.hidden.is_empty() {
            self.hidden = Arc::default();
            self.update_units();
        }
    }

    /// Whether vertex `v` of layer `l` belongs to a hidden unit, as drawn now.
    pub(crate) fn hidden_test(&self) -> impl Fn(usize, u32) -> bool + '_ {
        let index = self.explode_index.as_deref();
        move |l, v| index.is_some_and(|ix| self.units.get(ix.unit(l, v)).is_some_and(|s| s[3] as u8 & UNIT_HIDDEN != 0))
    }

    /// The camera's view of a viewport of width × height.
    fn screen(&self, width: f32, height: f32) -> Screen {
        Screen { view_proj: self.camera.view_proj(width / height), size: (width, height) }
    }

    /// Whether a world point is shown: not cut away by the section box or plane.
    fn shown(&self) -> impl Fn(Vec3) -> bool {
        let (section, plane) = (self.active_section(), self.plane);
        move |w: Vec3| section.is_none_or(|s| s.contains(w)) && plane.is_none_or(|pl| !pl.cuts(w))
    }

    /// The ray through viewport point (x, y) (model space) and the nearest visible triangle it
    /// hits: (layer, triangle, how far the exploded view moved it (model space)).
    fn cast(&mut self, model: &Model, x: f32, y: f32, width: f32, height: f32) -> (Vec3, Vec3, Option<(usize, usize, Vec3)>) {
        let mm = self.model_matrix();
        let inv = (self.camera.view_proj(width / height) * mm).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let origin = inv.project_point3(Vec3::new(nx, ny, 0.0));
        let dir = inv.project_point3(Vec3::new(nx, ny, 1.0)) - origin;
        let shown = self.shown();
        let keep = |p: Vec3| shown(mm.transform_point3(p));
        // the blocks hold where the triangles are at rest, not pulled apart
        if !self.explode_on && self.pick_index.as_ref().is_none_or(|ix| ix.model != model.id) {
            self.pick_index = Some(PickIndex::new(model));
        }
        let index = self.pick_index.as_ref().filter(|_| !self.explode_on);
        // the exploded view moves the triangles: their offsets back in model space
        let back = mm.inverse();
        let shift = |l: usize, i: u32| back.transform_vector3(self.explode_offset(l, i));
        let hidden = self.hidden_test();
        let hit = pick_triangle(model, &self.visible, origin, dir, &shift, &keep, &hidden, index).map(|(l, t)| {
            let s = model.layers[l].indices.get(3 * t).map_or(Vec3::ZERO, |&i| shift(l, i));
            (l, t, s)
        });
        (origin, dir, hit)
    }

    /// What a measure click at (x, y) would take.
    fn measure_hover(&mut self, model: &Model, x: f32, y: f32, width: f32, height: f32) -> Option<Hover> {
        let (origin, dir, hit) = self.cast(model, x, y, width, height);
        let (l, t, s) = hit?;
        // the point on the triangle as it is (the exploded view is off while measuring)
        let (point, _) = face_at(&model.layers[l], t, origin - s, dir)?;
        let shown = self.shown();
        let pointer = Pointer { screen: self.screen(width, height), cursor: Vec2::new(x, y), shown: &shown };
        self.hover_scene = Some(self.hover_scene());
        self.measure.hover_at(model, l, t, point, &self.model_matrix(), &pointer)
    }

    /// Looks up what the cursor rests on for the measure tool. The next lookup waits twice as long
    /// as the last one took, so a large model keeps responding (the frame ticks come back for it).
    fn update_hover(&mut self) {
        let Some((x, y, width, height)) = self.hover_request else { return };
        let Some(model) = self.model.clone().filter(|_| self.measure.tool.is_some() && !self.plane_picking) else {
            self.hover_request = None;
            self.measure.hover = None;
            return;
        };
        if self.hover_last.is_some_and(|(end, took)| end.elapsed() < 2 * took) {
            return;
        }
        let start = Instant::now();
        self.measure.hover = self.measure_hover(&model, x, y, width.max(1.0), height.max(1.0));
        self.hover_last = Some((Instant::now(), start.elapsed()));
        self.hover_request = None;
    }

    // ---- ribbon ---------------------------------------------------------------------------

    fn ribbon(&self, p: Palette) -> Element<'_, Message> {
        let tab = |label: &'static str, t: RibbonTab| {
            let active = self.tab == t;
            button(text(label).size(12).font(if active { FONT_BOLD } else { FONT }))
                .padding([4, 14])
                .style(move |_, s| p.ribbon_button(active, s))
                .on_press(Message::Tab(t))
        };
        let file_tab = button(text("Open").size(12).font(FONT_BOLD))
            .padding([4, 14])
            .style(move |_, s| p.primary_button(s))
            .on_press(Message::OpenDialog);
        let tabs = row![
            file_tab,
            tab("Home", RibbonTab::Home),
            tab("View", RibbonTab::View),
            Space::new().width(Fill),
            text("Open 3D Viewer Studio").size(12).font(FONT_BOLD).color(p.accent),
            text(" · iced").size(12).color(p.text_muted),
        ]
        .spacing(4)
        .align_y(Center)
        .padding([4, 8]);

        let content: Element<'_, Message> = match self.tab {
            RibbonTab::Home => row![
                group(
                    p,
                    "File",
                    row![
                        big_button(p, icons::OPEN, "Import", Message::OpenDialog, false),
                        big_button(p, icons::SAVE_AS, "Save as", Message::SaveDialog, false),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Render mode",
                    row![
                        big_button(p, icons::SOLID, "Solid", Message::Mode(RenderMode::Solid), self.mode == RenderMode::Solid),
                        big_button(
                            p,
                            icons::WIREFRAME,
                            "Wireframe",
                            Message::Mode(RenderMode::Wireframe),
                            self.mode == RenderMode::Wireframe
                        ),
                        big_button(
                            p,
                            icons::EDGES,
                            "Edges",
                            Message::Mode(RenderMode::SolidEdges),
                            self.mode == RenderMode::SolidEdges
                        ),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Display",
                    row![
                        big_button(p, icons::GRID, "Grid", Message::ToggleGrid, self.grid),
                        big_button(p, icons::UP_AXIS, "Flip up", Message::ToggleFlipUp, self.flip_up),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Theme",
                    row(ThemeKind::ALL.map(|t| {
                        let active = self.theme == t;
                        let swatch = container(Space::new().width(14).height(14))
                            .style(move |_| Palette::swatch(t.palette().viewport));
                        button(column![swatch, text(t.label()).size(11)].spacing(4).align_x(Center))
                            .padding([6, 6])
                            .width(64)
                            .style(move |_, s| p.ribbon_button(active, s))
                            .on_press(Message::Theme(t))
                            .into()
                    }))
                    .spacing(2)
                    .into()
                ),
            ]
            .into(),
            RibbonTab::View => row![
                group(
                    p,
                    "Camera",
                    row![
                        big_button(p, icons::FIT, "Fit", Message::Fit, false),
                        big_button(p, icons::VIEW_ISO, "Iso", Message::Preset(ViewPreset::Iso), false),
                        big_button(p, icons::VIEW_TOP, "Top", Message::Preset(ViewPreset::Top), false),
                        big_button(p, icons::VIEW_FRONT, "Front", Message::Preset(ViewPreset::Front), false),
                        big_button(p, icons::VIEW_RIGHT, "Right", Message::Preset(ViewPreset::Right), false),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Projection",
                    row![
                        big_button(p, icons::PERSPECTIVE, "Perspective", Message::Ortho(false), !self.camera.ortho),
                        big_button(p, icons::ORTHO, "Orthographic", Message::Ortho(true), self.camera.ortho),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(p, "Explode", row![big_button(p, icons::EXPLODE, "Explode", Message::ToggleExplode, self.explode_on)].into()),
                separator(p),
                group(
                    p,
                    "Panels",
                    row![
                        big_button(p, icons::LAYERS, "Layers", Message::ToggleLayersPanel, self.show_layers),
                        big_button(p, icons::INFO, "Properties", Message::TogglePropertiesPanel, self.show_properties),
                    ]
                    .spacing(2)
                    .into()
                ),
            ]
            .into(),
        };

        column![
            container(tabs).width(Fill).style(move |_| p.title_bar()),
            container(content).width(Fill).height(84).padding([4, 6]).style(move |_| p.ribbon()),
        ]
        .into()
    }

    // ---- panels ---------------------------------------------------------------------------

    fn layers_panel(&self, p: Palette) -> Element<'_, Message> {
        let header = panel_header(p, "Explorer", None);
        panel(p, header, self.explorer_body(p, standalone_language()), 220)
    }

    /// The explorer's Model tab, organised as Revit's project browser: one search field, the
    /// models, then collapsible groups (views by kind, schedules, sheets, rooms by level) with
    /// their counts. The layers are in the right pane, per view.
    pub fn model_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some(model) = &self.model else {
            return text(l.text("Geen model geopend", "No model loaded")).size(12).color(p.text_muted).into();
        };
        let query = self.layer_filter.to_lowercase();
        let searching = !query.is_empty();
        let hit = |s: &str| !searching || s.to_lowercase().contains(&query);
        let indent = |content: Element<'static, Message>| -> Element<'static, Message> {
            container(content).padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 14.0 }).width(Fill).into()
        };
        let subheader = |label: &str| -> Element<'static, Message> {
            container(text(label.to_uppercase()).size(10).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 6.0, right: 0.0, bottom: 2.0, left: 20.0 })
                .into()
        };
        let mut body = column![].spacing(1);

        let plus = svg(icons::handle(icons::ADD)).width(16).height(16).style(move |_, _| svg::Style { color: Some(p.text_secondary) });
        let add = button(container(plus).center(26)).width(26).height(26).padding(0)
            .style(move |_, status| p.ribbon_button(false, status)).on_press(Message::AddDialog);
        body = body.push(container(row![
            text(l.text("Modellen", "Models")).size(12).font(FONT_BOLD).width(Fill),
            text(self.models.len().to_string()).size(11).color(p.text_secondary), add,
        ].spacing(6).align_y(Center)).padding([2, 6]).style(move |_| p.panel_header()));
        for (i, entry) in self.models.iter().enumerate() {
            let shown = entry.visible;
            let eye = svg(icons::handle(if shown { icons::EYE } else { icons::EYE_OFF })).width(16).height(16)
                .style(move |_, _| svg::Style { color: Some(if shown { p.text_secondary } else { p.text_muted }) });
            let label = column![
                text(entry.model.file_name()).size(12).color(if shown { p.text } else { p.text_muted })
                    .wrapping(Wrapping::WordOrGlyph),
                text(entry.model.format.clone()).size(10).color(p.text_secondary),
            ].spacing(2).width(Fill);
            body = body.push(button(row![eye, label].spacing(8).align_y(Center)).padding([6, 8]).width(Fill)
                .style(move |_, status| p.row_button(status)).on_press(Message::ModelVisible(i, !shown)));
        }
        body = body.push(rule::horizontal(1));

        // views, by kind
        if !model.views.is_empty() {
            let kinds = [
                (ViewKind::ThreeD, l.text("3D-aanzichten", "3D views")),
                (ViewKind::Plan, l.text("Plattegronden", "Floor plans")),
                (ViewKind::Section, l.text("Doorsneden", "Sections")),
                (ViewKind::Elevation, l.text("Aanzichten", "Elevations")),
                (ViewKind::Detail, l.text("Details", "Details")),
            ];
            let shown: Vec<usize> = (0..model.views.len())
                .filter(|&i| hit(&model.views[i].name) || model.views[i].level.as_deref().is_some_and(|lv| hit(lv)))
                .collect();
            let open = self.explorer_open[0] || searching;
            body = body.push(group_header(p, l.text("Weergaven", "Views"), Some("beta"), shown.len(), open, Message::ToggleExplorer(0)));
            if open {
                for (kind, label) in kinds {
                    let items: Vec<usize> = shown.iter().copied().filter(|&i| model.views[i].kind == kind).collect();
                    if items.is_empty() {
                        continue;
                    }
                    body = body.push(subheader(label));
                    for i in items {
                        let active = self.active_view == Some(i);
                        let item = button(
                            text(model.views[i].name.clone())
                                .size(12)
                                .color(if active { p.accent } else { p.text })
                                .wrapping(Wrapping::WordOrGlyph)
                                .width(Fill),
                        )
                        .padding([3, 6])
                        .width(Fill)
                        .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                        .on_press(Message::ApplyView(i));
                        body = body.push(indent(item.into()));
                    }
                }
            }
        }

        body = self.schedules_group(p, l, &query, body);
        body = self.sheets_group(p, l, &query, body);

        // rooms, by level (low to high, then rooms without a level)
        let rooms = &model.project.rooms;
        if !rooms.is_empty() {
            let shown: Vec<&crate::model::Room> = rooms
                .iter()
                .filter(|r| hit(&r.name) || hit(&r.number) || r.extra.iter().any(|(_, v)| hit(v)))
                .collect();
            let open = self.explorer_open[1] || searching;
            body = body.push(group_header(p, l.text("Ruimten", "Rooms"), None, shown.len(), open, Message::ToggleExplorer(1)));
            if open {
                let mut levels: Vec<Option<String>> = model.project.levels.iter().map(|(n, _)| Some(n.clone())).collect();
                levels.push(None);
                for level in levels {
                    let items: Vec<&&crate::model::Room> = shown.iter().filter(|r| r.level == level).collect();
                    if items.is_empty() {
                        continue;
                    }
                    body = body.push(subheader(level.as_deref().unwrap_or(l.text("Zonder niveau", "No level"))));
                    for r in items {
                        let extra = r.extra.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(" · ");
                        let item = row![
                            text(r.number.clone()).size(11).color(p.text_muted).width(30),
                            column![text(r.name.clone()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph)]
                                .push((!extra.is_empty()).then(|| text(extra).size(10).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)))
                                .width(Fill),
                        ]
                        .spacing(4)
                        .padding([2, 6]);
                        body = body.push(indent(item.into()));
                    }
                }
            }
        }

        let search = text_input(l.text("Zoeken in verkenner…", "Search explorer…"), &self.layer_filter)
            .on_input(Message::LayerFilter)
            .size(12)
            .padding([5, 8]);
        let id = iced::widget::Id::new("explorer");
        let list = scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).id(id.clone()).height(Fill).spacing(4);
        // a finger drags the list, also where it starts on a row
        column![search, crate::swipe::swipe(id, false, list)]
            .spacing(8)
            .into()
    }

    fn properties_panel(&self, p: Palette) -> Element<'_, Message> {
        let header = panel_header(p, "Properties", None);
        panel(p, header, self.properties_body(p, standalone_language()), 290)
    }

    /// The right pane's Properties tab: the view shown and project data (nothing selected) or the
    /// selection, the section box, then file, geometry and bounds. Labels and values share the
    /// width 2 : 3 and wrap, so long words never run into each other.
    fn properties_tab(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some(model) = &self.model else {
            return text(l.text("Open een model om de eigenschappen te zien", "Open a model to see its properties"))
                .size(12)
                .color(p.text_muted)
                .into();
        };
        let b = transform_bounds(&model.bounds, &self.model_matrix());
        let size = b.size();
        let visible_tris: usize = model
            .layers
            .iter()
            .zip(&self.visible)
            .filter(|(_, v)| **v)
            .map(|(l, _)| l.triangle_count())
            .sum();
        // point clouds count points: shown instead of triangles when the model has no triangles
        let (triangles, points) = (model.triangle_count(), model.point_count());
        let visible_points: usize = model.layers.iter().zip(&self.visible).filter(|(_, v)| **v).map(|(l, _)| l.point_count()).sum();
        let mut content = column![].spacing(5).padding(Padding { top: 0.0, right: 6.0, bottom: 8.0, left: 0.0 });
        let section = |title: &str| -> Element<'static, Message> {
            container(text(title.to_uppercase()).size(11).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 6.0, right: 0.0, bottom: 2.0, left: 0.0 })
                .into()
        };
        let prop = |k: &str, v: String| prop_row(p, k.to_string(), v);

        // project data while nothing is selected
        let pd = &model.project;
        if self.selected.is_none() && self.structural.selected.is_empty() && self.picked_grid.is_none() {
            if let Some(view) = self.view_properties(p, l) {
                content = content.push(section(l.text("Weergave", "View"))).push(view).push(rule::horizontal(1));
            }
            if !pd.info.is_empty() {
                content = content.push(section(l.text("Project", "Project")));
                for (k, v) in &pd.info {
                    let label = match k.as_str() {
                        "number" => l.text("Projectnummer", "Project number"),
                        "name" => l.text("Projectnaam", "Project name"),
                        "client" => l.text("Opdrachtgever", "Client"),
                        "status" => l.text("Status", "Status"),
                        other => other,
                    };
                    content = content.push(prop(label, v.clone()));
                }
                content = content.push(rule::horizontal(1));
            }
            // the levels are not listed here: they have their own tab in the explorer
            if !pd.phases.is_empty() {
                content = content
                    .push(section(l.text("Fasen", "Phases")))
                    .push(text(pd.phases.join(" → ")).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph))
                    .push(rule::horizontal(1));
            }
        }

        // the selection
        if let Some((li, element)) = self.selected {
            content = content.push(section(l.text("Selectie", "Selection")));
            if self.sel.len() > 1 {
                let count = format!("{} {}", self.sel.len(), l.text("elementen geselecteerd", "elements selected"));
                content = content.push(container(text(count).size(12).color(p.text_secondary)).padding([2, 8]));
            }
            let mut props = element.map(|id| model.element_props(id)).unwrap_or_default();
            // a modelled element's definition is kept for the file, not shown
            props.retain(|(k, _)| k != crate::export::ifcx::DEFINITION);
            if props.is_empty() {
                props.push(("category".into(), model.layers[li].name.clone()));
                if let Some(id) = element {
                    props.push(("id".into(), id.to_string()));
                }
            }
            // the order Revit's properties palette uses, unknown keys after
            props.sort_by_key(|(k, _)| PROP_ORDER.iter().position(|o| o.0 == k).unwrap_or(PROP_ORDER.len()));
            for (k, v) in props {
                let label = PROP_ORDER.iter().find(|o| o.0 == k).map_or(k.as_str(), |o| l.text(o.1, o.2)).to_string();
                if element.is_none() {
                    content = content.push(prop_row(p, label, v));
                    continue;
                }
                // every element with this value, one click away
                let pick = button(svg(icons::handle(crate::b2::small("view.fit-selection"))).width(12).height(12))
                    .padding(2)
                    .style(move |_, s| p.row_button(s))
                    .on_press(Message::SelectWhere(k.clone(), v.clone()));
                let pick = iced::widget::tooltip(
                    pick,
                    container(text(l.text("Alles met deze waarde selecteren", "Select all with this value")).size(11)).padding(4).style(move |_| container::Style { background: Some(Background::Color(p.surface)), border: Border { color: p.text_muted, width: 1.0, radius: 3.0.into() }, text_color: Some(p.text), ..container::Style::default() }),
                    iced::widget::tooltip::Position::Left,
                );
                content = content.push(row![container(prop_row(p, label, v)).width(Fill), pick].spacing(2).align_y(Center));
            }
            content = content.push(rule::horizontal(1));
        }

        // the picked grid line
        if let Some(grid) = self.grid_properties(p, l) {
            content = content.push(section(l.text("Stramien", "Grid"))).push(grid).push(rule::horizontal(1));
        }

        // structural objects: properties built from the document
        if let Some(&id) = self.structural.selected.first() {
            content = content.push(section(l.text("Selectie", "Selection")));
            if self.structural.selected.len() > 1 {
                content = content.push(prop(l.text("Geselecteerd", "Selected"), self.structural.selected.len().to_string()));
            }
            for (k, v) in model.element_props(id) {
                content = content.push(prop_row(p, k, v));
            }
            content = content.push(rule::horizontal(1));
        }

        // the measurements
        if self.measure.tool.is_some() || !self.measure.results.is_empty() {
            content = content.push(self.measurements_body(p, l)).push(rule::horizontal(1));
        }

        // the section box
        if let (Some(s), true, Some(world)) = (self.section, self.section_on, self.world_bounds()) {
            let r = s.local_bounds(&world);
            content = content.push(section(l.text("Doorsnedebox", "Section box")));
            let slider_row = |label: String, control: Element<'static, Message>, value: String| -> Element<'static, Message> {
                column![
                    row![
                        text(label).size(12).color(p.text_secondary).width(Fill),
                        text(value).size(11).color(p.text_muted),
                    ]
                    .spacing(6),
                    control,
                ]
                .spacing(2)
                .into()
            };
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                let span = (r.max[axis] - r.min[axis]).max(1e-6);
                for is_min in [true, false] {
                    let v = if is_min { s.min[axis] } else { s.max[axis] };
                    let percent = ((v - r.min[axis]) / span * 100.0).clamp(0.0, 100.0);
                    content = content.push(slider_row(
                        format!("{name} {}", if is_min { "min" } else { "max" }),
                        slider(0.0..=100.0, percent, move |v| Message::SectionSlider(axis, is_min, v)).step(0.1_f32).width(Fill).into(),
                        format!("{v:.2} m"),
                    ));
                }
            }
            let degrees = s.turn.to_degrees();
            content = content
                .push(slider_row(
                    l.text("Draaiing", "Turn").to_string(),
                    slider(-180.0..=180.0, degrees, Message::SectionTurn).step(0.5_f32).width(Fill).into(),
                    format!("{degrees:.1}°"),
                ))
                .push(rule::horizontal(1));
        }

        // the section plane
        if let (Some(plane), Some(world)) = (self.plane, self.world_bounds()) {
            let (lo, hi) = plane.range(&world);
            let percent = ((plane.offset - lo) / (hi - lo).max(1e-6) * 100.0).clamp(0.0, 100.0);
            let n = plane.normal;
            content = content
                .push(section(l.text("Doorsnedevlak", "Section plane")))
                .push(prop(l.text("Normaal", "Normal"), format!("{:.2}, {:.2}, {:.2}", n.x, n.y, n.z)))
                .push(
                    column![
                        row![
                            text(l.text("Positie", "Position")).size(12).color(p.text_secondary).width(Fill),
                            text(format!("{:.2} m", plane.offset)).size(11).color(p.text_muted),
                        ]
                        .spacing(6),
                        slider(0.0..=100.0, percent, Message::PlaneSlider).step(0.1_f32).width(Fill),
                    ]
                    .spacing(2),
                )
                .push(rule::horizontal(1));
        }

        // the exploded view: the mode (storeys only with levels), an amount per axis, reset
        if let (true, Some(index)) = (self.explode_on, &self.explode_index) {
            let mode = index.mode(self.explode_mode);
            let small = |label: &'static str, active: bool, msg: Option<Message>| {
                button(text(label).size(11)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press_maybe(msg)
            };
            let storeys = (index.storeys() > 0).then_some(Message::ExplodeMode(ExplodeMode::Storeys));
            content = content.push(section("Explode")).push(
                row![
                    small(l.text("Verdiepingen", "Storeys"), mode == ExplodeMode::Storeys, storeys),
                    small(l.text("Elementen", "Elements"), mode == ExplodeMode::Elements, Some(Message::ExplodeMode(ExplodeMode::Elements))),
                    Space::new().width(Fill),
                    small(l.text("Herstellen", "Reset"), false, (self.explode != [0.0; 3]).then_some(Message::ResetExplode)),
                ]
                .spacing(4)
                .align_y(Center),
            );
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                let v = self.explode[axis];
                content = content.push(
                    row![
                        text(name).size(12).font(FONT_BOLD).color(AXIS_COLORS[axis]).width(14),
                        slider(0.0..=100.0, v, move |v| Message::Explode(axis, v)).step(1.0_f32).width(Fill),
                        text(format!("{v:.0}%")).size(11).color(p.text_muted).width(34).align_x(iced::alignment::Horizontal::Right),
                    ]
                    .spacing(8)
                    .align_y(Center),
                );
            }
            content = content.push(rule::horizontal(1));
        }

        content = content
            .push(section(l.text("Bestand", "File")))
            .push(prop(l.text("Naam", "Name"), model.file_name()))
            .push(prop(l.text("Formaat", "Format"), model.format.clone()))
            .push(prop(l.text("Grootte", "Size"), format_bytes(model.file_size)))
            .push(prop(l.text("Map", "Folder"), model.path.parent().map(|d| d.display().to_string()).unwrap_or_default()))
            .push(prop(l.text("Laadtijd", "Load time"), format!("{:.2} s", model.load_time.as_secs_f32())))
            .push(rule::horizontal(1))
            .push(section(l.text("Geometrie", "Geometry")))
            .push(prop(l.text("Lagen", "Layers"), model.layers.len().to_string()))
            .push(prop(l.text("Hoekpunten", "Vertices"), format_count(model.vertex_count())))
            .push((triangles > 0 || points == 0).then(|| prop(l.text("Driehoeken", "Triangles"), format_count(triangles))))
            .push((points > 0).then(|| prop(l.text("Punten", "Points"), format_count(points))))
            .push(prop(l.text("Zichtbaar", "Visible"), format_count(if triangles == 0 { visible_points } else { visible_tris })))
            .push(rule::horizontal(1))
            .push(section(l.text("Grenzen (Z omhoog)", "Bounds (Z up)")))
            .push(prop("Min", format_vec(b.min)))
            .push(prop("Max", format_vec(b.max)))
            .push(prop(l.text("Afmeting", "Size"), format_vec(size)))
            .push(prop(
                l.text("Opwaartse as", "Up axis"),
                if model.source_y_up != self.flip_up { l.text("Y (omgezet)", "Y (converted)").into() } else { "Z".into() },
            ));
        // the scrollbar gets its own strip next to the text
        let id = iced::widget::Id::new("properties");
        crate::swipe::swipe(id.clone(), false, scrollable(content).id(id).height(Fill).spacing(4))
    }

    /// The measurements in the properties pane: each result with its details and a delete button,
    /// the totals, and the units with their decimals.
    fn measurements_body(&self, p: Palette, l: Language) -> Element<'static, Message> {
        let m = &self.measure;
        let units = &m.units;
        let heading = |title: &str| -> Element<'static, Message> {
            container(text(title.to_uppercase()).size(11).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 6.0, right: 0.0, bottom: 2.0, left: 0.0 })
                .into()
        };
        let chip = |label: String, active: bool, msg: Option<Message>| {
            button(text(label).size(11)).padding([2, 6]).style(move |_, s| p.ribbon_button(active, s)).on_press_maybe(msg)
        };
        let mut body = column![heading(l.text("Metingen", "Measurements"))].spacing(5);
        if m.results.is_empty() {
            body = body.push(
                text(l.text("Nog geen metingen", "No measurements yet")).size(12).color(p.text_muted),
            );
        }
        for (i, result) in m.results.iter().enumerate() {
            body = body.push(
                row![
                    text(format!("{}. {}", i + 1, result.title(l)))
                        .size(12)
                        .color(p.text_secondary)
                        .width(Fill)
                        .wrapping(Wrapping::WordOrGlyph),
                    text(result.label(units, l)).size(12).font(FONT_BOLD).color(p.text),
                    chip("×".into(), false, Some(Message::MeasureDelete(i))),
                ]
                .spacing(6)
                .align_y(Center),
            );
            for (k, v) in result.details(units, l) {
                body = body.push(container(prop_row(p, k.to_string(), v)).padding(Padding { left: 14.0, ..Padding::ZERO }));
            }
        }
        for (category, total, count) in m.totals() {
            if count > 1 {
                body = body.push(prop_row(
                    p,
                    {
                        // German keeps its nouns capitalised
                        let name = category.label(l);
                        let name = if l == Language::German { name.to_string() } else { name.to_lowercase() };
                        format!("{} ({count})", l.text("Totaal {category}", "Total {category}").replace("{category}", &name))
                    },
                    units.format(l, category, total),
                ));
            }
        }
        if !m.results.is_empty() {
            body = body.push(chip(l.text("Alles wissen", "Clear all").into(), false, Some(Message::MeasureClear)));
        }

        // units and decimals
        body = body.push(heading(l.text("Eenheden", "Units")));
        let unit_row = |category: Category, choices: Element<'static, Message>, decimals: u8| -> Element<'static, Message> {
            let step = |label: &str, to: Option<u8>| chip(label.into(), false, to.map(|n| Message::MeasureDecimals(category, n)));
            row![
                text(category.label(l)).size(12).color(p.text_secondary).width(Fill),
                choices,
                step("−", decimals.checked_sub(1)),
                text(decimals.to_string()).size(11).color(p.text).width(12).align_x(Center),
                step("+", (decimals < MAX_DECIMALS).then_some(decimals + 1)),
            ]
            .spacing(3)
            .align_y(Center)
            .into()
        };
        body = body
            .push(unit_row(
                Category::Length,
                row(LengthUnit::ALL.map(|u| chip(u.label().into(), units.length == u, Some(Message::MeasureLength(u))).into())).spacing(2).into(),
                units.length_decimals,
            ))
            .push(unit_row(
                Category::Area,
                row(AreaUnit::ALL.map(|u| chip(u.label().into(), units.area == u, Some(Message::MeasureArea(u))).into())).spacing(2).into(),
                units.area_decimals,
            ))
            .push(unit_row(
                Category::Volume,
                row(VolumeUnit::ALL.map(|u| chip(u.label().into(), units.volume == u, Some(Message::MeasureVolume(u))).into())).spacing(2).into(),
                units.volume_decimals,
            ))
            .push(unit_row(Category::Angle, text("°").size(11).color(p.text_muted).into(), units.angle_decimals));
        body.into()
    }

    // ---- viewport -------------------------------------------------------------------------

    /// The 3D viewport with its overlays; the shell's workspace.
    pub fn viewport_view(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let settings = RenderSettings {
            mode: self.mode,
            grid: self.grid,
            background: srgb3(p.viewport),
            grid_minor: srgb4(p.grid_minor),
            grid_major: srgb4(p.grid_major),
            model_matrix: self.model_matrix(),
            section: self.active_section(),
            plane: self.plane.zip(self.world_bounds()).map(|(p, w)| p.uniform(&w)),
            caps: self.section_fill,
            grids: self.grids_shown(),
            bounds: self.world_bounds(),
        };
        // the grid lines and levels where the view shown draws them
        let datums = self.datums();
        let segments = Arc::new(datums.iter().map(|d| [d.line.a, d.line.b]).collect::<Vec<_>>());
        let scene = shader(Viewport3D {
            model: self.model.as_ref(),
            visible: &self.visible,
            camera: self.camera,
            settings,
            explode: self.explode_index.as_ref(),
            offsets: &self.units,
            hover: (self.measure.tool.is_some() || self.structural_hover_on()) && !self.plane_picking,
            crosshair: (self.measure.tool.is_some() && !self.plane_picking) || (self.shown_view().is_some() && (self.dimension_tool || self.note_tool)),
            structural_selected: &self.structural.selected_marks,
            structural_hovered: &self.structural.hovered_marks,
            structural_shown: self.structural.shown_marks.as_ref(),
            datum: Some(segments),
            overlay: self.overlay_lines.clone(),
        })
            .width(Fill)
            .height(Fill);

        let overlay: Option<Element<'_, Message>> = match (&self.model, &self.status) {
            (_, Status::Loading { file, since, progress }) => {
                Some(center(loading_card(p, l, file, *since, progress)).into())
            }
            (None, _) => Some(center(self.welcome(p)).into()),
            _ => None,
        };
        // the gestures for whatever pressed last: fingers on a phone or tablet, else the mouse
        let hint = if self.fingers {
            l.text(
                "Tik: selecteren · Eén vinger: draaien · Twee vingers: zoomen en pannen · Lang drukken: menu, en slepen: pannen",
                "Tap: select · One finger: orbit · Two fingers: zoom and pan · Long press: menu, then drag: pan",
            )
        } else {
            l.text(
                "Klik: selecteren · Rechtsklik: menu · Esc: wissen · Middelste knop: pannen · Shift + middelste of links slepen: orbit · Wiel: zoomen · F: passend",
                "Click: select · Right click: menu · Esc: clear · Middle drag: pan · Shift + middle or left drag: orbit · Wheel: zoom · F: fit",
            )
        };
        let hint = container(
            text(hint)
                .size(11)
                .color(p.text_muted),
        )
        .padding(8)
        .align_bottom(Fill)
        .align_left(Fill);
        let cube = (self.model.is_some() && overlay.is_none()).then(|| {
            canvas(CubeOverlay { yaw: self.camera.yaw, pitch: self.camera.pitch, ortho: self.camera.ortho })
                .width(Fill)
                .height(Fill)
        });
        // a view's tab keeps its box without the handles: its crop is set on the sheet
        let section = self.active_section().filter(|_| overlay.is_none() && self.shown_view().is_none()).map(|section| {
            canvas(SectionOverlay { section, view_proj: self.camera.view_proj(self.aspect()) }).width(Fill).height(Fill)
        });
        let plane = self.plane.zip(self.world_bounds()).filter(|_| overlay.is_none() && !self.plane_picking).map(|(plane, world)| {
            canvas(PlaneOverlay { plane, world, view_proj: self.camera.view_proj(self.aspect()) }).width(Fill).height(Fill)
        });
        let banner = if self.plane_picking {
            Some(l.text("Klik op een vlak om door te snijden · Esc: annuleren", "Click a face to cut there · Esc: cancel").to_string())
        } else if let Some((tool, _)) = self.modeller.tool {
            let hint = tool.hint(l, self.modeller.points.len());
            Some(match self.modeller.refusal {
                Some(refusal) => format!("{}: {} · {}", tool.label(l), l.translate(refusal.reason()), hint),
                None => format!("{}: {}", tool.label(l), hint),
            })
        } else if let Some(refusal) = self.modeller.refusal {
            Some(l.translate(refusal.reason()).to_string())
        } else {
            self.measure.tool.map(|tool| {
                format!("{}: {} · Esc: {}", tool.label(l), tool.hint(l, self.measure.picks.len()), l.text("stoppen", "stop"))
            })
        };
        // what a click would take, found in this view (not one the camera left)
        let view_proj = self.camera.view_proj(self.aspect());
        let hover = self.measure.hover.as_ref().filter(|h| {
            self.measure.tool.is_some_and(|t| t != Tool::Volume) && h.screen.view_proj == view_proj && self.hover_scene_holds()
        });
        // hidden while the model is taken apart: they belong to the model as it is
        let measures = (overlay.is_none()
            && !self.explode_on
            && (!self.measure.results.is_empty() || !self.measure.picks.is_empty() || hover.is_some()))
        .then(|| {
            canvas(crate::measure::overlay::MeasureOverlay {
                results: &self.measure.results,
                picks: &self.measure.picks,
                hover,
                tool: self.measure.tool,
                units: self.measure.units,
                language: l,
                view_proj: self.camera.view_proj(self.aspect()),
                ink: p.accent,
                label_bg: p.surface,
                label_text: p.text,
            })
            .width(Fill)
            .height(Fill)
        });
        let picking = banner.filter(|_| overlay.is_none()).map(|banner| {
            container(
                container(text(banner).size(12).color(Color::WHITE))
                    .padding([6, 12])
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
                        border: Border { radius: 4.0.into(), ..Default::default() },
                        ..Default::default()
                    }),
            )
            .padding(12)
            .align_top(Fill)
            .center_x(Fill)
        });
        let grids = (overlay.is_none() && !datums.is_empty()).then(|| {
                canvas(crate::annotations::GridOverlay {
                    datums,
                    view_proj: self.camera.view_proj(self.aspect()),
                    fill: p.surface,
                    ink: p.text_secondary,
                    picked: self.picked_grid,
                    accent: p.accent,
                })
                .width(Fill)
                .height(Fill)
            });
        let view_labels = (overlay.is_none() && !self.views_3d_labels.is_empty()).then(|| {
            canvas(views_in_3d::ViewLabels { labels: self.views_3d_labels.clone(), view_proj: self.camera.view_proj(self.aspect()), ink: p.text })
                .width(Fill)
                .height(Fill)
        });
        let measurements = (!self.measurements.is_empty() && overlay.is_none()).then(|| {
            canvas(crate::annotations::MeasureOverlay {
                items: &self.measurements,
                view_proj: self.camera.view_proj(self.aspect()),
                ink: p.accent,
                fill: p.surface,
                text: p.text,
            })
            .width(Fill)
            .height(Fill)
        });
        let boxed = self.box_select.map(|(from, to)| canvas(crate::viewport::box_select::BoxOverlay { from, to }).width(Fill).height(Fill));
        // the dimensions and texts of the view shown in its tab
        let marks = self.marks_layer(p, l).filter(|_| overlay.is_none()).map(|m| canvas(m).width(Fill).height(Fill));
        let menu = self.context_menu.filter(|_| overlay.is_none()).map(|at| self.context_menu_view(p, l, at));
        stack![scene, hint]
            .push(grids)
            .push(view_labels)
            .push(section)
            .push(plane)
            .push(measures)
            .push(measurements)
            .push(marks)
            .push(boxed)
            .push(picking)
            .push(cube)
            .push(menu)
            .push(overlay)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The right-click menu at viewport point `at`, kept inside the viewport; a click beside it
    /// closes it.
    fn context_menu_view(&self, p: Palette, l: Language, at: Vec2) -> Element<'_, Message> {
        use openaec_ui::design::command::{Shortcut, ShortcutKey};
        let key = |c: char| Shortcut { key: ShortcutKey::Char(c), ctrl: false, shift: false, alt: false }.label();
        let selected = self.has_selection();
        let item = |label: &'static str, keys: &'static str, item: MenuItem, on: bool| -> Element<'static, Message> {
            let color = if on { p.text } else { p.text_muted };
            button(row![text(label).size(12).color(color).width(Fill), text(keys).size(11).color(p.text_muted)].spacing(24))
                .width(Fill)
                .padding([5, 10])
                .style(move |_, s| p.row_button(s))
                .on_press_maybe(on.then_some(Message::ContextMenu(item)))
                .into()
        };
        let items = column![
            item(l.text("Verbergen in aanzicht", "Hide in view"), key('h'), MenuItem::Hide, selected),
            item(l.text("Isoleren", "Isolate"), key('i'), MenuItem::Isolate, selected),
            item(l.text("Alles tonen", "Show all"), Shortcut::ctrl_shift('h').label(), MenuItem::ShowAll, !self.hidden.is_empty()),
            rule::horizontal(1),
            item(l.text("Laag selecteren", "Select layer"), "", MenuItem::SelectLayer, !self.sel.is_empty()),
            item(l.text("Selectie opheffen", "Clear selection"), "Esc", MenuItem::ClearSelection, selected),
            rule::horizontal(1),
            item(l.text("Ongedaan maken", "Undo"), Shortcut::ctrl('z').label(), MenuItem::Undo, self.history.can_undo()),
        ]
        .width(MENU_WIDTH);
        let menu = container(items).padding(4).style(move |_| container::Style { background: Some(Background::Color(p.surface)), ..p.overlay() });
        // at the cursor, moved in where it would stick out of the viewport
        let (w, h) = self.viewport_size;
        let x = at.x.min(w - MENU_WIDTH - 12.0).max(0.0);
        let y = at.y.min(h - MENU_HEIGHT).max(0.0);
        let beside = iced::widget::mouse_area(Space::new().width(Fill).height(Fill))
            .on_press(Message::CloseContextMenu)
            .on_right_press(Message::CloseContextMenu);
        stack![beside, container(menu).padding(Padding { top: y, left: x, ..Padding::ZERO })].width(Fill).height(Fill).into()
    }

    fn welcome(&self, p: Palette) -> Element<'_, Message> {
        let mut exts: Vec<String> = loaders::supported_extensions().iter().map(|e| format!(".{e}")).collect();
        exts.sort();
        container(
            column![
                text("Open 3D Viewer Studio").size(24).font(FONT_BOLD).color(p.text),
                text("Rust + iced edition").size(13).color(p.accent),
                Space::new().height(8),
                text("Open a model or drop a file on the window.").size(13).color(p.text_secondary),
                text(exts.join("  ")).size(12).color(p.text_muted),
                Space::new().height(8),
                button(
                    row![
                        svg(icons::handle(icons::OPEN))
                            .width(18)
                            .height(18)
                            .style(move |_, _| svg::Style { color: Some(p.accent_text) }),
                        text("Open file…").size(13).font(FONT_BOLD),
                    ]
                    .spacing(8)
                    .align_y(Center)
                )
                .padding([8, 18])
                .style(move |_, s| p.primary_button(s))
                .on_press(Message::OpenDialog),
            ]
            .spacing(6)
            .align_x(Center),
        )
        .padding([28, 40])
        .style(move |_| p.overlay())
        .into()
    }

    // ---- status bar -----------------------------------------------------------------------

    fn status_bar(&self, p: Palette) -> Element<'_, Message> {
        let (msg, color) = match &self.status {
            Status::Ready => ("Ready".to_string(), p.status_text),
            Status::Loading { file, since, .. } => {
                (format!("Loading {file}… {:.0} s", since.elapsed().as_secs_f32()), p.accent)
            }
            Status::Saving { file, since } => (format!("Saving {file}… {:.0} s", since.elapsed().as_secs_f32()), p.accent),
            Status::Loaded(s) => (s.clone(), p.status_text),
            Status::Error(e) => (format!("Error: {e}"), iced::Color::from_rgb8(0xF8, 0x71, 0x71)),
        };
        let item = |label: &'static str, value: String| {
            row![text(format!("{label}:")).size(11).color(Palette::dim(p.status_text)), text(value).size(11)].spacing(4)
        };
        let mut right = row![].spacing(16).align_y(Center);
        if let Some(m) = &self.model {
            right = right
                .push(item("Layers", m.layers.len().to_string()))
                .push(item("Vertices", format_count(m.vertex_count())));
            let (triangles, points) = (m.triangle_count(), m.point_count());
            if triangles > 0 || points == 0 {
                right = right.push(item("Triangles", format_count(triangles)));
            }
            if points > 0 {
                right = right.push(item("Points", format_count(points)));
            }
        }
        right = right.push(item(
            "Mode",
            match self.mode {
                RenderMode::Solid => "Solid",
                RenderMode::Wireframe => "Wireframe",
                RenderMode::SolidEdges => "Edges",
            }
            .into(),
        ));
        container(
            row![text(msg).size(11).color(color).width(Fill), right]
                .align_y(Vertical::Center)
                .padding([0, 10]),
        )
        .height(24)
        .align_y(Center)
        .width(Fill)
        .style(move |_| p.status_bar())
        .into()
    }
}

/// The axes on the exploded view's sliders: X and Y as the axis lines of the ground grid (red,
/// green), Z blue.
const AXIS_COLORS: [Color; 3] = [Color::from_rgb8(217, 70, 60), Color::from_rgb8(80, 170, 80), Color::from_rgb8(60, 120, 216)];

/// Element property keys (from the loaders) and their labels, in display order.
const PROP_ORDER: [(&str, &str, &str); 7] = [
    ("category", "Categorie", "Category"),
    ("family", "Familie", "Family"),
    ("type", "Type", "Type"),
    ("level", "Niveau", "Level"),
    ("materials", "Materialen", "Materials"),
    ("id", "Element-id", "Element id"),
    ("class", "Klasse", "Class"),
];

/// Overall progress of a load: each step owns a share of the bar and fills it asymptotically
/// with its running time (the loaders report steps, not percentages).
fn load_fraction(phase: loaders::Phase, seconds: f32, slow: bool) -> f32 {
    use loaders::Phase;
    let (start, share, tau) = match phase {
        Phase::Read => (0.0, 0.3, if slow { 3.0 } else { 0.8 }),
        Phase::Convert => (0.3, 0.6, if slow { 15.0 } else { 1.5 }),
        Phase::Scene => (0.9, 0.09, 0.5),
    };
    start + share * (1.0 - (-seconds / tau).exp())
}

/// Where the ray `origin + t·dir` meets triangle `t` of `layer`, and the triangle's normal
/// (model space, not oriented).
pub(crate) fn face_at(layer: &crate::model::Layer, t: usize, origin: Vec3, dir: Vec3) -> Option<(Vec3, Vec3)> {
    let tri = layer.indices.get(3 * t..3 * t + 3)?;
    let at = |i: u32| layer.positions.get(3 * i as usize..3 * i as usize + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
    let (a, b, c) = (at(tri[0])?, at(tri[1])?, at(tri[2])?);
    let normal = (b - a).cross(c - a).try_normalize()?;
    let along = normal.dot(dir);
    if along.abs() < 1e-12 {
        return None;
    }
    Some((origin + dir * (normal.dot(a - origin) / along), normal))
}

/// Triangles per block of a [`PickIndex`].
const TRI_BLOCK: usize = 256;

/// The bounding box (model space) of every [`TRI_BLOCK`] triangles of each layer, so a ray skips
/// the blocks it misses. Triangles of one element or face follow each other, so the boxes are
/// small. Only for the model at rest: the exploded view moves the triangles.
pub struct PickIndex {
    model: u64,
    layers: Vec<Vec<(Vec3, Vec3)>>,
}

impl PickIndex {
    pub fn new(model: &Model) -> Self {
        let layers = model
            .layers
            .iter()
            .map(|layer| {
                let pos = &layer.positions;
                let at = |i: u32| pos.get(3 * i as usize..3 * i as usize + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
                layer
                    .indices
                    .chunks(3 * TRI_BLOCK)
                    .map(|block| crate::measure::geometry::bounds_of(block.iter().filter_map(|&i| at(i))))
                    .collect()
            })
            .collect();
        Self { model: model.id, layers }
    }
}

/// Whether the ray `origin + t·dir` meets the box `lo`..`hi` for some 0 < t < `max`.
fn ray_meets_box(origin: Vec3, dir: Vec3, (lo, hi): (Vec3, Vec3), max: f32) -> bool {
    let (mut near, mut far) = (0.0f32, max);
    for a in 0..3 {
        // a little room for flat boxes and rounding
        let pad = 1e-5 * (1.0 + hi[a].abs().max(lo[a].abs()));
        let (lo, hi) = (lo[a] - pad, hi[a] + pad);
        if dir[a].abs() < 1e-20 {
            if origin[a] < lo || origin[a] > hi {
                return false;
            }
            continue;
        }
        let (t0, t1) = ((lo - origin[a]) / dir[a], (hi - origin[a]) / dir[a]);
        near = near.max(t0.min(t1));
        far = far.min(t0.max(t1));
        if near > far {
            return false;
        }
    }
    true
}

/// What is shown, as undo keeps it: the elements hidden with Hide or Isolate, every layer's
/// switch, and per model its switch and its own layer switches.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub hidden: Arc<UnitKeySet>,
    pub layers: Vec<bool>,
    pub models: Vec<(bool, Vec<bool>)>,
}

/// The messages that change what is shown, and so go into the undo history when they do.
fn changes_visibility(message: &Message) -> bool {
    matches!(
        message,
        Message::Layer(..)
            | Message::AllLayers(_)
            | Message::LayerGroup(..)
            | Message::ModelVisible(..)
            | Message::HideSelection
            | Message::IsolateSelection
            | Message::ShowAllElements
    )
}

/// What a click on triangle `t` of layer `l` selects: its element, or the layer when the format
/// has no elements.
fn pickable_at(model: &Model, l: usize, t: usize) -> Pickable {
    match model.layers[l].element_at(t) {
        Some(id) => Pickable::Element(id),
        None => Pickable::Layer(l),
    }
}

/// The nearest triangle hit by the ray `origin + t·dir` (t > 0) among the visible layers:
/// (layer, triangle), skipping hits that `keep` rejects (the section box). A triangle is where
/// `shift` (layer, vertex) moves its first vertex: the exploded view, whose units never share a
/// vertex. With `index` (the model at rest), blocks of triangles the ray misses are skipped.
/// Möller–Trumbore, two-sided.
pub fn pick_triangle(
    model: &Model,
    visible: &[bool],
    origin: Vec3,
    dir: Vec3,
    shift: &dyn Fn(usize, u32) -> Vec3,
    keep: &dyn Fn(Vec3) -> bool,
    hidden: &dyn Fn(usize, u32) -> bool,
    index: Option<&PickIndex>,
) -> Option<(usize, usize)> {
    let mut best: Option<(f32, usize, usize)> = None;
    for (l, layer) in model.layers.iter().enumerate() {
        if !visible.get(l).copied().unwrap_or(true) {
            continue;
        }
        let pos = &layer.positions;
        let at = |i: u32| {
            let i = 3 * i as usize;
            Vec3::new(pos[i], pos[i + 1], pos[i + 2])
        };
        let blocks = index.filter(|ix| ix.model == model.id).and_then(|ix| ix.layers.get(l));
        for (b, block) in layer.indices.chunks(3 * TRI_BLOCK).enumerate() {
            // a block the ray misses, or meets only beyond the nearest hit so far: skipped
            let far = best.map_or(f32::INFINITY, |(d, _, _)| d);
            if blocks.and_then(|bx| bx.get(b)).is_some_and(|&bx| !ray_meets_box(origin, dir, bx, far)) {
                continue;
            }
            for (i, tri) in block.chunks_exact(3).enumerate() {
                let t = b * TRI_BLOCK + i;
                if tri.iter().any(|&i| 3 * i as usize + 2 >= pos.len()) || hidden(l, tri[0]) {
                    continue;
                }
                let (a, b, c) = (at(tri[0]), at(tri[1]), at(tri[2]));
                let (e1, e2) = (b - a, c - a);
                let h = dir.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-12 {
                    continue;
                }
                let f = 1.0 / det;
                // the ray moved back instead of the triangle
                let s = origin - shift(l, tri[0]) - a;
                let u = f * s.dot(h);
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let q = s.cross(e1);
                let v = f * dir.dot(q);
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let d = f * e2.dot(q);
                if d > 0.0 && best.is_none_or(|(bd, _, _)| d < bd) && keep(origin + dir * d) {
                    best = Some((d, l, t));
                }
            }
        }
    }
    best.map(|(_, l, t)| (l, t))
}

// ---- small view helpers ---------------------------------------------------------------------

/// The loading card (as the OpenAEC dialogs: 12 px radius, amber accent): format badge, file name
/// and elapsed time; the three steps; a bar that eases within the current step with a moving
/// sheen; the step and percentage; for Revit after a few seconds a hint why it takes a while.
fn loading_card<'a>(
    p: Palette,
    l: Language,
    file: &'a str,
    since: Instant,
    progress: &loaders::Progress,
) -> Element<'a, Message> {
    let mono = openaec_ui::fonts::MONO;
    let (phase, in_phase) = progress.get();
    let elapsed = since.elapsed().as_secs_f32();
    let ext = std::path::Path::new(file).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let slow = matches!(ext.as_str(), "RVT" | "RFA");
    let fraction = load_fraction(phase, in_phase.as_secs_f32(), slow);

    let badge = container(text(ext).size(10).font(mono).color(p.accent)).padding([3, 6]).style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.12, ..p.accent })),
        border: Border { color: Color { a: 0.35, ..p.accent }, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    });
    let name = container(text(file).size(13).font(FONT_BOLD).color(p.text).wrapping(text::Wrapping::None))
        .width(Fill)
        .clip(true);
    let head = row![badge, name, text(format!("{elapsed:.1} s")).size(11).font(mono).color(p.text_muted)]
        .spacing(10)
        .align_y(Center);

    let labels = [
        l.text("Bestand lezen", "Read file"),
        l.text("Geometrie omzetten", "Convert geometry"),
        l.text("Scène opbouwen", "Build scene"),
    ];
    let current = phase as usize;
    let steps = row(labels.iter().enumerate().map(|(i, label)| {
        let (done, active) = (i < current, i == current);
        let number = text((i + 1).to_string()).size(9).font(mono).color(if done {
            p.accent_text
        } else if active {
            p.accent
        } else {
            p.text_muted
        });
        let dot = container(number).center(16).style(move |_| container::Style {
            background: done.then_some(Background::Color(p.accent)),
            border: Border {
                color: if done || active { p.accent } else { p.border },
                width: if active { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            ..Default::default()
        });
        let color = if active {
            p.text
        } else if done {
            p.text_secondary
        } else {
            p.text_muted
        };
        row![dot, text(*label).size(11).font(if active { FONT_BOLD } else { FONT }).color(color).wrapping(text::Wrapping::None)]
            .spacing(6)
            .align_y(Center)
            .width(Fill)
            .into()
    }))
    .spacing(6);

    // bar: the fill in thousandths, a sheen sweeping through it every 1.4 s
    let k = ((fraction * 1000.0).round() as u16).clamp(1, 999);
    let sweep = ((elapsed / 1.4).fract() * 1300.0) as u16;
    let sheen = row![
        Space::new().width(Length::FillPortion(sweep.max(1))),
        container(Space::new().height(6)).width(Length::FillPortion(300)).style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.35, ..Color::WHITE })),
            ..Default::default()
        }),
        Space::new().width(Length::FillPortion(1300u16.saturating_sub(sweep).max(1))),
    ];
    let fill = container(sheen).width(Length::FillPortion(k)).height(6).clip(true).style(move |_| container::Style {
        background: Some(Background::Color(p.accent)),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    });
    let bar = container(row![fill, Space::new().width(Length::FillPortion(1000 - k))])
        .height(6)
        .width(Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.6, ..p.border })),
            border: Border { radius: 3.0.into(), ..Default::default() },
            ..Default::default()
        });
    let step = match phase {
        loaders::Phase::Read => l.text("Bestand lezen…", "Reading file…"),
        loaders::Phase::Convert => l.text("Geometrie omzetten…", "Converting geometry…"),
        loaders::Phase::Scene => l.text("Scène opbouwen…", "Building scene…"),
    };
    let foot = row![
        text(step).size(11).color(p.text_secondary).width(Fill),
        text(format!("{:.0}%", fraction * 100.0)).size(11).font(mono).color(p.text),
    ]
    .align_y(Center);

    let hint = (slow && elapsed > 4.0).then(|| {
        container(
            text(l.text(
                "Revit-modellen worden uit de opgeslagen graphics van het bestand opgebouwd; grote projecten duren tot een minuut.",
                "Revit models are built from the file's saved graphics; large projects take up to a minute.",
            ))
            .size(11)
            .color(p.text_secondary),
        )
        .padding([8, 10])
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.08, ..p.text_muted })),
            border: Border { radius: 8.0.into(), ..Default::default() },
            ..Default::default()
        })
    });

    container(column![head, steps, bar, foot].push(hint).spacing(12))
        .width(420)
        .padding([18, 20])
        .style(move |_| container::Style {
            text_color: Some(p.text),
            background: Some(Background::Color(p.surface)),
            border: Border { color: p.border, width: 1.0, radius: 12.0.into() },
            shadow: Shadow { color: Color { a: 0.18, ..Color::BLACK }, offset: iced::Vector::new(0.0, 8.0), blur_radius: 28.0 },
            ..Default::default()
        })
        .into()
}

/// A property: label and value side by side (2 : 3), both wrapping, so long words never overlap;
/// a long label (a project parameter such as "opdrachtgever_projectmanager") gets its own line
/// with the value below it, instead of being broken mid-word.
fn prop_row(p: Palette, label: String, value: String) -> Element<'static, Message> {
    if label.chars().count() > 16 {
        return column![
            text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph),
            container(text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph))
                .padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 10.0 }),
        ]
        .spacing(1)
        .into();
    }
    row![
        text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)),
        text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(3)),
    ]
    .spacing(8)
    .into()
}

/// The header of an explorer group: open/closed arrow, title, optional tag, count.
/// Separates a group from a layer in a layer name: "Panel Be1 › STUD".
pub const LAYER_GROUP: &str = " › ";

/// The group of a layer name and the layer's own name (the whole name without a group).
fn layer_group(name: &str) -> (Option<&str>, &str) {
    match name.split_once(LAYER_GROUP) {
        Some((group, rest)) if !group.trim().is_empty() && !rest.trim().is_empty() => (Some(group), rest),
        _ => (None, name),
    }
}

fn group_header(p: Palette, title: &str, tag: Option<&'static str>, count: usize, open: bool, msg: Message) -> Element<'static, Message> {
    use openaec_ui::icons::Icon;
    let arrow = svg(svg::Handle::from_memory(if open { Icon::DisclosureExpanded } else { Icon::DisclosureCollapsed }.bytes()))
        .width(12)
        .height(12)
        .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
    button(
        row![arrow, text(title.to_string()).size(12).font(FONT_BOLD).color(p.text)]
            .push(tag.map(|t| text(t).size(10).color(p.accent)))
            .push(Space::new().width(Fill))
            .push(text(count.to_string()).size(11).color(p.text_muted))
            .spacing(6)
            .align_y(Center),
    )
    .padding([6, 4])
    .width(Fill)
    .style(move |_, s| p.row_button(s))
    .on_press(msg)
    .into()
}

fn big_button<'a>(p: Palette, icon: &'static str, label: &'static str, msg: Message, active: bool) -> Element<'a, Message> {
    let tint = if active { p.accent } else { p.text };
    button(
        column![
            svg(icons::handle(icon))
                .width(24)
                .height(24)
                .style(move |_, status| svg::Style {
                    color: Some(if matches!(status, svg::Status::Hovered) { p.accent } else { tint })
                }),
            text(label).size(11),
        ]
        .spacing(4)
        .align_x(Center),
    )
    .padding([6, 8])
    .width(Length::Shrink)
    .style(move |_, s| p.ribbon_button(active, s))
    .on_press(msg)
    .into()
}

fn group<'a>(p: Palette, label: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    column![
        container(content).height(Fill).align_y(Center),
        text(label).size(10).color(p.group_label).font(FONT_BOLD),
    ]
    .spacing(2)
    .padding([0, 6])
    .align_x(Center)
    .into()
}

fn separator<'a>(p: Palette) -> Element<'a, Message> {
    container(rule::vertical(1).style(move |_| rule::Style {
        color: p.border,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }))
    .padding([6, 2])
    .height(Fill)
    .into()
}

fn panel_header<'a>(p: Palette, title: &'static str, badge: Option<String>) -> Element<'a, Message> {
    container(
        row![text(title).size(12).font(FONT_BOLD).color(p.text).width(Fill)]
            .push(badge.map(|b| text(b).size(11).color(p.text_muted)))
            .align_y(Center),
    )
    .padding([7, 10])
    .width(Fill)
    .style(move |_| p.panel_header())
    .into()
}

/// The language of the stand-alone layout (`--screenshot`): English, or the language of
/// `O3V_LANG` (`nl`, `de`, `fr`).
pub(crate) fn standalone_language() -> Language {
    std::env::var("O3V_LANG").ok().and_then(|code| Language::from_code(&code)).unwrap_or(Language::English)
}

/// A side panel of the stand-alone layout, as wide as the shell's pane (explorer 220, properties
/// 290), so `--screenshot` shows what the window shows.
fn panel<'a>(p: Palette, header: Element<'a, Message>, content: Element<'a, Message>, width: u32) -> Element<'a, Message> {
    container(column![header, container(content).padding(10).height(Fill)])
        .width(width)
        .height(Fill)
        .style(move |_| p.panel())
        .into()
}

pub fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn format_bytes(n: u64) -> String {
    match n {
        0..1024 => format!("{n} B"),
        1024..1_048_576 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

fn format_vec(v: [f32; 3]) -> String {
    format!("{:.2}, {:.2}, {:.2}", v[0], v[1], v[2])
}

impl Palette {
    fn dim(c: iced::Color) -> iced::Color {
        iced::Color { a: c.a * 0.7, ..c }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_groups_split_on_the_arrow() {
        assert_eq!(layer_group("Panel Be1 › STUD"), (Some("Panel Be1"), "STUD"));
        assert_eq!(layer_group("Walls"), (None, "Walls"));
        // A name that only starts or ends with the separator is not grouped.
        assert_eq!(layer_group(" › STUD"), (None, " › STUD"));
    }

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view())
    }

    #[test]
    fn the_right_click_menu_sends_its_items() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(10)), false);
        app.context_menu = Some(Vec2::new(300.0, 200.0));
        let mut ui = simulator(&app);
        ui.click("Hide in view").expect("the menu is shown");
        ui.click("Clear selection").expect("its items");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            matches!(messages[..], [Message::ContextMenu(MenuItem::Hide), Message::ContextMenu(MenuItem::ClearSelection)]),
            "{messages:?}"
        );
    }

    #[test]
    fn ribbon_buttons_send_messages() {
        let mut app = App::default();
        app.set_model(two_squares());
        let mut ui = simulator(&app);
        ui.click("Wireframe").expect("Wireframe button");
        ui.click("Grid").expect("Grid button");
        ui.click("Save as").expect("Save as button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(messages[0], Message::Mode(RenderMode::Wireframe)), "{messages:?}");
        assert!(matches!(messages[1], Message::ToggleGrid), "{messages:?}");
        assert!(matches!(messages[2], Message::SaveDialog), "{messages:?}");
    }

    #[test]
    fn an_ifcx_file_added_to_an_ifcx_model_layers_over_it() {
        fn source(name: &str, text: &str) -> loaders::Source {
            loaders::Source::Bytes { name: name.into(), data: Arc::new(text.as_bytes().to_vec()) }
        }
        // the load the task would run, done here
        fn finish(app: &mut App) {
            let model = loaders::load_stack(&app.loading_stack.0, &loaders::Progress::default()).map(Arc::new);
            let _ = app.update(Message::Loaded(model));
        }
        let base = r#"{"header": {"ifcxVersion": "ifcx_alpha"}, "imports": [], "schemas": {}, "data": [
            {"path": "w", "attributes": {"bsi::ifc::class": {"code": "IfcWall"},
             "usd::usdgeom::mesh": {"points": [[0,0,0],[1,0,0],[0,0,1]], "faceVertexIndices": [0,1,2]}}}]}"#;
        let overlay = r#"{"header": {"ifcxVersion": "ifcx_alpha"}, "imports": [], "schemas": {}, "data": [
            {"path": "w", "attributes": {"bsi::ifc::prop::FireRating": "R60"}}]}"#;
        let mut app = App::default();
        let _ = app.update(Message::Picked(Some(source("base.ifcx", base))));
        finish(&mut app);
        let _ = app.update(Message::AddPicked(Some(source("rating.ifcx", overlay))));
        assert_eq!(app.loading_stack.0.len(), 2);
        finish(&mut app);
        assert_eq!(app.models.len(), 1, "composed into the open model");
        let model = app.model.clone().unwrap();
        assert_eq!(model.triangle_count(), 1);
        assert!(model.props.values().any(|p| p.contains("R60")));
        // after another kind of file the next IFCX stands on its own
        let _ = app.update(Message::AddPicked(Some(loaders::Source::Bytes {
            name: "tri.obj".into(),
            data: Arc::new(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec()),
        })));
        finish(&mut app);
        assert_eq!(app.models.len(), 2, "{:?}", app.status);
        let _ = app.update(Message::AddPicked(Some(source("rating.ifcx", overlay))));
        assert_eq!(app.loading_stack.0.len(), 1);
    }

    #[test]
    fn add_keeps_models_separate_and_open_replaces_them() {
        fn source(name: &str, x: f32) -> Arc<Model> {
            let layer = crate::model::Layer {
                name: "Walls".into(),
                positions: vec![x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x, 1.0, 0.0],
                indices: vec![0, 1, 2],
                elements: vec![(7, 0, 3)],
                props: vec![(7, serde_json::json!({"source": name}).to_string())],
                ..Default::default()
            };
            Arc::new(Model::new(name.into(), "OBJ", vec![layer]))
        }
        let mut app = App::default();
        app.set_model(source("first.obj", 0.0));
        let _ = app.update(Message::Layer(0, false));
        app.add_model(source("second.obj", 2.0));
        assert_eq!(app.models.len(), 2);
        assert_eq!(app.visible, vec![false, true]);
        let scene = app.model.as_ref().unwrap();
        let first_id = scene.layers[0].elements[0].0;
        let second_id = scene.layers[1].elements[0].0;
        assert_ne!(first_id, second_id, "IDs from separate files must not collide");
        assert!(scene.props[&first_id].contains("first.obj"));
        assert!(scene.props[&second_id].contains("second.obj"));
        let _ = app.update(Message::ModelVisible(1, false));
        assert_eq!(app.visible, vec![false, false]);
        let _ = app.update(Message::ModelVisible(1, true));
        assert_eq!(app.visible, vec![false, true]);
        let _ = app.update(Message::ModelVisible(0, false));
        let _ = app.update(Message::ModelVisible(0, true));
        assert!(!app.visible[0], "a model toggle preserves its hidden layers");
        let mut ui = simulator(&app);
        ui.click("second.obj").expect("model visibility row");
        assert!(ui.into_messages().any(|m| matches!(m, Message::ModelVisible(1, false))));
        app.set_model(source("replacement.obj", 4.0));
        assert_eq!(app.models.len(), 1);
        assert_eq!(app.model.as_ref().unwrap().file_name(), "replacement.obj");
        assert_eq!(app.visible, vec![true]);
    }

    #[test]
    fn dragging_and_scrolling_the_viewport_navigates() {
        use iced::mouse;
        let mut app = App::default();
        app.status = Status::Loaded(String::new()); // no overlay over the viewport
        app.model = None;
        let mut ui = simulator(&app);
        let start = iced::Point::new(700.0, 300.0);
        ui.point_at(start);
        ui.simulate([iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
        ui.point_at(iced::Point::new(740.0, 310.0));
        ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved { position: iced::Point::new(740.0, 310.0) })]);
        ui.simulate([iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
        ui.simulate([iced::Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 } })]);
        let nav: Vec<Navigate> = ui
            .into_messages()
            .filter_map(|m| match m {
                Message::Navigate(n) => Some(n),
                _ => None,
            })
            .collect();
        assert!(nav.iter().any(|n| matches!(n, Navigate::Orbit { dx, dy } if *dx == 40.0 && *dy == 10.0)), "{nav:?}");
        assert!(nav.iter().any(|n| matches!(n, Navigate::Zoom { steps, .. } if *steps == 1.0)), "{nav:?}");
    }

    #[test]
    fn touch_orbits_pinches_pans_and_taps() {
        use iced::touch::{Event as T, Finger};
        let navigation = |events: &[T], wait: bool| -> Vec<Navigate> {
            let mut app = App::default();
            app.status = Status::Loaded(String::new());
            app.model = None;
            let mut ui = simulator(&app);
            for (i, event) in events.iter().enumerate() {
                if wait && i == 1 {
                    std::thread::sleep(std::time::Duration::from_millis(550));
                }
                ui.simulate([iced::Event::Touch(*event)]);
            }
            ui.into_messages().filter_map(|m| if let Message::Navigate(n) = m { Some(n) } else { None }).collect()
        };
        let (one, two) = (Finger(1), Finger(2));
        let p = |x, y| iced::Point::new(x, y);

        // one finger drags: orbit, and no pick
        let nav = navigation(&[T::FingerPressed { id: one, position: p(700.0, 300.0) }, T::FingerMoved { id: one, position: p(760.0, 320.0) }, T::FingerLifted { id: one, position: p(760.0, 320.0) }], false);
        assert!(nav.iter().any(|n| matches!(n, Navigate::Orbit { dx, dy } if *dx == 60.0 && *dy == 20.0)), "{nav:?}");
        assert!(!nav.iter().any(|n| matches!(n, Navigate::Pick { .. })), "{nav:?}");

        // a tap picks where the finger went down
        let nav = navigation(&[T::FingerPressed { id: one, position: p(700.0, 300.0) }, T::FingerLifted { id: one, position: p(702.0, 301.0) }], false);
        assert!(nav.iter().any(|n| matches!(n, Navigate::Pick { .. })), "{nav:?}");

        // a long press, then a drag: pan (as a right drag)
        let nav = navigation(&[T::FingerPressed { id: one, position: p(700.0, 300.0) }, T::FingerMoved { id: one, position: p(740.0, 300.0) }, T::FingerLifted { id: one, position: p(740.0, 300.0) }], true);
        assert!(nav.iter().any(|n| matches!(n, Navigate::Pan { dx, .. } if *dx == 40.0)), "{nav:?}");
        assert!(!nav.iter().any(|n| matches!(n, Navigate::Orbit { .. } | Navigate::Pick { .. })), "{nav:?}");

        // two fingers spread: zoom in about their middle; the last finger alone does not orbit
        let nav = navigation(
            &[
                T::FingerPressed { id: one, position: p(600.0, 300.0) },
                T::FingerPressed { id: two, position: p(800.0, 300.0) },
                T::FingerMoved { id: two, position: p(1000.0, 300.0) },
                T::FingerLifted { id: one, position: p(600.0, 300.0) },
                T::FingerMoved { id: two, position: p(900.0, 350.0) },
                T::FingerLifted { id: two, position: p(900.0, 350.0) },
            ],
            false,
        );
        assert!(nav.iter().any(|n| matches!(n, Navigate::Pinch { steps, dx, .. } if *steps > 0.0 && *dx == 100.0)), "{nav:?}");
        assert!(!nav.iter().any(|n| matches!(n, Navigate::Orbit { .. } | Navigate::Pick { .. })), "{nav:?}");
    }

    #[test]
    fn picks_the_nearest_triangle_and_its_element() {
        use crate::model::Layer;
        // two unit squares facing +Z, at z = 0 (element 10) and z = 1 (element 20)
        let mut positions = Vec::new();
        for z in [0.0, 1.0] {
            positions.extend_from_slice(&[0.0, 0.0, z, 1.0, 0.0, z, 1.0, 1.0, z, 0.0, 1.0, z]);
        }
        let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices,
            elements: vec![(10, 0, 6), (20, 6, 6)],
            props: vec![(20, r#"{"id":"20","category":"Walls","level":"00 begane grond"}"#.into())],
            ..Layer::default()
        };
        let model = Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer]);
        let still = |_: usize, _: u32| Vec3::ZERO;
        let hit = pick_triangle(&model, &[true], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true, &|_, _| false, None);
        let (l, t) = hit.expect("hit");
        assert_eq!(model.layers[l].element_at(t), Some(20));
        let index = PickIndex::new(&model);
        assert_eq!(pick_triangle(&model, &[true], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true, &|_, _| false, Some(&index)), hit);
        assert_eq!(model.element_ranges(20), vec![(0, 6, 6)]);
        let props = model.element_props(20);
        assert!(props.contains(&("level".to_string(), "00 begane grond".to_string())), "{props:?}");
        assert!(pick_triangle(&model, &[false], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true, &|_, _| false, None).is_none());
        assert!(pick_triangle(&model, &[true], Vec3::new(3.0, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true, &|_, _| false, None).is_none());
        // a section box below z = 0.5 cuts the upper square away: the lower one is hit
        let below = |p: Vec3| p.z < 0.5;
        let (l, t) = pick_triangle(&model, &[true], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &below, &|_, _| false, None).expect("hit");
        assert_eq!(model.layers[l].element_at(t), Some(10));
    }

    #[test]
    fn the_pick_index_skips_blocks_and_finds_the_same_triangle() {
        use crate::model::Layer;
        // 1000 unit squares in a row at x = 2i, facing +Z, and a ray down onto each of a few
        let mut layer = Layer { name: "Row".into(), ..Layer::default() };
        for i in 0..1000u32 {
            let x = 2.0 * i as f32;
            layer.positions.extend_from_slice(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0]);
            layer.indices.extend_from_slice(&[4 * i, 4 * i + 1, 4 * i + 2, 4 * i, 4 * i + 2, 4 * i + 3]);
        }
        let model = Model::new(PathBuf::from("row.obj"), "OBJ", vec![layer]);
        let index = PickIndex::new(&model);
        assert_eq!(index.layers[0].len(), 2000_usize.div_ceil(TRI_BLOCK));
        let still = |_: usize, _: u32| Vec3::ZERO;
        let down = Vec3::new(0.0, 0.0, -1.0);
        for x in [0.3, 255.5, 256.7, 1400.2, 1998.9, 1999.5] {
            let origin = Vec3::new(x, 0.6, 5.0);
            let plain = pick_triangle(&model, &[true], origin, down, &still, &|_| true, &|_, _| false, None);
            assert_eq!(pick_triangle(&model, &[true], origin, down, &still, &|_| true, &|_, _| false, Some(&index)), plain, "x = {x}");
            assert_eq!(plain.is_some(), x.rem_euclid(2.0) < 1.0, "x = {x}");
        }
        // a slanted ray, across many blocks
        let slanted = Vec3::new(-0.3, 0.0, -1.0);
        let origin = Vec3::new(1001.0, 0.5, 2.0);
        assert_eq!(
            pick_triangle(&model, &[true], origin, slanted, &still, &|_| true, &|_, _| false, Some(&index)),
            pick_triangle(&model, &[true], origin, slanted, &still, &|_| true, &|_, _| false, None),
        );
    }

    /// Two unit squares facing +Z side by side: element 10 at x 0..1, element 20 at x 2..3.
    fn two_squares() -> Arc<Model> {
        use crate::model::Layer;
        let mut positions = Vec::new();
        for x in [0.0, 2.0] {
            positions.extend_from_slice(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0]);
        }
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        Arc::new(Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer]))
    }

    /// Where world point `p` is in the viewport of `app`, as the viewport reports a click.
    fn screen_point(app: &App, p: Vec3) -> (f32, f32) {
        let (w, h) = app.viewport_size;
        let n = (app.camera.view_proj(w / h) * app.model_matrix()).project_point3(p);
        ((n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h)
    }

    /// A model of one square, element `id`, at x, named `name`.
    fn named_square(name: &str, id: u64, x: f32) -> Arc<Model> {
        use crate::model::Layer;
        let layer = Layer {
            name: "Walls".into(),
            positions: vec![x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            elements: vec![(id, 0, 6)],
            props: vec![(id, format!(r#"{{"Name":"{name}"}}"#))],
            ..Layer::default()
        };
        Arc::new(Model::new(PathBuf::from(format!("{name}.rvt")), "Revit", vec![layer]))
    }

    /// Two unit squares facing +Z, element 10 at z = 0 under element 20 at z = 1.
    fn stacked_squares() -> Arc<Model> {
        use crate::model::Layer;
        let mut positions = Vec::new();
        for z in [0.0, 1.0] {
            positions.extend_from_slice(&[0.0, 0.0, z, 1.0, 0.0, z, 1.0, 1.0, z, 0.0, 1.0, z]);
        }
        let layer = Layer {
            name: "Slabs".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        Arc::new(Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer]))
    }

    /// The flags of the unit of element `id`.
    fn element_flags(app: &App, id: u64) -> u8 {
        let unit = app.explode_index.as_ref().and_then(|i| i.element_unit(id)).expect("unit");
        app.units.get(unit).map_or(0, |s| s[3] as u8)
    }

    #[test]
    fn hide_hides_selected_units_and_clears_selection() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(20)), false);
        let _ = app.update(Message::HideSelection);
        assert_eq!(element_flags(&app, 20) & UNIT_HIDDEN, UNIT_HIDDEN);
        assert_eq!(element_flags(&app, 10), 0);
        assert!(app.sel.is_empty());
    }

    #[test]
    fn isolate_hides_everything_else() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(20)), false);
        let _ = app.update(Message::IsolateSelection);
        assert_eq!(element_flags(&app, 10) & UNIT_HIDDEN, UNIT_HIDDEN);
        assert_eq!(element_flags(&app, 20) & UNIT_HIDDEN, 0);
        assert!(app.sel.contains(Pickable::Element(20)));
    }

    #[test]
    fn show_all_clears_hidden() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(20)), false);
        let _ = app.update(Message::HideSelection);
        let _ = app.update(Message::ShowAllElements);
        assert!(app.hidden.is_empty());
        assert_eq!(element_flags(&app, 20), 0);
    }

    #[test]
    fn pick_passes_through_hidden_element() {
        let mut app = App::default();
        app.set_model(stacked_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let (x, y) = screen_point(&app, Vec3::new(0.5, 0.5, 1.0));
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert_eq!(app.sel.primary(), Some(Pickable::Element(20)));
        let _ = app.update(Message::HideSelection);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert_eq!(app.sel.primary(), Some(Pickable::Element(10)));
    }

    #[test]
    fn hide_works_for_formats_without_elements() {
        use crate::model::Layer;
        let layer = Layer { name: "Mesh".into(), positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0], indices: vec![0, 1, 2], ..Layer::default() };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        app.click(Some(Pickable::Layer(0)), false);
        let _ = app.update(Message::HideSelection);
        let unit = app.explode_index.as_ref().and_then(|i| i.layer_rest_unit(0)).expect("unit");
        assert_eq!(app.units[unit][3] as u8 & UNIT_HIDDEN, UNIT_HIDDEN);
    }

    #[test]
    fn hidden_survive_add_model() {
        let mut app = App::default();
        app.set_model(named_square("a", 7, 0.0));
        app.click(Some(Pickable::Element(7)), false);
        let _ = app.update(Message::HideSelection);
        app.add_model(named_square("b", 7, 2.0));
        let model = app.model.clone().unwrap();
        let id = model.element_with_key(crate::model::ElementKey { model: 0, source: 7 }).unwrap();
        let other = model.element_with_key(crate::model::ElementKey { model: 1, source: 7 }).unwrap();
        assert_eq!(element_flags(&app, id) & UNIT_HIDDEN, UNIT_HIDDEN);
        assert_eq!(element_flags(&app, other), 0);
    }

    #[test]
    fn explode_and_hidden_combine() {
        let mut app = App::default();
        app.set_model(two_squares());
        let _ = app.update(Message::ToggleExplode);
        let _ = app.update(Message::Explode(0, 50.0));
        app.click(Some(Pickable::Element(20)), false);
        let _ = app.update(Message::HideSelection);
        let unit = app.explode_index.as_ref().and_then(|i| i.element_unit(20)).unwrap();
        assert_ne!(app.units[unit][0], 0.0, "still moved apart");
        assert_eq!(app.units[unit][3] as u8 & UNIT_HIDDEN, UNIT_HIDDEN);
    }

    #[test]
    fn a_right_click_selects_what_lies_there_and_its_menu_hides_it() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let (x, y) = screen_point(&app, Vec3::new(0.5, 0.5, 0.0));
        let _ = app.update(Message::Navigate(Navigate::Context { x, y, width: w, height: h }));
        assert_eq!(app.sel.items(), &[Pickable::Element(10)]);
        assert_eq!(app.context_menu, Some(Vec2::new(x, y)));
        let _ = app.update(Message::ContextMenu(MenuItem::Hide));
        assert!(app.context_menu.is_none(), "choosing closes the menu");
        assert_eq!(app.hidden.len(), 1);
        assert!(app.sel.is_empty());
        let _ = app.update(Message::Undo);
        assert!(app.hidden.is_empty(), "undo brings it back");
    }

    #[test]
    fn a_right_click_keeps_the_selection_it_lies_in() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let ((x1, y1), (x2, y2)) = (screen_point(&app, Vec3::new(0.5, 0.5, 0.0)), screen_point(&app, Vec3::new(2.5, 0.5, 0.0)));
        let _ = app.update(Message::Navigate(Navigate::Pick { x: x1, y: y1, width: w, height: h, add: false }));
        let _ = app.update(Message::Navigate(Navigate::Pick { x: x2, y: y2, width: w, height: h, add: true }));
        let both = [Pickable::Element(10), Pickable::Element(20)];
        let _ = app.update(Message::Navigate(Navigate::Context { x: x2, y: y2, width: w, height: h }));
        assert_eq!(app.sel.items(), &both, "a right click in the selection keeps it");
        // Esc closes the menu first, and keeps the selection
        let _ = app.update(Message::Escape);
        assert!(app.context_menu.is_none());
        assert_eq!(app.sel.items(), &both);
        // empty space keeps the selection too; moving the camera closes the menu
        let _ = app.update(Message::Navigate(Navigate::Context { x: 2.0, y: 2.0, width: w, height: h }));
        assert_eq!(app.sel.items(), &both);
        assert!(app.context_menu.is_some());
        let _ = app.update(Message::Navigate(Navigate::Orbit { dx: 5.0, dy: 0.0 }));
        assert!(app.context_menu.is_none());
        // isolating from the menu hides nothing of the selection
        let _ = app.update(Message::Navigate(Navigate::Context { x: x1, y: y1, width: w, height: h }));
        let _ = app.update(Message::ContextMenu(MenuItem::Isolate));
        assert!(app.hidden.is_empty() && app.context_menu.is_none());
    }

    #[test]
    fn box_select_skips_what_the_section_cuts_away() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        // a section box around the first square only
        app.section = Some(SectionBox::around(&Bounds { min: [-0.5, -0.5, -1.0], max: [1.5, 1.5, 1.0] }));
        app.section_on = true;
        let (w, h) = app.viewport_size;
        let (x0, y0) = screen_point(&app, Vec3::new(-0.5, 1.5, 0.0));
        let (x1, y1) = screen_point(&app, Vec3::new(3.5, -0.5, 0.0));
        let _ = app.update(Message::Navigate(Navigate::Box { x0, y0, x1, y1, width: w, height: h, done: true }));
        assert_eq!(app.sel.items(), &[Pickable::Element(10)]);
    }

    #[test]
    fn box_select_finds_structural_nodes_and_members() {
        use bim_structural::{sam_id, IdKind};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/bim-structural/tests/fixtures/portal_frame.samjson");
        let model = crate::loaders::load(&path).expect("portal frame");
        let el = |source| model.structural[0].element_of(source).expect("a structural object");
        let mut want = vec![el(sam_id(IdKind::Node, 1)), el(sam_id(IdKind::Node, 2)), el(sam_id(IdKind::Member, 1))];
        want.sort_unstable();
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app.camera.set_preset(ViewPreset::Front);
        app.fit();
        // a window around the left column (x = 0): its two nodes and the column, wholly inside
        let (w, h) = app.viewport_size;
        let (ax, ay) = screen_point(&app, Vec3::new(-0.5, 0.0, 5.0));
        let (bx, by) = screen_point(&app, Vec3::new(0.5, 0.0, -1.0));
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let _ = app.update(Message::Navigate(Navigate::Box { x0, y0: ay, x1, y1: by, width: w, height: h, done: true }));
        let mut got: Vec<u64> = app.sel.items().iter().filter_map(|i| match i { Pickable::Element(id) => Some(*id), _ => None }).collect();
        got.sort_unstable();
        assert_eq!(got, want);
    }

    #[test]
    fn hiding_drops_the_measure_highlight() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let _ = app.update(Message::Measure(Tool::Distance));
        let (x, y) = screen_point(&app, Vec3::new(0.5, 0.5, 0.0));
        let _ = app.update(Message::Navigate(Navigate::Hover { x, y, width: w, height: h }));
        app.hover_last = None;
        assert!(app.measure.hover.is_some());
        app.click(Some(Pickable::Element(10)), false);
        let _ = app.update(Message::HideSelection);
        assert!(app.measure.hover.is_none(), "looked again: nothing shown there");
    }

    #[test]
    fn select_where_selects_all_matches() {
        use crate::model::Layer;
        let layer = Layer {
            name: "Walls".into(),
            positions: vec![0.0; 27],
            indices: (0..9).collect(),
            elements: vec![(1, 0, 3), (2, 3, 3), (3, 6, 3)],
            props: vec![(1, r#"{"level":"00"}"#.into()), (2, r#"{"level":"01"}"#.into()), (3, r#"{"level":"00"}"#.into())],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer])));
        app.click(Some(Pickable::Element(2)), false);
        let _ = app.update(Message::SelectWhere("level".into(), "00".into()));
        assert_eq!(app.sel.items(), &[Pickable::Element(1), Pickable::Element(3)]);
    }

    #[test]
    fn select_layer_of_selection() {
        use crate::model::Layer;
        let walls = two_squares().layers[0].clone();
        let mesh = Layer { name: "Mesh".into(), positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0], indices: vec![0, 1, 2], ..Layer::default() };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.rvt"), "Revit", vec![walls, mesh])));
        app.click(Some(Pickable::Element(20)), false);
        let _ = app.update(Message::SelectLayerOfSelection);
        assert_eq!(app.sel.items(), &[Pickable::Element(20), Pickable::Element(10)]);
        app.click(Some(Pickable::Layer(1)), true);
        let _ = app.update(Message::SelectLayerOfSelection);
        assert_eq!(app.sel.len(), 3);
    }

    #[test]
    fn ctrl_drag_box_adds_what_it_holds() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let (x0, y0) = screen_point(&app, Vec3::new(-0.5, 1.5, 0.0));
        let (x1, y1) = screen_point(&app, Vec3::new(3.5, -0.5, 0.0));
        let _ = app.update(Message::Navigate(Navigate::Box { x0, y0, x1, y1, width: w, height: h, done: false }));
        assert!(app.box_select.is_some(), "drawn while dragging");
        let _ = app.update(Message::Navigate(Navigate::Box { x0, y0, x1, y1, width: w, height: h, done: true }));
        assert!(app.box_select.is_none());
        assert_eq!(app.sel.len(), 2);
    }

    #[test]
    fn undo_restores_hidden_and_layers() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(10)), false);
        let _ = app.update(Message::HideSelection);
        let _ = app.update(Message::Layer(0, false));
        assert!(!app.visible[0]);
        let _ = app.update(Message::Undo);
        assert!(app.visible[0]);
        assert_eq!(element_flags(&app, 10) & UNIT_HIDDEN, UNIT_HIDDEN);
        let _ = app.update(Message::Undo);
        assert_eq!(element_flags(&app, 10) & UNIT_HIDDEN, 0);
        let _ = app.update(Message::Redo);
        assert_eq!(element_flags(&app, 10) & UNIT_HIDDEN, UNIT_HIDDEN);
    }

    #[test]
    fn no_entry_for_noop() {
        let mut app = App::default();
        app.set_model(two_squares());
        let _ = app.update(Message::ShowAllElements);
        let _ = app.update(Message::Layer(0, true));
        assert!(!app.history.can_undo());
    }

    #[test]
    fn undo_survives_add_model() {
        let mut app = App::default();
        app.set_model(named_square("a", 7, 0.0));
        app.click(Some(Pickable::Element(7)), false);
        let _ = app.update(Message::HideSelection);
        app.add_model(named_square("b", 7, 2.0));
        let id = app.model.as_ref().unwrap().element_with_key(crate::model::ElementKey { model: 0, source: 7 }).unwrap();
        assert_eq!(element_flags(&app, id) & UNIT_HIDDEN, UNIT_HIDDEN);
        let _ = app.update(Message::Undo);
        assert_eq!(element_flags(&app, id) & UNIT_HIDDEN, 0);
        assert_eq!(app.visible.len(), 2, "the added model's layer stays");
    }

    #[test]
    fn ids_stay_the_same_after_add() {
        let mut app = App::default();
        let name = |app: &App, id: u64| app.model.as_ref().unwrap().element_props(id).first().map(|p| p.1.clone());
        app.set_model(named_square("a", 7, 0.0));
        app.add_model(named_square("b", 7, 2.0));
        assert_eq!(name(&app, 7).as_deref(), Some("a"), "the first model keeps its ids");
        let b = app.model.as_ref().unwrap().element_with_key(crate::model::ElementKey { model: 1, source: 7 }).unwrap();
        app.add_model(named_square("c", 7, 4.0));
        assert_eq!(name(&app, 7).as_deref(), Some("a"));
        assert_eq!(name(&app, b).as_deref(), Some("b"), "an added model keeps its ids when the next is added");
    }

    #[test]
    fn keys_survive_add_model() {
        let mut app = App::default();
        app.set_model(named_square("a", 7, 0.0));
        let key = app.model.as_ref().unwrap().element_key(7);
        let before = app.model.as_ref().unwrap().element_props(7);
        app.add_model(named_square("b", 7, 2.0));
        let model = app.model.clone().unwrap();
        let id = model.element_with_key(key).expect("still there");
        assert_eq!(model.element_props(id), before);
    }

    #[test]
    fn unit_keys_round_trip_after_add() {
        let mut app = App::default();
        app.set_model(named_square("a", 7, 0.0));
        app.add_model(named_square("b", 7, 2.0));
        let index = app.explode_index.clone().unwrap();
        for unit in 1..index.len() {
            let key = app.unit_key(index.owner(unit).unwrap());
            assert_eq!(app.unit_of_key(key), Some(unit));
        }
        assert_eq!(app.unit_key(UnitOwner::LayerRest(1)), UnitKey::LayerRest { model: 1, layer: 0 });
    }

    #[test]
    fn ctrl_click_adds_and_removes() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(20)), false);
        app.click(Some(Pickable::Element(10)), true);
        assert_eq!(app.sel.len(), 2);
        assert_eq!(app.sel.primary(), Some(Pickable::Element(20)));
        app.click(Some(Pickable::Element(10)), true);
        assert_eq!(app.sel.len(), 1);
    }

    #[test]
    fn plain_click_on_empty_space_clears_but_ctrl_click_keeps() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(10)), false);
        app.click(None, true);
        assert_eq!(app.sel.len(), 1);
        assert_eq!(app.selected, Some((0, Some(10))));
        app.click(None, false);
        assert!(app.sel.is_empty());
        assert_eq!(app.selected, None);
        assert!(app.selection.is_empty());
    }

    #[test]
    fn ctrl_pick_reaches_click_with_add() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let (x, y) = screen_point(&app, Vec3::new(0.5, 0.5, 0.0));
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        let (x, y) = screen_point(&app, Vec3::new(2.5, 0.5, 0.0));
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: true }));
        assert_eq!(app.sel.items(), &[Pickable::Element(10), Pickable::Element(20)]);
        assert_eq!(app.selection, vec![(0, 0, 6), (0, 6, 6)]);
    }

    #[test]
    fn selected_units_carry_the_selected_flag() {
        let mut app = App::default();
        app.set_model(two_squares());
        app.click(Some(Pickable::Element(20)), false);
        let index = app.explode_index.clone().expect("units");
        let flags = |id: u64| app.units[index.element_unit(id).expect("unit")][3] as u8;
        assert_eq!(flags(20) & UNIT_SELECTED, UNIT_SELECTED);
        assert_eq!(flags(10), 0);
    }

    #[test]
    fn the_exploded_view_is_picked_and_framed_where_it_is_drawn() {
        let mut app = App::default();
        app.set_model(two_squares());
        let model = app.model.clone().unwrap();
        let _ = app.update(Message::ToggleExplode);
        let _ = app.update(Message::Explode(0, 100.0));
        // no levels: elements mode; the middle is x 1.5, so the squares move by -2 and +2
        let down = Vec3::new(0.0, 0.0, -1.0);
        let shift = |l: usize, i: u32| app.explode_offset(l, i);
        let hit = pick_triangle(&model, &[true], Vec3::new(4.5, 0.5, 5.0), down, &shift, &|_| true, &|_, _| false, None);
        assert_eq!(hit.map(|(l, t)| model.layers[l].element_at(t)), Some(Some(20)));
        assert!(pick_triangle(&model, &[true], Vec3::new(2.5, 0.5, 5.0), down, &shift, &|_| true, &|_, _| false, None).is_none());
        let b = app.world_bounds().unwrap();
        assert_eq!((b.min[0], b.max[0]), (-2.0, 5.0));
        app.selection = model.element_ranges(20);
        let s = app.selection_bounds().unwrap();
        assert_eq!((s.min[0], s.max[0]), (4.0, 5.0));
        // off: back at rest, the amounts kept for the next time
        let _ = app.update(Message::ToggleExplode);
        let b = app.world_bounds().unwrap();
        assert_eq!((b.min[0], b.max[0], app.explode[0]), (0.0, 3.0, 100.0));
        assert!(app.explode_offsets.iter().all(|o| *o == [0.0; 4]));
    }

    #[test]
    fn explode_controls_send_messages() {
        let mut app = App::default();
        app.set_model(two_squares());
        let _ = app.update(Message::ToggleExplode);
        let _ = app.update(Message::Explode(2, 40.0));
        let mut ui = simulator(&app);
        // no levels: the storeys button is disabled
        ui.click("Storeys").expect("Storeys button");
        ui.click("Elements").expect("Elements button");
        ui.click("Reset").expect("Reset button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(messages[..], [Message::ExplodeMode(ExplodeMode::Elements), Message::ResetExplode]), "{messages:?}");
    }

    #[test]
    fn measures_between_two_clicked_corners() {
        use crate::model::Layer;
        let layer = Layer {
            name: "Plate".into(),
            positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let vp = app.camera.view_proj(w / h);
        let screen = |p: Vec3| {
            let n = vp.project_point3(p);
            ((n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h)
        };
        let _ = app.update(Message::Measure(Tool::Distance));
        // a few pixels off the corners: they snap
        for (corner, off) in [(Vec3::new(0.0, 0.0, 0.0), 0.01), (Vec3::new(2.0, 1.0, 0.0), -0.01)] {
            let (x, y) = screen(corner + Vec3::new(off, off, 0.0));
            let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        }
        assert_eq!(app.measure.results.len(), 1);
        let (_, d) = app.measure.results[0].value();
        assert!((d - 5f64.sqrt()).abs() < 1e-5, "{d}");
        assert!(app.selected.is_none(), "measuring does not select");
        let _ = app.update(Message::Escape);
        assert!(app.measure.tool.is_none());
    }

    #[test]
    fn hovering_highlights_what_the_click_takes() {
        use crate::measure::hover::SnapKind;
        use crate::model::Layer;
        let layer = Layer {
            name: "Plate".into(),
            positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let vp = app.camera.view_proj(w / h);
        let screen = |p: Vec3| {
            let n = vp.project_point3(p);
            ((n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h)
        };
        let hover = |app: &mut App, x: f32, y: f32| {
            let _ = app.update(Message::Navigate(Navigate::Hover { x, y, width: w, height: h }));
            app.hover_last = None; // no pause between the looks of a test
        };
        // nothing to measure: nothing highlighted
        let (x, y) = screen(Vec3::new(1.0, 0.5, 0.0));
        hover(&mut app, x, y);
        assert!(app.measure.hover.is_none());

        let _ = app.update(Message::Measure(Tool::Distance));
        hover(&mut app, x, y);
        assert_eq!(app.measure.hover.as_ref().map(|h| h.kind), Some(SnapKind::Face));
        // 3 px from the corner (0, 0): the corner
        let (cx, cy) = screen(Vec3::ZERO);
        hover(&mut app, cx + 3.0, cy - 2.0);
        let highlighted = app.measure.hover.clone().expect("hover");
        assert_eq!(highlighted.kind, SnapKind::Corner);
        // the click lands a pixel or two off: it takes the highlighted corner
        let _ = app.update(Message::Navigate(Navigate::Pick { x: cx + 5.0, y: cy - 3.0, width: w, height: h, add: false }));
        assert_eq!(app.measure.picks, vec![highlighted.snap]);
        // orbiting drops the highlight; leaving the viewport too
        hover(&mut app, x, y);
        let _ = app.update(Message::Navigate(Navigate::Orbit { dx: 5.0, dy: 0.0 }));
        assert!(app.measure.hover.is_none());
        hover(&mut app, x, y);
        let _ = app.update(Message::Navigate(Navigate::HoverOff));
        assert!(app.measure.hover.is_none());
        // the layer under the cursor is hidden (a shortcut, remote control): the highlight goes,
        // and a click there measures nothing
        hover(&mut app, x, y);
        assert!(app.measure.hover.is_some());
        let _ = app.update(Message::Layer(0, false));
        assert!(app.measure.hover.is_none(), "looked again: nothing shown there");
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert_eq!(app.measure.picks.len(), 1, "only the corner from before");
    }

    #[test]
    fn load_fraction_grows_and_stays_below_one() {
        use loaders::Phase;
        let mut last = 0.0;
        for (phase, t) in [(Phase::Read, 0.0), (Phase::Read, 5.0), (Phase::Convert, 0.0), (Phase::Convert, 60.0), (Phase::Scene, 9.0)] {
            let f = load_fraction(phase, t, true);
            assert!(f >= last && f < 1.0, "{phase:?} {t}: {f}");
            last = f;
        }
    }

    #[test]
    fn the_section_box_takes_over_a_level_or_upright_plane() {
        use crate::model::Layer;
        // a unit cube's top and bottom
        let layer = Layer {
            name: "Cube".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            indices: vec![0, 1, 2, 3, 4, 5],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        app.plane = Some(SectionPlane { normal: Vec3::Z, offset: 0.4 });
        let _ = app.update(Message::ToggleSection);
        let b = app.active_section().expect("box on");
        assert!(app.plane.is_none());
        assert!((b.max.z - 0.4).abs() < 1e-6, "{b:?}");
        // a sloped plane stays next to the box
        let _ = app.update(Message::ToggleSection);
        app.plane = Some(SectionPlane { normal: Vec3::new(0.0, 0.6, 0.8), offset: 0.4 });
        let _ = app.update(Message::ToggleSection);
        assert!(app.active_section().is_some() && app.plane.is_some());
    }

    #[test]
    fn reset_puts_the_section_plane_back_where_it_was_placed() {
        use crate::model::Layer;
        let layer = Layer {
            name: "Cube".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            indices: vec![0, 1, 2, 3, 4, 5],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        let placed = SectionPlane { normal: Vec3::new(0.0, 0.6, 0.8), offset: 0.4 };
        (app.plane, app.plane_placed) = (Some(placed), Some(placed));
        let _ = app.update(Message::PlaneMove(0.2));
        let _ = app.update(Message::FlipPlane);
        assert_ne!(app.plane, Some(placed));
        let _ = app.update(Message::ResetSection);
        assert_eq!(app.plane, Some(placed));
        assert!(!app.plane_picking);
    }

    #[test]
    fn counts_are_grouped() {
        assert_eq!(format_count(1234567), "1 234 567");
        assert_eq!(format_count(12), "12");
    }
}
