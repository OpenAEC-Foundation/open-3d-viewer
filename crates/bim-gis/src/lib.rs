//! GIS for a BIM project of Open BIM Suite: where the project lies and what lies around it.
//!
//! - [`rd`]: the Dutch national grid RD New (EPSG:28992) and latitude and longitude (WGS 84),
//!   both ways; coordinates as typed; how the grid lies in a project ([`Frame`]).
//! - [`pdok`]: the open services of PDOK, the geodata platform of the Dutch government: finding
//!   a place by its address or name, and the parcels and buildings around a place.
//! - [`heights`]: the ground heights of the national height model around a place.
//! - [`bro`]: the cone penetration tests of the register of the subsurface around a place.
//! - [`map`]: maps as pictures, of a web map service or of the tiles of the background map.
//!
//! Headless and without a network: this crate says what to ask a service and reads what it
//! answers; the caller sends the request. So it builds for the browser too and its tests need no
//! connection.
//!
//! Lengths are metres.

pub mod bro;
pub mod heights;
pub mod map;
pub mod pdok;
pub mod rd;

pub use rd::{Frame, LatLon, Rd};
