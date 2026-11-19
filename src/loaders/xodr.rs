//! Adapter from the OpenDRIVE reader (crate xodr-read) to the app's scene model.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct XodrLoader;

impl Loader for XodrLoader {
    fn name(&self) -> &'static str { "OpenDRIVE" }
    fn extensions(&self) -> &'static [&'static str] { &["xodr"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let net = xodr_read::read(&String::from_utf8_lossy(&data))?;
        let props = |props: Vec<(String, String)>| serde_json::Value::Object(props.into_iter().map(|(k, v)| (k, v.into())).collect()).to_string();
        let mut layers: Vec<Layer> = net.surfaces.into_iter().map(|s| Layer {
            name: s.lane_type, color: s.color, positions: s.positions, normals: s.normals, indices: s.indices, elements: s.elements,
            props: s.props.into_iter().map(|(id, p)| (id, props(p))).collect(), ..Layer::default()
        }).collect();
        let mut lines = Layer { name: "reference lines".into(), color: [0.95, 0.80, 0.20], ..Layer::default() };
        for (_, points) in &net.reference_lines {
            let first = (lines.positions.len() / 3) as u32;
            for p in points {
                lines.positions.extend_from_slice(p);
                lines.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
            }
            for k in 1..points.len() as u32 {
                lines.lines.extend([first + k - 1, first + k]);
            }
        }
        if !lines.lines.is_empty() {
            layers.push(lines);
        }
        let mut info = net.info;
        if let Some(crs) = net.geo_reference {
            info.push(("Coordinate reference system".into(), crs));
        }
        info.push(("OpenDRIVE world origin".into(), format!("{:.0}, {:.0}, {:.0}", net.origin[0], net.origin[1], net.origin[2])));
        if !net.skipped.is_empty() {
            info.push(("Roads left out".into(), net.skipped.join("; ")));
        }
        let origin = Some(glam::DVec3::from_array(net.origin));
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() }, structural: Vec::new(), origin })
    }
}
