//! The office pile renvooi in paper millimetres. Model levels remain project coordinates.
use super::{Group, Row, metres, mm};
use crate::document::{Align, Fill, Note, PlanView};
use crate::typeface::Face;
use glam::DVec2;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Bind the known sheet layout only after finding its title, diagram and complete eight-column table.
/// Other editable instructions remain ordinary view notes at their authored positions.
pub(super) fn adopt(plan: &mut PlanView) -> bool {
    let texts: Vec<_> = plan.notes.iter().map(|n| words(&n.text)).collect();
    let index = |s: &str| texts.iter().position(|n| n.eq_ignore_ascii_case(s));
    let (Some(table), Some(datum), Some(symbol)) = (
        texts.iter().position(|s| s.starts_with("PAALSTAAT ")),
        index("PEIL T.O.V. NAP:"),
        index("Symbool"),
    ) else {
        return false;
    };
    if !(datum + 1 < table && table < symbol) {
        return false;
    }
    let headers = [
        "SCHACHT",
        "AANTAL",
        "LENGTE",
        "INHEINIVEAU",
        "O.K.",
        "B.K.",
        "AFHAK",
    ];
    let mut cols = Vec::new();
    for h in headers {
        let Some(i) = (symbol + 1..texts.len()).find(|&i| texts[i].starts_with(h)) else {
            return false;
        };
        cols.push(i);
    }
    if !cols.windows(2).all(|w| w[0] < w[1]) {
        return false;
    }
    let legend = &plan.presentation["legend"];
    let Some(source_rows) = legend["rows"].as_array() else {
        return false;
    };
    let data = cols[6] + 1;
    if texts.len() - data != 7 * source_rows.len() {
        return false;
    }
    let mut specifications = Vec::new();
    for (row, cells) in source_rows.iter().zip(texts[data..].chunks_exact(7)) {
        let mut spec = row.clone();
        spec["foot"] = json!(cells[4]);
        spec["uncut"] = json!(cells[5]);
        spec["cutoffs"] = json!(cells[6]);
        specifications.push(spec);
    }
    let legend = &mut plan.presentation["legend"];
    legend["layout"] = json!("pile-renvooi");
    legend["nap"] = json!(true);
    legend["title"] = json!(texts[0]);
    legend["tableTitle"] = json!(texts[table]);
    legend["datum"] = json!(texts[datum + 1]);
    legend["cutoffLabel"] = json!(texts[cols[6]]);
    legend["footLabel"] = json!(texts[cols[4]]);
    legend["importedSpecifications"] = json!(specifications);
    let mut retained = Vec::new();
    for (i, mut note) in std::mem::take(&mut plan.notes)
        .into_iter()
        .enumerate()
        .take(datum)
        .skip(1)
    {
        let text = &texts[i];
        if i == 1 {
            legend["execution"] = json!(text);
            continue;
        }
        if let Some(v) = text.strip_prefix("Sondeerrapport:") {
            legend["report"] = json!(v.trim());
            continue;
        }
        if let Some(v) = text.strip_prefix("Funderingsadvies:") {
            legend["advice"] = json!(v.trim());
            continue;
        }
        let lower = text.to_lowercase();
        if lower.starts_with("netto modelpaallengte")
            || lower.starts_with("modelpaallengte")
            || lower.starts_with("paalkop ")
            || lower.starts_with("puntniveau ")
        {
            continue;
        }
        if lower.starts_with("prefab betonpaal ") {
            legend["shaftCaption"] = true.into();
            continue;
        }
        if lower.starts_with("paalnummers ") {
            legend["numberSuffix"] =
                json!(text.split_once(';').map_or("", |(_, suffix)| suffix.trim()));
            continue;
        }
        if lower.starts_with("afhakmaat opgegeven:") {
            legend["cutoffCaption"] = true.into();
            continue;
        }
        if lower.starts_with("referentie en richting") || lower.starts_with("b.k. ongesnelde paal:")
        {
            note.face.color = Some([215, 22, 20]);
        }
        retained.push(note);
    }
    plan.notes = retained;
    true
}

struct Diagram {
    left: f64,
    top: f64,
    unit: f64,
    notes: Vec<Note>,
    lines: Vec<[[f64; 2]; 2]>,
}

impl Diagram {
    fn at(&self, x: f64, y: f64) -> [f64; 2] {
        [self.left + x * self.unit, self.top - y * self.unit]
    }
    fn line(&mut self, a: [f64; 2], b: [f64; 2]) {
        self.lines.push([self.at(a[0], a[1]), self.at(b[0], b[1])]);
    }
    fn dash(&mut self, x0: f64, x1: f64, y: f64) {
        let mut x = x0;
        while x < x1 {
            self.line([x, y], [(x + 2.4).min(x1), y]);
            x += 3.6;
        }
    }
    fn text(&mut self, x: f64, y: f64, text: &str, size: f64) -> &mut Note {
        self.notes.push(Note {
            at: self.at(x, y),
            text: text.to_owned(),
            size: size / 1000.0,
            angle: 0.0,
            align: Align::Left,
            level: false,
            face: Face::of("Calibri"),
        });
        self.notes.last_mut().unwrap()
    }
    fn area(&mut self, points: &[[f64; 2]], color: [u8; 3], fills: &mut Vec<Fill>) {
        fills.push(Fill {
            color,
            loops: vec![points.iter().map(|p| self.at(p[0], p[1])).collect()],
        });
        for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
            self.line(a, b);
        }
    }
}

fn display(value: &Value) -> &str {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("—")
}

fn matching_spec(spec: &Value, g: &Group, row: &Row) -> bool {
    spec["type"].as_str() == Some(g.kind.as_str())
        && ["width", "height", "length", "head", "toe"]
            .into_iter()
            .zip([mm(row.size[0]), mm(row.size[1]), g.length, g.head, g.toe])
            .all(|(k, v)| spec[k].as_f64().is_some_and(|n| mm(n) == v))
}

pub(super) fn draw(
    plan: &mut PlanView,
    groups: BTreeMap<Group, Row>,
    ids: Vec<u64>,
    numbers: &[u64],
    style: &bim_library::house::HouseStyle,
) -> (Vec<Note>, Vec<[[f64; 2]; 2]>) {
    let unit = plan.scale / 1000.0;
    let (left, top) = plan.crop.map_or((0.0, 0.0), |(lo, hi)| (lo[0], hi[1]));
    let mut d = Diagram {
        left,
        top,
        unit,
        notes: vec![],
        lines: vec![],
    };
    let l = &mut plan.presentation["legend"];
    let rich = l["layout"] == "pile-renvooi";
    let title = l["title"]
        .as_str()
        .filter(|_| rich)
        .unwrap_or("Renvooi prefab betonpalen");
    let w = 225.0;
    let mut purple = vec![[1., 16.], [1., 10.]];
    let mut teal = vec![[194., 5.], [220., 5.]];
    for k in 0..=8 {
        let a = (k as f64 / 8.0) * std::f64::consts::FRAC_PI_2;
        purple.push([6. - 5. * a.cos(), 10. - 5. * a.sin()]);
        teal.push([220. + 5. * a.sin(), 10. - 5. * a.cos()]);
    }
    purple.extend([[194., 5.], [183., 16.]]);
    teal.extend([[w, 16.], [194., 16.]]);
    d.area(
        &purple,
        style.colour("primary").unwrap_or([49, 19, 52]),
        &mut plan.symbol_fills,
    );
    d.area(
        &teal,
        style.colour("accent").unwrap_or([99, 177, 168]),
        &mut plan.symbol_fills,
    );
    let heading = d.text(5., 12.8, title, 5.3);
    heading.face.bold = true;
    heading.face.color = Some([255; 3]);
    let execution = display(&l["execution"]);
    let face = Face::of("Calibri");
    for (i, line) in face.wrap(execution, 39., 1090.).lines().enumerate() {
        d.text(5.3, 24. + 5.3 * i as f64, line, 3.9);
    }
    for (y, label, key) in [
        (47.6, "Sondeerrapport", "report"),
        (53.2, "Funderingsadvies", "advice"),
    ] {
        let full = format!("{label}: {}", display(&l[key]));
        for (i, line) in face.wrap(&full, 37., 1110.).lines().enumerate() {
            d.text(5.3, y + 4.6 * i as f64, line, 3.7);
        }
    }
    if l["shaftCaption"] == true {
        let caption = if groups.len() == 1 {
            let (g, r) = groups.first_key_value().unwrap();
            format!("Prefab betonpaal {}, {} mm.", g.kind, r.shaft)
        } else {
            "Paalschachten volgens onderstaande paalstaat.".into()
        };
        d.text(5.3, 122.2, &caption, 3.9);
    }
    if let Some(suffix) = l["numberSuffix"].as_str() {
        let mut n = numbers.to_vec();
        n.sort_unstable();
        let range = match (n.first(), n.last()) {
            (Some(first), Some(last)) if last - first + 1 == n.len() as u64 => {
                format!("Paalnummers {first:02}–{last:02}")
            }
            _ => format!("{} paalposities", n.len()),
        };
        let caption = if suffix.is_empty() {
            range
        } else {
            format!("{range}; {suffix}")
        };
        d.text(5.3, 128.0, &caption, 3.9);
    }
    let datum = display(&l["datum"]);
    d.text(125., 142.0, "PEIL T.O.V. NAP:", 4.3);
    d.text(165., 142.0, datum, 4.3).face.bold = true;
    let is_nap = l["nap"] == true;
    if let Some((g, _)) = groups.first_key_value() {
        let suffix = if is_nap { " m NAP" } else { " m" };
        let description = if groups.len() == 1 {
            format!("Netto modelpaallengte: {} m.", metres(g.length, false))
        } else {
            format!("Paalschets {} · {} m", g.kind, metres(g.length, false))
        };
        d.text(120., 36., &description, 3.8);
        let (a, b) = (50., 118.);
        for y in [a, b] {
            d.dash(131., w, y);
            d.line([129., y], [132., y]);
        }
        d.line([131., a - 1.7], [131., b + 1.7]);
        for x in [164., 181.] {
            d.line([x, a], [x, 78.]);
            d.line([x, 92.], [x, b]);
        }
        for y in [78., 92.] {
            d.dash(158., 190., y);
        }
        for (y, text) in [
            (a, format!("paalkop {}{suffix}", metres(g.head, true))),
            (b, format!("puntniveau {}{suffix}", metres(g.toe, true))),
        ] {
            d.text(w - 1., y - 1.2, &text, 3.55).align = Align::Right;
        }
        let measure = d.text(
            129.,
            84.,
            &format!("modelpaallengte {} m", metres(g.length, false)),
            3.25,
        );
        measure.angle = std::f64::consts::FRAC_PI_2;
        measure.align = Align::Center;
    }
    d.line([1., 0.], [1., 157.]);
    d.line([1., 157.], [w, 157.]);
    d.text(
        5.3,
        155.4,
        l["tableTitle"]
            .as_str()
            .unwrap_or("PAALSTAAT PREFAB BETONPAAL"),
        5.8,
    )
    .face
    .bold = true;
    let imported = l
        .as_object_mut()
        .and_then(|o| o.remove("importedSpecifications"))
        .unwrap_or_default();
    let columns = [1., 18.4, 44.4, 59.7, 75.8, 101.8, 134.7, 178.8, w];
    let headings = [
        "Symbool",
        "SCHACHT",
        "AANTAL",
        "LENGTE",
        "INHEINIVEAU",
        l["footLabel"].as_str().unwrap_or("O.K. VOET"),
        "B.K. ONGESNELDE PAAL",
        l["cutoffLabel"].as_str().unwrap_or("AFHAKHOOGTE"),
    ];
    let units = [
        "",
        "[mm]",
        "",
        "[m]",
        if is_nap { "[m t.o.v. NAP]" } else { "[m]" },
        "[mm t.o.v. PEIL]",
        "[mm t.o.v. PEIL]",
        "[mm]",
    ];
    for (i, c) in headings.iter().enumerate() {
        if !units[i].is_empty() {
            d.text(columns[i] + 1.1, 165.0, units[i], 3.0);
        }
        d.text(columns[i] + 1.1, 174.9, c, 2.95);
    }
    let table_end = 181.5 + 13.5 * groups.len() as f64;
    for x in columns {
        d.line([x, 160.8], [x, table_end]);
    }
    for y in [160.8, 176.1, 181.5] {
        d.line([1., y], [w, y]);
    }
    let mut rows = Vec::new();
    let count = groups.len();
    for (i, (g, r)) in groups.into_iter().enumerate() {
        let key = json!([g.kind, g.profile, g.grade, g.length, g.toe, g.head]).to_string();
        if let Some(spec) = imported
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| matching_spec(s, &g, &r))
        {
            for prop in ["foot", "uncut", "cutoffs"] {
                if l[prop][key.as_str()].is_null() {
                    l[prop][key.as_str()] = spec[prop].clone();
                }
            }
        }
        let y = 187.3 + 13.5 * i as f64;
        let centre = DVec2::from(d.at(9.6, y));
        r.symbol.draw(
            centre,
            unit * 2.35 / r.symbol.radius,
            true,
            &mut d.lines,
            &mut plan.symbol_fills,
        );
        if count > 1 {
            d.text(9.6, y + 6.0, &g.kind, 2.1).align = Align::Center;
        }
        let values = [
            r.shaft.clone(),
            r.count.to_string(),
            metres(g.length, false),
            metres(g.toe, false),
            display(&l["foot"][key.as_str()]).into(),
            display(&l["uncut"][key.as_str()]).into(),
            display(&l["cutoffs"][key.as_str()]).into(),
        ];
        for (c, text) in values.iter().enumerate() {
            d.text(columns[c + 1] + 1.1, y, text, 3.45);
        }
        d.line([1., y + 7.7], [w, y + 7.7]);
        if count == 1 && l["cutoffCaption"] == true {
            let amount = values[6].trim_end_matches('*').trim();
            d.text(
                5.3,
                96.3,
                &format!("Afhakmaat opgegeven: {amount} mm."),
                3.9,
            )
            .face
            .color = Some([215, 22, 20]);
        }
        rows.push(json!({"key":key,"type":g.kind,"width":r.size[0],"height":r.size[1],"count":r.count,"length":g.length as f64/1000.,"toe":g.toe as f64/1000.,"head":g.head as f64/1000.}));
    }
    let bottom = top - table_end * unit;
    if !rich {
        if let Some(previous) = l["bodyBottom"].as_f64() {
            for note in &mut plan.notes {
                note.at[1] += bottom - previous;
            }
        } else if let Some(y) = plan.notes.iter().map(|n| n.at[1]).max_by(f64::total_cmp) {
            let shift = (bottom - 6. * unit - y).min(0.);
            for note in &mut plan.notes {
                note.at[1] += shift;
            }
        }
    }
    let foot = plan
        .notes
        .iter()
        .fold(bottom, |min, n| min.min(n.at[1] - n.size * plan.scale));
    plan.crop = Some(([left, foot - 2. * unit], [left + (w + 3.) * unit, top]));
    l["bodyBottom"] = json!(bottom);
    l["rows"] = json!(rows);
    l["elementIds"] = json!(ids);
    (d.notes, d.lines)
}
