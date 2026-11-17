//! The Dutch national grid RD New (EPSG:28992) and latitude and longitude (WGS 84).
//!
//! Both ways by the series of Schreutelkamp and Strang van Hees around the origin of the grid at
//! Amersfoort. Within the Netherlands they stay within half a metre of the official
//! transformation of the national geodetic service: 0.30 m at the addresses in the tests, from
//! Maastricht to Delfzijl. That places a building on a map; it is not for setting out.

/// A point of RD New: metres east and north.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rd {
    pub x: f64,
    pub y: f64,
}

/// A latitude and longitude, WGS 84, degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// The origin of the grid: the tower of Amersfoort.
const ORIGIN: Rd = Rd { x: 155_000.0, y: 463_000.0 };
const ORIGIN_WGS84: LatLon = LatLon { lat: 52.155_174_40, lon: 5.387_206_21 };

impl Rd {
    pub fn new(x: f64, y: f64) -> Self {
        Rd { x, y }
    }

    /// Whether the point lies where the grid is valid: the Netherlands and its coastal waters.
    pub fn valid(self) -> bool {
        (-7_000.0..=300_000.0).contains(&self.x) && (289_000.0..=629_000.0).contains(&self.y)
    }

    /// The point as latitude and longitude.
    pub fn to_wgs84(self) -> LatLon {
        let (dx, dy) = ((self.x - ORIGIN.x) * 1e-5, (self.y - ORIGIN.y) * 1e-5);
        let lat = 3235.653_89 * dy - 32.582_97 * dx * dx - 0.247_50 * dy * dy - 0.849_78 * dx * dx * dy - 0.065_50 * dy.powi(3) - 0.017_09 * dx * dx * dy * dy
            - 0.007_38 * dx
            + 0.005_30 * dx.powi(4)
            - 0.000_39 * dx * dx * dy.powi(3)
            + 0.000_33 * dx.powi(4) * dy
            - 0.000_12 * dx * dy;
        let lon = 5260.529_16 * dx + 105.946_84 * dx * dy + 2.456_56 * dx * dy * dy - 0.818_85 * dx.powi(3) + 0.055_94 * dx * dy.powi(3) - 0.056_07 * dx.powi(3) * dy
            + 0.011_99 * dy
            - 0.002_56 * dx.powi(3) * dy * dy
            + 0.001_28 * dx * dy.powi(4)
            + 0.000_22 * dy * dy
            - 0.000_22 * dx * dx
            + 0.000_26 * dx.powi(5);
        LatLon { lat: ORIGIN_WGS84.lat + lat / 3600.0, lon: ORIGIN_WGS84.lon + lon / 3600.0 }
    }

    /// The distance to another point (metres).
    pub fn distance(self, other: Rd) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Self {
        LatLon { lat, lon }
    }

    /// The place as a point of RD New.
    pub fn to_rd(self) -> Rd {
        let (p, l) = (0.36 * (self.lat - ORIGIN_WGS84.lat), 0.36 * (self.lon - ORIGIN_WGS84.lon));
        let x = 190_094.945 * l - 11_832.228 * p * l - 114.221 * p * p * l - 32.391 * l.powi(3) - 0.705 * p - 2.340 * p.powi(3) * l - 0.608 * p * l.powi(3) - 0.008 * l * l
            + 0.148 * p * p * l.powi(3);
        let y = 309_056.544 * p + 3_638.893 * l * l + 73.077 * p * p - 157.984 * p * l * l + 59.788 * p.powi(3) + 0.433 * l - 6.439 * p * p * l * l - 0.032 * p * l
            + 0.092 * l.powi(4)
            - 0.054 * p * l.powi(4);
        Rd { x: ORIGIN.x + x, y: ORIGIN.y + y }
    }
}

/// Coordinates as typed: a point of the grid as `155000 463000` (x y), or a latitude and
/// longitude as `52.155, 5.387` (either order). Numbers are parted by spaces, a comma or a
/// semicolon and have a decimal point. `None` for anything else, and for a place outside the grid.
pub fn typed(text: &str) -> Option<Rd> {
    let numbers: Vec<f64> = text.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|part| !part.is_empty()).map(str::parse).collect::<Result<_, _>>().ok()?;
    let [a, b] = numbers[..] else { return None };
    let degrees = |lat: f64, lon: f64| ((50.0..=54.0).contains(&lat) && (3.0..=7.5).contains(&lon)).then(|| LatLon::new(lat, lon).to_rd());
    [Some(Rd::new(a, b)), degrees(a, b), degrees(b, a)].into_iter().flatten().find(|rd| rd.valid())
}

/// How the grid lies in a project: the grid point at the zero of the project's coordinates, and
/// the angle of the project's x axis from east (degrees, counter-clockwise), as the map
/// conversion of an IFC file gives them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: Rd,
    pub rotation: f64,
}

impl Frame {
    /// The frame with its zero at `origin` and its x axis to the east.
    pub fn at(origin: Rd) -> Self {
        Frame { origin, rotation: 0.0 }
    }

    /// A grid point in the project's coordinates (metres).
    pub fn local(&self, point: Rd) -> [f64; 2] {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (east, north) = (point.x - self.origin.x, point.y - self.origin.y);
        [east * cos + north * sin, north * cos - east * sin]
    }

    /// The grid point of a point in the project's coordinates.
    pub fn rd(&self, x: f64, y: f64) -> Rd {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        Rd::new(self.origin.x + x * cos - y * sin, self.origin.y + x * sin + y * cos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_coordinates_are_a_grid_point_or_a_latitude_and_longitude() {
        assert_eq!(typed("155000 463000"), Some(Rd::new(155_000.0, 463_000.0)));
        assert_eq!(typed(" 154792.05; 463172.16 "), Some(Rd::new(154_792.05, 463_172.16)));
        // latitude and longitude, in either order
        for text in ["52.15517440, 5.38720621", "5.38720621 52.15517440"] {
            assert!(typed(text).is_some_and(|rd| rd.distance(ORIGIN) < 0.01), "{text}");
        }
        // an address, one number, three numbers, a place outside the grid
        for text in ["Stadhuisplein 1", "155000", "1 2 3", "48.8566, 2.3522", "0 0", ""] {
            assert_eq!(typed(text), None, "{text}");
        }
    }

    #[test]
    fn a_frame_turns_grid_points_into_the_projects_coordinates_and_back() {
        let plain = Frame::at(Rd::new(155_000.0, 463_000.0));
        assert_eq!(plain.local(Rd::new(155_010.0, 463_020.0)), [10.0, 20.0]);
        // the project's x axis points north: a point 10 m east lies at y = -10
        let turned = Frame { origin: Rd::new(155_000.0, 463_000.0), rotation: 90.0 };
        let [x, y] = turned.local(Rd::new(155_010.0, 463_020.0));
        assert!((x - 20.0).abs() < 1e-9 && (y + 10.0).abs() < 1e-9, "{x} {y}");
        assert!(turned.rd(x, y).distance(Rd::new(155_010.0, 463_020.0)) < 1e-9);
        let skewed = Frame { origin: Rd::new(120_700.0, 487_525.0), rotation: -17.5 };
        let [x, y] = skewed.local(Rd::new(120_800.0, 487_400.0));
        assert!(skewed.rd(x, y).distance(Rd::new(120_800.0, 487_400.0)) < 1e-9);
        assert!((x.hypot(y) - 100f64.hypot(125.0)).abs() < 1e-9);
    }

    /// The metres between two places close together.
    fn metres(a: LatLon, b: LatLon) -> f64 {
        ((a.lat - b.lat) * 111_200.0).hypot((a.lon - b.lon) * 111_200.0 * a.lat.to_radians().cos())
    }

    #[test]
    fn the_origin_of_the_grid_is_the_tower_of_amersfoort() {
        let place = ORIGIN.to_wgs84();
        assert!(metres(place, ORIGIN_WGS84) < 1e-6, "{place:?}");
        assert!(ORIGIN_WGS84.to_rd().distance(ORIGIN) < 1e-6);
    }

    #[test]
    fn the_towers_of_amsterdam_and_groningen_lie_where_the_series_was_fitted() {
        // the Westertoren and the Martinitoren, the check points published with the series
        for (rd, place) in [(Rd::new(120_700.723, 487_525.501), LatLon::new(52.374_532_53, 4.883_525_59)), (Rd::new(233_883.131, 582_065.167), LatLon::new(53.219_383_17, 6.568_200_53))] {
            assert!(metres(rd.to_wgs84(), place) < 0.01, "{rd:?} {:?}", rd.to_wgs84());
            assert!(place.to_rd().distance(rd) < 0.01, "{place:?} {:?}", place.to_rd());
        }
    }

    #[test]
    fn addresses_across_the_country_stay_within_half_a_metre_of_the_official_transformation() {
        // an address in Maastricht, Den Helder, Vlissingen, Delfzijl, Enschede and Amersfoort, each
        // in both systems as the national address register gives them
        let addresses = [
            (176_275.88, 317_803.07, 50.849_668_54, 5.689_297_22),
            (112_908.073, 552_551.17, 52.958_342_73, 4.760_824_61),
            (28_051.121, 385_979.312, 51.448_591_88, 3.560_862_05),
            (257_645.968, 594_950.372, 53.331_097_53, 6.927_978_21),
            (258_026.36, 471_295.24, 52.220_100_65, 6.894_954_59),
            (154_792.05, 463_172.16, 52.156_719_63, 5.384_164_72),
        ];
        for (x, y, lat, lon) in addresses {
            let (rd, place) = (Rd::new(x, y), LatLon::new(lat, lon));
            assert!(rd.valid());
            assert!(metres(rd.to_wgs84(), place) < 0.5, "{rd:?}: {} m", metres(rd.to_wgs84(), place));
            assert!(place.to_rd().distance(rd) < 0.5, "{place:?}: {} m", place.to_rd().distance(rd));
        }
    }

    #[test]
    fn there_and_back_is_the_same_point() {
        for rd in [Rd::new(28_051.0, 385_979.0), Rd::new(155_000.0, 463_000.0), Rd::new(257_646.0, 594_950.0)] {
            assert!(rd.to_wgs84().to_rd().distance(rd) < 0.01, "{rd:?}");
        }
    }

    #[test]
    fn a_point_outside_the_netherlands_is_not_valid() {
        assert!(!Rd::new(0.0, 0.0).valid());
        assert!(!Rd::new(5.38, 52.15).valid());
        assert!(!LatLon::new(48.8566, 2.3522).to_rd().valid());
    }
}
