//! Adapter from the 3MF reader (crate threemf-read, repository 3mf-dll) to the app's scene model.
use super::{Loader, Source};
use crate::model::Layer;

pub struct ThreeMfLoader;

impl Loader for ThreeMfLoader {
    fn name(&self) -> &'static str { "3MF" }
    fn extensions(&self) -> &'static [&'static str] { &["3mf"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let items = threemf_read::read(&source.read()?)?;
        Ok(items.into_iter().enumerate().map(|(n, item)| {
            let id = n as u64 + 1;
            let props = serde_json::Value::Object(item.props.into_iter().map(|(k, v)| (k, v.into())).collect()).to_string();
            let count = item.positions.len() as u32 / 3;
            // the slice contours follow the triangles as line segments
            let first = count;
            let mut positions = item.positions;
            let mut normals = item.normals;
            positions.extend_from_slice(&item.lines);
            normals.extend(std::iter::repeat_n([0.0, 0.0, 1.0], item.lines.len() / 3).flatten());
            let colors = match (item.colors, item.line_colors) {
                (None, None) => None,
                (colors, lines) => Some(colors.unwrap_or_default().into_iter().chain(lines.unwrap_or_default()).collect()),
            };
            let lines: Vec<u32> = (first..first + item.lines.len() as u32 / 3).collect();
            Layer { name: item.name, color: item.color, positions, normals, colors, indices: (0..count).collect(), elements: vec![(id, 0, count)], props: vec![(id, props)], lines, ..Layer::default() }
        }).collect())
    }
}
