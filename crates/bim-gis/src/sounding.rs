//! A referenced cone penetration test used on a registered site plan.
use crate::rd::{Frame, Rd};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Sounding {
    pub id: String,
    pub rd: [f64; 2],
    pub crs: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

impl Sounding {
    /// RD metres to plan metres, including the direction of its drawing axes.
    pub fn local(&self, frame: Frame) -> Option<[f64; 2]> {
        let point = Rd::new(self.rd[0], self.rd[1]);
        (self.crs == "EPSG:28992"
            && !self.id.trim().is_empty()
            && !self.source.trim().is_empty()
            && point.valid()
            && frame.origin.valid()
            && frame.rotation.is_finite())
        .then(|| frame.local(point))
    }
}
