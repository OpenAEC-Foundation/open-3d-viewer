//! The `sam-json` v1 document as serde types. The JSON Schema is generated from these types;
//! unknown fields are rejected everywhere. Vectors are plain arrays; matrices are column-major
//! (`global_transform` and `shared_frame` 16 numbers with the translation at 12-14, node frames
//! three column vectors x, y, z, `transform` of composite parts 6 numbers
//! `[xx, xy, yx, yy, tx, ty]`).

use serde::{Deserialize, Serialize};

pub type V2 = [f64; 2];
pub type V3 = [f64; 3];
pub type M4 = [f64; 16];

pub const IDENTITY4: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn identity4() -> M4 {
    IDENTITY4
}
fn is_identity4(m: &M4) -> bool {
    *m == IDENTITY4
}
fn one() -> f64 {
    1.0
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// A `sam-json` v1 document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// Always "sam-json".
    pub format: String,
    /// Always 1.
    pub version: u32,
    pub source: Source,
    /// Always "SI".
    pub units: String,
    /// The anchor the viewer subtracts for display, world metres.
    #[serde(default)]
    pub origin: V3,
    pub models: Vec<Model>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Column-major 4×4 applied to reach the Z-up global frame (rigid).
    #[serde(default = "identity4")]
    pub global_transform: M4,
    #[serde(default)]
    pub conventions: Conventions,
    #[serde(default)]
    pub eccentricity_policy: EccentricityPolicy,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Conventions {
    #[serde(default)]
    pub gravity_was: Gravity,
    #[serde(default)]
    pub mz_sign_was: MzSign,
    #[serde(default)]
    pub reactions_were: ReactionSense,
    #[serde(default)]
    pub result_signs: SignStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_z_rule: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub enum Gravity {
    #[default]
    #[serde(rename = "-z")]
    NegZ,
    #[serde(rename = "+z")]
    PosZ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MzSign {
    #[default]
    TensionMinusY,
    TensionPlusY,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReactionSense {
    #[default]
    OnStructure,
    OnSupport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SignStatus {
    Verified,
    #[default]
    AssumedCanonical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EccentricityPolicy {
    CardinalCancelsPosition,
    Additive,
    #[default]
    Explicit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub dimension: Dimension,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ModelKind,
    /// Column-major 4×4, world metres (IFC SharedPlacement); identity when absent.
    #[serde(default = "identity4", skip_serializing_if = "is_identity4")]
    pub shared_frame: M4,
    #[serde(default)]
    pub materials: Vec<Material>,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub surfaces: Vec<Surface>,
    #[serde(default)]
    pub line_supports: Vec<LineSupport>,
    #[serde(default)]
    pub load_cases: Vec<LoadCase>,
    #[serde(default)]
    pub combinations: Vec<Combination>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub results: Vec<ResultSet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub enum Dimension {
    #[default]
    #[serde(rename = "3d")]
    Space3d,
    /// A plane model with the unit normal of its plane.
    #[serde(rename = "2d")]
    Plane2d { normal: V3 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    #[default]
    Analytical,
    PhysicalAxes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub id: u64,
    pub name: String,
    /// Young's modulus, Pa.
    #[serde(rename = "E", default, skip_serializing_if = "Option::is_none")]
    pub e: Option<f64>,
    /// Shear modulus, Pa.
    #[serde(rename = "G", default, skip_serializing_if = "Option::is_none")]
    pub g: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nu: Option<f64>,
    /// Density, kg/m³.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rho: Option<f64>,
    /// Thermal expansion, 1/K.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_t: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<u64>,
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<Props>,
}

/// Section shapes in metres, origin at the bounding-box centre. Radii and slopes default to 0.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub enum Shape {
    I {
        h: f64,
        b: f64,
        tw: f64,
        tf: f64,
        #[serde(default)]
        r: f64,
        /// Inner flange face slope, rad.
        #[serde(default)]
        slope: f64,
    },
    AsymI {
        h: f64,
        bt: f64,
        bb: f64,
        tw: f64,
        tft: f64,
        tfb: f64,
        #[serde(default)]
        r: f64,
    },
    U {
        h: f64,
        b: f64,
        tw: f64,
        tf: f64,
        #[serde(default)]
        r1: f64,
        #[serde(default)]
        r2: f64,
        #[serde(default)]
        slope: f64,
    },
    L {
        h: f64,
        b: f64,
        t: f64,
        #[serde(default)]
        r1: f64,
        #[serde(default)]
        r2: f64,
    },
    T {
        h: f64,
        b: f64,
        tw: f64,
        tf: f64,
        #[serde(default)]
        r: f64,
    },
    C {
        h: f64,
        b: f64,
        t: f64,
        #[serde(default)]
        lip: f64,
        #[serde(default)]
        r: f64,
    },
    Z {
        h: f64,
        b: f64,
        tw: f64,
        tf: f64,
        #[serde(default)]
        r: f64,
    },
    Rect { b: f64, h: f64 },
    Circle { d: f64 },
    Chs { d: f64, t: f64 },
    Rhs {
        b: f64,
        h: f64,
        t: f64,
        #[serde(default)]
        r_out: f64,
        #[serde(default)]
        r_in: f64,
    },
    #[serde(rename = "polygon")]
    Polygon {
        outer: Vec<V2>,
        #[serde(default)]
        holes: Vec<Vec<V2>>,
    },
    #[serde(rename = "composite")]
    Composite(Vec<CompositePart>),
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompositePart {
    pub shape: Shape,
    /// Column-major 2×3 `[xx, xy, yx, yy, tx, ty]`.
    pub transform: [f64; 6],
}

/// Section properties as the source gives them; all optional.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Props {
    #[serde(rename = "A", default, skip_serializing_if = "Option::is_none")]
    pub a: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub centroid: Option<V2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shear_centre: Option<V2>,
    #[serde(rename = "Iy", default, skip_serializing_if = "Option::is_none")]
    pub iy: Option<f64>,
    #[serde(rename = "Iz", default, skip_serializing_if = "Option::is_none")]
    pub iz: Option<f64>,
    #[serde(rename = "Iyz", default, skip_serializing_if = "Option::is_none")]
    pub iyz: Option<f64>,
    #[serde(rename = "It", default, skip_serializing_if = "Option::is_none")]
    pub it: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: u64,
    pub no: String,
    pub p: V3,
    /// Skewed support frame: the frame's x, y and z axes in global coordinates.
    #[serde(default)]
    pub frame: Option<[V3; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<Support>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub origin: NodeOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NodeOrigin {
    #[default]
    Analytical,
    DerivedFromPhysical { element: u64, approximate: bool },
}

/// One DOF: restrained (`fixed`), free, or elastic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Dof {
    Fixed,
    Free,
    Spring(f64),
    TensionOnly(f64),
    CompressionOnly(f64),
    Failure { k: f64, limit: f64 },
    Slippage { k: f64, gap: f64 },
    /// Displacement-force pairs.
    Diagram(Vec<V2>),
}

/// A nodal or line support: `dofs`, or a `named` type from the preset table, or both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Support {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dofs: Option<[Dof; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub frame: DofFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DofFrame {
    #[default]
    Global,
    /// The node's `frame`.
    Node,
    /// The local frame of this member.
    Member(u64),
}

/// A member-end release (always member local): `dofs` (`fixed` = connected), `named`, or both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Release {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dofs: Option<[Dof; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub id: u64,
    pub no: String,
    pub kind: MemberKind,
    /// Start, intermediate (ordered along the member), end.
    pub nodes: Vec<u64>,
    #[serde(default)]
    pub geometry: Curve,
    /// Unit local z, perpendicular to the axis; always given.
    pub z: V3,
    #[serde(default)]
    pub orientation: Orientation,
    /// Start and end (taper) section.
    #[serde(default)]
    pub section: Option<(u64, Option<u64>)>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub section_rotation: f64,
    #[serde(default)]
    pub insertion: Insertion,
    #[serde(default)]
    pub releases: [Option<Release>; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Beam,
    Truss,
    Cable,
    TensionOnly,
    CompressionOnly,
    Rigid,
    Spring,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    #[default]
    Line,
    Arc { mid: V3 },
    /// The vertices strictly between the start and end node.
    Polyline(Vec<V3>),
}

/// Provenance of `z`; never interpreted.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    IfcAxis,
    Beta { angle: f64, rule: String },
    RefNode(u64),
    RefVector(V3),
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Insertion {
    /// IFC4 IfcCardinalPointReference 1-19, 0 = none.
    #[serde(default)]
    pub cardinal_point: u32,
    /// The profile definition's own Position, `[xx, xy, yx, yy, tx, ty]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_position: Option<[f64; 6]>,
    /// IfcMaterialProfileWithOffsets at start and end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_offsets: Option<[V2; 2]>,
    /// Member-wide (ey, ez), local axes; the profile moves by +e.
    #[serde(default)]
    pub eccentricity: V2,
    #[serde(default)]
    pub end_offsets: EndOffsets,
}

impl Default for Insertion {
    fn default() -> Self {
        Insertion { cardinal_point: 0, profile_position: None, profile_offsets: None, eccentricity: [0.0; 2], end_offsets: EndOffsets::default() }
    }
}

/// Rigid offsets from node to member end.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EndOffsets {
    #[serde(default)]
    pub start: V3,
    #[serde(default)]
    pub end: V3,
    #[serde(default)]
    pub frame: OffsetFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OffsetFrame {
    #[default]
    Local,
    Global,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub id: u64,
    pub no: String,
    pub kind: SurfaceKind,
    /// Node loop.
    pub boundary: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<u64>>,
    pub thickness: Thickness,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset_z: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_dir: Option<V3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    Plate,
    Membrane,
    Shell,
    Rigid,
    LoadTransfer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Thickness {
    Uniform(f64),
    /// Three points of the plane `[x, y, z, t]`.
    Linear([[f64; 4]; 3]),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LineSupport {
    pub id: u64,
    pub no: String,
    pub along: Carrier,
    /// Springs per length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dofs: Option<[Dof; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub frame: DofFrame,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Carrier {
    Member(u64),
    SurfaceEdge { surface: u64, from: u64, to: u64 },
    Polyline(Vec<V3>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoadCase {
    pub id: u64,
    pub no: String,
    pub name: String,
    pub action_type: ActionType,
    #[serde(default)]
    pub action_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_weight: Option<V3>,
    #[serde(default = "one")]
    pub coefficient: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Permanent,
    Variable,
    Accidental,
    User(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Combination {
    pub id: u64,
    pub no: String,
    pub name: String,
    pub purpose: Purpose,
    /// `[load, factor]` pairs; shown, never evaluated.
    pub terms: Vec<(LoadRef, f64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Uls,
    Sls,
    Als,
    User(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoadRef {
    Case(u64),
    Combination(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: u64,
    pub load: LoadRef,
    pub target: Target,
    pub frame: LoadFrame,
    pub kind: Load,
    #[serde(default, skip_serializing_if = "is_false")]
    pub destabilising: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Node(u64),
    Member(u64),
    /// Part of a member, absolute metres from its start node.
    MemberSpan { member: u64, span: V2 },
    Surface(u64),
    Free(Vec<V3>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoadFrame {
    Global,
    Local,
    GlobalProjected,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Load {
    NodalForce {
        f: V3,
        #[serde(default)]
        m: V3,
    },
    MemberPoint {
        x: f64,
        f: V3,
        #[serde(default)]
        m: V3,
    },
    MemberDistributed {
        dir: V3,
        #[serde(default)]
        moment: bool,
        dist: Distribution,
    },
    Surface { dir: V3, dist: SurfaceDistribution },
    Temperature {
        dt: f64,
        #[serde(default)]
        dt_y: f64,
        #[serde(default)]
        dt_z: f64,
    },
    Displacement { u: [Option<f64>; 6] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Distribution {
    Uniform(f64),
    Linear { q1: f64, q2: f64 },
    Trapezoidal { a: f64, b: f64, q1: f64, q2: f64 },
    /// `[x, q]` pairs, x ascending.
    Polygonal(Vec<V2>),
    Parabolic(f64),
    Sine(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceDistribution {
    Uniform(f64),
    Bilinear(V3),
    /// `[u, v, p]` samples in the surface plane.
    Sampled(Vec<V3>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ResultSet {
    pub id: u64,
    pub no: String,
    pub name: String,
    #[serde(rename = "for")]
    pub for_: ResultFor,
    pub theory: Theory,
    pub linear: bool,
    /// The tables this set has.
    pub tables: Vec<TableName>,
    /// Inline result blocks, base64, by table name.
    #[serde(default)]
    pub inline: Option<Inline>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ResultFor {
    Case(u64),
    Combination(u64),
    Envelope(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Theory {
    FirstOrder,
    SecondOrder,
    ThirdOrder,
    FullNonlinear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TableName {
    MemberForces,
    MemberDeformations,
    NodeDisplacements,
    Reactions,
    Envelope,
}

/// Base64 result blocks.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Inline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_forces: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_deformations: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_displacements: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub analytical: u64,
    pub physical: u64,
    pub source: LinkSource,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LinkSource {
    IfcRelAssignsToProduct,
    RevitAssociation,
    Matched { distance: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    #[serde(default)]
    pub object: Option<u64>,
    pub code: String,
    #[serde(default)]
    pub message: String,
}

impl Inline {
    /// `(table, base64)` pairs present.
    pub fn entries(&self) -> Vec<(TableName, &str)> {
        [
            (TableName::MemberForces, &self.member_forces),
            (TableName::MemberDeformations, &self.member_deformations),
            (TableName::NodeDisplacements, &self.node_displacements),
            (TableName::Reactions, &self.reactions),
            (TableName::Envelope, &self.envelope),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_deref().map(|s| (k, s)))
        .collect()
    }

    pub fn set(&mut self, table: TableName, b64: String) {
        let slot = match table {
            TableName::MemberForces => &mut self.member_forces,
            TableName::MemberDeformations => &mut self.member_deformations,
            TableName::NodeDisplacements => &mut self.node_displacements,
            TableName::Reactions => &mut self.reactions,
            TableName::Envelope => &mut self.envelope,
        };
        *slot = Some(b64);
    }
}

impl TableName {
    pub fn kind(self) -> crate::results::TableKind {
        use crate::results::TableKind as K;
        match self {
            TableName::MemberForces => K::MemberForces,
            TableName::MemberDeformations => K::MemberDeformations,
            TableName::NodeDisplacements => K::NodeDisplacements,
            TableName::Reactions => K::Reactions,
            TableName::Envelope => K::Envelope,
        }
    }

    pub fn from_kind(kind: crate::results::TableKind) -> Self {
        use crate::results::TableKind as K;
        match kind {
            K::MemberForces => TableName::MemberForces,
            K::MemberDeformations => TableName::MemberDeformations,
            K::NodeDisplacements => TableName::NodeDisplacements,
            K::Reactions => TableName::Reactions,
            K::Envelope => TableName::Envelope,
        }
    }
}
