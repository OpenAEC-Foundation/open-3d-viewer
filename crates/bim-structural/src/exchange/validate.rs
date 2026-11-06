//! `sam-json` v1 conformance: one rule per check. `sam-check` and [`super::read`] run it.

use std::collections::{HashMap, HashSet};

use glam::{DMat3, DVec3};
use serde::ser::{self, Serialize};

use super::wire as w;
use crate::base64;
use crate::frame::{curve_length, start_tangent};
use crate::id::{kind_byte, kind_of, normalise, Id, IdKind};
use crate::model::Curve;
use crate::results::{self, ResultBlock};
use crate::section;
use crate::{hinge, support};

/// The conformance rules. Each has a stable code (see [`Rule::code`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    /// The text parses as the schema (types, required fields, no unknown fields).
    Schema,
    /// `format` is "sam-json", `version` 1, `units` "SI".
    Format,
    /// No NaN or infinity anywhere.
    Finite,
    /// Nodes, members, surfaces and line supports use `sam_id` with their kind; other shifted
    /// ids carry their own kind.
    IdKind,
    /// Model ids are unique in the document and item ids unique in their model.
    IdUnique,
    /// Every referenced node exists.
    NodeRef,
    /// Every referenced member exists.
    MemberRef,
    /// Every referenced surface exists.
    SurfaceRef,
    /// Every referenced section exists.
    SectionRef,
    /// Every referenced material exists.
    MaterialRef,
    /// Action loads and combination terms name existing cases or combinations.
    LoadRef,
    /// Result sets are for an existing case or combination.
    ResultRef,
    /// Links start at a node, member or surface of the model.
    LinkRef,
    /// A member has at least two nodes, distinct ends and a non-zero length.
    MemberNodes,
    /// `z` is a unit vector perpendicular to the member axis at its start (within 1e-9).
    MemberZ,
    /// Cardinal points are 0-19.
    CardinalPoint,
    /// Supports and releases give `dofs` or a `named` type (unknown names and negative
    /// springs are warnings).
    Dofs,
    /// A `node` frame needs the node's `frame`; a `member` frame names a member at that node;
    /// line supports have no node frame.
    DofFrame,
    /// Node frames are orthonormal and right-handed (within 1e-9).
    NodeFrame,
    /// `global_transform` and `shared_frame` are rigid (rotation and translation).
    RigidTransform,
    /// A plane model's normal is a unit vector.
    Dimension,
    /// Section dimensions are positive and fit together.
    SectionShape,
    /// Surface boundaries have at least three distinct nodes and a positive thickness; line
    /// support polylines have at least two points.
    SurfaceBoundary,
    /// Load positions lie on the member and ascend.
    LoadPosition,
    /// Result blocks decode and match their set, kind, table list and ids.
    ResultBlock,
    /// Stations ascend within the member, with jumps as exact pairs.
    Stations,
}

impl Rule {
    pub const ALL: [Rule; 26] = [
        Rule::Schema,
        Rule::Format,
        Rule::Finite,
        Rule::IdKind,
        Rule::IdUnique,
        Rule::NodeRef,
        Rule::MemberRef,
        Rule::SurfaceRef,
        Rule::SectionRef,
        Rule::MaterialRef,
        Rule::LoadRef,
        Rule::ResultRef,
        Rule::LinkRef,
        Rule::MemberNodes,
        Rule::MemberZ,
        Rule::CardinalPoint,
        Rule::Dofs,
        Rule::DofFrame,
        Rule::NodeFrame,
        Rule::RigidTransform,
        Rule::Dimension,
        Rule::SectionShape,
        Rule::SurfaceBoundary,
        Rule::LoadPosition,
        Rule::ResultBlock,
        Rule::Stations,
    ];

    /// Stable kebab-case code.
    pub fn code(self) -> &'static str {
        match self {
            Rule::Schema => "schema",
            Rule::Format => "format",
            Rule::Finite => "finite",
            Rule::IdKind => "id-kind",
            Rule::IdUnique => "id-unique",
            Rule::NodeRef => "node-ref",
            Rule::MemberRef => "member-ref",
            Rule::SurfaceRef => "surface-ref",
            Rule::SectionRef => "section-ref",
            Rule::MaterialRef => "material-ref",
            Rule::LoadRef => "load-ref",
            Rule::ResultRef => "result-ref",
            Rule::LinkRef => "link-ref",
            Rule::MemberNodes => "member-nodes",
            Rule::MemberZ => "member-z",
            Rule::CardinalPoint => "cardinal-point",
            Rule::Dofs => "dofs",
            Rule::DofFrame => "dof-frame",
            Rule::NodeFrame => "node-frame",
            Rule::RigidTransform => "rigid-transform",
            Rule::Dimension => "dimension",
            Rule::SectionShape => "section-shape",
            Rule::SurfaceBoundary => "surface-boundary",
            Rule::LoadPosition => "load-position",
            Rule::ResultBlock => "result-block",
            Rule::Stations => "stations",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One finding.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub rule: Rule,
    pub severity: Severity,
    /// JSON-pointer-like location, e.g. `/models/0/members/2/z`.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}[{}] {}: {}", self.rule.code(), if self.path.is_empty() { "/" } else { &self.path }, self.message)
    }
}

/// The findings of a check.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Warning)
    }
    /// No errors (warnings allowed).
    pub fn is_ok(&self) -> bool {
        self.errors().next().is_none()
    }
    /// Rules with at least one error.
    pub fn failed_rules(&self) -> Vec<Rule> {
        let mut r: Vec<Rule> = self.errors().map(|i| i.rule).collect();
        r.sort();
        r.dedup();
        r
    }
    fn error(&mut self, rule: Rule, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue { rule, severity: Severity::Error, path: path.into(), message: message.into() });
    }
    fn warn(&mut self, rule: Rule, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue { rule, severity: Severity::Warning, path: path.into(), message: message.into() });
    }
}

/// Parses `sam-json` text into the wire document; a failure is a `schema` error (or `finite`
/// for numbers out of the f64 range).
pub fn parse(text: &str) -> Result<w::Document, Report> {
    serde_json::from_str::<w::Document>(text).map_err(|e| {
        let mut r = Report::default();
        let msg = e.to_string();
        let rule = if msg.contains("number out of range") { Rule::Finite } else { Rule::Schema };
        r.error(rule, format!("line {} column {}", e.line(), e.column()), msg);
        r
    })
}

/// Checks text and optional sidecar blocks `(name, bytes)`.
pub fn check(text: &str, blocks: &[(String, Vec<u8>)]) -> Report {
    match parse(text) {
        Err(r) => r,
        Ok(doc) => {
            let mut r = validate(&doc);
            for (name, bytes) in blocks {
                r.issues.extend(validate_block(&doc, name, bytes).issues);
            }
            r
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Finite numbers: a serializer that only looks at floats.

#[derive(Default)]
struct FloatScan {
    path: Vec<String>,
    counters: Vec<usize>,
    bad: Vec<(String, f64)>,
}

#[derive(Debug)]
struct ScanError;
impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("scan")
    }
}
impl std::error::Error for ScanError {}
impl ser::Error for ScanError {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        ScanError
    }
}

impl FloatScan {
    fn float(&mut self, v: f64) {
        if !v.is_finite() {
            self.bad.push((format!("/{}", self.path.join("/")), v));
        }
    }
    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ScanError> {
        let i = self.counters.last_mut().map(|c| {
            let i = *c;
            *c += 1;
            i
        });
        self.path.push(i.unwrap_or(0).to_string());
        value.serialize(&mut *self)?;
        self.path.pop();
        Ok(())
    }
    fn field<T: ?Sized + Serialize>(&mut self, key: &str, value: &T) -> Result<(), ScanError> {
        self.path.push(key.to_string());
        value.serialize(&mut *self)?;
        self.path.pop();
        Ok(())
    }
    fn open(&mut self) {
        self.counters.push(0);
    }
    fn close(&mut self) {
        self.counters.pop();
    }
}

macro_rules! ignore {
    ($($name:ident: $t:ty),*) => {
        $(fn $name(self, _v: $t) -> Result<(), ScanError> { Ok(()) })*
    };
}

impl<'a> ser::Serializer for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64,
            serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64, serialize_char: char,
            serialize_str: &str, serialize_bytes: &[u8]);

    fn serialize_f32(self, v: f32) -> Result<(), ScanError> {
        self.float(v as f64);
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<(), ScanError> {
        self.float(v);
        Ok(())
    }
    fn serialize_none(self) -> Result<(), ScanError> {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), ScanError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), ScanError> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), ScanError> {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), ScanError> {
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> Result<(), ScanError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _: &'static str, _: u32, variant: &'static str, value: &T) -> Result<(), ScanError> {
        self.field(variant, value)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, ScanError> {
        self.open();
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, ScanError> {
        self.open();
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, ScanError> {
        self.open();
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Self, ScanError> {
        self.path.push(variant.to_string());
        self.open();
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, ScanError> {
        self.open();
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, ScanError> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Self, ScanError> {
        self.path.push(variant.to_string());
        Ok(self)
    }
}

impl<'a> ser::SerializeSeq for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ScanError> {
        self.element(value)
    }
    fn end(self) -> Result<(), ScanError> {
        self.close();
        Ok(())
    }
}
impl<'a> ser::SerializeTuple for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ScanError> {
        self.element(value)
    }
    fn end(self) -> Result<(), ScanError> {
        self.close();
        Ok(())
    }
}
impl<'a> ser::SerializeTupleStruct for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ScanError> {
        self.element(value)
    }
    fn end(self) -> Result<(), ScanError> {
        self.close();
        Ok(())
    }
}
impl<'a> ser::SerializeTupleVariant for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ScanError> {
        self.element(value)
    }
    fn end(self) -> Result<(), ScanError> {
        self.close();
        self.path.pop();
        Ok(())
    }
}
impl<'a> ser::SerializeMap for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, _key: &T) -> Result<(), ScanError> {
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ScanError> {
        self.element(value)
    }
    fn end(self) -> Result<(), ScanError> {
        self.close();
        Ok(())
    }
}
impl<'a> ser::SerializeStruct for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), ScanError> {
        self.field(key, value)
    }
    fn end(self) -> Result<(), ScanError> {
        Ok(())
    }
}
impl<'a> ser::SerializeStructVariant for &'a mut FloatScan {
    type Ok = ();
    type Error = ScanError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), ScanError> {
        self.field(key, value)
    }
    fn end(self) -> Result<(), ScanError> {
        self.path.pop();
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The checks

fn v3(a: w::V3) -> DVec3 {
    DVec3::from_array(a)
}

fn curve_of(c: &w::Curve) -> Curve {
    match c {
        w::Curve::Line => Curve::Line,
        w::Curve::Arc { mid } => Curve::Arc { mid: v3(*mid) },
        w::Curve::Polyline(p) => Curve::Polyline(p.iter().copied().map(v3).collect()),
    }
}

fn is_rigid(m: &w::M4) -> Result<(), String> {
    let r = DMat3::from_cols(DVec3::new(m[0], m[1], m[2]), DVec3::new(m[4], m[5], m[6]), DVec3::new(m[8], m[9], m[10]));
    if [m[3], m[7], m[11]].iter().any(|&x| x != 0.0) || m[15] != 1.0 {
        return Err("last row must be 0 0 0 1".into());
    }
    orthonormal(r)
}

fn orthonormal(r: DMat3) -> Result<(), String> {
    let e = (r.transpose() * r - DMat3::IDENTITY).to_cols_array().iter().fold(0.0f64, |a, &b| a.max(b.abs()));
    if e > 1e-9 {
        return Err(format!("not orthonormal (deviation {e:.3e})"));
    }
    if r.determinant() < 0.0 {
        return Err("mirrored (determinant −1); rotate instead".into());
    }
    Ok(())
}

/// Ids of one model's items, normalised.
struct Index<'a> {
    nodes: HashMap<Id, &'a w::Node>,
    members: HashMap<Id, &'a w::Member>,
    surfaces: HashSet<Id>,
    sections: HashSet<Id>,
    materials: HashSet<Id>,
    cases: HashSet<Id>,
    combinations: HashSet<Id>,
}

impl<'a> Index<'a> {
    fn new(m: &'a w::Model) -> Self {
        Index {
            nodes: m.nodes.iter().map(|n| (n.id, n)).collect(),
            members: m.members.iter().map(|x| (x.id, x)).collect(),
            surfaces: m.surfaces.iter().map(|x| x.id).collect(),
            sections: m.sections.iter().map(|x| normalise(IdKind::Section, x.id)).collect(),
            materials: m.materials.iter().map(|x| normalise(IdKind::Material, x.id)).collect(),
            cases: m.load_cases.iter().map(|x| normalise(IdKind::LoadCase, x.id)).collect(),
            combinations: m.combinations.iter().map(|x| normalise(IdKind::Combination, x.id)).collect(),
        }
    }

    fn member_length(&self, id: Id) -> Option<f64> {
        let m = self.members.get(&id)?;
        let a = self.nodes.get(m.nodes.first()?)?.p;
        let b = self.nodes.get(m.nodes.last()?)?.p;
        Some(curve_length(v3(a), v3(b), &curve_of(&m.geometry)))
    }
}

/// Validates a parsed document, including its inline result blocks.
pub fn validate(doc: &w::Document) -> Report {
    let mut r = Report::default();

    // finite
    let mut scan = FloatScan::default();
    let _ = doc.serialize(&mut scan);
    for (path, v) in scan.bad {
        r.error(Rule::Finite, path, format!("non-finite number {v}"));
    }

    // format
    if doc.format != "sam-json" {
        r.error(Rule::Format, "/format", format!("expected \"sam-json\", got {:?}", doc.format));
    }
    if doc.version != 1 {
        r.error(Rule::Format, "/version", format!("expected 1, got {}", doc.version));
    }
    if doc.units != "SI" {
        r.error(Rule::Format, "/units", format!("expected \"SI\", got {:?}", doc.units));
    }

    // rigid transforms
    if let Err(e) = is_rigid(&doc.source.global_transform) {
        r.error(Rule::RigidTransform, "/source/global_transform", e);
    }

    let mut model_ids = HashSet::new();
    for (mi, m) in doc.models.iter().enumerate() {
        let base = format!("/models/{mi}");
        if !model_ids.insert(m.id) {
            r.error(Rule::IdUnique, format!("{base}/id"), format!("model id {} used twice", m.id));
        }
        validate_model(m, &base, &mut r);
    }
    r
}

fn validate_model(m: &w::Model, base: &str, r: &mut Report) {
    let ix = Index::new(m);

    if let Err(e) = is_rigid(&m.shared_frame) {
        r.error(Rule::RigidTransform, format!("{base}/shared_frame"), e);
    }
    if let w::Dimension::Plane2d { normal } = m.dimension {
        let n = v3(normal).length();
        if (n - 1.0).abs() > 1e-9 {
            r.error(Rule::Dimension, format!("{base}/dimension"), format!("plane normal has length {n}"));
        }
    }

    // id-kind and id-unique
    let mut seen: HashSet<Id> = HashSet::new();
    let mut id_checks = |r: &mut Report, path: String, id: Id, kind: IdKind, must_shift: bool| {
        let kb = kind_byte(id);
        if must_shift && kind_of(id) != Some(kind) {
            r.error(Rule::IdKind, format!("{path}/id"), format!("{} id {id} is not sam_id({}, n) (kind byte {kb})", kind.name(), kind as u8));
        } else if !must_shift && kb != 0 && kind_of(id) != Some(kind) {
            r.error(Rule::IdKind, format!("{path}/id"), format!("{} id {id} has kind byte {kb}, expected 0 or {}", kind.name(), kind as u8));
        }
        if !seen.insert(normalise(kind, id)) {
            r.error(Rule::IdUnique, format!("{path}/id"), format!("{} id {id} used twice", kind.name()));
        }
    };
    for (i, x) in m.nodes.iter().enumerate() {
        id_checks(r, format!("{base}/nodes/{i}"), x.id, IdKind::Node, true);
    }
    for (i, x) in m.members.iter().enumerate() {
        id_checks(r, format!("{base}/members/{i}"), x.id, IdKind::Member, true);
    }
    for (i, x) in m.surfaces.iter().enumerate() {
        id_checks(r, format!("{base}/surfaces/{i}"), x.id, IdKind::Surface, true);
    }
    for (i, x) in m.line_supports.iter().enumerate() {
        id_checks(r, format!("{base}/line_supports/{i}"), x.id, IdKind::LineSupport, true);
    }
    for (i, x) in m.sections.iter().enumerate() {
        id_checks(r, format!("{base}/sections/{i}"), x.id, IdKind::Section, false);
    }
    for (i, x) in m.materials.iter().enumerate() {
        id_checks(r, format!("{base}/materials/{i}"), x.id, IdKind::Material, false);
    }
    for (i, x) in m.load_cases.iter().enumerate() {
        id_checks(r, format!("{base}/load_cases/{i}"), x.id, IdKind::LoadCase, false);
    }
    for (i, x) in m.combinations.iter().enumerate() {
        id_checks(r, format!("{base}/combinations/{i}"), x.id, IdKind::Combination, false);
    }
    for (i, x) in m.actions.iter().enumerate() {
        id_checks(r, format!("{base}/actions/{i}"), x.id, IdKind::Action, false);
    }
    for (i, x) in m.results.iter().enumerate() {
        id_checks(r, format!("{base}/results/{i}"), x.id, IdKind::ResultSet, false);
    }

    let node_ref = |r: &mut Report, path: String, id: Id| {
        if !ix.nodes.contains_key(&id) {
            r.error(Rule::NodeRef, path, format!("node {id} does not exist"));
        }
    };
    let member_ref = |r: &mut Report, path: String, id: Id| {
        if !ix.members.contains_key(&id) {
            r.error(Rule::MemberRef, path, format!("member {id} does not exist"));
        }
    };
    let surface_ref = |r: &mut Report, path: String, id: Id| {
        if !ix.surfaces.contains(&id) {
            r.error(Rule::SurfaceRef, path, format!("surface {id} does not exist"));
        }
    };
    let section_ref = |r: &mut Report, path: String, id: Id| {
        if !ix.sections.contains(&normalise(IdKind::Section, id)) {
            r.error(Rule::SectionRef, path, format!("section {id} does not exist"));
        }
    };
    let material_ref = |r: &mut Report, path: String, id: Option<Id>| {
        if let Some(id) = id {
            if !ix.materials.contains(&normalise(IdKind::Material, id)) {
                r.error(Rule::MaterialRef, path, format!("material {id} does not exist"));
            }
        }
    };
    let load_ref = |r: &mut Report, path: String, l: w::LoadRef| match l {
        w::LoadRef::Case(id) if !ix.cases.contains(&normalise(IdKind::LoadCase, id)) => r.error(Rule::LoadRef, path, format!("load case {id} does not exist")),
        w::LoadRef::Combination(id) if !ix.combinations.contains(&normalise(IdKind::Combination, id)) => {
            r.error(Rule::LoadRef, path, format!("combination {id} does not exist"))
        }
        _ => {}
    };

    // sections and materials
    for (i, s) in m.sections.iter().enumerate() {
        material_ref(r, format!("{base}/sections/{i}/material"), s.material);
        let shape = super::convert_shape(&s.shape);
        if !matches!(shape, crate::model::Shape::Unknown) {
            if let Err(e) = section::check(&shape) {
                r.error(Rule::SectionShape, format!("{base}/sections/{i}/shape"), e.to_string());
            }
        }
    }

    // nodes
    for (i, n) in m.nodes.iter().enumerate() {
        let path = format!("{base}/nodes/{i}");
        if let Some(f) = n.frame {
            if let Err(e) = orthonormal(DMat3::from_cols(v3(f[0]), v3(f[1]), v3(f[2]))) {
                r.error(Rule::NodeFrame, format!("{path}/frame"), e);
            }
        }
        if let Some(s) = &n.support {
            check_dofs(r, &format!("{path}/support"), &s.dofs, &s.named, false);
            match s.frame {
                w::DofFrame::Node if n.frame.is_none() => r.error(Rule::DofFrame, format!("{path}/support/frame"), "support frame \"node\" but the node has no frame"),
                w::DofFrame::Member(mid) => match ix.members.get(&mid) {
                    Some(mem) if mem.nodes.contains(&n.id) => {}
                    Some(_) => r.error(Rule::DofFrame, format!("{path}/support/frame"), format!("member {mid} does not connect node {}", n.id)),
                    None => r.error(Rule::DofFrame, format!("{path}/support/frame"), format!("member {mid} does not exist")),
                },
                _ => {}
            }
        }
    }

    // members
    for (i, mem) in m.members.iter().enumerate() {
        let path = format!("{base}/members/{i}");
        for (k, n) in mem.nodes.iter().enumerate() {
            node_ref(r, format!("{path}/nodes/{k}"), *n);
        }
        if let w::Orientation::RefNode(n) = mem.orientation {
            node_ref(r, format!("{path}/orientation/ref_node"), n);
        }
        if let Some((a, b)) = mem.section {
            section_ref(r, format!("{path}/section/0"), a);
            if let Some(b) = b {
                section_ref(r, format!("{path}/section/1"), b);
            }
        }
        material_ref(r, format!("{path}/material"), mem.material);
        if mem.insertion.cardinal_point > 19 {
            r.error(Rule::CardinalPoint, format!("{path}/insertion/cardinal_point"), format!("{} is not 0-19", mem.insertion.cardinal_point));
        }
        for (k, rel) in mem.releases.iter().enumerate() {
            if let Some(rel) = rel {
                check_dofs(r, &format!("{path}/releases/{k}"), &rel.dofs, &rel.named, true);
            }
        }
        // member-nodes and member-z
        if mem.nodes.len() < 2 {
            r.error(Rule::MemberNodes, format!("{path}/nodes"), "a member needs at least two nodes");
            continue;
        }
        let (first, last) = (mem.nodes[0], *mem.nodes.last().unwrap());
        if first == last {
            r.error(Rule::MemberNodes, format!("{path}/nodes"), "start and end node are the same");
            continue;
        }
        if mem.nodes.windows(2).any(|p| p[0] == p[1]) {
            r.error(Rule::MemberNodes, format!("{path}/nodes"), "a node repeats in sequence");
        }
        let (Some(a), Some(b)) = (ix.nodes.get(&first), ix.nodes.get(&last)) else { continue };
        let (pa, pb) = (v3(a.p), v3(b.p));
        let curve = curve_of(&mem.geometry);
        if curve_length(pa, pb, &curve) < crate::frame::MIN_LENGTH || (pb - pa).length() < crate::frame::MIN_LENGTH {
            r.error(Rule::MemberNodes, format!("{path}/nodes"), "zero-length member");
            continue;
        }
        let z = v3(mem.z);
        if (z.length() - 1.0).abs() > 1e-9 {
            r.error(Rule::MemberZ, format!("{path}/z"), format!("z has length {}", z.length()));
        }
        match start_tangent(pa, pb, &curve) {
            Some(t) if (z.dot(t)).abs() > 1e-9 => r.error(Rule::MemberZ, format!("{path}/z"), format!("z is not perpendicular to the axis (z·x = {:.3e})", z.dot(t))),
            Some(_) => {}
            None => r.error(Rule::MemberNodes, format!("{path}/geometry"), "degenerate axis curve"),
        }
    }

    // surfaces
    for (i, s) in m.surfaces.iter().enumerate() {
        let path = format!("{base}/surfaces/{i}");
        for (k, n) in s.boundary.iter().enumerate() {
            node_ref(r, format!("{path}/boundary/{k}"), *n);
        }
        for (o, op) in s.openings.iter().enumerate() {
            for (k, n) in op.iter().enumerate() {
                node_ref(r, format!("{path}/openings/{o}/{k}"), *n);
            }
        }
        material_ref(r, format!("{path}/material"), s.material);
        let distinct: HashSet<_> = s.boundary.iter().collect();
        if s.boundary.len() < 3 || distinct.len() != s.boundary.len() {
            r.error(Rule::SurfaceBoundary, format!("{path}/boundary"), "a boundary needs at least three distinct nodes");
        }
        let thick = match &s.thickness {
            w::Thickness::Uniform(t) => vec![*t],
            w::Thickness::Linear(p) => p.iter().map(|q| q[3]).collect(),
            w::Thickness::Unknown => vec![],
        };
        if thick.iter().any(|t| !(*t > 0.0)) {
            r.error(Rule::SurfaceBoundary, format!("{path}/thickness"), "thickness must be positive");
        }
    }

    // line supports
    for (i, l) in m.line_supports.iter().enumerate() {
        let path = format!("{base}/line_supports/{i}");
        match &l.along {
            w::Carrier::Member(id) => member_ref(r, format!("{path}/along/member"), *id),
            w::Carrier::SurfaceEdge { surface, from, to } => {
                surface_ref(r, format!("{path}/along/surface_edge/surface"), *surface);
                node_ref(r, format!("{path}/along/surface_edge/from"), *from);
                node_ref(r, format!("{path}/along/surface_edge/to"), *to);
            }
            w::Carrier::Polyline(p) => {
                if p.len() < 2 {
                    r.error(Rule::SurfaceBoundary, format!("{path}/along/polyline"), "a polyline needs at least two points");
                }
            }
        }
        check_dofs(r, &path, &l.dofs, &l.named, false);
        match l.frame {
            w::DofFrame::Node => r.error(Rule::DofFrame, format!("{path}/frame"), "a line support has no node frame"),
            w::DofFrame::Member(mid) if !ix.members.contains_key(&mid) => r.error(Rule::DofFrame, format!("{path}/frame"), format!("member {mid} does not exist")),
            _ => {}
        }
    }

    // loads
    for (i, c) in m.combinations.iter().enumerate() {
        for (k, (l, _)) in c.terms.iter().enumerate() {
            load_ref(r, format!("{base}/combinations/{i}/terms/{k}"), *l);
        }
    }
    for (i, a) in m.actions.iter().enumerate() {
        let path = format!("{base}/actions/{i}");
        load_ref(r, format!("{path}/load"), a.load);
        let mut member_len = None;
        match &a.target {
            w::Target::Node(n) => node_ref(r, format!("{path}/target/node"), *n),
            w::Target::Member(id) => {
                member_ref(r, format!("{path}/target/member"), *id);
                member_len = ix.member_length(*id);
            }
            w::Target::MemberSpan { member, span } => {
                member_ref(r, format!("{path}/target/member_span/member"), *member);
                member_len = ix.member_length(*member);
                if let Some(len) = member_len {
                    if !(span[0] >= -1e-9 && span[0] <= span[1] && span[1] <= len + 1e-9) {
                        r.error(Rule::LoadPosition, format!("{path}/target/member_span/span"), format!("span {span:?} not within 0..{len}"));
                    }
                }
            }
            w::Target::Surface(s) => surface_ref(r, format!("{path}/target/surface"), *s),
            w::Target::Free(p) => {
                if p.is_empty() {
                    r.error(Rule::LoadPosition, format!("{path}/target/free"), "no points");
                }
            }
        }
        if let Some(len) = member_len {
            let on = |x: f64| x >= -1e-9 && x <= len + 1e-9;
            match &a.kind {
                w::Load::MemberPoint { x, .. } if !on(*x) => r.error(Rule::LoadPosition, format!("{path}/kind/member_point/x"), format!("x = {x} not within 0..{len}")),
                w::Load::MemberDistributed { dist, .. } => match dist {
                    w::Distribution::Trapezoidal { a, b, .. } if !(on(*a) && on(*b) && a <= b) => {
                        r.error(Rule::LoadPosition, format!("{path}/kind/member_distributed/dist"), format!("a = {a}, b = {b} not ordered within 0..{len}"))
                    }
                    w::Distribution::Polygonal(p) if p.iter().any(|q| !on(q[0])) || p.windows(2).any(|w| w[1][0] < w[0][0]) || p.len() < 2 => {
                        r.error(Rule::LoadPosition, format!("{path}/kind/member_distributed/dist"), format!("polygonal stations must ascend within 0..{len}"))
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    // results
    for (i, rs) in m.results.iter().enumerate() {
        let path = format!("{base}/results/{i}");
        match rs.for_ {
            w::ResultFor::Case(id) if !ix.cases.contains(&normalise(IdKind::LoadCase, id)) => r.error(Rule::ResultRef, format!("{path}/for"), format!("load case {id} does not exist")),
            w::ResultFor::Combination(id) if !ix.combinations.contains(&normalise(IdKind::Combination, id)) => {
                r.error(Rule::ResultRef, format!("{path}/for"), format!("combination {id} does not exist"))
            }
            _ => {}
        }
        if let Some(inline) = &rs.inline {
            for (table, b64) in inline.entries() {
                let bpath = format!("{path}/inline/{}", table.kind().name());
                match base64::decode(b64) {
                    Err(e) => r.error(Rule::ResultBlock, bpath, e),
                    Ok(bytes) => match results::decode(&bytes) {
                        Err(e) => r.error(Rule::ResultBlock, bpath, e),
                        Ok(block) => {
                            if block.kind != table.kind() {
                                r.error(Rule::ResultBlock, bpath.clone(), format!("block holds {} under {}", block.kind.name(), table.kind().name()));
                            }
                            check_block(r, &bpath, &ix, rs, &block);
                        }
                    },
                }
            }
        }
    }

    // links
    for (i, l) in m.links.iter().enumerate() {
        if !(ix.nodes.contains_key(&l.analytical) || ix.members.contains_key(&l.analytical) || ix.surfaces.contains(&l.analytical)) {
            r.error(Rule::LinkRef, format!("{base}/links/{i}/analytical"), format!("{} is not a node, member or surface of the model", l.analytical));
        }
    }
}

fn check_dofs(r: &mut Report, path: &str, dofs: &Option<[w::Dof; 6]>, named: &Option<String>, release: bool) {
    match (dofs, named) {
        (None, None) => r.error(Rule::Dofs, path.to_string(), "give dofs or a named type"),
        (_, Some(name)) => {
            let known = if release { hinge::is_known(name) } else { support::is_known(name) };
            if !known {
                r.warn(Rule::Dofs, format!("{path}/named"), format!("\"{name}\" is not in the preset table; read as custom"));
            }
        }
        _ => {}
    }
    if let Some(d) = dofs {
        for (k, d) in d.iter().enumerate() {
            let k_value = match d {
                w::Dof::Spring(k) | w::Dof::TensionOnly(k) | w::Dof::CompressionOnly(k) => Some(*k),
                w::Dof::Failure { k, .. } | w::Dof::Slippage { k, .. } => Some(*k),
                _ => None,
            };
            if k_value.is_some_and(|k| k < 0.0) {
                r.warn(Rule::Dofs, format!("{path}/dofs/{k}"), "negative spring stiffness");
            }
        }
    }
}

fn check_block(r: &mut Report, path: &str, ix: &Index, rs: &w::ResultSet, block: &ResultBlock) {
    if block.set != rs.id && block.set != normalise(IdKind::ResultSet, rs.id) {
        r.error(Rule::ResultBlock, path.to_string(), format!("block is for set {}, not {}", block.set, rs.id));
    }
    if !rs.tables.iter().any(|t| t.kind() == block.kind) {
        r.error(Rule::ResultBlock, path.to_string(), format!("{} is not listed in tables", block.kind.name()));
    }
    for v in block.values() {
        if v.iter().any(|x| !x.is_finite()) {
            r.error(Rule::Finite, path.to_string(), "non-finite value in result block");
            break;
        }
    }
    let member_kind = block.kind.is_member();
    for (row, id) in block.ids().iter().enumerate() {
        let ok = if member_kind { ix.members.contains_key(id) } else { ix.nodes.contains_key(id) };
        if !ok {
            r.error(Rule::ResultBlock, format!("{path}/rows/{row}"), format!("{} {id} does not exist", if member_kind { "member" } else { "node" }));
        }
    }
    if let Some((offsets, x)) = block.stations() {
        for (row, id) in block.ids().iter().enumerate() {
            let s = &x[offsets[row] as usize..offsets[row + 1] as usize];
            let len = ix.member_length(*id);
            let tol = 1e-6;
            let mut bad = None;
            if s.is_empty() {
                bad = Some("no stations".to_string());
            }
            for k in 1..s.len() {
                if s[k] < s[k - 1] {
                    bad = Some(format!("station {} below the previous {}", s[k], s[k - 1]));
                } else if s[k] == s[k - 1] && k >= 2 && s[k - 2] == s[k] {
                    bad = Some(format!("station {} appears more than twice", s[k]));
                }
            }
            if let (Some(len), Some(first), Some(last)) = (len, s.first(), s.last()) {
                if *first < -tol || *last > len + tol {
                    bad = Some(format!("stations {first}..{last} outside 0..{len}"));
                }
            }
            if let Some(msg) = bad {
                r.error(Rule::Stations, format!("{path}/rows/{row}"), format!("member {id}: {msg}"));
            }
        }
    }
}

/// Validates a sidecar block against the document (`name` labels the messages).
pub fn validate_block(doc: &w::Document, name: &str, bytes: &[u8]) -> Report {
    let mut r = Report::default();
    let block = match results::decode(bytes) {
        Ok(b) => b,
        Err(e) => {
            r.error(Rule::ResultBlock, name.to_string(), e);
            return r;
        }
    };
    for m in &doc.models {
        if let Some(rs) = m.results.iter().find(|rs| rs.id == block.set || normalise(IdKind::ResultSet, rs.id) == block.set) {
            let ix = Index::new(m);
            check_block(&mut r, name, &ix, rs, &block);
            return r;
        }
    }
    r.error(Rule::ResultBlock, name.to_string(), format!("no result set {} in the document", block.set));
    r
}
