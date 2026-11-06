//! Section properties against closed forms, and cardinal anchors against hand values.
//!
//! The hand values are computed here independently of the crate: by decomposing each section
//! into rectangles, trapezoids and fillet "spandrels" (the area between two edges meeting at a
//! corner and the tangent arc), using the nominal dimensions of the standard tables typed below.

use std::f64::consts::PI;

use bim_structural::section::{self, cardinal_anchor, outline, props, CHORD_TOL, MAX_PER_QUARTER};
use bim_structural::{ImportedProps, PropsSource, SectionProps, Shape};
use glam::DVec2;

fn props_of(shape: &Shape) -> SectionProps {
    props(&outline(shape, CHORD_TOL, MAX_PER_QUARTER).unwrap())
}

fn within(actual: f64, expected: f64, rel: f64, what: &str) {
    let e = ((actual - expected) / expected).abs();
    assert!(e <= rel, "{what}: {actual} vs {expected} ({:.4} %)", e * 100.0);
}

// ---------------------------------------------------------------------------------------------
// Closed forms within 0.5 %

#[test]
fn rectangle_closed_form() {
    let (b, h) = (0.1, 0.3);
    let p = props_of(&Shape::Rect { b, h });
    within(p.area, b * h, 0.005, "A");
    within(p.iy, b * h.powi(3) / 12.0, 0.005, "Iy");
    within(p.iz, h * b.powi(3) / 12.0, 0.005, "Iz");
    assert!(p.iyz.abs() < 1e-15 && p.centroid.length() < 1e-15);
}

#[test]
fn circle_closed_form() {
    let d = 0.1;
    let p = props_of(&Shape::Circle { d });
    within(p.area, PI * d * d / 4.0, 0.005, "A");
    within(p.iy, PI * d.powi(4) / 64.0, 0.005, "Iy");
    within(p.iz, PI * d.powi(4) / 64.0, 0.005, "Iz");
    let (dd, t) = (0.1683, 0.0071);
    let p = props_of(&Shape::Chs { d: dd, t });
    let di = dd - 2.0 * t;
    within(p.area, PI * (dd * dd - di * di) / 4.0, 0.005, "CHS A");
    within(p.iy, PI * (dd.powi(4) - di.powi(4)) / 64.0, 0.005, "CHS Iy");
}

#[test]
fn i_section_closed_form() {
    // IPE 300 nominal dimensions (EN 10365): h 300, b 150, tw 7.1, tf 10.7, r 15 mm.
    let (h, b, tw, tf, r) = (0.300, 0.150, 0.0071, 0.0107, 0.015);
    let p = props_of(&Shape::I { h, b, tw, tf, r, slope: 0.0 });
    // Four root fillets: spandrel area (1 − π/4) r², centroid g·r from the corner.
    let fa = (1.0 - PI / 4.0) * r * r;
    let g = (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let a = 2.0 * b * tf + (h - 2.0 * tf) * tw + 4.0 * fa;
    let iy = (b * h.powi(3) - (b - tw) * (h - 2.0 * tf).powi(3)) / 12.0 + 4.0 * fa * (h / 2.0 - tf - g * r).powi(2);
    let iz = (2.0 * tf * b.powi(3) + (h - 2.0 * tf) * tw.powi(3)) / 12.0 + 4.0 * fa * (tw / 2.0 + g * r).powi(2);
    within(p.area, a, 0.005, "A");
    within(p.iy, iy, 0.005, "Iy");
    within(p.iz, iz, 0.005, "Iz");
    // And the published table values: A 53.81 cm², Iy 8356 cm⁴, Iz 603.8 cm⁴.
    within(p.area, 53.81e-4, 0.005, "A table");
    within(p.iy, 8356e-8, 0.005, "Iy table");
    within(p.iz, 603.8e-8, 0.005, "Iz table");
    assert!(p.iyz.abs() < 1e-18);
}

#[test]
fn shear_centre_sources() {
    let poly = Shape::Polygon { outer: vec![DVec2::new(-0.1, -0.1), DVec2::new(0.1, -0.1), DVec2::new(-0.1, 0.1)], holes: vec![] };
    let (_, src) = section::compute(&poly, None);
    assert_eq!(src, PropsSource::ShearCentreApproximate);
    let imported = ImportedProps { shear_centre: Some(DVec2::new(-0.05, -0.05)), ..Default::default() };
    let (p, src) = section::compute(&poly, Some(&imported));
    assert_eq!(src, PropsSource::Imported);
    assert_eq!(p.shear_centre, DVec2::new(-0.05, -0.05));
    let (_, src) = section::compute(&Shape::Rect { b: 1.0, h: 1.0 }, None);
    assert_eq!(src, PropsSource::Computed);
    let (_, src) = section::compute(&Shape::Unknown, None);
    assert_eq!(src, PropsSource::Unavailable);
}

// ---------------------------------------------------------------------------------------------
// Hand decomposition

/// A region with area and first moments.
#[derive(Clone, Copy, Default)]
struct Part {
    a: f64,
    su: f64,
    sv: f64,
}

impl Part {
    fn at(a: f64, c: DVec2) -> Part {
        Part { a, su: a * c.x, sv: a * c.y }
    }
    fn add(self, o: Part) -> Part {
        Part { a: self.a + o.a, su: self.su + o.su, sv: self.sv + o.sv }
    }
    fn sub(self, o: Part) -> Part {
        Part { a: self.a - o.a, su: self.su - o.su, sv: self.sv - o.sv }
    }
    fn centroid(self) -> DVec2 {
        DVec2::new(self.su / self.a, self.sv / self.a)
    }
}

fn rect(u0: f64, u1: f64, v0: f64, v1: f64) -> Part {
    Part::at((u1 - u0) * (v1 - v0), DVec2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0))
}

/// The region between two straight edges leaving corner `p` along unit vectors `d1`, `d2`
/// and the circular arc of radius `r` tangent to both: the kite P-T1-C-T2 minus the sector.
fn spandrel(p: DVec2, d1: DVec2, d2: DVec2, r: f64) -> Part {
    let alpha = d1.dot(d2).clamp(-1.0, 1.0).acos(); // angle at the corner
    let bis = (d1 + d2).normalize();
    let t = r / (alpha / 2.0).tan();
    let t1 = p + d1 * t;
    let t2 = p + d2 * t;
    let c = p + bis * (r / (alpha / 2.0).sin());
    let tri1 = Part::at(r * t / 2.0, (p + t1 + c) / 3.0);
    let tri2 = Part::at(r * t / 2.0, (p + t2 + c) / 3.0);
    let beta = (PI - alpha) / 2.0; // half the sector angle
    let sector = Part::at(beta * r * r, c - bis * (2.0 * r * beta.sin() / (3.0 * beta)));
    tri1.add(tri2).sub(sector)
}

fn expected_anchors(lo: DVec2, hi: DVec2, centroid: DVec2, shear: DVec2) -> Vec<(u8, DVec2)> {
    // Looking along the extrusion: left = +u (u_max), right = −u (u_min), top = +v.
    let mid = (lo + hi) / 2.0;
    let (l, c, r) = (hi.x, mid.x, lo.x);
    let (b, m, t) = (lo.y, mid.y, hi.y);
    let p = DVec2::new;
    let mut out = vec![
        (1, p(l, b)),
        (2, p(c, b)),
        (3, p(r, b)),
        (4, p(l, m)),
        (5, p(c, m)),
        (6, p(r, m)),
        (7, p(l, t)),
        (8, p(c, t)),
        (9, p(r, t)),
        (10, centroid),
        (11, p(centroid.x, b)),
        (12, p(l, centroid.y)),
        (13, p(r, centroid.y)),
        (14, p(centroid.x, t)),
        (15, shear),
    ];
    out.extend([(16, p(shear.x, b)), (17, p(l, shear.y)), (18, p(r, shear.y)), (19, p(shear.x, t))]);
    out
}

fn assert_anchors(name: &str, shape: &Shape, expected: &[(u8, DVec2)]) {
    let pr = props_of(shape);
    for &(cp, want) in expected {
        let got = cardinal_anchor(&pr, cp).unwrap();
        assert!((got - want).abs().max_element() <= 1e-9, "{name} CP {cp}: {got} vs {want}");
    }
    assert_eq!(cardinal_anchor(&pr, 0), Some(DVec2::ZERO));
    assert_eq!(cardinal_anchor(&pr, 20), None);
}

#[test]
fn cardinal_anchors_ipe_200() {
    // IPE 200 (EN 10365): h 200, b 100, tw 5.6, tf 8.5, r 12 mm. Doubly symmetric.
    let shape = Shape::I { h: 0.200, b: 0.100, tw: 0.0056, tf: 0.0085, r: 0.012, slope: 0.0 };
    let (lo, hi) = (DVec2::new(-0.05, -0.1), DVec2::new(0.05, 0.1));
    assert_anchors("IPE 200", &shape, &expected_anchors(lo, hi, DVec2::ZERO, DVec2::ZERO));
    // Spot checks in words: CP 1 is bottom left = (+u, −v), CP 9 top right = (−u, +v).
    let pr = props_of(&shape);
    assert_eq!(cardinal_anchor(&pr, 1).unwrap(), DVec2::new(0.05, -0.1));
    assert_eq!(cardinal_anchor(&pr, 9).unwrap(), DVec2::new(-0.05, 0.1));
}

#[test]
fn cardinal_anchors_upn_200() {
    // UPN 200 (DIN 1026-1 / EN 10279): h 200, b 75, tw (s) 8.5, tf (t) 11.5, r1 11.5, r2 6 mm,
    // inner flange slope 8 %. Web at −u, flanges towards +u; tf is measured at u = 0, so the
    // inner face of the top flange is v = h/2 − tf + s·u.
    let (h, b, tw, tf, r1, r2, s) = (0.200, 0.075, 0.0085, 0.0115, 0.0115, 0.006, 0.08);
    let shape = Shape::U { h, b, tw, tf, r1, r2, slope: f64::atan(s) };
    let (u1, u2) = (-b / 2.0 + tw, b / 2.0);

    // Web over the full depth.
    let web = rect(-b / 2.0, u1, -h / 2.0, h / 2.0);
    // Top flange outstand: thickness tf − s·u between u1 and u2 (trapezoid); v-moment by
    // symmetry with the bottom flange is not needed.
    let fa = tf * (u2 - u1) - s * (u2 * u2 - u1 * u1) / 2.0;
    let fsu = tf * (u2 * u2 - u1 * u1) / 2.0 - s * (u2.powi(3) - u1.powi(3)) / 3.0;
    let flange = Part { a: fa, su: fsu, sv: 0.0 };
    // Root fillet: corner on the web face at the flange's inner face; edges down the web face
    // and along the inner flange face towards the toe.
    let along = DVec2::new(1.0, s).normalize();
    let root = spandrel(DVec2::new(u1, h / 2.0 - tf + s * u1), DVec2::new(0.0, -1.0), along, r1);
    // Toe rounding: corner of the inner face and the toe face; edges back along the inner face
    // and up the toe face. It removes material.
    let toe = spandrel(DVec2::new(u2, h / 2.0 - tf + s * u2), -along, DVec2::new(0.0, 1.0), r2);
    let top_half = flange.add(Part { sv: 0.0, ..root }).sub(Part { sv: 0.0, ..toe });
    let total = web.add(top_half).add(top_half);
    let centroid = DVec2::new(total.centroid().x, 0.0);

    // Shear centre: e = 3 b'² tf / (6 b' tf + h' tw) from the web centreline, b' = b − tw/2,
    // h' = h − tf, on the side away from the flanges.
    let (bp, hp) = (b - tw / 2.0, h - tf);
    let e = 3.0 * bp * bp * tf / (6.0 * bp * tf + hp * tw);
    let shear = DVec2::new(-b / 2.0 + tw / 2.0 - e, 0.0);

    let (lo, hi) = (DVec2::new(-b / 2.0, -h / 2.0), DVec2::new(b / 2.0, h / 2.0));
    assert_anchors("UPN 200", &shape, &expected_anchors(lo, hi, centroid, shear));
    // The table value of the centroid distance from the web back is 20.1 mm.
    assert!((centroid.x + b / 2.0 - 0.0201).abs() < 0.0002, "{}", centroid.x + b / 2.0);
    let pr = props_of(&shape);
    within(pr.area, 32.2e-4, 0.005, "UPN 200 A table");
}

#[test]
fn cardinal_anchors_l_100x10() {
    // L 100×100×10 (EN 10056-1): h = b 100, t 10, r1 12, r2 6 mm. Heel at (−b/2, −h/2).
    let (h, b, t, r1, r2) = (0.100, 0.100, 0.010, 0.012, 0.006);
    let shape = Shape::L { h, b, t, r1, r2 };
    let (u0, v0) = (-b / 2.0, -h / 2.0);
    let vertical = rect(u0, u0 + t, v0, v0 + h);
    let horizontal = rect(u0 + t, u0 + b, v0, v0 + t);
    let root = spandrel(DVec2::new(u0 + t, v0 + t), DVec2::new(0.0, 1.0), DVec2::new(1.0, 0.0), r1);
    let toe_h = spandrel(DVec2::new(u0 + b, v0 + t), DVec2::new(-1.0, 0.0), DVec2::new(0.0, -1.0), r2);
    let toe_v = spandrel(DVec2::new(u0 + t, v0 + h), DVec2::new(0.0, -1.0), DVec2::new(-1.0, 0.0), r2);
    let total = vertical.add(horizontal).add(root).sub(toe_h).sub(toe_v);
    let centroid = total.centroid();
    let shear = DVec2::new(u0 + t / 2.0, v0 + t / 2.0); // leg centreline intersection
    let (lo, hi) = (DVec2::new(u0, v0), DVec2::new(-u0, -v0));
    assert_anchors("L 100x10", &shape, &expected_anchors(lo, hi, centroid, shear));
    // Table: e = 28.2 mm from the heel, A = 19.2 cm².
    assert!((centroid.x - u0 - 0.0282).abs() < 0.0002 && (centroid.y - v0 - 0.0282).abs() < 0.0002, "{centroid}");
    within(props_of(&shape).area, 19.2e-4, 0.005, "L A table");
}

#[test]
fn plain_c_shear_centre_is_the_u_closed_form() {
    // A plain cold-formed channel (lip 0) is a U with tf = tw = t: e = 3 b'² t / (6 b' t + h' t)
    // from the web centreline with b' = b − t/2 (the flange runs to its free tip), h' = h − t.
    let (h, b, t) = (0.2, 0.07, 0.002);
    let c = props_of(&Shape::C { h, b, t, lip: 0.0, r: 0.003 });
    let (bp, hp) = (b - t / 2.0, h - t);
    let e = 3.0 * bp * bp * t / (6.0 * bp * t + hp * t);
    let shear = DVec2::new(-b / 2.0 + t / 2.0 - e, 0.0);
    assert!((c.shear_centre - shear).length() < 1e-12, "{} vs {shear}", c.shear_centre);
    assert!((shear.x + 0.057338).abs() < 1e-6);
    // The same dimensions as a U give the same shear centre, so anchors 15-19 agree.
    let u = props_of(&Shape::U { h, b, tw: t, tf: t, r1: 0.0, r2: 0.0, slope: 0.0 });
    assert!((u.shear_centre - c.shear_centre).length() < 1e-12);
    for cp in 15..=19 {
        let (a, b) = (cardinal_anchor(&c, cp).unwrap(), cardinal_anchor(&u, cp).unwrap());
        assert!((a - b).length() < 1e-12, "CP {cp}: {a} vs {b}");
    }
    // With a lip the flange centreline ends on the lip centreline: (b − t) in the closed form.
    let lipped = props_of(&Shape::C { h, b, t, lip: 0.02, r: 0.003 });
    let (a, w, cc) = (h - t, b - t, 0.02 - t / 2.0);
    let m = w * (3.0 * a * a * w + cc * (6.0 * a * a - 8.0 * cc * cc)) / (a.powi(3) + 6.0 * a * a * w + cc * (8.0 * cc * cc - 12.0 * a * cc + 6.0 * a * a));
    assert!((lipped.shear_centre.x - (-b / 2.0 + t / 2.0 - m)).abs() < 1e-12);
}

#[test]
fn properties_beyond_f64_are_unavailable() {
    let huge = Shape::I { h: 1e200, b: 1e199, tw: 1e197, tf: 1e198, r: 0.0, slope: 0.0 };
    assert!(section::check(&huge).is_ok());
    let (p, src) = section::compute(&huge, None);
    assert_eq!(src, PropsSource::Unavailable);
    assert!(section::props_finite(&SectionProps { bbox: [DVec2::splat(-1.0), DVec2::ONE], area: 1.0, ..p }));
    assert!(!section::props_finite(&p));
}
