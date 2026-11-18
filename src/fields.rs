//! Values a simulation file gives at the points or cells of its mesh (a temperature, a
//! displacement, a stress), per time step, and the colours they paint the mesh's layers in.
//!
//! A loader fills [`Fields`] with the values and, per layer it made, the point and the cell each
//! vertex comes from. What can be chosen is a [`Channel`]: a scalar field, or the length or one
//! component of a vector or tensor field. A channel is coloured over its range across all the
//! steps, so the colours of one step compare with those of another, on the ramp the point clouds
//! use for their height ([`crate::cloud_color::ramp`]).

/// Where the values of a field are given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Point,
    Cell,
}

/// One field of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub at: At,
    pub components: usize,
    /// `components` numbers per point or cell.
    pub values: Vec<f32>,
}

impl Values {
    /// Of point or cell `k`: component `c`, or the length for `None`.
    pub fn value(&self, k: usize, c: Option<usize>) -> f32 {
        let n = self.components.max(1);
        let Some(v) = self.values.get(k * n..k * n + n) else { return f32::NAN };
        match (c, v) {
            (Some(c), _) => v.get(c).copied().unwrap_or(f32::NAN),
            (None, [one]) => *one,
            (None, _) => v.iter().map(|x| x * x).sum::<f32>().sqrt(),
        }
    }
}

/// A field as it can be chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// "temperature", "displacement (length)", "displacement X".
    pub label: String,
    /// Index into the fields of a step.
    pub field: usize,
    /// The component, or `None` for the value itself (the length of a vector).
    pub component: Option<usize>,
    pub at: At,
    /// The smallest and largest value over all steps (equal when there is one value).
    pub range: (f32, f32),
}

/// Of one layer of the model: where each vertex comes from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayerSource {
    /// The layer's place in the model's layers.
    pub layer: usize,
    pub point: Vec<u32>,
    pub cell: Vec<u32>,
}

/// One step of a series (or the only one).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Step {
    pub time: Option<f64>,
    /// Indexed as the fields' names.
    pub fields: Vec<Option<Values>>,
}

/// The fields of a model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fields {
    /// The names of the fields, with where each is given and its number of components.
    pub names: Vec<(String, At, usize)>,
    pub steps: Vec<Step>,
    pub layers: Vec<LayerSource>,
    pub channels: Vec<Channel>,
}

/// The colour of a vertex without a value.
const NO_VALUE: [u8; 4] = [150, 150, 155, 255];

impl Fields {
    /// Works out the channels and their ranges once the names, steps and layers are in.
    pub fn finish(&mut self) {
        let axes = ["X", "Y", "Z"];
        let mut channels = Vec::new();
        for (f, (name, at, components)) in self.names.iter().enumerate() {
            let choices: Vec<(String, Option<usize>)> = match components {
                1 => vec![(name.clone(), None)],
                2 | 3 => std::iter::once((format!("{name} (length)"), None)).chain((0..*components).map(|c| (format!("{name} {}", axes[c]), Some(c)))).collect(),
                _ => std::iter::once((format!("{name} (norm)"), None)).chain((0..*components).map(|c| (format!("{name} {}", c + 1), Some(c)))).collect(),
            };
            for (label, component) in choices {
                let mut range = (f32::INFINITY, f32::NEG_INFINITY);
                for step in &self.steps {
                    let Some(Some(v)) = step.fields.get(f) else { continue };
                    for k in 0..v.values.len() / v.components.max(1) {
                        let x = v.value(k, component);
                        if x.is_finite() {
                            range = (range.0.min(x), range.1.max(x));
                        }
                    }
                }
                if range.0 <= range.1 {
                    channels.push(Channel { label, field: f, component, at: *at, range });
                }
            }
        }
        self.channels = channels;
    }

    /// The colours of the vertices of `layer` (an entry of [`Fields::layers`]) for `channel` at
    /// `step`.
    pub fn colours(&self, layer: &LayerSource, channel: usize, step: usize) -> Vec<[u8; 4]> {
        let n = layer.point.len();
        let Some(ch) = self.channels.get(channel) else { return vec![NO_VALUE; n] };
        let Some(Some(values)) = self.steps.get(step).and_then(|s| s.fields.get(ch.field)) else { return vec![NO_VALUE; n] };
        let from = match ch.at {
            At::Point => &layer.point,
            At::Cell => &layer.cell,
        };
        let (low, high) = ch.range;
        let span = if high > low { high - low } else { 1.0 };
        from.iter()
            .map(|&k| {
                let x = values.value(k as usize, ch.component);
                if !x.is_finite() {
                    return NO_VALUE;
                }
                let [r, g, b] = crate::cloud_color::ramp((x - low) / span);
                [r, g, b, 255]
            })
            .collect()
    }

    /// How a step is named: its time, or its number.
    pub fn step_name(&self, step: usize) -> String {
        match self.steps.get(step).and_then(|s| s.time) {
            Some(t) => format!("t = {}", number(t)),
            None => format!("{}", step + 1),
        }
    }
}

/// A value as the legend writes it: four significant digits, without trailing zeros.
pub fn number(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return format!("{x}");
    }
    let magnitude = x.abs().log10().floor() as i32;
    if !(-4..6).contains(&magnitude) {
        return format!("{x:.3e}");
    }
    let decimals = (3 - magnitude).max(0) as usize;
    let s = format!("{x:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Fields {
        let mut f = Fields {
            names: vec![("T".into(), At::Point, 1), ("U".into(), At::Cell, 3)],
            steps: vec![
                Step { time: Some(0.0), fields: vec![Some(Values { at: At::Point, components: 1, values: vec![0.0, 10.0] }), Some(Values { at: At::Cell, components: 3, values: vec![3.0, 4.0, 0.0] })] },
                Step { time: Some(0.5), fields: vec![Some(Values { at: At::Point, components: 1, values: vec![20.0, f32::NAN] }), None] },
            ],
            layers: vec![LayerSource { layer: 0, point: vec![0, 1, 1], cell: vec![0, 0, 0] }],
            channels: Vec::new(),
        };
        f.finish();
        f
    }

    #[test]
    fn a_vector_gives_its_length_and_each_component() {
        let f = fields();
        let labels: Vec<&str> = f.channels.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["T", "U (length)", "U X", "U Y", "U Z"]);
        // over both steps
        assert_eq!(f.channels[0].range, (0.0, 20.0));
        assert_eq!(f.channels[1].range, (5.0, 5.0));
    }

    #[test]
    fn colours_run_from_low_to_high_and_grey_without_a_value() {
        let f = fields();
        let c = f.colours(&f.layers[0], 0, 0);
        assert_eq!(c[0][..3], crate::cloud_color::ramp(0.0));
        assert_eq!(c[1][..3], crate::cloud_color::ramp(0.5));
        let later = f.colours(&f.layers[0], 0, 1);
        assert_eq!(later[0][..3], crate::cloud_color::ramp(1.0));
        assert_eq!(later[1], NO_VALUE);
        // a field a step does not have
        assert_eq!(f.colours(&f.layers[0], 1, 1), vec![NO_VALUE; 3]);
        assert_eq!((f.step_name(1), number(0.000123456), number(1234.5678), number(2.5)), ("t = 0.5".to_string(), "0.0001235".to_string(), "1235".to_string(), "2.5".to_string()));
    }
}
