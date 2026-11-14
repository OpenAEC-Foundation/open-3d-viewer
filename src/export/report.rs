//! A report as a PDF of A4 pages in the house style: a cover (the millimetre grid, the logo, the
//! title, a picture of the model and what the report is about), then pages that each carry the
//! logo and the title at the top over an amber line, and the page's number at the foot. The
//! pages hold headings, paragraphs, tiles of counts, pictures and tables, laid out from the top
//! down and onto the next page when one is full. A table's rows wrap their text in their
//! columns; its headings come back at the top of each page it runs onto; a row can be marked
//! with the colour of how it came out.
//!
//! The text is set in Inter, the house font, embedded (WinAnsi: a character outside it is `?`).
//! Pictures go in as JPEG as they are; the logo with its transparency.

use super::pdf::{literal, num_to, utf16};
use std::io::Write as _;
use std::sync::OnceLock;

/// How a row came out, as its mark shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Pass,
    Warning,
    Fail,
    Neutral,
}

impl Mark {
    fn rgb(self) -> &'static str {
        match self {
            Mark::Pass => "0.118 0.478 0.235",
            Mark::Warning => ACCENT,
            Mark::Fail => "0.702 0.149 0.118",
            Mark::Neutral => "0.6 0.62 0.65",
        }
    }
}

/// A row of a table: its cells, and a mark at its start.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cells: Vec<String>,
    pub mark: Option<Mark>,
}

/// A picture as a JPEG file and its size in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Jpeg {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The cover: the title, what the report is about, the lines of what it covers (label and
/// value), and a picture of the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Cover {
    pub title: String,
    pub subtitle: String,
    pub facts: Vec<(String, String)>,
    pub picture: Option<Jpeg>,
}

/// What a report holds, from the top down.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading(String),
    /// A smaller heading.
    Subheading(String),
    Text(String),
    /// Text in grey and smaller: who, when, how it stands.
    Note(String),
    /// Tiles of counts: each a number and what it counts, marked.
    Counts(Vec<(String, String, Mark)>),
    /// A picture as wide as the page (at most as high as `max_height` points), its caption under it.
    Picture { picture: Jpeg, caption: String, max_height: f64 },
    /// A table: its columns' shares of the width, its headings and its rows.
    Table { widths: Vec<f64>, headings: Vec<String>, rows: Vec<Row> },
    /// What follows starts a page.
    PageBreak,
}

const PAGE: (f64, f64) = (595.28, 841.89);
const LEFT: f64 = 48.0;
const TOP: f64 = 74.0;
const BOTTOM: f64 = 60.0;
const WIDTH: f64 = PAGE.0 - 2.0 * LEFT;
/// The amber of the house style.
const ACCENT: &str = "0.851 0.467 0.024";
/// Points per millimetre.
const MM: f64 = 72.0 / 25.4;

/// The house font, regular and semibold: its file and the widths of the WinAnsi characters
/// (thousandths of an em).
struct Font {
    file: &'static [u8],
    widths: [u16; 256],
    ascent: i16,
    descent: i16,
    cap: i16,
    bbox: [i16; 4],
    units: u16,
}

/// The Unicode character of a WinAnsi code.
fn win_ansi_char(code: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161,
        0x203A, 0x0153, 0, 0x017E, 0x0178,
    ];
    match code {
        0x80..=0x9F => char::from_u32(u32::from(HIGH[usize::from(code - 0x80)])).unwrap_or('?'),
        _ => char::from(code),
    }
}

fn font(file: &'static [u8]) -> Font {
    let mut f = Font { file, widths: [500; 256], ascent: 969, descent: -242, cap: 727, bbox: [-200, -250, 1200, 1000], units: 1000 };
    if let Ok(face) = ttf_parser::Face::parse(file, 0) {
        let units = face.units_per_em().max(1);
        let scale = |v: i32| (v * 1000 / i32::from(units)) as i16;
        f.units = units;
        f.ascent = scale(face.ascender().into());
        f.descent = scale(face.descender().into());
        f.cap = scale(face.capital_height().unwrap_or(727).into());
        let b = face.global_bounding_box();
        f.bbox = [scale(b.x_min.into()), scale(b.y_min.into()), scale(b.x_max.into()), scale(b.y_max.into())];
        let missing = face.glyph_index('?').and_then(|g| face.glyph_hor_advance(g)).unwrap_or(units / 2);
        for code in 32..=255u8 {
            let advance = face.glyph_index(win_ansi_char(code)).and_then(|g| face.glyph_hor_advance(g)).unwrap_or(missing);
            f.widths[usize::from(code)] = (u32::from(advance) * 1000 / u32::from(units)) as u16;
        }
    }
    f
}

fn fonts() -> &'static [Font; 2] {
    static FONTS: OnceLock<[Font; 2]> = OnceLock::new();
    FONTS.get_or_init(|| [font(crate::INTER_REGULAR), font(crate::INTER_SEMIBOLD)])
}

/// The WinAnsi code of a character, as the text is written; `?` for one it lacks.
fn code(c: char) -> u8 {
    (32..=255u8).find(|&b| win_ansi_char(b) == c).unwrap_or(b'?')
}

/// How wide `text` is at `size` points.
fn measure(text: &str, size: f64, bold: bool) -> f64 {
    let widths = &fonts()[usize::from(bold)].widths;
    text.chars().map(|c| f64::from(widths[usize::from(code(c))])).sum::<f64>() / 1000.0 * size
}

/// The lines `text` takes in `room` points at `size`: broken between words, a word too long
/// for a line broken where it must.
fn wrap(text: &str, size: f64, bold: bool, room: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let tried = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if measure(&tried, size, bold) <= room || line.is_empty() && measure(word, size, bold) <= room {
                line = tried;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // a word longer than the line: as much of it as fits on each
            let mut rest = word.to_string();
            while measure(&rest, size, bold) > room && rest.chars().count() > 1 {
                let mut cut = rest.chars().count() - 1;
                while cut > 1 && measure(&rest.chars().take(cut).collect::<String>(), size, bold) > room {
                    cut -= 1;
                }
                lines.push(rest.chars().take(cut).collect());
                rest = rest.chars().skip(cut).collect();
            }
            line = rest;
        }
        lines.push(line);
    }
    lines
}

/// The pages as they are laid out, and the pictures they show (by their place in `pictures`).
struct Pages {
    done: Vec<String>,
    page: String,
    /// Where the next line goes: points down from the top of the page.
    y: f64,
    pictures: Vec<Jpeg>,
}

impl Pages {
    /// Room for `height` points more, on this page or the next; true when a page was begun.
    fn room(&mut self, height: f64) -> bool {
        if self.y + height > PAGE.1 - BOTTOM && self.y > TOP + 1.0 {
            self.next();
            return true;
        }
        false
    }

    fn next(&mut self) {
        self.done.push(std::mem::take(&mut self.page));
        self.y = TOP;
    }

    fn text_at(&mut self, x: f64, baseline: f64, size: f64, bold: bool, gray: &str, text: &str) {
        use std::fmt::Write;
        let _ = writeln!(self.page, "BT {gray} rg /F{} {} Tf {} {} Td {} Tj ET", if bold { 2 } else { 1 }, num_to(size, 2), num_to(x, 2), num_to(baseline, 2), literal(text));
    }

    fn text(&mut self, x: f64, size: f64, bold: bool, gray: &str, text: &str) {
        let baseline = PAGE.1 - self.y - size * 0.82;
        self.text_at(x, baseline, size, bold, gray, text);
    }

    fn fill(&mut self, x: f64, height: f64, w: f64, rgb: &str) {
        use std::fmt::Write;
        let _ = writeln!(self.page, "{rgb} rg {} {} {} {} re f", num_to(x, 2), num_to(PAGE.1 - self.y - height, 2), num_to(w, 2), num_to(height, 2));
    }

    fn picture(&mut self, picture: &Jpeg, x: f64, w: f64, h: f64) {
        use std::fmt::Write;
        self.pictures.push(picture.clone());
        let n = self.pictures.len();
        let _ = writeln!(self.page, "q {} 0 0 {} {} {} cm /Im{n} Do Q", num_to(w, 2), num_to(h, 2), num_to(x, 2), num_to(PAGE.1 - self.y - h, 2));
        // a hairline round it
        let _ = writeln!(self.page, "0.8 0.82 0.85 RG 0.5 w {} {} {} {} re S", num_to(x, 2), num_to(PAGE.1 - self.y - h, 2), num_to(w, 2), num_to(h, 2));
    }

    fn lines(&mut self, lines: &[String], size: f64, bold: bool, gray: &str) {
        for line in lines {
            self.room(size * 1.4);
            self.text(LEFT, size, bold, gray, line);
            self.y += size * 1.4;
        }
    }
}

const INK: &str = "0.09 0.1 0.11";
const MUTED: &str = "0.4 0.43 0.47";

/// A table's rows as lines per cell, the headings first and again at the top of each page.
fn table(p: &mut Pages, widths: &[f64], headings: &[String], rows: &[Row]) {
    const SIZE: f64 = 8.0;
    const LINE: f64 = SIZE * 1.35;
    const PAD: f64 = 4.0;
    const MARK: f64 = 5.0;
    let total: f64 = widths.iter().sum::<f64>().max(1e-9);
    let cols: Vec<f64> = widths.iter().map(|w| w / total * (WIDTH - MARK)).collect();
    let xs: Vec<f64> = cols
        .iter()
        .scan(LEFT + MARK, |x, w| {
            let at = *x;
            *x += w;
            Some(at)
        })
        .collect();
    let cells = |texts: &[String], bold: bool| -> Vec<Vec<String>> { cols.iter().enumerate().map(|(k, w)| wrap(texts.get(k).map(String::as_str).unwrap_or(""), SIZE, bold, w - 2.0 * PAD)).collect() };
    let head = cells(headings, true);
    let head_height = head.iter().map(Vec::len).max().unwrap_or(1) as f64 * LINE + 2.0 * PAD;
    let draw_head = |p: &mut Pages| {
        p.fill(LEFT, head_height, WIDTH, "0.11 0.12 0.13");
        let top = p.y;
        for (k, lines) in head.iter().enumerate() {
            p.y = top + PAD;
            for line in lines {
                p.text(xs[k] + PAD, SIZE, true, "1 1 1", line);
                p.y += LINE;
            }
        }
        p.y = top + head_height;
    };
    p.room(head_height + LINE + 2.0 * PAD);
    draw_head(p);
    for (n, row) in rows.iter().enumerate() {
        let wrapped = cells(&row.cells, false);
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1) as f64 * LINE + 2.0 * PAD;
        if p.room(height) {
            draw_head(p);
        }
        if n % 2 == 1 {
            p.fill(LEFT, height, WIDTH, "0.965 0.968 0.972");
        }
        if let Some(mark) = row.mark {
            p.fill(LEFT, height, MARK - 1.5, mark.rgb());
        }
        let top = p.y;
        for (k, lines) in wrapped.iter().enumerate() {
            p.y = top + PAD;
            for line in lines {
                p.text(xs[k] + PAD, SIZE, false, INK, line);
                p.y += LINE;
            }
        }
        p.y = top + height;
    }
    p.y += 10.0;
}

/// The cover: the millimetre grid over the page, the logo and the name of the suite, the title
/// over an amber bar, the picture of the model, and what the report covers.
fn cover(p: &mut Pages, c: &Cover, logo: bool) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    // the millimetre grid: every millimetre faint, every centimetre a little stronger
    let (w, h) = PAGE;
    for (step, gray, weight) in [(1.0, "0.93", 0.12), (10.0, "0.82", 0.3)] {
        let _ = writeln!(s, "{gray} G {weight} w");
        let mut x = 0.0;
        while x <= w {
            let _ = writeln!(s, "{} 0 m {} {} l", num_to(x, 2), num_to(x, 2), num_to(h, 2));
            x += step * MM;
        }
        let mut y = 0.0;
        while y <= h {
            let _ = writeln!(s, "0 {} m {} {} l", num_to(y, 2), num_to(w, 2), num_to(y, 2));
            y += step * MM;
        }
        s.push_str("S\n");
    }
    p.page = s;
    // the logo and the suite
    if logo {
        let _ = writeln!(p.page, "q 34 0 0 34 {} {} cm /Logo Do Q", num_to(LEFT, 2), num_to(h - 48.0 - 34.0, 2));
    }
    p.text_at(LEFT + (if logo { 44.0 } else { 0.0 }), h - 48.0 - 22.0, 13.0, true, INK, "Open BIM Suite");
    // the title over an amber bar, white behind it so the grid does not run through
    p.y = 150.0;
    let title = wrap(&c.title, 30.0, true, WIDTH - 20.0);
    let sub = wrap(&c.subtitle, 12.0, false, WIDTH - 20.0);
    let block = title.len() as f64 * 38.0 + sub.len() as f64 * 17.0 + 26.0;
    p.fill(LEFT - 10.0, block, WIDTH + 20.0, "1 1 1");
    p.fill(LEFT - 10.0, block, 5.0, ACCENT);
    p.y += 8.0;
    for line in &title {
        p.text(LEFT + 6.0, 30.0, true, INK, line);
        p.y += 38.0;
    }
    for line in &sub {
        p.text(LEFT + 6.0, 12.0, false, MUTED, line);
        p.y += 17.0;
    }
    p.y = 150.0 + block + 24.0;
    // the picture of the model, as large as the page lets it be
    if let Some(picture) = &c.picture {
        let room_h = 330.0;
        let ratio = f64::from(picture.height.max(1)) / f64::from(picture.width.max(1));
        let (pw, ph) = if WIDTH * ratio <= room_h { (WIDTH, WIDTH * ratio) } else { (room_h / ratio, room_h) };
        let x = LEFT + (WIDTH - pw) / 2.0;
        p.fill(x - 6.0, ph + 12.0, pw + 12.0, "1 1 1");
        p.y += 6.0;
        p.picture(picture, x, pw, ph);
        p.y += ph + 30.0;
    }
    // what it covers
    let facts_h = c.facts.len() as f64 * 18.0 + 16.0;
    p.y = p.y.max(PAGE.1 - 70.0 - facts_h);
    p.fill(LEFT - 10.0, facts_h, WIDTH + 20.0, "1 1 1");
    p.y += 8.0;
    for (label, value) in &c.facts {
        p.text(LEFT + 6.0, 9.0, false, MUTED, label);
        let lines = wrap(value, 9.5, true, WIDTH - 140.0);
        p.text(LEFT + 130.0, 9.5, true, INK, lines.first().map(String::as_str).unwrap_or(""));
        p.y += 18.0;
    }
    std::mem::take(&mut p.page)
}

/// Compresses with zlib, as a PDF's FlateDecode reads it.
fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = z.write_all(bytes);
    z.finish().unwrap_or_default()
}

/// The logo: its SVG drawn 256 pixels wide, as RGB and its transparency.
fn logo() -> Option<(u32, u32, Vec<u8>, Vec<u8>)> {
    use resvg::{tiny_skia, usvg};
    const SVG: &str = include_str!("../../assets/logo.svg");
    const SIZE: u32 = 256;
    let tree = usvg::Tree::from_str(SVG, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(SIZE, SIZE)?;
    let size = tree.size();
    resvg::render(&tree, tiny_skia::Transform::from_scale(SIZE as f32 / size.width(), SIZE as f32 / size.height()), &mut pixmap.as_mut());
    let (mut rgb, mut alpha) = (Vec::with_capacity((SIZE * SIZE * 3) as usize), Vec::with_capacity((SIZE * SIZE) as usize));
    // the pixels come with their colour multiplied by their transparency
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgb.extend_from_slice(&[c.red(), c.green(), c.blue()]);
        alpha.push(c.alpha());
    }
    Some((SIZE, SIZE, rgb, alpha))
}

/// The report as a PDF file: `title` heads each page after the cover.
pub fn report_pdf(cover_page: &Cover, title: &str, date: &str, blocks: &[Block]) -> Vec<u8> {
    use std::fmt::Write;
    let logo = logo();
    let mut p = Pages { done: Vec::new(), page: String::new(), y: TOP, pictures: Vec::new() };
    let first = cover(&mut p, cover_page, logo.is_some());
    p.done.push(first);
    p.y = TOP;
    for block in blocks {
        match block {
            Block::Heading(text) => {
                p.y += 8.0;
                p.room(16.0 * 1.4 + 50.0);
                let top = p.y;
                p.fill(LEFT, 13.0, 4.0, ACCENT);
                p.y = top - 1.0;
                for line in wrap(text, 15.0, true, WIDTH - 12.0) {
                    p.text(LEFT + 11.0, 15.0, true, INK, &line);
                    p.y += 15.0 * 1.4;
                }
                p.y += 4.0;
            }
            Block::Subheading(text) => {
                p.y += 6.0;
                p.room(11.0 * 1.4 + 40.0);
                p.lines(&wrap(text, 11.0, true, WIDTH), 11.0, true, INK);
                p.y += 2.0;
            }
            Block::Text(text) => {
                p.lines(&wrap(text, 9.5, false, WIDTH), 9.5, false, INK);
                p.y += 4.0;
            }
            Block::Note(text) => {
                p.lines(&wrap(text, 8.5, false, WIDTH), 8.5, false, MUTED);
                p.y += 3.0;
            }
            Block::Counts(tiles) => {
                let n = tiles.len().max(1) as f64;
                let w = (WIDTH - 8.0 * (n - 1.0)) / n;
                p.room(50.0);
                let top = p.y;
                for (k, (number, what, mark)) in tiles.iter().enumerate() {
                    let x = LEFT + k as f64 * (w + 8.0);
                    p.y = top;
                    p.fill(x, 46.0, w, "0.955 0.958 0.962");
                    p.fill(x, 46.0, 3.0, mark.rgb());
                    p.y = top + 7.0;
                    p.text(x + 11.0, 17.0, true, INK, number);
                    p.y = top + 29.0;
                    p.text(x + 11.0, 8.0, false, MUTED, what);
                }
                p.y = top + 58.0;
            }
            Block::Picture { picture, caption, max_height } => {
                let ratio = f64::from(picture.height.max(1)) / f64::from(picture.width.max(1));
                let (w, h) = if WIDTH * ratio <= *max_height { (WIDTH, WIDTH * ratio) } else { (max_height / ratio, *max_height) };
                p.room(h + 20.0);
                p.picture(picture, LEFT, w, h);
                p.y += h + 3.0;
                if !caption.is_empty() {
                    p.lines(&wrap(caption, 8.0, false, WIDTH), 8.0, false, MUTED);
                }
                p.y += 6.0;
            }
            Block::Table { widths, headings, rows } => table(&mut p, widths, headings, rows),
            Block::PageBreak => {
                if p.y > TOP + 1.0 {
                    p.next();
                }
            }
        }
    }
    if !p.page.is_empty() {
        let last = std::mem::take(&mut p.page);
        p.done.push(last);
    }
    let count = p.done.len();
    // each page after the cover: the logo and the title at the top over the amber line, the date
    // at the right; at the foot the page's number
    let streams: Vec<String> = p
        .done
        .iter()
        .enumerate()
        .map(|(k, body)| {
            let mut s = String::new();
            if k > 0 {
                let top = PAGE.1 - 40.0;
                if logo.is_some() {
                    let _ = writeln!(s, "q 16 0 0 16 {} {} cm /Logo Do Q", num_to(LEFT, 2), num_to(top - 3.0, 2));
                }
                let x = LEFT + if logo.is_some() { 22.0 } else { 0.0 };
                let _ = writeln!(s, "BT {INK} rg /F2 9 Tf {} {} Td {} Tj ET", num_to(x, 2), num_to(top + 2.0, 2), literal(title));
                let _ = writeln!(s, "BT {MUTED} rg /F1 8 Tf {} {} Td {} Tj ET", num_to(PAGE.0 - LEFT - measure(date, 8.0, false), 2), num_to(top + 2.0, 2), literal(date));
                let _ = writeln!(s, "{ACCENT} rg {} {} {} 1.5 re f", num_to(LEFT, 2), num_to(top - 9.0, 2), num_to(WIDTH, 2));
            }
            s.push_str(body);
            let foot = format!("{} / {count}", k + 1);
            let _ = writeln!(s, "BT {MUTED} rg /F1 8 Tf {} 30 Td {} Tj ET", num_to(PAGE.0 - LEFT - measure(&foot, 8.0, false), 2), literal(&foot));
            if k > 0 {
                let _ = writeln!(s, "BT {MUTED} rg /F1 8 Tf {} 30 Td (Open BIM Suite) Tj ET", num_to(LEFT, 2));
            }
            s
        })
        .collect();
    // the objects: 1 catalog, 2 pages, 3–8 the two fonts (font, descriptor, file each),
    // 9 information, 10–11 the logo and its transparency, then the pictures, then each page and
    // its contents
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let pictures_from = 12;
    let pages_from = pictures_from + p.pictures.len();
    let kids: Vec<String> = (0..count).map(|k| format!("{} 0 R", pages_from + 2 * k)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {count} >>", kids.join(" ")).into_bytes());
    for (k, (f, name)) in fonts().iter().zip(["Inter-Regular", "Inter-SemiBold"]).enumerate() {
        let base = 3 + 3 * k;
        let widths: Vec<String> = (32..=255).map(|c| f.widths[c].to_string()).collect();
        objects.push(format!("<< /Type /Font /Subtype /TrueType /BaseFont /{name} /FirstChar 32 /LastChar 255 /Widths [{}] /Encoding /WinAnsiEncoding /FontDescriptor {} 0 R >>", widths.join(" "), base + 1).into_bytes());
        objects.push(
            format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV {} /FontFile2 {} 0 R >>",
                f.bbox[0], f.bbox[1], f.bbox[2], f.bbox[3], f.ascent, f.descent, f.cap, if k == 0 { 80 } else { 120 }, base + 2
            )
            .into_bytes(),
        );
        let packed = deflate(f.file);
        let mut stream = format!("<< /Length {} /Length1 {} /Filter /FlateDecode >>\nstream\n", packed.len(), f.file.len()).into_bytes();
        stream.extend_from_slice(&packed);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
        let _ = f.units;
    }
    objects.push(format!("<< /Title {} /Producer (Open BIM Suite) >>", utf16(title)).into_bytes());
    let image = |w: u32, h: u32, color: &str, filter: &str, extra: &str, bytes: &[u8]| -> Vec<u8> {
        let mut o = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {color} /BitsPerComponent 8 /Filter {filter}{extra} /Length {} >>\nstream\n", bytes.len()).into_bytes();
        o.extend_from_slice(bytes);
        o.extend_from_slice(b"\nendstream");
        o
    };
    match &logo {
        Some((w, h, rgb, alpha)) => {
            objects.push(image(*w, *h, "/DeviceRGB", "/FlateDecode", " /SMask 11 0 R", &deflate(rgb)));
            objects.push(image(*w, *h, "/DeviceGray", "/FlateDecode", "", &deflate(alpha)));
        }
        None => {
            objects.push(image(1, 1, "/DeviceRGB", "/FlateDecode", "", &deflate(&[255, 255, 255])));
            objects.push(image(1, 1, "/DeviceGray", "/FlateDecode", "", &deflate(&[255])));
        }
    }
    for picture in &p.pictures {
        objects.push(image(picture.width, picture.height, "/DeviceRGB", "/DCTDecode", "", &picture.bytes));
    }
    let xobjects: String = std::iter::once("/Logo 10 0 R".to_string()).chain((0..p.pictures.len()).map(|k| format!("/Im{} {} 0 R", k + 1, pictures_from + k))).collect::<Vec<_>>().join(" ");
    for (k, stream) in streams.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {} 0 R /Resources << /Font << /F1 3 0 R /F2 6 0 R >> /XObject << {xobjects} >> >> >>",
                num_to(PAGE.0, 2),
                num_to(PAGE.1, 2),
                pages_from + 2 * k + 1
            )
            .into_bytes(),
        );
        let packed = deflate(stream.as_bytes());
        let mut o = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
        o.extend_from_slice(&packed);
        o.extend_from_slice(b"\nendstream");
        objects.push(o);
    }
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = write!(tail, "{offset:010} 00000 n \n");
    }
    let _ = write!(tail, "trailer\n<< /Size {} /Root 1 0 R /Info 9 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.extend_from_slice(tail.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inflate(bytes: &[u8]) -> String {
        use std::io::Read;
        let mut s = String::new();
        let mut z = flate2::read::ZlibDecoder::new(bytes);
        let mut raw = Vec::new();
        let _ = z.read_to_end(&mut raw);
        s.extend(raw.iter().map(|&b| b as char));
        s
    }

    /// The page contents of a PDF this module wrote, unpacked.
    fn contents(pdf: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = 0;
        let marker = b"/Filter /FlateDecode >>\nstream\n";
        while let Some(k) = pdf[at..].windows(marker.len()).position(|w| w == marker) {
            let start = at + k + marker.len();
            let end = start + pdf[start..].windows(10).position(|w| w == b"\nendstream").unwrap_or(0);
            out.push(inflate(&pdf[start..end]));
            at = end;
        }
        out
    }

    fn cover() -> Cover {
        Cover { title: "Controle- en clashrapport".into(), subtitle: "2044 Beatrixschool".into(), facts: vec![("Datum".into(), "2026-11-14".into())], picture: Some(Jpeg { bytes: vec![0xFF, 0xD8, 0xFF, 0xD9], width: 400, height: 300 }) }
    }

    #[test]
    fn a_long_table_runs_onto_more_pages_and_each_page_is_numbered() {
        let rows: Vec<Row> = (0..200).map(|k| Row { cells: vec![format!("Wand {k}"), "De deur is 800 mm breed, minder dan 850 mm".into()], mark: Some(if k % 2 == 0 { Mark::Fail } else { Mark::Pass }) }).collect();
        let blocks = [
            Block::Counts(vec![("5".into(), "geslaagd".into(), Mark::Pass), ("2".into(), "waarschuwingen".into(), Mark::Warning)]),
            Block::Heading("Bevindingen".into()),
            Block::Table { widths: vec![1.0, 3.0], headings: vec!["Element".into(), "Waarom".into()], rows },
        ];
        let bytes = report_pdf(&cover(), "Controlerapport", "2026-11-14", &blocks);
        let s: String = bytes.iter().map(|&b| b as char).collect();
        assert!(s.starts_with("%PDF-1.4") && s.trim_end().ends_with("%%EOF"));
        let pages = s.matches("/Type /Page /Parent").count();
        assert!(pages >= 4, "{pages} pages");
        assert!(s.contains(&format!("/Count {pages}")));
        let pages_text = contents(&bytes);
        // the cover has the grid, the title and the picture; the last page its number
        assert!(pages_text.iter().any(|c| c.contains("(Controle- en clashrapport)") && c.contains("/Im1 Do")), "the cover");
        assert!(pages_text.iter().any(|c| c.contains(&format!("({pages} / {pages})"))), "the last page's foot");
        // the headings come back on each page the table runs onto
        assert!(pages_text.iter().filter(|c| c.contains("(Waarom)")).count() >= pages - 2);
        // the house font is in it
        assert!(s.contains("/BaseFont /Inter-Regular") && s.contains("/FontFile2"));
    }

    #[test]
    fn text_wraps_between_words_and_a_long_word_where_it_must() {
        let lines = wrap("een twee drie vier vijf zes zeven acht", 10.0, false, 60.0);
        assert!(lines.len() > 2 && lines.iter().all(|l| measure(l, 10.0, false) <= 60.0), "{lines:?}");
        let long = wrap("Basic Wall:NLRS_21_WA_CLT_100_zonder_spaties_heel_lang", 10.0, false, 80.0);
        assert!(long.len() > 1 && long.iter().all(|l| measure(l, 10.0, false) <= 80.0 + 1e-6), "{long:?}");
        assert_eq!(long.concat(), "Basic Wall:NLRS_21_WA_CLT_100_zonder_spaties_heel_lang".replace(' ', ""));
    }

    #[test]
    fn the_house_font_gives_its_own_widths() {
        assert!(measure("W", 10.0, false) > measure("i", 10.0, false));
        assert!(measure("Wand", 10.0, true) >= measure("Wand", 10.0, false));
        assert_eq!(code('€'), 0x80);
        assert_eq!(code('é'), 0xE9);
        assert!(logo().is_some(), "the logo reads out of its SVG");
    }
}
