//! ASAM OpenDRIVE road networks (`.xodr`) as 3D road surfaces.
//!
//! The file is read by the `opendrive` crate (OpenDRIVE 1.7). Each road's reference line is
//! followed along its plan view (lines, arcs, spirals, cubic polynomials and parametric cubic
//! curves), raised by its elevation profile and tilted by its superelevation; the lanes of each
//! lane section are laid beside it by their widths (or borders) and the lane offset, and raised
//! by their heights (kerbs, pavements). Each lane is a surface of its own, gathered per lane
//! type, so a driving lane, a footpath and a cycle path each have their colour.
//!
//! Coordinates are the file's (metres, Z up), after its header offset; the network is moved to a
//! local origin near its first point, which is returned with the geo reference of the file.

use opendrive::core::OpenDrive;
use opendrive::lane::lane_choice::LaneChoice;
use opendrive::lane::lane_type::LaneType;
use opendrive::lane::Lane;
use opendrive::road::geometry::geometry_type::GeometryType;
use opendrive::road::geometry::param_poly_3_p_range::ParamPoly3pRange;
use opendrive::road::geometry::Geometry;
use opendrive::road::Road;
use std::collections::BTreeMap;
use uom::si::angle::radian;
use uom::si::curvature::radian_per_meter;
use uom::si::length::meter;

/// A road network.
#[derive(Debug, Clone, Default)]
pub struct Network {
    /// The surfaces, one per lane type that occurs.
    pub surfaces: Vec<Surface>,
    /// The reference line of each road, as a polyline in local coordinates.
    pub reference_lines: Vec<(String, Vec<[f32; 3]>)>,
    /// The point of the file's coordinates that local (0, 0, 0) is (metres).
    pub origin: [f64; 3],
    /// The geo reference of the file (a PROJ string or WKT), if it has one.
    pub geo_reference: Option<String>,
    /// What the header says: name, version, date, vendor, revision.
    pub info: Vec<(String, String)>,
    /// Roads left out, each with why.
    pub skipped: Vec<String>,
}

/// The lanes of one type.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    /// The lane type, as OpenDRIVE names it (`driving`, `sidewalk`, ..).
    pub lane_type: String,
    /// The colour to draw it in (sRGB, 0..1).
    pub color: [f32; 3],
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    /// The lanes: an id, the first index and the index count in `indices`.
    pub elements: Vec<(u64, u32, u32)>,
    /// Per lane: what it is (road, lane id, section, type, junction).
    pub props: Vec<(u64, Vec<(String, String)>)>,
}

/// The colour of a lane type and its name.
pub fn lane_style(lane: &LaneType) -> (&'static str, [f32; 3]) {
    match lane {
        LaneType::Driving | LaneType::Bidirectional => ("driving", [0.27, 0.27, 0.29]),
        LaneType::Exit | LaneType::Entry | LaneType::OnRamp | LaneType::OffRamp | LaneType::ConnectingRamp => ("ramp", [0.31, 0.31, 0.33]),
        LaneType::Stop => ("stop", [0.42, 0.36, 0.30]),
        LaneType::Shoulder => ("shoulder", [0.52, 0.50, 0.44]),
        LaneType::Border => ("border", [0.60, 0.58, 0.52]),
        LaneType::Restricted => ("restricted", [0.62, 0.52, 0.36]),
        LaneType::Parking => ("parking", [0.38, 0.40, 0.48]),
        LaneType::Median => ("median", [0.36, 0.52, 0.30]),
        LaneType::Biking => ("biking", [0.66, 0.30, 0.26]),
        LaneType::Sidewalk => ("sidewalk", [0.76, 0.75, 0.71]),
        LaneType::Curb => ("curb", [0.86, 0.85, 0.82]),
        LaneType::Tram | LaneType::Rail => ("rail", [0.47, 0.38, 0.30]),
        LaneType::Bus | LaneType::Taxi | LaneType::HOV => ("bus", [0.55, 0.27, 0.30]),
        LaneType::RoadWorks => ("roadWorks", [0.85, 0.55, 0.15]),
        LaneType::Special1 | LaneType::Special2 | LaneType::Special3 => ("special", [0.50, 0.45, 0.55]),
        LaneType::None => ("none", [0.66, 0.66, 0.62]),
    }
}

/// Reads an OpenDRIVE file's text. The road objects and signals are not drawn, so they are
/// taken out before the file is parsed: a value the parser does not know there (files in use
/// hold many) then does not stop the roads from opening.
pub fn read(text: &str) -> Result<Network, String> {
    let text = without(text, &["objects", "signals"]);
    let drive = OpenDrive::from_xml_str(&text).map_err(|e| format!("OpenDRIVE: {e}"))?;
    Ok(network(&drive))
}

/// `text` without the elements named `names` and what they hold; they do not nest in
/// themselves.
fn without(text: &str, names: &[&str]) -> String {
    let mut out = text.to_string();
    for name in names {
        let open = format!("<{name}");
        let close = format!("</{name}>");
        let mut from = 0;
        while let Some(start) = out[from..].find(&open).map(|k| k + from) {
            // `<objects>`, `<objects ...>` or `<objects/>`, not `<objectsX`
            let after = out[start + open.len()..].chars().next();
            if !matches!(after, Some('>' | '/' | ' ' | '\t' | '\r' | '\n')) {
                from = start + open.len();
                continue;
            }
            let Some(tag_end) = out[start..].find('>').map(|k| k + start) else { break };
            let end = if out[..tag_end].ends_with('/') {
                tag_end + 1
            } else {
                match out[tag_end..].find(&close) {
                    Some(k) => tag_end + k + close.len(),
                    None => break,
                }
            };
            out.replace_range(start..end, "");
            from = start;
        }
    }
    out
}

/// The surfaces of a parsed network.
pub fn network(drive: &OpenDrive) -> Network {
    let header = &drive.header;
    let mut net = Network::default();
    net.info.push(("OpenDRIVE".into(), format!("{}.{}", header.rev_major, header.rev_minor)));
    for (key, value) in [("Name", &header.name), ("Version", &header.version), ("Date", &header.date), ("Vendor", &header.vendor)] {
        if let Some(value) = value.as_ref().filter(|v| !v.trim().is_empty()) {
            net.info.push((key.into(), value.clone()));
        }
    }
    net.geo_reference = header.geo_reference.as_ref().and_then(|g| g.proj.clone()).map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    let offset = header.offset.as_ref().map(|o| (o.x.get::<meter>(), o.y.get::<meter>(), o.z.get::<meter>(), o.hdg.get::<radian>())).unwrap_or((0.0, 0.0, 0.0, 0.0));
    // the header offset places the file's frame: turned by its heading, then moved
    let (so, co) = offset.3.sin_cos();
    let place = |p: [f64; 3]| [offset.0 + co * p[0] - so * p[1], offset.1 + so * p[0] + co * p[1], offset.2 + p[2]];
    let origin: [f64; 3] = drive.road.first().map(|r| r.plan_view.geometry.first()).map(|g| place([g.x.get::<meter>(), g.y.get::<meter>(), 0.0])).unwrap_or([0.0; 3]);
    net.origin = [origin[0].round(), origin[1].round(), 0.0];
    let local = |p: [f64; 3]| {
        let w = place(p);
        [(w[0] - net.origin[0]) as f32, (w[1] - net.origin[1]) as f32, (w[2] - net.origin[2]) as f32]
    };
    let mut surfaces: BTreeMap<&'static str, Surface> = BTreeMap::new();
    let mut next_id = 1u64;
    for road in &drive.road {
        let line = ReferenceLine::new(road);
        if line.length <= 0.0 {
            net.skipped.push(format!("road {}: no length", road.id));
            continue;
        }
        net.reference_lines.push((road.id.clone(), line.samples(0.0, line.length).iter().map(|&s| local(line.point(s, 0.0, 0.0))).collect()));
        let sections = road.lanes.lane_section.as_slice();
        for (k, section) in sections.iter().enumerate() {
            let start = section.s.max(0.0);
            let end = sections.get(k + 1).map(|n| n.s).unwrap_or(line.length).min(line.length);
            if end - start < 1e-6 {
                continue;
            }
            let samples = line.samples(start, end);
            let mut sides: Vec<(i64, &Lane)> = Vec::new();
            if let Some(left) = &section.left {
                sides.extend(left.lane.iter().map(|l| (l.id, &l.base)));
            }
            if let Some(right) = &section.right {
                sides.extend(right.lane.iter().map(|l| (l.id, &l.base)));
            }
            for side in [1i64, -1] {
                // the lanes of one side from the centre outward
                let mut lanes: Vec<(i64, &Lane)> = sides.iter().copied().filter(|(id, _)| id.signum() == side).collect();
                lanes.sort_by_key(|(id, _)| id.abs());
                // per sample, the inner border of the next lane (t), starting at the lane offset
                let mut inner: Vec<f64> = samples.iter().map(|&s| line.lane_offset(s)).collect();
                for (id, lane) in lanes {
                    let outer: Vec<f64> = samples.iter().zip(&inner).map(|(&s, &t)| match border(lane, s - start) {
                        Some(b) => line.lane_offset(s) + b,
                        None => t + side as f64 * width(lane, s - start).max(0.0),
                    }).collect();
                    let (kind, color) = lane_style(&lane.r#type);
                    let surface = surfaces.entry(kind).or_insert_with(|| Surface { lane_type: kind.into(), color, ..Surface::default() });
                    let first = surface.indices.len() as u32;
                    strip(surface, &samples, &inner, &outer, |s, t, along| {
                        let (hi, ho) = height(lane, s - start);
                        local(line.point(s, t, hi + (ho - hi) * along))
                    });
                    let count = surface.indices.len() as u32 - first;
                    if count > 0 {
                        surface.elements.push((next_id, first, count));
                        let mut props = vec![("Road".into(), road.id.clone()), ("Lane".into(), id.to_string()), ("Lane type".into(), kind.into()), ("Section".into(), format!("{start:.2} m"))];
                        if let Some(name) = road.name.as_ref().filter(|n| !n.is_empty()) {
                            props.insert(1, ("Road name".into(), name.clone()));
                        }
                        if road.junction != "-1" && !road.junction.is_empty() {
                            props.push(("Junction".into(), road.junction.clone()));
                        }
                        surface.props.push((next_id, props));
                        next_id += 1;
                    }
                    inner = outer;
                }
            }
        }
    }
    net.surfaces = surfaces.into_values().collect();
    net
}

/// The width of a lane `ds` after the start of its section (the record that applies).
fn width(lane: &Lane, ds: f64) -> f64 {
    let records: Vec<(f64, [f64; 4])> = lane.choice.iter().filter_map(|c| match c {
        LaneChoice::Width(w) => Some((w.s_offset.get::<meter>(), [w.a, w.b, w.c, w.d])),
        _ => None,
    }).collect();
    cubic_at(&records, ds).unwrap_or(0.0)
}

/// The outer border of a lane from the lane offset line, when it is given by borders.
fn border(lane: &Lane, ds: f64) -> Option<f64> {
    let records: Vec<(f64, [f64; 4])> = lane.choice.iter().filter_map(|c| match c {
        LaneChoice::Border(b) => Some((b.s_offset.get::<meter>(), [b.a, b.b, b.c, b.d])),
        _ => None,
    }).collect();
    if records.is_empty() || lane.choice.iter().any(|c| matches!(c, LaneChoice::Width(_))) {
        return None;
    }
    cubic_at(&records, ds)
}

/// The inner and outer height of a lane above the road (a raised pavement).
fn height(lane: &Lane, ds: f64) -> (f64, f64) {
    lane.height.iter().filter(|h| h.s_offset.get::<meter>() <= ds + 1e-9).last().map(|h| (h.inner.get::<meter>(), h.outer.get::<meter>())).unwrap_or((0.0, 0.0))
}

/// The value of the cubic record that applies at `s`: the last one starting at or before it.
fn cubic_at(records: &[(f64, [f64; 4])], s: f64) -> Option<f64> {
    let (s0, [a, b, c, d]) = records.iter().filter(|(s0, _)| *s0 <= s + 1e-9).last().or(records.first())?;
    let ds = s - s0;
    Some(a + b * ds + c * ds * ds + d * ds * ds * ds)
}

/// Triangles between the inner and outer border of a lane over `samples`; `at(s, t, along)`
/// gives the point at `t`, `along` 0 at the inner and 1 at the outer border. Lanes of no width
/// add nothing.
fn strip(surface: &mut Surface, samples: &[f64], inner: &[f64], outer: &[f64], at: impl Fn(f64, f64, f64) -> [f32; 3]) {
    if inner.iter().zip(outer).all(|(a, b)| (a - b).abs() < 1e-4) {
        return;
    }
    let base = (surface.positions.len() / 3) as u32;
    for (k, &s) in samples.iter().enumerate() {
        for (t, along) in [(inner[k], 0.0), (outer[k], 1.0)] {
            surface.positions.extend_from_slice(&at(s, t, along));
        }
    }
    let count = samples.len() as u32 * 2;
    let mut normals = vec![[0.0f32; 3]; count as usize];
    for k in 0..samples.len() as u32 - 1 {
        let (a, b, c, d) = (base + 2 * k, base + 2 * k + 1, base + 2 * k + 3, base + 2 * k + 2);
        for tri in [[a, b, c], [a, c, d]] {
            let p = |i: u32| {
                let i = i as usize * 3;
                [surface.positions[i], surface.positions[i + 1], surface.positions[i + 2]]
            };
            let (p0, p1, p2) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let tri = if n[2] < 0.0 {
                n = n.map(|v| -v);
                [tri[0], tri[2], tri[1]]
            } else {
                tri
            };
            if n.iter().all(|v| v.abs() < 1e-12) {
                continue;
            }
            surface.indices.extend_from_slice(&tri);
            for i in tri {
                let m = &mut normals[(i - base) as usize];
                for j in 0..3 {
                    m[j] += n[j];
                }
            }
        }
    }
    for n in normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let n = if len > 0.0 { n.map(|v| v / len) } else { [0.0, 0.0, 1.0] };
        surface.normals.extend_from_slice(&n);
    }
}

/// The longest step along a road between two samples (metres), and the most its heading may turn
/// in one step (radians).
const STEP: f64 = 2.0;
const TURN: f64 = 0.05;

/// A road's reference line with its profiles.
struct ReferenceLine<'a> {
    road: &'a Road,
    length: f64,
}

impl<'a> ReferenceLine<'a> {
    fn new(road: &'a Road) -> Self {
        let length = road.length.get::<meter>();
        let end = road.plan_view.geometry.iter().map(|g| g.s.get::<meter>() + g.length.get::<meter>()).fold(0.0, f64::max);
        Self { road, length: if length > 0.0 { length } else { end } }
    }

    /// The stations to sample between `from` and `to`: every geometry's start, and steps short
    /// enough for the curvature, the lane widths and the elevation.
    fn samples(&self, from: f64, to: f64) -> Vec<f64> {
        let mut out = vec![from];
        let mut breaks: Vec<f64> = self.road.plan_view.geometry.iter().map(|g| g.s.get::<meter>()).filter(|&s| s > from + 1e-6 && s < to - 1e-6).collect();
        breaks.push(to);
        let mut s = from;
        for stop in breaks {
            while s < stop - 1e-6 {
                let k = self.curvature(s).abs();
                let step = if k > 1e-9 { (TURN / k).min(STEP) } else { STEP }.max(0.05);
                s = (s + step).min(stop);
                out.push(s);
            }
        }
        out
    }

    fn geometry(&self, s: f64) -> &Geometry {
        let all = &self.road.plan_view.geometry;
        all.iter().filter(|g| g.s.get::<meter>() <= s + 1e-9).last().unwrap_or(all.first())
    }

    fn curvature(&self, s: f64) -> f64 {
        let g = self.geometry(s);
        let ds = s - g.s.get::<meter>();
        match &g.r#type {
            GeometryType::Line(_) => 0.0,
            GeometryType::Arc(a) => a.curvature.get::<radian_per_meter>(),
            GeometryType::Spiral(sp) => {
                let (k0, k1) = (sp.curvature_start.get::<radian_per_meter>(), sp.curvature_end.get::<radian_per_meter>());
                k0 + (k1 - k0) * ds / g.length.get::<meter>().max(1e-9)
            }
            // a cubic: about its heading change per metre near `ds`
            _ => {
                let (_, _, h0) = plan_point(g, ds);
                let (_, _, h1) = plan_point(g, ds + 0.5);
                (h1 - h0) / 0.5
            }
        }
    }

    /// The lane offset at `s` (the centre lane's distance from the reference line).
    fn lane_offset(&self, s: f64) -> f64 {
        let records: Vec<(f64, [f64; 4])> = self.road.lanes.lane_offset.iter().map(|o| (o.s, [o.a, o.b, o.c, o.d])).collect();
        if records.iter().all(|(s0, _)| *s0 > s) {
            return 0.0;
        }
        cubic_at(&records, s).unwrap_or(0.0)
    }

    fn elevation(&self, s: f64) -> f64 {
        let records: Vec<(f64, [f64; 4])> = self.road.elevation_profile.iter().flat_map(|p| p.elevation.iter()).map(|e| (e.s, [e.a, e.b, e.c, e.d])).collect();
        cubic_at(&records, s).unwrap_or(0.0)
    }

    fn superelevation(&self, s: f64) -> f64 {
        let records: Vec<(f64, [f64; 4])> = self.road.lateral_profile.iter().flat_map(|p| p.super_elevation.iter()).map(|e| (e.s, [e.a, e.b, e.c, e.d])).collect();
        cubic_at(&records, s).unwrap_or(0.0)
    }

    /// The point at station `s`, `t` to the left of the reference line, `h` above the road.
    fn point(&self, s: f64, t: f64, h: f64) -> [f64; 3] {
        let g = self.geometry(s);
        let (x, y, heading) = plan_point(g, s - g.s.get::<meter>());
        let roll = self.superelevation(s);
        let (sr, cr) = roll.sin_cos();
        let (sh, ch) = heading.sin_cos();
        let lateral = t * cr;
        [x - sh * lateral, y + ch * lateral, self.elevation(s) + t * sr + h]
    }
}

/// The plan position and heading `ds` along one geometry record.
fn plan_point(g: &Geometry, ds: f64) -> (f64, f64, f64) {
    let (x0, y0, h0) = (g.x.get::<meter>(), g.y.get::<meter>(), g.hdg.get::<radian>());
    let length = g.length.get::<meter>();
    let ds = ds.clamp(0.0, length.max(0.0));
    let local = |u: f64, v: f64, h: f64| {
        let (s, c) = h0.sin_cos();
        (x0 + c * u - s * v, y0 + s * u + c * v, h0 + h)
    };
    match &g.r#type {
        GeometryType::Line(_) => local(ds, 0.0, 0.0),
        GeometryType::Arc(a) => {
            let k = a.curvature.get::<radian_per_meter>();
            if k.abs() < 1e-12 {
                return local(ds, 0.0, 0.0);
            }
            let h = k * ds;
            local(h.sin() / k, (1.0 - h.cos()) / k, h)
        }
        GeometryType::Spiral(sp) => {
            let k0 = sp.curvature_start.get::<radian_per_meter>();
            let c = (sp.curvature_end.get::<radian_per_meter>() - k0) / length.max(1e-9);
            let heading = |s: f64| k0 * s + c * s * s / 2.0;
            // Simpson's rule over steps of at most 0.5 m
            let n = ((ds / 0.5).ceil() as usize).max(1) * 2;
            let step = ds / n as f64;
            let (mut u, mut v) = (0.0, 0.0);
            for i in 0..=n {
                let w = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
                let h = heading(i as f64 * step);
                u += w * h.cos();
                v += w * h.sin();
            }
            local(u * step / 3.0, v * step / 3.0, heading(ds))
        }
        GeometryType::Poly3(p) => {
            // v as a cubic of the local u; u found for the arc length `ds`
            let f = |u: f64| p.a + p.b * u + p.c * u * u + p.d * u * u * u;
            let df = |u: f64| p.b + 2.0 * p.c * u + 3.0 * p.d * u * u;
            let u = arc_to_parameter(|u| (1.0, df(u)), ds);
            let d = df(u);
            local(u, f(u), d.atan())
        }
        GeometryType::ParamPoly3(p) => {
            let du = |q: f64| (p.b_u + 2.0 * p.c_u * q + 3.0 * p.d_u * q * q, p.b_v + 2.0 * p.c_v * q + 3.0 * p.d_v * q * q);
            let q = match p.p_range {
                ParamPoly3pRange::ArcLength => ds,
                ParamPoly3pRange::Normalized => if length > 0.0 { ds / length } else { 0.0 },
            };
            let u = p.a_u + p.b_u * q + p.c_u * q * q + p.d_u * q * q * q;
            let v = p.a_v + p.b_v * q + p.c_v * q * q + p.d_v * q * q * q;
            let (tu, tv) = du(q);
            local(u, v, tv.atan2(tu))
        }
    }
}

/// The parameter at which a curve with derivative `d(p)` has run the arc length `length`.
fn arc_to_parameter(d: impl Fn(f64) -> (f64, f64), length: f64) -> f64 {
    let speed = |p: f64| {
        let (a, b) = d(p);
        (a * a + b * b).sqrt()
    };
    let (mut p, mut run) = (0.0, 0.0);
    let step = 0.05;
    while run < length {
        let next = speed(p + step / 2.0) * step;
        if run + next >= length {
            return p + step * (length - run) / next.max(1e-12);
        }
        run += next;
        p += step;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight road of two lanes and a pavement on the right, then a quarter circle, with a
    /// slope; a cycle lane on the left; a spiral and a parametric cubic in a second road.
    const NETWORK: &str = r#"<?xml version="1.0" standalone="yes"?>
<OpenDRIVE>
  <header revMajor="1" revMinor="7" name="Test" version="1.00" date="2026-11-18" north="0" south="0" east="0" west="0" vendor="Open BIM Suite">
    <geoReference><![CDATA[+proj=utm +zone=31 +datum=WGS84]]></geoReference>
    <offset x="600000" y="5800000" z="0" hdg="0"/>
  </header>
  <road name="Main" length="115.70796326794897" id="1" junction="-1">
    <link/>
    <planView>
      <geometry s="0" x="0" y="0" hdg="0" length="100"><line/></geometry>
      <geometry s="100" x="100" y="0" hdg="0" length="15.707963267948966"><arc curvature="0.1"/></geometry>
    </planView>
    <elevationProfile>
      <elevation s="0" a="0" b="0.02" c="0" d="0"/>
    </elevationProfile>
    <lateralProfile/>
    <lanes>
      <laneSection s="0">
        <left>
          <lane id="1" type="biking" level="false"><link/><width sOffset="0" a="1.5" b="0" c="0" d="0"/></lane>
        </left>
        <center>
          <lane id="0" type="none" level="false"><link/></lane>
        </center>
        <right>
          <lane id="-1" type="driving" level="false"><link/><width sOffset="0" a="3.5" b="0" c="0" d="0"/></lane>
          <lane id="-2" type="sidewalk" level="false"><link/><width sOffset="0" a="2" b="0" c="0" d="0"/><height sOffset="0" inner="0.15" outer="0.15"/></lane>
        </right>
      </laneSection>
    </lanes>
  </road>
  <road name="" length="50" id="2" junction="-1">
    <link/>
    <planView>
      <geometry s="0" x="0" y="-50" hdg="0" length="30"><spiral curvStart="0" curvEnd="0.02"/></geometry>
      <geometry s="30" x="30" y="-50" hdg="0" length="20"><paramPoly3 aU="0" bU="20" cU="0" dU="0" aV="0" bV="0" cV="0" dV="0" pRange="normalized"/></geometry>
    </planView>
    <lanes>
      <laneSection s="0">
        <center><lane id="0" type="none" level="false"><link/></lane></center>
        <right><lane id="-1" type="driving" level="false"><link/><width sOffset="0" a="3" b="0" c="0" d="0"/></lane></right>
      </laneSection>
    </lanes>
  </road>
</OpenDRIVE>
"#;

    fn surface<'a>(net: &'a Network, kind: &str) -> &'a Surface {
        net.surfaces.iter().find(|s| s.lane_type == kind).unwrap_or_else(|| panic!("{kind}"))
    }

    fn bounds(s: &Surface, first: u32, count: u32) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for &i in &s.indices[first as usize..(first + count) as usize] {
            for k in 0..3 {
                let v = s.positions[i as usize * 3 + k];
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
        (lo, hi)
    }

    #[test]
    fn lays_the_lanes_beside_the_reference_line() {
        let net = read(NETWORK).unwrap();
        assert_eq!(net.geo_reference.as_deref(), Some("+proj=utm +zone=31 +datum=WGS84"));
        assert_eq!(net.origin, [600000.0, 5800000.0, 0.0], "the header offset moves the network");
        assert!(net.info.contains(&("Vendor".into(), "Open BIM Suite".into())));
        let kinds: Vec<&str> = net.surfaces.iter().map(|s| s.lane_type.as_str()).collect();
        assert_eq!(kinds, ["biking", "driving", "sidewalk"]);
        // the driving lane of road 1: 3.5 m right of the line, rising 2 %, then turning left
        let driving = surface(&net, "driving");
        let (id, first, count) = driving.elements[0];
        assert_eq!(driving.props[0].0, id);
        assert!(driving.props[0].1.contains(&("Road name".into(), "Main".into())));
        let (lo, hi) = bounds(driving, first, count);
        assert!((lo[0] - 0.0).abs() < 1e-3 && (lo[1] + 3.5).abs() < 1e-3, "{lo:?}");
        // the quarter circle of radius 10 ends at (110, 10), heading north; the lane's outer
        // edge is 13.5 m from its centre (100, 10)
        assert!((hi[0] - 113.5).abs() < 0.02, "{hi:?}");
        assert!((hi[1] - 10.0).abs() < 0.02, "{hi:?}");
        assert!((hi[2] - 0.02 * 115.70796).abs() < 0.01, "{hi:?}");
        // the cycle lane lies on the left
        let biking = surface(&net, "biking");
        let (lo, hi) = bounds(biking, biking.elements[0].1, biking.elements[0].2);
        assert!(lo[1].abs() < 1e-3 && lo[0].abs() < 1e-3, "{lo:?}");
        // on the inside of the bend: its inner edge is the reference line, ending 10 m east of the
        // bend's centre
        assert!((hi[0] - 110.0).abs() < 0.02, "{hi:?}");
        // the pavement is raised 15 cm, beyond the driving lane
        let pavement = surface(&net, "sidewalk");
        let (lo, _) = bounds(pavement, pavement.elements[0].1, pavement.elements[0].2);
        assert!((lo[1] + 5.5).abs() < 1e-3 && (lo[2] - 0.15).abs() < 1e-3, "{lo:?}");
        // every face looks up
        for s in &net.surfaces {
            assert!(s.normals.chunks(3).all(|n| n[2] > 0.9), "{}", s.lane_type);
            assert_eq!(s.positions.len(), s.normals.len());
        }
    }

    #[test]
    fn follows_spirals_and_parametric_cubics() {
        let net = read(NETWORK).unwrap();
        let (_, line) = &net.reference_lines[1];
        // the cubic is a 20 m straight on from the end of the spiral, at its own start point
        let last = line.last().unwrap();
        assert!((last[0] - 50.0).abs() < 1e-3 && (last[1] + 50.0).abs() < 1e-3, "{last:?}");
        // a clothoid from straight to 1/50 over 30 m turns 0.3 rad and ends near (29.73, 2.99)
        let (x, y, h) = plan_point(&OpenDrive::from_xml_str(NETWORK).unwrap().road[1].plan_view.geometry[0], 30.0);
        assert!((h - 0.3).abs() < 1e-9 && (x - 29.73).abs() < 0.01 && (y + 50.0 - 2.99).abs() < 0.01, "{x} {y} {h}");
    }

    #[test]
    fn a_bad_file_says_so() {
        assert!(read("<OpenDRIVE>").is_err());
    }

    #[test]
    fn leaves_objects_and_signals_out() {
        let text = "<road><objects><object type=\"rail-pole\"/></objects><objectsX/><signals/><lanes/></road>";
        assert_eq!(without(text, &["objects", "signals"]), "<road><objectsX/><lanes/></road>");
    }
}
