//! The structural analysis model: f64, SI units, world metres, canonical conventions.
//!
//! Conventions (normative for every importer and for the FE DLL):
//!
//! - **Units:** SI base units in f64: m, N, N·m, N/m, N/m² (Pa), rad, K. Translational springs
//!   N/m, rotational springs N·m/rad, line-support springs N/m², surface-support springs N/m³.
//! - **Global frame:** right-handed, Z up, gravity −Z. A source with gravity +Z is rotated 180°
//!   about X, `(x, y, z) → (x, −y, −z)`, never mirrored; recorded in
//!   [`SourceInfo::global_transform`].
//! - **Member local frame:** x from start to end; `z = normalize(z_ref − (z_ref·x) x)`;
//!   `y = z × x`. Profile u (width) maps to local y, profile v (height) to local z. See
//!   [`crate::frame`].
//! - **Internal forces:** the positive cut face has outward normal +x; forces on it act along
//!   +x, +y, +z (N > 0 is tension), moment vectors along +x, +y, +z.
//!   `σ = N/A + My·z/Iy − Mz·y/Iz`: +My puts tension on the +z fibre, +Mz on the −y fibre.
//! - **Reactions:** the force the support exerts on the structure, in the support frame.
//! - **Stations:** absolute metres from the member's start node along the analytical member; a
//!   jump is two stations with the same x, left value first.

use std::collections::HashMap;

use glam::{DAffine2, DMat3, DMat4, DVec2, DVec3};

use crate::id::Id;
use crate::results::{ResultPayload, TableKind};

/// A `sam-json` file: one or more analysis models from one source.
#[derive(Clone, Debug, PartialEq)]
pub struct SamDocument {
    /// The anchor the viewer subtracts for display (world metres).
    pub origin: DVec3,
    pub source: SourceInfo,
    pub models: Vec<AnalysisModel>,
}

/// Whether a model is spatial or a plane frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Dimension {
    #[default]
    Space3d,
    /// A plane model; `normal` is the unit normal of its plane.
    Plane2d { normal: DVec3 },
}

/// Where the model's members come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ModelKind {
    #[default]
    Analytical,
    /// Derived from physical elements (axes of beams and columns).
    PhysicalAxes,
}

/// What kind of item an id names inside a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Node,
    Member,
    Surface,
    LineSupport,
    Section,
    Material,
    LoadCase,
    Combination,
    Action,
    ResultSet,
}

/// An item's kind and its slot in the matching `Vec` of [`AnalysisModel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemRef {
    pub kind: ItemKind,
    pub slot: u32,
}

/// One analysis model.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisModel {
    pub id: Id,
    pub name: String,
    pub dimension: Dimension,
    pub kind: ModelKind,
    pub source: SourceInfo,
    /// IFC SharedPlacement in world metres; identity otherwise.
    pub shared_frame: DMat4,
    pub nodes: Vec<Node>,
    pub members: Vec<Member>,
    pub surfaces: Vec<SurfaceMember>,
    pub line_supports: Vec<LineSupport>,
    pub sections: Vec<Section>,
    pub materials: Vec<Material>,
    pub load_cases: Vec<LoadCase>,
    pub combinations: Vec<Combination>,
    /// Sorted by load case, for draw ranges.
    pub actions: Vec<Action>,
    pub results: Vec<ResultSet>,
    pub links: Vec<PhysicalLink>,
    pub diagnostics: Vec<Diagnostic>,
    index: HashMap<Id, ItemRef>,
}

impl AnalysisModel {
    /// An empty model.
    pub fn new(id: Id, name: impl Into<String>, source: SourceInfo) -> Self {
        AnalysisModel {
            id,
            name: name.into(),
            dimension: Dimension::Space3d,
            kind: ModelKind::Analytical,
            source,
            shared_frame: DMat4::IDENTITY,
            nodes: Vec::new(),
            members: Vec::new(),
            surfaces: Vec::new(),
            line_supports: Vec::new(),
            sections: Vec::new(),
            materials: Vec::new(),
            load_cases: Vec::new(),
            combinations: Vec::new(),
            actions: Vec::new(),
            results: Vec::new(),
            links: Vec::new(),
            diagnostics: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Rebuilds the id index after the item lists changed. Returns ids that occur more than once
    /// (the first occurrence stays indexed).
    pub fn rebuild_index(&mut self) -> Vec<Id> {
        let mut index = HashMap::new();
        let mut duplicates = Vec::new();
        let mut add = |id: Id, kind: ItemKind, slot: usize| {
            if index.contains_key(&id) {
                duplicates.push(id);
            } else {
                index.insert(id, ItemRef { kind, slot: slot as u32 });
            }
        };
        for (i, x) in self.nodes.iter().enumerate() {
            add(x.id, ItemKind::Node, i);
        }
        for (i, x) in self.members.iter().enumerate() {
            add(x.id, ItemKind::Member, i);
        }
        for (i, x) in self.surfaces.iter().enumerate() {
            add(x.id, ItemKind::Surface, i);
        }
        for (i, x) in self.line_supports.iter().enumerate() {
            add(x.id, ItemKind::LineSupport, i);
        }
        for (i, x) in self.sections.iter().enumerate() {
            add(x.id, ItemKind::Section, i);
        }
        for (i, x) in self.materials.iter().enumerate() {
            add(x.id, ItemKind::Material, i);
        }
        for (i, x) in self.load_cases.iter().enumerate() {
            add(x.id, ItemKind::LoadCase, i);
        }
        for (i, x) in self.combinations.iter().enumerate() {
            add(x.id, ItemKind::Combination, i);
        }
        for (i, x) in self.actions.iter().enumerate() {
            add(x.id, ItemKind::Action, i);
        }
        for (i, x) in self.results.iter().enumerate() {
            add(x.id, ItemKind::ResultSet, i);
        }
        self.index = index;
        duplicates
    }

    /// The kind and slot of an id.
    pub fn item(&self, id: Id) -> Option<ItemRef> {
        self.index.get(&id).copied()
    }

    fn slot(&self, id: Id, kind: ItemKind) -> Option<usize> {
        self.item(id).filter(|r| r.kind == kind).map(|r| r.slot as usize)
    }

    pub fn node(&self, id: Id) -> Option<&Node> {
        self.slot(id, ItemKind::Node).map(|i| &self.nodes[i])
    }
    pub fn member(&self, id: Id) -> Option<&Member> {
        self.slot(id, ItemKind::Member).map(|i| &self.members[i])
    }
    pub fn surface(&self, id: Id) -> Option<&SurfaceMember> {
        self.slot(id, ItemKind::Surface).map(|i| &self.surfaces[i])
    }
    pub fn section(&self, id: Id) -> Option<&Section> {
        self.slot(id, ItemKind::Section).map(|i| &self.sections[i])
    }
    pub fn material(&self, id: Id) -> Option<&Material> {
        self.slot(id, ItemKind::Material).map(|i| &self.materials[i])
    }
    pub fn load_case(&self, id: Id) -> Option<&LoadCase> {
        self.slot(id, ItemKind::LoadCase).map(|i| &self.load_cases[i])
    }
    pub fn result_set(&self, id: Id) -> Option<&ResultSet> {
        self.slot(id, ItemKind::ResultSet).map(|i| &self.results[i])
    }

    /// Start and end node positions of a member, if both nodes exist.
    pub fn member_ends(&self, member: &Member) -> Option<(DVec3, DVec3)> {
        let a = self.node(*member.nodes.first()?)?.p;
        let b = self.node(*member.nodes.last()?)?.p;
        Some((a, b))
    }
}

/// Where the model came from and which conventions it was normalised from.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    /// "IFC4", "IFC2X3", "sam-json/1", "fe-dll/1", "rvt-derived".
    pub format: String,
    pub file: String,
    pub application: Option<String>,
    /// Applied to reach the Z-up global frame.
    pub global_transform: DMat4,
    /// What the source used; already normalised away.
    pub conventions: Conventions,
    pub eccentricity_policy: EccentricityPolicy,
}

impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo {
            format: String::new(),
            file: String::new(),
            application: None,
            global_transform: DMat4::IDENTITY,
            conventions: Conventions::default(),
            eccentricity_policy: EccentricityPolicy::default(),
        }
    }
}

/// The source's conventions, recorded after conversion to the canonical ones.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Conventions {
    pub gravity_was: AxisSign,
    pub mz_sign_was: MzSign,
    pub reactions_were: ReactionSense,
    pub result_signs: SignStatus,
    /// How the source defined its default member frame.
    pub local_z_rule: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AxisSign {
    #[default]
    NegZ,
    PosZ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MzSign {
    /// Canonical: +Mz puts tension on the −y fibre.
    #[default]
    TensionMinusY,
    TensionPlusY,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReactionSense {
    /// Canonical: the force the support exerts on the structure.
    #[default]
    OnStructure,
    OnSupport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SignStatus {
    Verified,
    #[default]
    AssumedCanonical,
}

/// How the independent offset mechanisms of [`Insertion`] combine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EccentricityPolicy {
    /// Default for IFC: a cardinal point cancels the profile Position translation.
    #[default]
    CardinalCancelsPosition,
    /// Offsets add up, when a source says so.
    Additive,
    /// The producer reduced everything to `eccentricity` (FE DLL default).
    Explicit,
}

/// A node.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: Id,
    /// Source numbering, kept as text.
    pub number: String,
    /// World metres.
    pub p: DVec3,
    /// Skewed support / condition coordinate system (columns: the frame's x, y, z in global).
    pub frame: Option<DMat3>,
    pub support: Option<Support>,
    pub origin: NodeOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum NodeOrigin {
    #[default]
    Analytical,
    DerivedFromPhysical { element: Id, approximate: bool },
}

/// One degree of freedom of a support, line support or member end.
#[derive(Clone, Debug, PartialEq)]
pub enum Dof {
    /// Support: restrained; member end: connected.
    Fixed,
    /// Support: free; member end: released.
    Free,
    /// SI stiffness, sign kept as given (negative values are flagged).
    Spring(f64),
    Nonlinear { kind: NonlinearKind, base: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum NonlinearKind {
    TensionOnly,
    CompressionOnly,
    Failure { limit: f64 },
    Slippage { gap: f64 },
    /// Displacement-force pairs; `base` is 0.
    Diagram(Vec<(f64, f64)>),
}

/// ux uy uz rx ry rz.
pub type Dofs = [Dof; 6];

/// All six DOFs the same.
pub fn dofs_all(d: Dof) -> Dofs {
    [d.clone(), d.clone(), d.clone(), d.clone(), d.clone(), d]
}

/// A nodal support.
#[derive(Clone, Debug, PartialEq)]
pub struct Support {
    pub dofs: Dofs,
    pub frame: DofFrame,
    /// Source name when the source gave a type, not DOFs.
    pub named: Option<String>,
    /// Display hint, from [`crate::support::classify`].
    pub preset: SupportPreset,
}

/// The frame support DOFs are given in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DofFrame {
    #[default]
    Global,
    NodeLocal,
    MemberLocal(Id),
}

/// The symbol a support is drawn with. `free` lists which translations (x, y, z of the support
/// frame) are free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportPreset {
    Fixed,
    Pinned,
    Roller { free: [bool; 3] },
    Sliding { free: [bool; 3] },
    Spring,
    /// No DOF restrained (the `free` preset).
    Free,
    Custom,
}

/// A member (bar).
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub id: Id,
    pub number: String,
    pub kind: MemberKind,
    /// Start, intermediate..., end.
    pub nodes: Vec<Id>,
    pub geometry: Curve,
    /// Resolved local z reference, authoritative.
    pub z_ref: DVec3,
    /// Provenance of `z_ref`; never interpreted.
    pub orientation: Orientation,
    /// Start and end (taper) section; `None` when the source gives none.
    pub section: Option<(Id, Option<Id>)>,
    /// Extra rotation of the profile about x, rad.
    pub section_rotation: f64,
    pub insertion: Insertion,
    /// Member local; `None` = rigid.
    pub releases: [Option<Hinge>; 2],
    pub material: Option<Id>,
    /// Physical element ids in the same file.
    pub links: Vec<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Beam,
    Truss,
    Cable,
    TensionOnly,
    CompressionOnly,
    Rigid,
    Spring,
}

/// The axis curve of a member between its end nodes.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Line,
    /// A circular arc through `mid`.
    Arc { mid: DVec3 },
    /// The vertices strictly between the start and end node.
    Polyline(Vec<DVec3>),
}

/// Where `z_ref` came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Orientation {
    IfcAxis,
    Beta { angle: f64, rule: String },
    RefNode(Id),
    RefVector(DVec3),
    Default,
}

/// A member-end release. `Fixed` = connected, `Free` = released.
#[derive(Clone, Debug, PartialEq)]
pub struct Hinge {
    pub dofs: Dofs,
    pub named: Option<String>,
}

/// The independent offset mechanisms, kept apart so none is applied twice.
#[derive(Clone, Debug, PartialEq)]
pub struct Insertion {
    /// 0 = none, 1..=19 (IFC4 IfcCardinalPointReference).
    pub cardinal_point: u8,
    /// The profile definition's own Position (u, v), as read.
    pub profile_position: DAffine2,
    /// IfcMaterialProfileWithOffsets, start and end.
    pub profile_offsets: Option<[DVec2; 2]>,
    /// Member-wide (ey, ez) in local axes; the profile moves by +e.
    pub eccentricity: DVec2,
    /// Rigid offsets node -> member end.
    pub end_offsets: [DVec3; 2],
    pub end_offset_frame: OffsetFrame,
}

impl Default for Insertion {
    fn default() -> Self {
        Insertion {
            cardinal_point: 0,
            profile_position: DAffine2::IDENTITY,
            profile_offsets: None,
            eccentricity: DVec2::ZERO,
            end_offsets: [DVec3::ZERO; 2],
            end_offset_frame: OffsetFrame::MemberLocal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OffsetFrame {
    #[default]
    MemberLocal,
    Global,
}

/// A surface member (plate, shell...).
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceMember {
    pub id: Id,
    pub number: String,
    pub kind: SurfaceKind,
    /// Node loop.
    pub boundary: Vec<Id>,
    pub openings: Vec<Vec<Id>>,
    pub thickness: Thickness,
    /// Mid-plane offset from the reference surface.
    pub offset_z: f64,
    pub x_dir: Option<DVec3>,
    pub material: Option<Id>,
    pub links: Vec<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Plate,
    Membrane,
    Shell,
    Rigid,
    LoadTransfer,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Thickness {
    Uniform(f64),
    /// Thickness at three points of the plane.
    Linear([(DVec3, f64); 3]),
    Unknown,
}

/// A line support (IfcStructuralCurveConnection, surface edges).
#[derive(Clone, Debug, PartialEq)]
pub struct LineSupport {
    pub id: Id,
    pub number: String,
    pub along: SupportCarrier,
    /// Springs per length.
    pub dofs: Dofs,
    pub frame: DofFrame,
    pub named: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SupportCarrier {
    Member(Id),
    SurfaceEdge { surface: Id, from: Id, to: Id },
    Polyline(Vec<DVec3>),
}

/// A cross-section.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub id: Id,
    /// "HEB 200", as given.
    pub name: String,
    pub shape: Shape,
    /// Computed from the outline; used for geometry.
    pub props: SectionProps,
    pub props_source: PropsSource,
    /// Properties the source gave; they win for labels, never for geometry.
    pub imported: Option<ImportedProps>,
    pub material: Option<Id>,
}

/// Section shapes. Dimensions in metres, origin at the bounding-box centre (IFC parametric
/// profiles). The orientation of each parametric shape in profile (u, v) is documented on
/// [`crate::section::outline`].
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    I { h: f64, b: f64, tw: f64, tf: f64, r: f64, slope: f64 },
    AsymI { h: f64, bt: f64, bb: f64, tw: f64, tft: f64, tfb: f64, r: f64 },
    U { h: f64, b: f64, tw: f64, tf: f64, r1: f64, r2: f64, slope: f64 },
    L { h: f64, b: f64, t: f64, r1: f64, r2: f64 },
    T { h: f64, b: f64, tw: f64, tf: f64, r: f64 },
    C { h: f64, b: f64, t: f64, lip: f64, r: f64 },
    Z { h: f64, b: f64, tw: f64, tf: f64, r: f64 },
    Rect { b: f64, h: f64 },
    Circle { d: f64 },
    Chs { d: f64, t: f64 },
    Rhs { b: f64, h: f64, t: f64, r_out: f64, r_in: f64 },
    Polygon { outer: Vec<DVec2>, holes: Vec<Vec<DVec2>> },
    /// Parts must not overlap.
    Composite(Vec<(Shape, DAffine2)>),
    /// Name only; drawn as a placeholder.
    Unknown,
}

/// Section properties in profile (u, v) = local (y, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionProps {
    pub area: f64,
    pub centroid: DVec2,
    pub shear_centre: DVec2,
    /// ∫ v² dA about the centroid (bending about local y).
    pub iy: f64,
    /// ∫ u² dA about the centroid (bending about local z).
    pub iz: f64,
    /// ∫ u v dA about the centroid.
    pub iyz: f64,
    /// Torsion constant, when known.
    pub it: Option<f64>,
    /// `[min, max]` of the outline.
    pub bbox: [DVec2; 2],
}

impl Default for SectionProps {
    fn default() -> Self {
        SectionProps {
            area: 0.0,
            centroid: DVec2::ZERO,
            shear_centre: DVec2::ZERO,
            iy: 0.0,
            iz: 0.0,
            iyz: 0.0,
            it: None,
            bbox: [DVec2::ZERO; 2],
        }
    }
}

/// Where the values in [`Section::props`] come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropsSource {
    /// All from the outline, the shear centre from symmetry or a closed form.
    Computed,
    /// From the outline, the shear centre from the imported properties.
    Imported,
    /// From the outline, the shear centre approximated by the centroid.
    ShearCentreApproximate,
    /// No outline (`Shape::Unknown`): zero properties.
    Unavailable,
}

/// Section properties as a source gives them; every value optional.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ImportedProps {
    pub area: Option<f64>,
    pub centroid: Option<DVec2>,
    pub shear_centre: Option<DVec2>,
    pub iy: Option<f64>,
    pub iz: Option<f64>,
    pub iyz: Option<f64>,
    pub it: Option<f64>,
}

/// A material.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub id: Id,
    pub name: String,
    pub e: Option<f64>,
    pub g: Option<f64>,
    pub nu: Option<f64>,
    pub density: Option<f64>,
    pub alpha_t: Option<f64>,
}

/// A load case.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadCase {
    pub id: Id,
    pub number: String,
    pub name: String,
    pub action_type: ActionType,
    /// IFC enum text: DEAD_LOAD_G, WIND_W, ...
    pub action_source: String,
    /// Factors.
    pub self_weight: Option<DVec3>,
    pub coefficient: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionType {
    Permanent,
    Variable,
    Accidental,
    User(String),
}

/// A load combination; shown, never evaluated.
#[derive(Clone, Debug, PartialEq)]
pub struct Combination {
    pub id: Id,
    pub number: String,
    pub name: String,
    pub purpose: Purpose,
    pub terms: Vec<(LoadRef, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Purpose {
    Uls,
    Sls,
    Als,
    User(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoadRef {
    Case(Id),
    Combination(Id),
}

/// A load on an item.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub id: Id,
    pub load: LoadRef,
    pub target: Target,
    pub frame: LoadFrame,
    pub kind: Load,
    pub destabilising: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Node(Id),
    /// `span`: absolute metres from the start node.
    Member { id: Id, span: Option<(f64, f64)> },
    Surface(Id),
    Free(Vec<DVec3>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadFrame {
    Global,
    Local,
    GlobalProjected,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    NodalForce { f: DVec3, m: DVec3 },
    MemberPoint { x: f64, f: DVec3, m: DVec3 },
    MemberDistributed { dir: DVec3, moment: bool, dist: Distribution },
    Surface { dir: DVec3, dist: SurfaceDistribution },
    Temperature { dt: f64, dt_y: f64, dt_z: f64 },
    Displacement { u: [Option<f64>; 6] },
}

/// Intensity as given in its frame; x in absolute metres.
#[derive(Clone, Debug, PartialEq)]
pub enum Distribution {
    Uniform(f64),
    Linear { q1: f64, q2: f64 },
    Trapezoidal { a: f64, b: f64, q1: f64, q2: f64 },
    Polygonal(Vec<(f64, f64)>),
    Parabolic { peak: f64 },
    Sine { peak: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceDistribution {
    Uniform(f64),
    Bilinear([f64; 3]),
    /// (u, v) in the surface plane and intensity.
    Sampled(Vec<(DVec2, f64)>),
}

/// A result set.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultSet {
    pub id: Id,
    pub number: String,
    pub name: String,
    pub for_: ResultFor,
    pub theory: Theory,
    pub linear: bool,
    pub available: Vec<TableKind>,
    /// Tables carried inline in the file. Sidecar and DLL tables are fetched through a
    /// [`ResultSource`] when the set is opened.
    pub inline: Option<ResultPayload>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResultFor {
    Case(Id),
    Combination(Id),
    Envelope(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theory {
    First,
    Second,
    Third,
    FullNonlinear,
}

/// Fetches result tables on first use (FE DLL, sidecar files).
pub trait ResultSource: Send + Sync {
    fn fetch(&self, set: Id, kind: TableKind) -> Result<ResultPayload, String>;
}

/// A link from an analysis item to a physical element.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicalLink {
    pub analytical: Id,
    pub physical: Id,
    pub source: LinkSource,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkSource {
    IfcRelAssignsToProduct,
    RevitAssociation,
    Matched { distance: f64 },
}

/// Something an importer skipped, guessed or converted.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub object: Option<Id>,
    /// `named-support-expanded`, `default-local-z`, ...
    pub code: String,
    pub message: String,
}
