//! Civil and site design for Open BIM Suite: the ground a project stands on and what is built
//! in and on it.
//!
//! - [`points`]: survey points as surveyors' files give them (a number, coordinates, a height
//!   and a code per line, in any of the usual column orders).
//! - [`tin`]: a terrain surface as a triangulated irregular network: built from points, with
//!   breaklines the triangles follow and a boundary they stay within, or from a grid of heights.
//!   A surface gives the height at a place, its contours, and volumes against a level or
//!   another surface.
//!
//! Headless: no GUI and no files; the caller reads and draws. Lengths are metres, x is east,
//! y is north, z is up.

pub mod points;
pub mod tin;

pub use points::SurveyPoint;
pub use tin::{Tin, Volume};
