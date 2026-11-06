//! Conversion between the `sam-json` serde types and the document.

use glam::{DAffine2, DMat3, DMat4, DVec2, DVec3};

use super::wire as w;
use crate::base64;
use crate::hinge;
use crate::id::{normalise, Id, IdKind};
use crate::model::*;
use crate::results::{self, ResultPayload};
use crate::section;
use crate::support;

fn v2(a: w::V2) -> DVec2 {
    DVec2::from_array(a)
}
fn v3(a: w::V3) -> DVec3 {
    DVec3::from_array(a)
}
fn aff(a: [f64; 6]) -> DAffine2 {
    DAffine2::from_cols_array(&a)
}

/// The `source.format` of models read from `sam-json`.
pub const FORMAT: &str = "sam-json/1";

fn dof_in(d: &w::Dof) -> Dof {
    match d {
        w::Dof::Fixed => Dof::Fixed,
        w::Dof::Free => Dof::Free,
        w::Dof::Spring(k) => Dof::Spring(*k),
        w::Dof::TensionOnly(k) => Dof::Nonlinear { kind: NonlinearKind::TensionOnly, base: *k },
        w::Dof::CompressionOnly(k) => Dof::Nonlinear { kind: NonlinearKind::CompressionOnly, base: *k },
        w::Dof::Failure { k, limit } => Dof::Nonlinear { kind: NonlinearKind::Failure { limit: *limit }, base: *k },
        w::Dof::Slippage { k, gap } => Dof::Nonlinear { kind: NonlinearKind::Slippage { gap: *gap }, base: *k },
        w::Dof::Diagram(pts) => Dof::Nonlinear { kind: NonlinearKind::Diagram(pts.iter().map(|p| (p[0], p[1])).collect()), base: 0.0 },
    }
}

fn dof_out(d: &Dof) -> w::Dof {
    match d {
        Dof::Fixed => w::Dof::Fixed,
        Dof::Free => w::Dof::Free,
        Dof::Spring(k) => w::Dof::Spring(*k),
        Dof::Nonlinear { kind, base } => match kind {
            NonlinearKind::TensionOnly => w::Dof::TensionOnly(*base),
            NonlinearKind::CompressionOnly => w::Dof::CompressionOnly(*base),
            NonlinearKind::Failure { limit } => w::Dof::Failure { k: *base, limit: *limit },
            NonlinearKind::Slippage { gap } => w::Dof::Slippage { k: *base, gap: *gap },
            NonlinearKind::Diagram(pts) => w::Dof::Diagram(pts.iter().map(|&(a, b)| [a, b]).collect()),
        },
    }
}

fn dofs_in(d: &[w::Dof; 6]) -> Dofs {
    std::array::from_fn(|i| dof_in(&d[i]))
}

fn dofs_out(d: &Dofs) -> [w::Dof; 6] {
    std::array::from_fn(|i| dof_out(&d[i]))
}

fn frame_in(f: w::DofFrame) -> DofFrame {
    match f {
        w::DofFrame::Global => DofFrame::Global,
        w::DofFrame::Node => DofFrame::NodeLocal,
        w::DofFrame::Member(m) => DofFrame::MemberLocal(m),
    }
}

fn frame_out(f: DofFrame) -> w::DofFrame {
    match f {
        DofFrame::Global => w::DofFrame::Global,
        DofFrame::NodeLocal => w::DofFrame::Node,
        DofFrame::MemberLocal(m) => w::DofFrame::Member(m),
    }
}

/// Support DOFs from `dofs` or a named preset; the diagnostic when a name was expanded.
fn support_dofs(dofs: &Option<[w::Dof; 6]>, named: &Option<String>, object: Id, diags: &mut Vec<Diagnostic>) -> (Dofs, bool) {
    if let Some(d) = dofs {
        let unknown = named.as_deref().is_some_and(|n| !support::is_known(n));
        return (dofs_in(d), unknown);
    }
    let name = named.as_deref().unwrap_or("");
    match support::expand(name) {
        Some(d) => {
            diags.push(Diagnostic {
                object: Some(object),
                code: "named-support-expanded".into(),
                message: format!("support type \"{name}\" expanded to {}", support::code(&d)),
            });
            (d, false)
        }
        None => {
            diags.push(Diagnostic {
                object: Some(object),
                code: "unknown-support-name".into(),
                message: format!("support type \"{name}\" is not in the preset table; all DOFs left free"),
            });
            (dofs_all(Dof::Free), true)
        }
    }
}

fn release_in(r: &w::Release, member: Id, diags: &mut Vec<Diagnostic>) -> Hinge {
    let dofs = match (&r.dofs, r.named.as_deref()) {
        (Some(d), _) => dofs_in(d),
        (None, name) => {
            let name = name.unwrap_or("");
            match hinge::expand(name) {
                Some(d) => {
                    diags.push(Diagnostic {
                        object: Some(member),
                        code: "named-release-expanded".into(),
                        message: format!("release type \"{name}\" expanded to {}", support::code(&d)),
                    });
                    d
                }
                None => {
                    diags.push(Diagnostic {
                        object: Some(member),
                        code: "unknown-release-name".into(),
                        message: format!("release type \"{name}\" is not in the preset table; the end stays connected"),
                    });
                    dofs_all(Dof::Fixed)
                }
            }
        }
    };
    Hinge { dofs, named: r.named.clone() }
}

pub(super) fn shape_in(s: &w::Shape) -> Shape {
    match s {
        &w::Shape::I { h, b, tw, tf, r, slope } => Shape::I { h, b, tw, tf, r, slope },
        &w::Shape::AsymI { h, bt, bb, tw, tft, tfb, r } => Shape::AsymI { h, bt, bb, tw, tft, tfb, r },
        &w::Shape::U { h, b, tw, tf, r1, r2, slope } => Shape::U { h, b, tw, tf, r1, r2, slope },
        &w::Shape::L { h, b, t, r1, r2 } => Shape::L { h, b, t, r1, r2 },
        &w::Shape::T { h, b, tw, tf, r } => Shape::T { h, b, tw, tf, r },
        &w::Shape::C { h, b, t, lip, r } => Shape::C { h, b, t, lip, r },
        &w::Shape::Z { h, b, tw, tf, r } => Shape::Z { h, b, tw, tf, r },
        &w::Shape::Rect { b, h } => Shape::Rect { b, h },
        &w::Shape::Circle { d } => Shape::Circle { d },
        &w::Shape::Chs { d, t } => Shape::Chs { d, t },
        &w::Shape::Rhs { b, h, t, r_out, r_in } => Shape::Rhs { b, h, t, r_out, r_in },
        w::Shape::Polygon { outer, holes } => Shape::Polygon {
            outer: outer.iter().copied().map(v2).collect(),
            holes: holes.iter().map(|h| h.iter().copied().map(v2).collect()).collect(),
        },
        w::Shape::Composite(parts) => Shape::Composite(parts.iter().map(|p| (shape_in(&p.shape), aff(p.transform))).collect()),
        w::Shape::Unknown => Shape::Unknown,
    }
}

fn shape_out(s: &Shape) -> w::Shape {
    match s {
        &Shape::I { h, b, tw, tf, r, slope } => w::Shape::I { h, b, tw, tf, r, slope },
        &Shape::AsymI { h, bt, bb, tw, tft, tfb, r } => w::Shape::AsymI { h, bt, bb, tw, tft, tfb, r },
        &Shape::U { h, b, tw, tf, r1, r2, slope } => w::Shape::U { h, b, tw, tf, r1, r2, slope },
        &Shape::L { h, b, t, r1, r2 } => w::Shape::L { h, b, t, r1, r2 },
        &Shape::T { h, b, tw, tf, r } => w::Shape::T { h, b, tw, tf, r },
        &Shape::C { h, b, t, lip, r } => w::Shape::C { h, b, t, lip, r },
        &Shape::Z { h, b, tw, tf, r } => w::Shape::Z { h, b, tw, tf, r },
        &Shape::Rect { b, h } => w::Shape::Rect { b, h },
        &Shape::Circle { d } => w::Shape::Circle { d },
        &Shape::Chs { d, t } => w::Shape::Chs { d, t },
        &Shape::Rhs { b, h, t, r_out, r_in } => w::Shape::Rhs { b, h, t, r_out, r_in },
        Shape::Polygon { outer, holes } => w::Shape::Polygon {
            outer: outer.iter().map(|p| p.to_array()).collect(),
            holes: holes.iter().map(|h| h.iter().map(|p| p.to_array()).collect()).collect(),
        },
        Shape::Composite(parts) => w::Shape::Composite(parts.iter().map(|(s, t)| w::CompositePart { shape: shape_out(s), transform: t.to_cols_array() }).collect()),
        Shape::Unknown => w::Shape::Unknown,
    }
}

fn load_ref_in(r: w::LoadRef) -> LoadRef {
    match r {
        w::LoadRef::Case(id) => LoadRef::Case(normalise(IdKind::LoadCase, id)),
        w::LoadRef::Combination(id) => LoadRef::Combination(normalise(IdKind::Combination, id)),
    }
}

fn load_ref_out(r: LoadRef) -> w::LoadRef {
    match r {
        LoadRef::Case(id) => w::LoadRef::Case(id),
        LoadRef::Combination(id) => w::LoadRef::Combination(id),
    }
}

fn load_in(l: &w::Load) -> Load {
    match l {
        w::Load::NodalForce { f, m } => Load::NodalForce { f: v3(*f), m: v3(*m) },
        w::Load::MemberPoint { x, f, m } => Load::MemberPoint { x: *x, f: v3(*f), m: v3(*m) },
        w::Load::MemberDistributed { dir, moment, dist } => Load::MemberDistributed {
            dir: v3(*dir),
            moment: *moment,
            dist: match dist {
                w::Distribution::Uniform(q) => Distribution::Uniform(*q),
                &w::Distribution::Linear { q1, q2 } => Distribution::Linear { q1, q2 },
                &w::Distribution::Trapezoidal { a, b, q1, q2 } => Distribution::Trapezoidal { a, b, q1, q2 },
                w::Distribution::Polygonal(p) => Distribution::Polygonal(p.iter().map(|p| (p[0], p[1])).collect()),
                w::Distribution::Parabolic(peak) => Distribution::Parabolic { peak: *peak },
                w::Distribution::Sine(peak) => Distribution::Sine { peak: *peak },
            },
        },
        w::Load::Surface { dir, dist } => Load::Surface {
            dir: v3(*dir),
            dist: match dist {
                w::SurfaceDistribution::Uniform(p) => SurfaceDistribution::Uniform(*p),
                w::SurfaceDistribution::Bilinear(p) => SurfaceDistribution::Bilinear(*p),
                w::SurfaceDistribution::Sampled(s) => SurfaceDistribution::Sampled(s.iter().map(|s| (DVec2::new(s[0], s[1]), s[2])).collect()),
            },
        },
        &w::Load::Temperature { dt, dt_y, dt_z } => Load::Temperature { dt, dt_y, dt_z },
        w::Load::Displacement { u } => Load::Displacement { u: *u },
    }
}

fn load_out(l: &Load) -> w::Load {
    match l {
        Load::NodalForce { f, m } => w::Load::NodalForce { f: f.to_array(), m: m.to_array() },
        Load::MemberPoint { x, f, m } => w::Load::MemberPoint { x: *x, f: f.to_array(), m: m.to_array() },
        Load::MemberDistributed { dir, moment, dist } => w::Load::MemberDistributed {
            dir: dir.to_array(),
            moment: *moment,
            dist: match dist {
                Distribution::Uniform(q) => w::Distribution::Uniform(*q),
                &Distribution::Linear { q1, q2 } => w::Distribution::Linear { q1, q2 },
                &Distribution::Trapezoidal { a, b, q1, q2 } => w::Distribution::Trapezoidal { a, b, q1, q2 },
                Distribution::Polygonal(p) => w::Distribution::Polygonal(p.iter().map(|&(x, q)| [x, q]).collect()),
                Distribution::Parabolic { peak } => w::Distribution::Parabolic(*peak),
                Distribution::Sine { peak } => w::Distribution::Sine(*peak),
            },
        },
        Load::Surface { dir, dist } => w::Load::Surface {
            dir: dir.to_array(),
            dist: match dist {
                SurfaceDistribution::Uniform(p) => w::SurfaceDistribution::Uniform(*p),
                SurfaceDistribution::Bilinear(p) => w::SurfaceDistribution::Bilinear(*p),
                SurfaceDistribution::Sampled(s) => w::SurfaceDistribution::Sampled(s.iter().map(|(uv, p)| [uv.x, uv.y, *p]).collect()),
            },
        },
        &Load::Temperature { dt, dt_y, dt_z } => w::Load::Temperature { dt, dt_y, dt_z },
        Load::Displacement { u } => w::Load::Displacement { u: *u },
    }
}

fn imported_in(p: &w::Props) -> ImportedProps {
    ImportedProps {
        area: p.a,
        centroid: p.centroid.map(v2),
        shear_centre: p.shear_centre.map(v2),
        iy: p.iy,
        iz: p.iz,
        iyz: p.iyz,
        it: p.it,
    }
}

fn imported_out(p: &ImportedProps) -> w::Props {
    w::Props {
        a: p.area,
        centroid: p.centroid.map(|c| c.to_array()),
        shear_centre: p.shear_centre.map(|c| c.to_array()),
        iy: p.iy,
        iz: p.iz,
        iyz: p.iyz,
        it: p.it,
    }
}

fn source_in(s: &w::Source) -> SourceInfo {
    SourceInfo {
        format: FORMAT.into(),
        file: s.file.clone().unwrap_or_default(),
        application: s.application.clone(),
        global_transform: DMat4::from_cols_array(&s.global_transform),
        conventions: Conventions {
            gravity_was: match s.conventions.gravity_was {
                w::Gravity::NegZ => AxisSign::NegZ,
                w::Gravity::PosZ => AxisSign::PosZ,
            },
            mz_sign_was: match s.conventions.mz_sign_was {
                w::MzSign::TensionMinusY => MzSign::TensionMinusY,
                w::MzSign::TensionPlusY => MzSign::TensionPlusY,
            },
            reactions_were: match s.conventions.reactions_were {
                w::ReactionSense::OnStructure => ReactionSense::OnStructure,
                w::ReactionSense::OnSupport => ReactionSense::OnSupport,
            },
            result_signs: match s.conventions.result_signs {
                w::SignStatus::Verified => SignStatus::Verified,
                w::SignStatus::AssumedCanonical => SignStatus::AssumedCanonical,
            },
            local_z_rule: s.conventions.local_z_rule.clone(),
        },
        eccentricity_policy: match s.eccentricity_policy {
            w::EccentricityPolicy::CardinalCancelsPosition => EccentricityPolicy::CardinalCancelsPosition,
            w::EccentricityPolicy::Additive => EccentricityPolicy::Additive,
            w::EccentricityPolicy::Explicit => EccentricityPolicy::Explicit,
        },
    }
}

fn source_out(s: &SourceInfo) -> w::Source {
    w::Source {
        application: s.application.clone(),
        file: (!s.file.is_empty()).then(|| s.file.clone()),
        global_transform: s.global_transform.to_cols_array(),
        conventions: w::Conventions {
            gravity_was: match s.conventions.gravity_was {
                AxisSign::NegZ => w::Gravity::NegZ,
                AxisSign::PosZ => w::Gravity::PosZ,
            },
            mz_sign_was: match s.conventions.mz_sign_was {
                MzSign::TensionMinusY => w::MzSign::TensionMinusY,
                MzSign::TensionPlusY => w::MzSign::TensionPlusY,
            },
            reactions_were: match s.conventions.reactions_were {
                ReactionSense::OnStructure => w::ReactionSense::OnStructure,
                ReactionSense::OnSupport => w::ReactionSense::OnSupport,
            },
            result_signs: match s.conventions.result_signs {
                SignStatus::Verified => w::SignStatus::Verified,
                SignStatus::AssumedCanonical => w::SignStatus::AssumedCanonical,
            },
            local_z_rule: s.conventions.local_z_rule.clone(),
        },
        eccentricity_policy: match s.eccentricity_policy {
            EccentricityPolicy::CardinalCancelsPosition => w::EccentricityPolicy::CardinalCancelsPosition,
            EccentricityPolicy::Additive => w::EccentricityPolicy::Additive,
            EccentricityPolicy::Explicit => w::EccentricityPolicy::Explicit,
        },
    }
}

fn theory_in(t: w::Theory) -> Theory {
    match t {
        w::Theory::FirstOrder => Theory::First,
        w::Theory::SecondOrder => Theory::Second,
        w::Theory::ThirdOrder => Theory::Third,
        w::Theory::FullNonlinear => Theory::FullNonlinear,
    }
}

fn theory_out(t: Theory) -> w::Theory {
    match t {
        Theory::First => w::Theory::FirstOrder,
        Theory::Second => w::Theory::SecondOrder,
        Theory::Third => w::Theory::ThirdOrder,
        Theory::FullNonlinear => w::Theory::FullNonlinear,
    }
}

/// Converts a validated wire document. Errors only on undecodable inline blocks.
pub fn document_in(doc: &w::Document) -> Result<SamDocument, String> {
    let source = source_in(&doc.source);
    let models = doc.models.iter().map(|m| model_in(m, &source)).collect::<Result<_, _>>()?;
    Ok(SamDocument { origin: v3(doc.origin), source, models })
}

fn model_in(m: &w::Model, source: &SourceInfo) -> Result<AnalysisModel, String> {
    let mut out = AnalysisModel::new(m.id, m.name.clone(), source.clone());
    out.dimension = match m.dimension {
        w::Dimension::Space3d => Dimension::Space3d,
        w::Dimension::Plane2d { normal } => Dimension::Plane2d { normal: v3(normal) },
    };
    out.kind = match m.kind {
        w::ModelKind::Analytical => ModelKind::Analytical,
        w::ModelKind::PhysicalAxes => ModelKind::PhysicalAxes,
    };
    out.shared_frame = DMat4::from_cols_array(&m.shared_frame);
    let mut diags = m
        .diagnostics
        .iter()
        .map(|d| Diagnostic { object: d.object, code: d.code.clone(), message: d.message.clone() })
        .collect::<Vec<_>>();
    let mut new_diags = Vec::new();
    // the sections a document this reader wrote already notes (one set, so a file with many
    // diagnostics is not scanned once per section)
    let noted_shear_centre: std::collections::HashSet<Id> =
        diags.iter().filter(|d| d.code == "shear-centre-approximated").filter_map(|d| d.object).collect();

    out.materials = m
        .materials
        .iter()
        .map(|x| Material { id: normalise(IdKind::Material, x.id), name: x.name.clone(), e: x.e, g: x.g, nu: x.nu, density: x.rho, alpha_t: x.alpha_t })
        .collect();
    out.sections = m
        .sections
        .iter()
        .map(|x| {
            let shape = shape_in(&x.shape);
            let imported = x.props.as_ref().map(imported_in);
            let (props, props_source) = section::compute(&shape, imported.as_ref());
            let id = normalise(IdKind::Section, x.id);
            // Once per section: a document this reader wrote already carries the note.
            let noted = noted_shear_centre.contains(&id);
            if props_source == PropsSource::ShearCentreApproximate && !noted {
                new_diags.push(Diagnostic {
                    object: Some(id),
                    code: "shear-centre-approximated".into(),
                    message: format!("section \"{}\": no shear centre for this shape and none imported; the centroid is used", x.name),
                });
            }
            Section {
                id,
                name: x.name.clone(),
                shape,
                props,
                props_source,
                imported,
                material: x.material.map(|id| normalise(IdKind::Material, id)),
            }
        })
        .collect();
    out.nodes = m
        .nodes
        .iter()
        .map(|x| Node {
            id: x.id,
            number: x.no.clone(),
            p: v3(x.p),
            frame: x.frame.map(|f| DMat3::from_cols(v3(f[0]), v3(f[1]), v3(f[2]))),
            support: x.support.as_ref().map(|s| {
                let (dofs, unknown) = support_dofs(&s.dofs, &s.named, x.id, &mut new_diags);
                let preset = if unknown { SupportPreset::Custom } else { support::classify(&dofs, out.dimension) };
                Support { dofs, frame: frame_in(s.frame), named: s.named.clone(), preset }
            }),
            origin: match x.origin {
                w::NodeOrigin::Analytical => NodeOrigin::Analytical,
                w::NodeOrigin::DerivedFromPhysical { element, approximate } => NodeOrigin::DerivedFromPhysical { element, approximate },
            },
        })
        .collect();
    out.members = m
        .members
        .iter()
        .map(|x| Member {
            id: x.id,
            number: x.no.clone(),
            kind: match x.kind {
                w::MemberKind::Beam => MemberKind::Beam,
                w::MemberKind::Truss => MemberKind::Truss,
                w::MemberKind::Cable => MemberKind::Cable,
                w::MemberKind::TensionOnly => MemberKind::TensionOnly,
                w::MemberKind::CompressionOnly => MemberKind::CompressionOnly,
                w::MemberKind::Rigid => MemberKind::Rigid,
                w::MemberKind::Spring => MemberKind::Spring,
            },
            nodes: x.nodes.clone(),
            geometry: match &x.geometry {
                w::Curve::Line => Curve::Line,
                w::Curve::Arc { mid } => Curve::Arc { mid: v3(*mid) },
                w::Curve::Polyline(p) => Curve::Polyline(p.iter().copied().map(v3).collect()),
            },
            z_ref: v3(x.z),
            orientation: match &x.orientation {
                w::Orientation::IfcAxis => Orientation::IfcAxis,
                w::Orientation::Beta { angle, rule } => Orientation::Beta { angle: *angle, rule: rule.clone() },
                w::Orientation::RefNode(n) => Orientation::RefNode(*n),
                w::Orientation::RefVector(v) => Orientation::RefVector(v3(*v)),
                w::Orientation::Default => Orientation::Default,
            },
            section: x.section.map(|(a, b)| (normalise(IdKind::Section, a), b.map(|b| normalise(IdKind::Section, b)))),
            section_rotation: x.section_rotation,
            insertion: Insertion {
                cardinal_point: x.insertion.cardinal_point as u8,
                profile_position: x.insertion.profile_position.map(aff).unwrap_or(DAffine2::IDENTITY),
                profile_offsets: x.insertion.profile_offsets.map(|[a, b]| [v2(a), v2(b)]),
                eccentricity: v2(x.insertion.eccentricity),
                end_offsets: [v3(x.insertion.end_offsets.start), v3(x.insertion.end_offsets.end)],
                end_offset_frame: match x.insertion.end_offsets.frame {
                    w::OffsetFrame::Local => OffsetFrame::MemberLocal,
                    w::OffsetFrame::Global => OffsetFrame::Global,
                },
            },
            releases: [0, 1].map(|i| x.releases[i].as_ref().map(|r| release_in(r, x.id, &mut new_diags))),
            material: x.material.map(|id| normalise(IdKind::Material, id)),
            links: x.links.clone(),
        })
        .collect();
    out.surfaces = m
        .surfaces
        .iter()
        .map(|x| SurfaceMember {
            id: x.id,
            number: x.no.clone(),
            kind: match x.kind {
                w::SurfaceKind::Plate => SurfaceKind::Plate,
                w::SurfaceKind::Membrane => SurfaceKind::Membrane,
                w::SurfaceKind::Shell => SurfaceKind::Shell,
                w::SurfaceKind::Rigid => SurfaceKind::Rigid,
                w::SurfaceKind::LoadTransfer => SurfaceKind::LoadTransfer,
            },
            boundary: x.boundary.clone(),
            openings: x.openings.clone(),
            thickness: match &x.thickness {
                w::Thickness::Uniform(t) => Thickness::Uniform(*t),
                w::Thickness::Linear(p) => Thickness::Linear(p.map(|q| (DVec3::new(q[0], q[1], q[2]), q[3]))),
                w::Thickness::Unknown => Thickness::Unknown,
            },
            offset_z: x.offset_z,
            x_dir: x.x_dir.map(v3),
            material: x.material.map(|id| normalise(IdKind::Material, id)),
            links: x.links.clone(),
        })
        .collect();
    out.line_supports = m
        .line_supports
        .iter()
        .map(|x| {
            let (dofs, _) = support_dofs(&x.dofs, &x.named, x.id, &mut new_diags);
            LineSupport {
                id: x.id,
                number: x.no.clone(),
                along: match &x.along {
                    w::Carrier::Member(m) => SupportCarrier::Member(*m),
                    &w::Carrier::SurfaceEdge { surface, from, to } => SupportCarrier::SurfaceEdge { surface, from, to },
                    w::Carrier::Polyline(p) => SupportCarrier::Polyline(p.iter().copied().map(v3).collect()),
                },
                dofs,
                frame: frame_in(x.frame),
                named: x.named.clone(),
            }
        })
        .collect();
    out.load_cases = m
        .load_cases
        .iter()
        .map(|x| LoadCase {
            id: normalise(IdKind::LoadCase, x.id),
            number: x.no.clone(),
            name: x.name.clone(),
            action_type: match &x.action_type {
                w::ActionType::Permanent => ActionType::Permanent,
                w::ActionType::Variable => ActionType::Variable,
                w::ActionType::Accidental => ActionType::Accidental,
                w::ActionType::User(s) => ActionType::User(s.clone()),
            },
            action_source: x.action_source.clone(),
            self_weight: x.self_weight.map(v3),
            coefficient: x.coefficient,
        })
        .collect();
    out.combinations = m
        .combinations
        .iter()
        .map(|x| Combination {
            id: normalise(IdKind::Combination, x.id),
            number: x.no.clone(),
            name: x.name.clone(),
            purpose: match &x.purpose {
                w::Purpose::Uls => Purpose::Uls,
                w::Purpose::Sls => Purpose::Sls,
                w::Purpose::Als => Purpose::Als,
                w::Purpose::User(s) => Purpose::User(s.clone()),
            },
            terms: x.terms.iter().map(|(r, f)| (load_ref_in(*r), *f)).collect(),
        })
        .collect();
    out.actions = m
        .actions
        .iter()
        .map(|x| Action {
            id: normalise(IdKind::Action, x.id),
            load: load_ref_in(x.load),
            target: match &x.target {
                w::Target::Node(n) => Target::Node(*n),
                w::Target::Member(id) => Target::Member { id: *id, span: None },
                w::Target::MemberSpan { member, span } => Target::Member { id: *member, span: Some((span[0], span[1])) },
                w::Target::Surface(s) => Target::Surface(*s),
                w::Target::Free(p) => Target::Free(p.iter().copied().map(v3).collect()),
            },
            frame: match x.frame {
                w::LoadFrame::Global => LoadFrame::Global,
                w::LoadFrame::Local => LoadFrame::Local,
                w::LoadFrame::GlobalProjected => LoadFrame::GlobalProjected,
            },
            kind: load_in(&x.kind),
            destabilising: x.destabilising,
        })
        .collect();
    out.actions.sort_by_key(|a| a.load);
    out.results = m
        .results
        .iter()
        .map(|x| -> Result<ResultSet, String> {
            let id = normalise(IdKind::ResultSet, x.id);
            let inline = match &x.inline {
                None => None,
                Some(inl) => {
                    let mut payload = ResultPayload::default();
                    for (table, b64) in inl.entries() {
                        let bytes = base64::decode(b64).map_err(|e| format!("result set {}: {}: {e}", x.id, table.kind().name()))?;
                        let mut block = results::decode(&bytes).map_err(|e| format!("result set {}: {}: {e}", x.id, table.kind().name()))?;
                        block.set = id;
                        payload.insert(block);
                    }
                    Some(payload)
                }
            };
            Ok(ResultSet {
                id,
                number: x.no.clone(),
                name: x.name.clone(),
                for_: match &x.for_ {
                    w::ResultFor::Case(c) => ResultFor::Case(normalise(IdKind::LoadCase, *c)),
                    w::ResultFor::Combination(c) => ResultFor::Combination(normalise(IdKind::Combination, *c)),
                    w::ResultFor::Envelope(s) => ResultFor::Envelope(s.clone()),
                },
                theory: theory_in(x.theory),
                linear: x.linear,
                available: x.tables.iter().map(|t| t.kind()).collect(),
                inline,
            })
        })
        .collect::<Result<_, _>>()?;
    out.links = m
        .links
        .iter()
        .map(|l| PhysicalLink {
            analytical: l.analytical,
            physical: l.physical,
            source: match &l.source {
                w::LinkSource::IfcRelAssignsToProduct => LinkSource::IfcRelAssignsToProduct,
                w::LinkSource::RevitAssociation => LinkSource::RevitAssociation,
                w::LinkSource::Matched { distance } => LinkSource::Matched { distance: *distance },
            },
        })
        .collect();
    diags.extend(new_diags);
    out.diagnostics = diags;
    out.rebuild_index();
    Ok(out)
}

/// Converts a document to its wire form. Supports and releases always carry their DOFs (and
/// their source name when they have one); ids are written in the shifted form.
pub fn document_out(doc: &SamDocument) -> w::Document {
    w::Document {
        format: "sam-json".into(),
        version: 1,
        source: source_out(&doc.source),
        units: "SI".into(),
        origin: doc.origin.to_array(),
        models: doc.models.iter().map(model_out).collect(),
    }
}

fn model_out(m: &AnalysisModel) -> w::Model {
    w::Model {
        id: m.id,
        name: m.name.clone(),
        dimension: match m.dimension {
            Dimension::Space3d => w::Dimension::Space3d,
            Dimension::Plane2d { normal } => w::Dimension::Plane2d { normal: normal.to_array() },
        },
        kind: match m.kind {
            ModelKind::Analytical => w::ModelKind::Analytical,
            ModelKind::PhysicalAxes => w::ModelKind::PhysicalAxes,
        },
        shared_frame: m.shared_frame.to_cols_array(),
        materials: m
            .materials
            .iter()
            .map(|x| w::Material { id: x.id, name: x.name.clone(), e: x.e, g: x.g, nu: x.nu, rho: x.density, alpha_t: x.alpha_t })
            .collect(),
        sections: m
            .sections
            .iter()
            .map(|x| w::Section { id: x.id, name: x.name.clone(), material: x.material, shape: shape_out(&x.shape), props: x.imported.as_ref().map(imported_out) })
            .collect(),
        nodes: m
            .nodes
            .iter()
            .map(|x| w::Node {
                id: x.id,
                no: x.number.clone(),
                p: x.p.to_array(),
                frame: x.frame.map(|f| [f.x_axis.to_array(), f.y_axis.to_array(), f.z_axis.to_array()]),
                support: x.support.as_ref().map(|s| w::Support { dofs: Some(dofs_out(&s.dofs)), named: s.named.clone(), frame: frame_out(s.frame) }),
                origin: match x.origin {
                    NodeOrigin::Analytical => w::NodeOrigin::Analytical,
                    NodeOrigin::DerivedFromPhysical { element, approximate } => w::NodeOrigin::DerivedFromPhysical { element, approximate },
                },
            })
            .collect(),
        members: m
            .members
            .iter()
            .map(|x| w::Member {
                id: x.id,
                no: x.number.clone(),
                kind: match x.kind {
                    MemberKind::Beam => w::MemberKind::Beam,
                    MemberKind::Truss => w::MemberKind::Truss,
                    MemberKind::Cable => w::MemberKind::Cable,
                    MemberKind::TensionOnly => w::MemberKind::TensionOnly,
                    MemberKind::CompressionOnly => w::MemberKind::CompressionOnly,
                    MemberKind::Rigid => w::MemberKind::Rigid,
                    MemberKind::Spring => w::MemberKind::Spring,
                },
                nodes: x.nodes.clone(),
                geometry: match &x.geometry {
                    Curve::Line => w::Curve::Line,
                    Curve::Arc { mid } => w::Curve::Arc { mid: mid.to_array() },
                    Curve::Polyline(p) => w::Curve::Polyline(p.iter().map(|p| p.to_array()).collect()),
                },
                z: x.z_ref.to_array(),
                orientation: match &x.orientation {
                    Orientation::IfcAxis => w::Orientation::IfcAxis,
                    Orientation::Beta { angle, rule } => w::Orientation::Beta { angle: *angle, rule: rule.clone() },
                    Orientation::RefNode(n) => w::Orientation::RefNode(*n),
                    Orientation::RefVector(v) => w::Orientation::RefVector(v.to_array()),
                    Orientation::Default => w::Orientation::Default,
                },
                section: x.section,
                section_rotation: x.section_rotation,
                insertion: w::Insertion {
                    cardinal_point: x.insertion.cardinal_point as u32,
                    profile_position: (x.insertion.profile_position != DAffine2::IDENTITY).then(|| x.insertion.profile_position.to_cols_array()),
                    profile_offsets: x.insertion.profile_offsets.map(|[a, b]| [a.to_array(), b.to_array()]),
                    eccentricity: x.insertion.eccentricity.to_array(),
                    end_offsets: w::EndOffsets {
                        start: x.insertion.end_offsets[0].to_array(),
                        end: x.insertion.end_offsets[1].to_array(),
                        frame: match x.insertion.end_offset_frame {
                            OffsetFrame::MemberLocal => w::OffsetFrame::Local,
                            OffsetFrame::Global => w::OffsetFrame::Global,
                        },
                    },
                },
                releases: [0, 1].map(|i| x.releases[i].as_ref().map(|h| w::Release { dofs: Some(dofs_out(&h.dofs)), named: h.named.clone() })),
                material: x.material,
                links: x.links.clone(),
            })
            .collect(),
        surfaces: m
            .surfaces
            .iter()
            .map(|x| w::Surface {
                id: x.id,
                no: x.number.clone(),
                kind: match x.kind {
                    SurfaceKind::Plate => w::SurfaceKind::Plate,
                    SurfaceKind::Membrane => w::SurfaceKind::Membrane,
                    SurfaceKind::Shell => w::SurfaceKind::Shell,
                    SurfaceKind::Rigid => w::SurfaceKind::Rigid,
                    SurfaceKind::LoadTransfer => w::SurfaceKind::LoadTransfer,
                },
                boundary: x.boundary.clone(),
                openings: x.openings.clone(),
                thickness: match &x.thickness {
                    Thickness::Uniform(t) => w::Thickness::Uniform(*t),
                    Thickness::Linear(p) => w::Thickness::Linear(p.map(|(q, t)| [q.x, q.y, q.z, t])),
                    Thickness::Unknown => w::Thickness::Unknown,
                },
                offset_z: x.offset_z,
                x_dir: x.x_dir.map(|v| v.to_array()),
                material: x.material,
                links: x.links.clone(),
            })
            .collect(),
        line_supports: m
            .line_supports
            .iter()
            .map(|x| w::LineSupport {
                id: x.id,
                no: x.number.clone(),
                along: match &x.along {
                    SupportCarrier::Member(id) => w::Carrier::Member(*id),
                    &SupportCarrier::SurfaceEdge { surface, from, to } => w::Carrier::SurfaceEdge { surface, from, to },
                    SupportCarrier::Polyline(p) => w::Carrier::Polyline(p.iter().map(|p| p.to_array()).collect()),
                },
                dofs: Some(dofs_out(&x.dofs)),
                named: x.named.clone(),
                frame: frame_out(x.frame),
            })
            .collect(),
        load_cases: m
            .load_cases
            .iter()
            .map(|x| w::LoadCase {
                id: x.id,
                no: x.number.clone(),
                name: x.name.clone(),
                action_type: match &x.action_type {
                    ActionType::Permanent => w::ActionType::Permanent,
                    ActionType::Variable => w::ActionType::Variable,
                    ActionType::Accidental => w::ActionType::Accidental,
                    ActionType::User(s) => w::ActionType::User(s.clone()),
                },
                action_source: x.action_source.clone(),
                self_weight: x.self_weight.map(|v| v.to_array()),
                coefficient: x.coefficient,
            })
            .collect(),
        combinations: m
            .combinations
            .iter()
            .map(|x| w::Combination {
                id: x.id,
                no: x.number.clone(),
                name: x.name.clone(),
                purpose: match &x.purpose {
                    Purpose::Uls => w::Purpose::Uls,
                    Purpose::Sls => w::Purpose::Sls,
                    Purpose::Als => w::Purpose::Als,
                    Purpose::User(s) => w::Purpose::User(s.clone()),
                },
                terms: x.terms.iter().map(|(r, f)| (load_ref_out(*r), *f)).collect(),
            })
            .collect(),
        actions: m
            .actions
            .iter()
            .map(|x| w::Action {
                id: x.id,
                load: load_ref_out(x.load),
                target: match &x.target {
                    Target::Node(n) => w::Target::Node(*n),
                    Target::Member { id, span: None } => w::Target::Member(*id),
                    Target::Member { id, span: Some((a, b)) } => w::Target::MemberSpan { member: *id, span: [*a, *b] },
                    Target::Surface(s) => w::Target::Surface(*s),
                    Target::Free(p) => w::Target::Free(p.iter().map(|p| p.to_array()).collect()),
                },
                frame: match x.frame {
                    LoadFrame::Global => w::LoadFrame::Global,
                    LoadFrame::Local => w::LoadFrame::Local,
                    LoadFrame::GlobalProjected => w::LoadFrame::GlobalProjected,
                },
                kind: load_out(&x.kind),
                destabilising: x.destabilising,
            })
            .collect(),
        results: m
            .results
            .iter()
            .map(|x| w::ResultSet {
                id: x.id,
                no: x.number.clone(),
                name: x.name.clone(),
                for_: match &x.for_ {
                    ResultFor::Case(c) => w::ResultFor::Case(*c),
                    ResultFor::Combination(c) => w::ResultFor::Combination(*c),
                    ResultFor::Envelope(s) => w::ResultFor::Envelope(s.clone()),
                },
                theory: theory_out(x.theory),
                linear: x.linear,
                tables: x.available.iter().map(|k| w::TableName::from_kind(*k)).collect(),
                inline: x.inline.as_ref().map(|p| {
                    let mut inl = w::Inline::default();
                    for block in p.blocks(x.id) {
                        inl.set(w::TableName::from_kind(block.kind), base64::encode(&results::encode(&block)));
                    }
                    inl
                }),
            })
            .collect(),
        links: m
            .links
            .iter()
            .map(|l| w::Link {
                analytical: l.analytical,
                physical: l.physical,
                source: match &l.source {
                    LinkSource::IfcRelAssignsToProduct => w::LinkSource::IfcRelAssignsToProduct,
                    LinkSource::RevitAssociation => w::LinkSource::RevitAssociation,
                    LinkSource::Matched { distance } => w::LinkSource::Matched { distance: *distance },
                },
            })
            .collect(),
        diagnostics: m.diagnostics.iter().map(|d| w::Diagnostic { object: d.object, code: d.code.clone(), message: d.message.clone() }).collect(),
    }
}
