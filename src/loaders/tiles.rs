//! Adapter from the 3D Tiles reader (crates tiles-read and tiles-model, repository 3d-tiles-dll)
//! to the app's scene model: a `tileset.json`, a folder or zip holding one, a single tile, or
//! quantized-mesh terrain.
use super::placed::Gather;
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{flat_shaded, Layer, ProjectData};
use tiles_model::Kind;

pub struct TilesLoader;

impl Loader for TilesLoader {
    fn name(&self) -> &'static str { "3D Tiles" }
    fn extensions(&self) -> &'static [&'static str] { &["b3dm", "i3dm", "pnts", "cmpt", "terrain", "zip"] }
    fn claims(&self, name: &str) -> bool { name.ends_with("tileset.json") || name == "layer.json" }
    fn opens_folder(&self, folder: &std::path::Path) -> bool { folder.join("tileset.json").is_file() || folder.join("layer.json").is_file() }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let options = tiles_read::Options::default();
        let tileset = match source.path() {
            Some(path) => tiles_read::open_path(path, &options)?,
            None => tiles_read::open_bytes(&source.name(), source.read()?.into_owned(), &options)?,
        };
        progress.set(Phase::Convert);
        let mut gather = Gather::default();
        for content in tileset.contents {
            let tile = [("Tile", content.uri.clone()), ("Geometric error (m)", format!("{}", content.geometric_error))];
            match content.kind {
                Kind::Gltf { glb, placements } => gather.gltf(&super::placed::glb(glb), &placements, &tile)?,
                Kind::Points { positions, colors } => gather.add(Layer { name: "Points".into(), positions: positions.concat(), colors: Some(colors.unwrap_or_else(|| vec![[255; 4]; positions.len()])), points: true, ..Layer::default() }, &[]),
                Kind::Mesh { positions, normals, indices } => {
                    let positions = positions.concat();
                    let (positions, normals, indices) = match normals {
                        Some(n) => (positions, n.concat(), indices),
                        None => flat_shaded(&positions, &indices),
                    };
                    gather.add(Layer { name: "Terrain".into(), color: [0.62, 0.58, 0.5], positions, normals, indices, ..Layer::default() }, &[]);
                }
            }
        }
        let mut info = tileset.info;
        if let Some([lat, lon, h]) = tileset.frame.geodetic {
            info.push(("Georeference latitude, longitude".into(), format!("{lat:.6}, {lon:.6}")));
            info.push(("Georeference height above the ellipsoid (m)".into(), format!("{h:.2}")));
            info.push(("Coordinate reference system".into(), "EPSG:4978 (earth-centred), shown in a local east-north-up frame at the georeference".into()));
        }
        info.push(("Tiles read".into(), format!("{} of {} looked at", tileset.tiles_read, tileset.tiles_visited)));
        if let Some((uri, why)) = tileset.failed.first() {
            info.push(("Tiles not read".into(), format!("{} (first: {uri}: {why})", tileset.failed.len())));
        }
        let project = ProjectData { info, ..ProjectData::default() };
        Ok(Loaded { structural: Vec::new(), origin: None, layers: gather.into_layers(), views: Vec::new(), project })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// A batched (b3dm) tileset from the 3D Tiles samples, placed on the earth.
    #[test]
    fn loads_a_batched_tileset() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/3d-tiles/tileset.json");
        let model = crate::loaders::load(&path).expect("3D Tiles");
        assert!(model.triangle_count() > 0);
        // in the local east-north-up frame: some tens of metres around the origin, standing up
        assert!(model.bounds.max[0] < 200.0 && model.bounds.min[0] > -200.0, "{:?}", model.bounds);
        assert!(model.bounds.max[2] > 1.0, "{:?}", model.bounds);
    }
}
