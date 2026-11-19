//! A project that shows every system type of the library and the joins between elements, written
//! as an IFCX file: a timber-frame house (corners, T junctions, openings, prefab units), a sample
//! of every system type per group, corners and T junctions of framed systems apart, wall joins and
//! joins of linear members.
//!
//! `SHOWCASE_OUT=path.ifcx cargo test --lib writes_the_systems_showcase -- --ignored --nocapture`

use super::*;
use bim_core::Opening;
use bim_library::{Lang, SystemGroup};
use std::iter::once;

/// What was placed and what the kernel refused.
#[derive(Default)]
struct Log {
    placed: usize,
    refused: Vec<String>,
}

fn add(app: &mut App, log: &mut Log, name: &str, shape: Shape) -> Option<ElementId> {
    let level = app.work_plane().2;
    match app.add_element(name, level, shape) {
        Ok(id) => {
            log.placed += 1;
            Some(id)
        }
        Err(refusal) => {
            log.refused.push(format!("{name}: {refusal:?}"));
            None
        }
    }
}

fn p(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A standing system of type `id` on the line `a`–`b`, or lying in the rectangle from `a` to `b`.
fn system(app: &App, id: &str, a: DVec2, b: DVec2, height: f64, openings: Vec<Opening>) -> Option<Shape> {
    let generic = SystemDef { start: a, end: b, base: 0.0, height, spacing: SYSTEM_SPACING, member: SYSTEM_MEMBER, panel: SYSTEM_PANEL, system_type: None, openings, breaks: Vec::new() };
    let t = app.library.system_type(id)?;
    if let Some(above) = t.lying {
        let (lo, hi) = (a.min(b), a.max(b));
        let rectangle = SystemDef { start: lo, end: p(hi.x, lo.y), base: generic.base + above, height: hi.y - lo.y, openings: Vec::new(), ..generic };
        return rectangle.laid(t).map(Shape::System);
    }
    generic.typed_from(&app.library, id).map(Shape::System)
}

fn connect(app: &mut App, log: &mut Log, name: &str, kind: ConnectionKind, primary: Option<ElementId>, secondary: Option<ElementId>) {
    let (Some(primary), Some(secondary)) = (primary, secondary) else { return };
    // a join that came by itself between the two is replaced, not doubled
    let own = app.modeller.model.iter().find(|e| matches!(&e.shape, Shape::Connection(c) if (c.primary == primary && c.secondary == secondary) || (c.primary == secondary && c.secondary == primary))).cloned();
    let shape = Shape::Connection(ConnectionDef { kind, primary, secondary });
    match own {
        Some(mut joint) => {
            joint.shape = shape;
            if let Err(refusal) = app.modeller.model.replace(joint) {
                log.refused.push(format!("{name}: {refusal:?}"));
            }
            app.show_modelled();
        }
        None => {
            add(app, log, name, shape);
        }
    }
}

const DOOR: Opening = Opening { along: 0.9, sill: 0.0, width: 0.93, height: 2.315, framing: None };

fn window(along: f64) -> Opening {
    Opening { along, sill: 0.9, width: 1.2, height: 1.2, framing: None }
}

/// The project: section `SHOWCASE_ONLY` of it (1 to 5), or all of it.
fn build() -> (App, Log) {
    let mut app = super::tests::modelling();
    let mut log = Log::default();
    let lib = app.library.clone();
    let name = |id: &str| lib.system_name(id, Lang::En).unwrap_or_else(|| id.to_string());
    let has = |id: &str| lib.system_type(id).is_some();

    let x0 = 12.0;
    let (w, d, h) = (7.2, 4.8, 2.7);
    let only: Option<u32> = std::env::var("SHOWCASE_ONLY").ok().and_then(|v| v.parse().ok());
    let on = |n: u32| only.is_none_or(|o| o == n);

    if on(1) {
    // 1. the house: timber frames 140 around, an interior frame ending on two of them, a floor
    //    under it and a ceiling in it
    let outer = "system.timber-frame-140";
    let mut frame = |app: &mut App, log: &mut Log, id: &str, a: DVec2, b: DVec2, openings: Vec<Opening>| system(app, id, a, b, h, openings).and_then(|s| add(app, log, &name(id), s));
    let south = frame(&mut app, &mut log, outer, p(0.0, 0.0), p(w, 0.0), vec![DOOR, window(2.7), window(5.1)]);
    let east = frame(&mut app, &mut log, outer, p(w, 0.0), p(w, d), vec![window(1.8)]);
    frame(&mut app, &mut log, outer, p(w, d), p(0.0, d), vec![window(1.2), Opening { along: 4.2, sill: 0.0, width: 1.8, height: 2.315, framing: None }]);
    frame(&mut app, &mut log, outer, p(0.0, d), p(0.0, 0.0), Vec::new());
    frame(&mut app, &mut log, "system.timber-frame-internal-89", p(4.2, 0.0), p(4.2, d), vec![Opening { along: 1.5, ..DOOR }]);
    // the short gable runs through at the south-east corner, by a connection of its own
    connect(&mut app, &mut log, "Corner", ConnectionKind::Butt, east, south);
    if let Some(s) = system(&app, "system.hollow-core-260", p(0.0, 0.0), p(w, d), h, Vec::new()) {
        add(&mut app, &mut log, &name("system.hollow-core-260"), s);
    }
    // a ceiling in either room, between the inner faces of the frames around it
    let clear = 0.2;
    for (a, b) in [(p(clear, clear), p(4.2 - clear, d - clear)), (p(4.2 + clear, clear), p(w - clear, d - clear))] {
        if let Some(s) = system(&app, "system.ceiling-600x600", a, b, h, Vec::new()) {
            add(&mut app, &mut log, &name("system.ceiling-600x600"), s);
        }
    }

    }

    if on(2) {
    // 2. a sample of every system type, a row per group
    for (row, group) in SystemGroup::ALL.into_iter().enumerate() {
        let y = -6.0 - 5.0 * row as f64;
        for (k, t) in lib.system_types_of(group).enumerate() {
            let x = 5.0 * k as f64;
            let framed = matches!(group, SystemGroup::TimberFrame | SystemGroup::MetalStud);
            let openings = if framed { vec![window(1.2)] } else { Vec::new() };
            let (a, b) = if t.lying.is_some() { (p(x, y), p(x + 3.6, y + 3.0)) } else { (p(x, y), p(x + 3.6, y)) };
            match system(&app, &t.id, a, b, h, openings) {
                Some(s) => {
                    add(&mut app, &mut log, &name(&t.id), s);
                }
                None => log.refused.push(format!("{}: no system of this type", t.id)),
            }
        }
    }

    }

    if on(3) {
    // 3. framed systems meeting: corners of one and of two thicknesses, a T, two kept apart, and
    //    the same corners in the other groups
    let mut meet = |app: &mut App, log: &mut Log, at: DVec2, first: &str, second: &str, tee: bool, kind: Option<ConnectionKind>| {
        if !has(first) || !has(second) {
            return;
        }
        let a = system(app, first, at, at + p(3.6, 0.0), h, Vec::new()).and_then(|s| add(app, log, &name(first), s));
        let from = if tee { at + p(1.8, 0.0) } else { at + p(3.6, 0.0) };
        let b = system(app, second, from, from + p(0.0, 2.4), h, Vec::new()).and_then(|s| add(app, log, &name(second), s));
        if let Some(kind) = kind {
            connect(app, log, "Connection", kind, b, a);
        }
    };
    meet(&mut app, &mut log, p(x0, 0.0), "system.timber-frame-140", "system.timber-frame-140", false, None);
    meet(&mut app, &mut log, p(x0 + 6.0, 0.0), "system.timber-frame-184", "system.timber-frame-140", false, None);
    meet(&mut app, &mut log, p(x0 + 12.0, 0.0), "system.timber-frame-235", "system.timber-frame-internal-89", true, None);
    meet(&mut app, &mut log, p(x0 + 18.0, 0.0), "system.timber-frame-140", "system.timber-frame-140", false, Some(ConnectionKind::Butt));
    meet(&mut app, &mut log, p(x0 + 24.0, 0.0), "system.timber-frame-140", "system.timber-frame-140", false, Some(ConnectionKind::Apart));
    let mut col = 0.0;
    for group in [SystemGroup::CurtainWall, SystemGroup::MetalStud, SystemGroup::Masonry, SystemGroup::Cladding] {
        if let Some(t) = lib.system_types_of(group).next() {
            meet(&mut app, &mut log, p(x0 + col, 4.5), &t.id, &t.id, false, None);
            meet(&mut app, &mut log, p(x0 + col + 6.0, 4.5), &t.id, &t.id, true, None);
            col += 12.0;
        }
    }

    }

    let wall = |a: DVec2, b: DVec2, thickness: f64, build_up: Option<&str>| {
        let plain = PlanarDef { form: PlanarForm::Wall { start: a, end: b, base: 0.0, height: h }, thickness, openings: Vec::new(), build_up: None };
        Shape::Planar(plain.with_build_up(build_up.and_then(|id| bim_core::BuildUpDef::from_library(&lib, id))))
    };
    let layered = lib.buildups_of(bim_library::Role::Wall).next().map(|b| b.id.clone());
    if on(4) {
    // 4. wall joins: a corner, a corner of two thicknesses, a T, a layered corner
    let y = 9.0;
    for (k, (t1, t2, tee, build_up)) in [(0.2, 0.2, false, None), (0.3, 0.1, false, None), (0.2, 0.1, true, None), (0.3, 0.3, false, layered.as_deref())].into_iter().enumerate() {
        let at = p(x0 + 6.0 * k as f64, y);
        add(&mut app, &mut log, "Wall", wall(at, at + p(3.6, 0.0), t1, build_up));
        let from = if tee { at + p(1.8, 0.0) } else { at + p(3.6, 0.0) };
        add(&mut app, &mut log, "Wall", wall(from, from + p(0.0, 2.4), t2, build_up));
    }

    }

    if on(5) {
    // 5. joins of linear members: timber in a U (butts at the posts, mitres at the far corners),
    //    steel with an end plate, and a beam that cuts its way through a wall
    let y = 13.5;
    let timber = |a: DVec3, b: DVec3, width: f64, height: f64| Shape::Linear(LinearDef { start: a, end: b, profile: ProfileDef::Rectangle { width, height }, roll: 0.0 });
    for at in [p(x0, y), p(x0, y + 3.0)] {
        add(&mut app, &mut log, "Post", timber(at.extend(0.0), at.extend(h), 0.14, 0.14));
    }
    for (a, b) in [(p(x0, y), p(x0 + 3.6, y)), (p(x0 + 3.6, y), p(x0 + 3.6, y + 3.0)), (p(x0 + 3.6, y + 3.0), p(x0, y + 3.0))] {
        add(&mut app, &mut log, "Beam", timber(a.extend(2.4), b.extend(2.4), 0.07, 0.2));
    }
    let steel = |d: &str| ProfileDef::steel(d);
    if let (Some(column), Some(beam)) = (steel("HEA200").or_else(|| steel("HEA 200")), steel("IPE200").or_else(|| steel("IPE 200"))) {
        let at = p(x0 + 8.0, y);
        let c = add(&mut app, &mut log, "Column", Shape::Linear(LinearDef { start: at.extend(0.0), end: at.extend(3.0), profile: column, roll: 0.0 }));
        let b = add(&mut app, &mut log, "Beam", Shape::Linear(LinearDef { start: at.extend(2.4), end: (at + p(3.6, 0.0)).extend(2.4), profile: beam, roll: 0.0 }));
        connect(&mut app, &mut log, "End plate", ConnectionKind::EndPlate(bim_core::join::EndPlate::default()), c, b);
    } else {
        log.refused.push("steel: HEA200 or IPE200 is not in the catalogue".into());
    }
    let at = p(x0 + 15.0, y);
    let cut = add(&mut app, &mut log, "Wall", wall(at, at + p(0.0, 3.0), 0.2, None));
    let through = add(&mut app, &mut log, "Beam", timber((at + p(-1.2, 1.5)).extend(2.0), (at + p(1.2, 1.5)).extend(2.0), 0.1, 0.25));
    connect(&mut app, &mut log, "Cut-out", ConnectionKind::Cut, through, cut);

    }

    (app, log)
}

#[test]
#[ignore = "writes a project file; run it by name"]
fn writes_the_systems_showcase() {
    let (app, log) = build();
    write(&app, log, "SHOWCASE_OUT", "target/systems-showcase.ifcx");
}

/// Writes `app`'s project to the file environment variable `var` names (else `default`), and
/// prints what the kernel made of it and what it refused.
fn write(app: &App, mut log: Log, var: &str, default: &str) {
    // what the kernel makes of it
    let model = &app.modeller.model;
    let (mut parts, mut joins) = (0, 0);
    for e in model.iter() {
        if matches!(e.shape, Shape::Connection(_)) {
            joins += 1;
        }
        match model.evaluate(e.id) {
            Ok(made) => parts += made.len(),
            Err(refusal) => log.refused.push(format!("{} (#{}): {refusal:?}", e.name, e.id.0)),
        }
    }
    let out = std::env::var(var).unwrap_or_else(|_| default.into());
    let file = std::path::Path::new(&out).file_name().and_then(|n| n.to_str()).unwrap_or("systems-showcase.ifcx").to_string();
    let scene = app.model.clone().expect("a scene");
    let bytes = crate::export::to_bytes(&scene, &app.document_now(), &file).expect("IFCX");
    std::fs::write(&out, &bytes).expect("the file is written");
    println!("wrote {out}: {} bytes, {} elements ({} placed, {joins} connections), {parts} parts", bytes.len(), model.len(), log.placed);
    for r in &log.refused {
        println!("refused: {r}");
    }
}

/// How much two boxes overlap along their shortest common side (metres); not above zero when
/// they are apart.
fn boxes_overlap(a: (DVec3, DVec3), b: (DVec3, DVec3)) -> f64 {
    (a.1.min(b.1) - a.0.max(b.0)).min_element()
}

/// The volume two solids share (m³), and whether the kernel could work it out.
fn shared(a: &bim_core::Solid, b: &bim_core::Solid) -> Result<f64, Refusal> {
    match bim_core::boolean(a, b, bim_core::BooleanOp::Intersection) {
        Ok(common) => Ok(common.volume()),
        Err(Refusal::Empty) => Ok(0.0),
        Err(refusal) => Err(refusal),
    }
}

/// Every pair of parts of the project that share volume: within an element and between two.
/// `cargo test --lib reports_overlaps_in_the_systems_showcase -- --ignored --nocapture`
#[test]
#[ignore = "a report; run it by name"]
fn reports_overlaps_in_the_systems_showcase() {
    let (app, _) = build();
    report_overlaps(&app);
}

/// Prints every pair of parts of `app`'s model that share volume.
fn report_overlaps(app: &App) {
    const CLEAR: f64 = 1e-4;
    const VOLUME: f64 = 1e-7;
    let model = &app.modeller.model;
    let made: Vec<(&Element, Vec<bim_core::Part>)> = model.iter().filter(|e| !matches!(e.shape, Shape::Connection(_))).filter_map(|e| Some((e, model.evaluate(e.id).ok()?))).collect();
    let boxes: Vec<Vec<(DVec3, DVec3)>> = made.iter().map(|(_, parts)| parts.iter().map(|p| p.solid.bounds()).collect()).collect();
    let whole = |k: usize| boxes[k].iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |acc, b| (acc.0.min(b.0), acc.1.max(b.1)));
    let mut found = 0;
    for i in 0..made.len() {
        for j in i..made.len() {
            if i != j && boxes_overlap(whole(i), whole(j)) <= CLEAR {
                continue;
            }
            let (mut volume, mut pairs, mut failed) = (0.0, 0, 0);
            let mut worst: Option<(f64, String)> = None;
            for (a, pa) in made[i].1.iter().enumerate() {
                for (b, pb) in made[j].1.iter().enumerate() {
                    if (i == j && b <= a) || pa.tile != pb.tile || boxes_overlap(boxes[i][a], boxes[j][b]) <= CLEAR {
                        continue;
                    }
                    match shared(&pa.solid, &pb.solid) {
                        Ok(v) if v > VOLUME => {
                            volume += v;
                            pairs += 1;
                            if worst.as_ref().is_none_or(|w| v > w.0) {
                                let at = (boxes[i][a].0.max(boxes[j][b].0) + boxes[i][a].1.min(boxes[j][b].1)) / 2.0;
                                worst = Some((v, format!("{} {:?} with {} {:?} at ({:.2}, {:.2}, {:.2})", pa.class, pa.material.as_deref().unwrap_or("-"), pb.class, pb.material.as_deref().unwrap_or("-"), at.x, at.y, at.z)));
                            }
                        }
                        Ok(_) => {}
                        Err(_) => failed += 1,
                    }
                }
            }
            if pairs > 0 || failed > 0 {
                found += 1;
                let (a, b) = (made[i].0, made[j].0);
                let who = if i == j { format!("within #{} {}", a.id.0, a.name) } else { format!("#{} {} and #{} {}", a.id.0, a.name, b.id.0, b.name) };
                println!("overlap: {who}: {:.1} dm³ in {pairs} pairs, {failed} not worked out; largest {}", volume * 1e3, worst.map_or("-".into(), |w| format!("{:.2} dm³, {}", w.0 * 1e3, w.1)));
            }
        }
    }
    println!("{found} overlapping of {} elements", made.len());
}

/// A standing system of type `id` on the line `a`–`b` from `base` up `height`.
fn standing(app: &App, id: &str, a: DVec2, b: DVec2, base: f64, height: f64, openings: Vec<Opening>) -> Option<Shape> {
    let generic = SystemDef { start: a, end: b, base, height, spacing: SYSTEM_SPACING, member: SYSTEM_MEMBER, panel: SYSTEM_PANEL, system_type: None, openings, breaks: Vec::new() };
    generic.typed_from(&app.library, id).map(Shape::System)
}

/// A lying system of type `id` in the rectangle from `a` to `b`, as high above `level` as its
/// type lies.
fn lying(app: &App, id: &str, a: DVec2, b: DVec2, level: f64) -> Option<Shape> {
    let t = app.library.system_type(id)?;
    let (lo, hi) = (a.min(b), a.max(b));
    let rectangle = SystemDef { start: lo, end: p(hi.x, lo.y), base: level + t.lying?, height: hi.y - lo.y, spacing: SYSTEM_SPACING, member: SYSTEM_MEMBER, panel: SYSTEM_PANEL, system_type: None, openings: Vec::new(), breaks: Vec::new() };
    rectangle.laid(t).map(Shape::System)
}

/// Windows 1.2 m wide every 2.4 m along a wall `length` long, clear of its ends, of the points
/// `junctions` where other walls stand against it, and of the openings it has already.
fn windows(length: f64, junctions: &[f64], fixed: &[Opening]) -> Vec<Opening> {
    const CLEAR: f64 = 0.6;
    let mut out = fixed.to_vec();
    let mut along = 0.9;
    while along + 1.2 <= length - 0.9 {
        let free = junctions.iter().all(|j| *j < along - CLEAR || *j > along + 1.2 + CLEAR) && fixed.iter().all(|o| o.along + o.width < along - CLEAR || o.along > along + 1.2 + CLEAR);
        if free {
            out.push(Opening { along, sill: 0.9, width: 1.2, height: 1.35, framing: None });
        }
        along += 2.4;
    }
    out.sort_by(|a, b| a.along.total_cmp(&b.along));
    out
}

/// A timber-frame building of two storeys on an L-shaped plan, 15.6 × 7.2 m: exterior frames
/// around both storeys with their six corners (one of them re-entrant), a party wall, a corridor
/// wall and room dividers that end on one another and on the exterior, doors and windows in all
/// of them, a floor of hollow-core slabs over either storey and a ceiling in every room.
fn build_building() -> (App, Log) {
    let mut app = super::tests::modelling();
    let mut log = Log::default();
    let _ = app.document.add_level("Level 1", 3.0);
    let _ = app.document.add_level("Level 2", 6.0);
    // an exterior type with rules of its own: units of 3600 or 2400 and none under 1200, a higher
    // header over a wider opening, and its framing clear of its openings
    let ruled = "system.timber-frame-184-prefab";
    if let Some(mut t) = app.library.system_type("system.timber-frame-184").cloned() {
        t.id = ruled.into();
        t.members.framing = t.members.framing.map(|f| {
            f.with_unit(4.2).with_lengths(&[3.6, 2.4], 1.2).with_wide(&[(1.1, 0.24), (2.0, 0.3)]).with_clearance(bim_library::Clearance { sides: 0.01, top: 0.015, bottom: 0.005 })
        });
        if let Err(e) = app.library.add_system_type(t) {
            log.refused.push(format!("{ruled}: {e:?}"));
        }
    }
    let lib = app.library.clone();
    let name = |id: &str| lib.system_name(id, Lang::En).unwrap_or_else(|| id.to_string());
    let (party, inner, floor, ceiling) = ("system.timber-frame-235", "system.timber-frame-internal-89", "system.hollow-core-260", "system.ceiling-600x600");
    let height = 2.7;
    let door = |along: f64| Opening { along, sill: 0.0, width: 0.93, height: 2.315, framing: None };

    // the ground slab under all of it
    let outline = vec![p(0.0, 0.0), p(15.6, 0.0), p(15.6, 4.8), p(9.6, 4.8), p(9.6, 7.2), p(0.0, 7.2)];
    let slab = PlanarForm::Slab { outline: outline.iter().map(|q| *q + (*q - p(7.8, 3.0)).signum() * 0.15).collect(), holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None };

    for storey in 0..2 {
        let base = 3.0 * storey as f64;
        let level = app.document.levels().iter().find(|l| (l.elevation - base).abs() < 1e-6).map(|l| l.path.clone());
        let mut place = |app: &mut App, log: &mut Log, name: &str, shape: Option<Shape>| match shape {
            Some(shape) => match app.add_element(name, level.clone(), shape) {
                Ok(_) => log.placed += 1,
                Err(refusal) => log.refused.push(format!("storey {storey}, {name}: {refusal:?}")),
            },
            None => log.refused.push(format!("storey {storey}, {name}: no such type")),
        };
        if storey == 0 {
            place(&mut app, &mut log, "Ground slab", Some(Shape::Planar(PlanarDef { form: slab.clone(), thickness: 0.25, openings: Vec::new(), build_up: None })));
        }
        // where the dividers stand: south of the corridor and north of it
        let (south, north): (&[f64], &[f64]) = if storey == 0 { (&[3.0, 5.4], &[4.2]) } else { (&[2.4, 4.8], &[3.0, 6.0]) };
        let outer = if storey == 0 { ruled } else { "system.timber-frame-140" };

        // the exterior, counter-clockwise, its outside to the right of every line
        let on_south: Vec<f64> = south.iter().copied().chain([8.4, 12.0]).collect();
        let on_north: Vec<f64> = north.iter().map(|x| 9.6 - x).chain([1.2]).collect();
        // the front door has a framing of its own: two king studs and two trimmers either side
        let doubled = bim_core::OpeningFraming { header: Some(0.3), kings: Some(2), trimmers: Some(2) };
        let entrances = if storey == 0 { vec![Opening { along: 6.4, sill: 0.0, width: 1.0, height: 2.315, framing: Some(doubled) }, Opening { along: 13.2, sill: 0.0, width: 1.0, height: 2.315, framing: None }] } else { Vec::new() };
        let exterior: [(DVec2, DVec2, Vec<f64>, Vec<Opening>); 6] = [
            (outline[0], outline[1], on_south, entrances),
            (outline[1], outline[2], Vec::new(), Vec::new()),
            (outline[2], outline[3], vec![3.6], Vec::new()),
            (outline[3], outline[4], Vec::new(), Vec::new()),
            (outline[4], outline[5], on_north, Vec::new()),
            (outline[5], outline[0], vec![3.6], Vec::new()),
        ];
        for (a, b, junctions, fixed) in exterior {
            let openings = windows((b - a).length(), &junctions, &fixed);
            let shape = standing(&app, outer, a, b, base, height, openings);
            place(&mut app, &mut log, &name(outer), shape);
        }

        // the party wall, the corridor wall, and the dividers that end on them and on the exterior
        let shape = standing(&app, party, p(8.4, 0.0), p(8.4, 7.2), base, height, Vec::new());
        place(&mut app, &mut log, &name(party), shape);
        let corridor_doors = if storey == 0 { vec![door(1.0), door(6.3)] } else { vec![door(0.9), door(6.9)] };
        let shape = standing(&app, inner, p(0.0, 3.6), p(8.4, 3.6), base, height, corridor_doors);
        place(&mut app, &mut log, &name(inner), shape);
        for x in south {
            let shape = standing(&app, inner, p(*x, 0.0), p(*x, 3.6), base, height, vec![door(1.2)]);
            place(&mut app, &mut log, &name(inner), shape);
        }
        for x in north {
            let shape = standing(&app, inner, p(*x, 3.6), p(*x, 7.2), base, height, vec![door(1.2)]);
            place(&mut app, &mut log, &name(inner), shape);
        }
        let shape = standing(&app, inner, p(12.0, 0.0), p(12.0, 4.8), base, height, vec![door(3.0)]);
        place(&mut app, &mut log, &name(inner), shape);
        if storey == 0 {
            let shape = standing(&app, inner, p(8.4, 2.4), p(12.0, 2.4), base, height, vec![door(1.3)]);
            place(&mut app, &mut log, &name(inner), shape);
        }

        // the floor over the storey, in two fields, and a ceiling in every room
        for (a, b) in [(p(0.0, 0.0), p(9.6, 7.2)), (p(9.6, 0.0), p(15.6, 4.8))] {
            let shape = lying(&app, floor, a, b, base);
            place(&mut app, &mut log, &name(floor), shape);
        }
        let xs = |at: &[f64]| -> Vec<f64> { once(0.0).chain(at.iter().copied()).chain(once(8.4)).collect() };
        let mut rooms: Vec<(DVec2, DVec2)> = Vec::new();
        rooms.extend(xs(south).windows(2).map(|x| (p(x[0], 0.0), p(x[1], 3.6))));
        rooms.extend(xs(north).windows(2).map(|x| (p(x[0], 3.6), p(x[1], 7.2))));
        rooms.push((p(12.0, 0.0), p(15.6, 4.8)));
        if storey == 0 {
            rooms.extend([(p(8.4, 0.0), p(12.0, 2.4)), (p(8.4, 2.4), p(12.0, 4.8))]);
        } else {
            rooms.push((p(8.4, 0.0), p(12.0, 4.8)));
        }
        let clear = DVec2::splat(0.3);
        for (a, b) in rooms {
            let shape = lying(&app, ceiling, a + clear, b - clear, base);
            place(&mut app, &mut log, &name(ceiling), shape);
        }
    }
    (app, log)
}

/// `BUILDING_OUT=path.ifcx cargo test --lib writes_the_timber_frame_building -- --ignored --nocapture`
#[test]
#[ignore = "writes a project file; run it by name"]
fn writes_the_timber_frame_building() {
    let (app, log) = build_building();
    write(&app, log, "BUILDING_OUT", "target/timber-frame-building.ifcx");
}

#[test]
#[ignore = "a report; run it by name"]
fn reports_overlaps_in_the_timber_frame_building() {
    let (app, _) = build_building();
    report_overlaps(&app);
    // every junction of the frames: which corners and T junctions the kernel made
    let model = &app.modeller.model;
    let (mut corners, mut tees, mut bare) = (0, 0, Vec::new());
    for e in model.iter() {
        let Shape::System(s) = &e.shape else { continue };
        if s.lies() {
            continue;
        }
        let made = bim_core::system::corners(model, e);
        let t = bim_core::system::tees(model, e);
        corners += made.iter().flatten().count();
        tees += t.ends.iter().flatten().count();
        if made.iter().all(Option::is_none) && t.ends.iter().all(Option::is_none) {
            bare.push(format!("#{} {}", e.id.0, e.name));
        }
    }
    println!("{corners} corner ends and {tees} ends against another's side; frames with neither end joined: {bare:?}");
}

/// Adds the building's standing systems to an empty model one at a time and prints, after each,
/// how many ends are joined: where the count falls, the system added last undid junctions.
#[test]
#[ignore = "a report; run it by name"]
fn reports_which_frame_undoes_the_junctions() {
    let (app, _) = build_building();
    let source = &app.modeller.model;
    let mut m = bim_core::Model::new(source.grid);
    for e in source.iter() {
        let Shape::System(s) = &e.shape else { continue };
        if s.lies() || s.base > 1.0 {
            continue;
        }
        let mut copy = e.clone();
        copy.name = format!("{} ({:.1},{:.1})-({:.1},{:.1}) openings {:?}", e.name, s.start.x, s.start.y, s.end.x, s.end.y, s.openings.iter().map(|o| (o.along, o.width)).collect::<Vec<_>>());
        let name = copy.name.clone();
        let added = m.add(copy);
        let (mut corners, mut tees) = (0, 0);
        for f in m.iter() {
            corners += bim_core::system::corners(&m, f).iter().flatten().count();
            tees += bim_core::system::tees(&m, f).ends.iter().flatten().count();
        }
        println!("{corners:2} corner ends, {tees:2} T ends after {name} {:?}", added.err());
    }
}
