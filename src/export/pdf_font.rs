//! WinAnsi TrueType resources shared by the report and drawing PDF writers.
use std::io::Write as _;

/// TrueType bytes and WinAnsi widths in thousandths of an em.
pub(super) struct Font {
    file: &'static [u8],
    pub(super) widths: [u16; 256],
    ascent: i16,
    descent: i16,
    cap: i16,
    bbox: [i16; 4],
}

/// The Unicode character of a WinAnsi code.
pub(super) fn win_ansi_char(code: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
        0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
        0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
    ];
    match code {
        0x80..=0x9F => char::from_u32(u32::from(HIGH[usize::from(code - 0x80)])).unwrap_or('?'),
        _ => char::from(code),
    }
}

pub(super) fn font(file: &'static [u8]) -> Font {
    let mut f = Font {
        file,
        widths: [500; 256],
        ascent: 969,
        descent: -242,
        cap: 727,
        bbox: [-200, -250, 1200, 1000],
    };
    if let Ok(face) = ttf_parser::Face::parse(file, 0) {
        let units = face.units_per_em().max(1);
        let scale = |v: i32| (v * 1000 / i32::from(units)) as i16;

        f.ascent = scale(face.ascender().into());
        f.descent = scale(face.descender().into());
        f.cap = face
            .capital_height()
            .or_else(|| {
                face.glyph_bounding_box(face.glyph_index('H')?)
                    .map(|b| b.y_max)
            })
            .map_or(727, |v| scale(v.into()));
        let b = face.global_bounding_box();
        f.bbox = [
            scale(b.x_min.into()),
            scale(b.y_min.into()),
            scale(b.x_max.into()),
            scale(b.y_max.into()),
        ];
        let missing = face
            .glyph_index('?')
            .and_then(|g| face.glyph_hor_advance(g))
            .unwrap_or(units / 2);
        for code in 32..=255u8 {
            let advance = face
                .glyph_index(win_ansi_char(code))
                .and_then(|g| face.glyph_hor_advance(g))
                .unwrap_or(missing);
            f.widths[usize::from(code)] = (u32::from(advance) * 1000 / u32::from(units)) as u16;
        }
    }
    f
}

pub(super) fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = z.write_all(bytes);
    z.finish().unwrap_or_default()
}

pub(super) fn push_font(objects: &mut Vec<Vec<u8>>, f: &Font, name: &str, stem: u16) {
    let base = objects.len() + 1;
    let widths: Vec<String> = (32..=255).map(|c| f.widths[c].to_string()).collect();
    objects.push(format!("<< /Type /Font /Subtype /TrueType /BaseFont /{name} /FirstChar 32 /LastChar 255 /Widths [{}] /Encoding /WinAnsiEncoding /FontDescriptor {} 0 R >>", widths.join(" "), base + 1).into_bytes());
    objects.push(
        format!(
        "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV {} /FontFile2 {} 0 R >>",
        f.bbox[0], f.bbox[1], f.bbox[2], f.bbox[3], f.ascent, f.descent, f.cap, stem, base + 2
        )
        .into_bytes(),
    );
    let packed = deflate(f.file);
    let mut stream = format!(
        "<< /Length {} /Length1 {} /Filter /FlateDecode >>\nstream\n",
        packed.len(),
        f.file.len()
    )
    .into_bytes();
    stream.extend_from_slice(&packed);
    stream.extend_from_slice(b"\nendstream");
    objects.push(stream);
}
