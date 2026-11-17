//! The open services of PDOK, the geodata platform of the Dutch government: what to ask and how
//! to read the answer. They need no key and no account.
//!
//! - The place search (Locatieserver): an address, street, town or postcode to a point of RD New.
//! - The cadastral map (BRK, the land registry): the parcels in a square.
//! - The register of buildings and addresses (BAG): the buildings in a square.
//!
//! A request tells the service where one is looking; nothing else goes along. The caller sends
//! it: no network here.

use crate::rd::{Frame, Rd};
use serde_json::{json, Map, Value};

const SEARCH: &str = "https://api.pdok.nl/bzk/locatieserver/search/v3_1/free";
const PARCELS: &str = "https://api.pdok.nl/kadaster/brk-kadastrale-kaart/ogc/v1/collections/perceel/items";
const BUILDINGS: &str = "https://api.pdok.nl/kadaster/bag/ogc/v2/collections/pand/items";
/// RD New as the services name it.
const RD_CRS: &str = "http://www.opengis.net/def/crs/EPSG/0/28992";
/// The features a service gives in one page, at most.
pub const PAGE: usize = 1000;

/// A place the search found.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// As the search writes it: "Stadhuisplein 1, 3811LM Amersfoort".
    pub name: String,
    /// What it is, in the search's own word: `adres`, `weg`, `woonplaats`, `postcode`, `gemeente`.
    pub kind: String,
    pub rd: Rd,
}

/// The address of a search for `query`, for the `rows` best places.
pub fn search_url(query: &str, rows: usize) -> String {
    format!("{SEARCH}?q={}&rows={rows}&fl=weergavenaam,type,centroide_rd", encode(query.trim()))
}

/// The places in the answer to a search, the best first. A place without a point is left out.
pub fn parse_search(answer: &str) -> Result<Vec<Place>, String> {
    let answer: Value = serde_json::from_str(answer).map_err(|e| format!("the place search answered something else than JSON: {e}"))?;
    let docs = answer["response"]["docs"].as_array().ok_or("the place search answered without a list of places")?;
    Ok(docs
        .iter()
        .filter_map(|doc| {
            Some(Place { name: doc["weergavenaam"].as_str()?.to_string(), kind: doc["type"].as_str().unwrap_or_default().to_string(), rd: point(doc["centroide_rd"].as_str()?)? })
        })
        .collect())
}

/// The point of a `POINT(x y)`.
fn point(wkt: &str) -> Option<Rd> {
    let inner = wkt.trim().strip_prefix("POINT")?.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut numbers = inner.split_whitespace().map(str::parse::<f64>);
    let rd = Rd::new(numbers.next()?.ok()?, numbers.next()?.ok()?);
    rd.valid().then_some(rd)
}

/// A square of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Rd,
    pub max: Rd,
}

impl Bounds {
    /// The square of `radius` to each side of `center`.
    pub fn around(center: Rd, radius: f64) -> Self {
        Bounds { min: Rd::new(center.x - radius, center.y - radius), max: Rd::new(center.x + radius, center.y + radius) }
    }

    pub fn center(self) -> Rd {
        Rd::new((self.min.x + self.max.x) / 2.0, (self.min.y + self.max.y) / 2.0)
    }
}

/// What can be asked for around a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// The parcels of the cadastral map.
    Parcels,
    /// The buildings of the register of buildings and addresses.
    Buildings,
}

impl Layer {
    pub const ALL: [Layer; 2] = [Layer::Parcels, Layer::Buildings];

    /// Its name in a file and on the command line.
    pub fn key(self) -> &'static str {
        match self {
            Layer::Parcels => "parcels",
            Layer::Buildings => "buildings",
        }
    }

    /// Where its features are asked for.
    fn items(self) -> &'static str {
        match self {
            Layer::Parcels => PARCELS,
            Layer::Buildings => BUILDINGS,
        }
    }

    /// Whose data it is, to say with the data.
    pub fn source(self) -> &'static str {
        match self {
            Layer::Parcels => "Kadaster, BRK Kadastrale kaart (PDOK)",
            Layer::Buildings => "Kadaster, BAG (PDOK)",
        }
    }
}

/// The address of the first page of a layer's features in `bounds`, in RD New.
pub fn items_url(layer: Layer, bounds: Bounds) -> String {
    let crs = encode(RD_CRS);
    format!("{}?f=json&limit={PAGE}&bbox={:.1},{:.1},{:.1},{:.1}&bbox-crs={crs}&crs={crs}", layer.items(), bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y)
}

/// A page of a layer's features.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The features as the service gives them: GeoJSON, coordinates in RD New.
    pub features: Vec<Value>,
    /// The address of the page after this one, when there is one.
    pub next: Option<String>,
}

/// The page in an answer. The address of the next page counts only when it is the layer's own:
/// an answer cannot send the caller elsewhere.
pub fn parse_page(layer: Layer, answer: &str) -> Result<Page, String> {
    let answer: Value = serde_json::from_str(answer).map_err(|e| format!("the service answered something else than JSON: {e}"))?;
    let features = answer["features"].as_array().ok_or("the service answered without features")?.clone();
    let next = answer["links"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|link| link["rel"] == "next")
        .and_then(|link| link["href"].as_str())
        .filter(|href| href.strip_prefix(layer.items()).is_some_and(|rest| rest.starts_with('?')))
        .map(str::to_string);
    Ok(Page { features, next })
}

/// What a feature is called: a parcel by its cadastral designation (`AMF00 E 7948`), a building
/// by the number of its register.
pub fn name(layer: Layer, properties: &Map<String, Value>) -> String {
    let text = |key: &str| match &properties.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    match layer {
        Layer::Parcels => [text("akr_kadastrale_gemeente_code_waarde"), text("sectie"), text("perceelnummer")].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" "),
        Layer::Buildings => text("identificatie"),
    }
}

/// The features of a layer for a GeoJSON reader, in the coordinates of a project.
#[derive(Debug, Clone, PartialEq)]
pub struct Collection {
    /// A GeoJSON feature collection in metres of the project (see [`Frame`]).
    pub geojson: Value,
    /// Its first position: what a reader that moves a file to a local origin takes off.
    pub first: Option<[f64; 2]>,
    /// The features in it.
    pub count: usize,
}

/// The features of a layer as one GeoJSON collection in the coordinates of `frame`. Each feature
/// has its `name` and keeps what the register says about it (links to other records left out).
/// A parcel comes as its boundary lines, a building as its area.
pub fn collection(layer: Layer, items: &[Value], frame: &Frame) -> Collection {
    let mut features = Vec::new();
    for item in items {
        let Some(geometry) = item.get("geometry").and_then(|g| shape(layer, g, frame)) else { continue };
        let given = item["properties"].as_object().cloned().unwrap_or_default();
        let mut properties = Map::new();
        properties.insert("name".into(), json!(name(layer, &given)));
        properties.insert("source".into(), json!(layer.source()));
        for (key, value) in given {
            // a link to another record of the register is no property of this one
            if key.ends_with(".href") || key == "rdf_seealso" || value.is_null() {
                continue;
            }
            properties.entry(key).or_insert(value);
        }
        features.push(json!({ "type": "Feature", "geometry": geometry, "properties": properties }));
    }
    let first = features.first().and_then(|f| first_position(&f["geometry"]["coordinates"]));
    let count = features.len();
    // a name that is not WGS 84: the coordinates are metres, however small
    let geojson = json!({
        "type": "FeatureCollection",
        "crs": { "type": "name", "properties": { "name": "Project metres (from EPSG:28992)" } },
        "features": features,
    });
    Collection { geojson, first, count }
}

/// A geometry of the service in the coordinates of `frame`: the rings of a parcel as lines, a
/// building as it is. `None` for anything but areas.
fn shape(layer: Layer, geometry: &Value, frame: &Frame) -> Option<Value> {
    let coordinates = local(&geometry["coordinates"], frame)?;
    match (geometry["type"].as_str()?, layer) {
        (kind @ ("Polygon" | "MultiPolygon"), Layer::Buildings) => Some(json!({ "type": kind, "coordinates": coordinates })),
        ("Polygon", Layer::Parcels) => Some(json!({ "type": "MultiLineString", "coordinates": coordinates })),
        ("MultiPolygon", Layer::Parcels) => {
            let rings: Vec<Value> = coordinates.as_array()?.iter().flat_map(|polygon| polygon.as_array().cloned().unwrap_or_default()).collect();
            Some(json!({ "type": "MultiLineString", "coordinates": rings }))
        }
        _ => None,
    }
}

/// Nested positions of the grid as positions of `frame`, heights dropped.
fn local(coordinates: &Value, frame: &Frame) -> Option<Value> {
    let list = coordinates.as_array()?;
    if let (Some(x), Some(y)) = (list.first().and_then(Value::as_f64), list.get(1).and_then(Value::as_f64)) {
        let [x, y] = frame.local(Rd::new(x, y));
        return Some(json!([x, y]));
    }
    list.iter().map(|inner| local(inner, frame)).collect::<Option<Vec<_>>>().map(Value::Array)
}

/// The first position in nested positions.
fn first_position(coordinates: &Value) -> Option<[f64; 2]> {
    let list = coordinates.as_array()?;
    match (list.first().and_then(Value::as_f64), list.get(1).and_then(Value::as_f64)) {
        (Some(x), Some(y)) => Some([x, y]),
        _ => first_position(list.first()?),
    }
}

/// `s` for the query of an address: everything but letters, digits and `-._~` escaped.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the place search answers, shortened.
    const FOUND: &str = r#"{"response":{"numFound":464241,"start":0,"docs":[
        {"type":"weg","weergavenaam":"Stadhuisplein, Amersfoort","centroide_rd":"POINT(154841.405 463136.52)"},
        {"type":"adres","weergavenaam":"Stadhuisplein 1, 3811LM Amersfoort","centroide_rd":"POINT(154792.05 463172.16)"},
        {"type":"adres","weergavenaam":"Without a point"},
        {"type":"adres","weergavenaam":"Elsewhere","centroide_rd":"POINT(0 0)"}]}}"#;

    #[test]
    fn a_search_is_asked_with_the_query_escaped() {
        let url = search_url("  Stadhuisplein 1, Amersfoort & co ", 5);
        assert_eq!(url, "https://api.pdok.nl/bzk/locatieserver/search/v3_1/free?q=Stadhuisplein%201%2C%20Amersfoort%20%26%20co&rows=5&fl=weergavenaam,type,centroide_rd");
        assert!(search_url("Curaçao", 1).contains("q=Cura%C3%A7ao&"));
    }

    #[test]
    fn the_places_found_come_with_their_point_and_one_without_is_left_out() {
        let places = parse_search(FOUND).unwrap();
        assert_eq!(places.len(), 2, "{places:?}");
        assert_eq!(places[1], Place { name: "Stadhuisplein 1, 3811LM Amersfoort".into(), kind: "adres".into(), rd: Rd::new(154_792.05, 463_172.16) });
        assert!(parse_search("<html>").unwrap_err().contains("JSON"));
        assert!(parse_search("{}").is_err());
    }

    #[test]
    fn features_are_asked_in_the_square_in_rd_new() {
        let bounds = Bounds::around(Rd::new(154_800.0, 463_200.0), 100.0);
        assert_eq!(bounds.center(), Rd::new(154_800.0, 463_200.0));
        let url = items_url(Layer::Parcels, bounds);
        assert!(url.starts_with("https://api.pdok.nl/kadaster/brk-kadastrale-kaart/ogc/v1/collections/perceel/items?f=json&limit=1000&bbox=154700.0,463100.0,154900.0,463300.0&bbox-crs=http%3A%2F%2F"), "{url}");
        assert!(url.ends_with("&crs=http%3A%2F%2Fwww.opengis.net%2Fdef%2Fcrs%2FEPSG%2F0%2F28992"), "{url}");
        assert!(items_url(Layer::Buildings, bounds).starts_with("https://api.pdok.nl/kadaster/bag/ogc/v2/collections/pand/items?"));
    }

    #[test]
    fn a_page_gives_its_features_and_the_next_page_of_the_same_service_only() {
        let page = |next: &str| format!(r#"{{"type":"FeatureCollection","links":[{{"rel":"self","href":"x"}},{{"rel":"next","href":"{next}"}}],"features":[{{"type":"Feature"}}]}}"#);
        let own = "https://api.pdok.nl/kadaster/bag/ogc/v2/collections/pand/items?cursor=abc";
        assert_eq!(parse_page(Layer::Buildings, &page(own)).unwrap(), Page { features: vec![json!({"type": "Feature"})], next: Some(own.into()) });
        // the next page of another service, or of another host, is not followed
        assert_eq!(parse_page(Layer::Parcels, &page(own)).unwrap().next, None);
        assert_eq!(parse_page(Layer::Buildings, &page("https://example.org/items?cursor=abc")).unwrap().next, None);
        assert_eq!(parse_page(Layer::Buildings, &page("https://api.pdok.nl/kadaster/bag/ogc/v2/collections/pand/items.evil.org/?x")).unwrap().next, None);
        // the last page has no next
        assert_eq!(parse_page(Layer::Buildings, r#"{"features":[]}"#).unwrap(), Page { features: vec![], next: None });
        assert!(parse_page(Layer::Buildings, r#"{"title":"Bad request"}"#).is_err());
    }

    #[test]
    fn the_collection_names_each_feature_and_keeps_what_the_register_says() {
        let frame = Frame::at(Rd::new(154_800.0, 463_200.0));
        let building = json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[154803.0,463203.0],[154796.0,463198.0],[154792.0,463204.0],[154803.0,463203.0]]]},
            "properties":{"identificatie":"0307100000370278","bouwjaar":1974,"status":"Pand in gebruik","name":"given","oppervlakte":null,"verblijfsobject.href":["https://example.org/a"],"rdf_seealso":"http://example.org/b"}});
        let without_shape = json!({"type":"Feature","geometry":null,"properties":{}});
        let point = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[154803.0,463203.0]},"properties":{}});
        let collection = collection(Layer::Buildings, &[without_shape, point, building], &frame);
        assert_eq!((collection.count, collection.first), (1, Some([3.0, 3.0])));
        // a name that says metres, so a reader does not take small numbers for degrees
        let crs = collection.geojson["crs"]["properties"]["name"].as_str().unwrap();
        assert!(!crs.contains("CRS84") && !crs.ends_with("4326"), "{crs}");
        let feature = &collection.geojson["features"][0];
        assert_eq!(feature["geometry"], json!({"type":"Polygon","coordinates":[[[3.0,3.0],[-4.0,-2.0],[-8.0,4.0],[3.0,3.0]]]}));
        let b = &feature["properties"];
        assert_eq!((b["name"].as_str(), b["bouwjaar"].as_i64(), b["source"].as_str()), (Some("0307100000370278"), Some(1974), Some("Kadaster, BAG (PDOK)")));
        assert!(b.get("verblijfsobject.href").is_none() && b.get("rdf_seealso").is_none() && b.get("oppervlakte").is_none());
    }

    #[test]
    fn a_parcel_comes_as_its_boundary_lines_in_the_frame_of_the_project() {
        // a parcel of two parts, the first with a hole; the project's x axis points north
        let parcel = json!({"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[
            [[[100010.0,400000.0],[100020.0,400000.0],[100020.0,400010.0],[100010.0,400000.0]], [[100012.0,400001.0],[100018.0,400001.0],[100018.0,400006.0],[100012.0,400001.0]]],
            [[[100030.0,400000.0],[100040.0,400000.0],[100040.0,400010.0],[100030.0,400000.0]]]]},
            "properties":{"akr_kadastrale_gemeente_code_waarde":"AMF00","sectie":"E","perceelnummer":7948,"kadastrale_grootte_waarde":6242}});
        let single = json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[100000.0,400000.0],[100005.0,400000.0],[100005.0,400005.0],[100000.0,400000.0]]]},"properties":{"sectie":"E","perceelnummer":1}});
        let frame = Frame { origin: Rd::new(100_000.0, 400_000.0), rotation: 90.0 };
        let collection = collection(Layer::Parcels, &[parcel, single], &frame);
        assert_eq!(collection.count, 2);
        let first = collection.first.unwrap();
        assert!(first[0].abs() < 1e-9 && (first[1] + 10.0).abs() < 1e-9, "{first:?}");
        let feature = &collection.geojson["features"][0];
        assert_eq!(feature["geometry"]["type"], "MultiLineString");
        assert_eq!(feature["geometry"]["coordinates"].as_array().unwrap().len(), 3);
        assert_eq!((feature["properties"]["name"].as_str(), feature["properties"]["kadastrale_grootte_waarde"].as_i64()), (Some("AMF00 E 7948"), Some(6242)));
        let single = &collection.geojson["features"][1];
        assert_eq!((single["geometry"]["type"].as_str(), single["geometry"]["coordinates"].as_array().unwrap().len(), single["properties"]["name"].as_str()), (Some("MultiLineString"), 1, Some("E 1")));
    }
}
