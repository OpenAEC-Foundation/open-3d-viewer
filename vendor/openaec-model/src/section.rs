//! UI-independent clipping geometry for saved model views.
use crate::model::Bounds;
use glam::Vec3;
const MIN_SIZE: f32 = 1e-3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionBox {
    /// Limits in the frame of the box: the world turned by −`turn` about the vertical through
    /// `pivot`.
    pub min: Vec3,
    pub max: Vec3,
    /// Turn about the vertical, radians, counter-clockwise seen from above.
    pub turn: f32,
    pub pivot: Vec3,
}

impl SectionBox {
    /// A box just around the bounds (world), not turned.
    pub fn around(bounds: &Bounds) -> Self {
        let min = Vec3::from_array(bounds.min);
        let max = Vec3::from_array(bounds.max);
        let pad = (max - min).max(Vec3::splat(MIN_SIZE)) * 0.01;
        Self { min: min - pad, max: max + pad, turn: 0.0, pivot: (min + max) * 0.5 }
    }

    /// The box `origin + x·basis[0] + y·basis[1] + z·basis[2]` for x, y, z within min..max (a
    /// Revit section or crop box), when one of its axes is vertical (the box turns only about the
    /// vertical).
    pub fn from_frame(origin: Vec3, basis: [Vec3; 3], min: Vec3, max: Vec3) -> Option<Self> {
        let along = basis.iter().position(|b| b.z.abs() < 0.001 && b.length() > 0.5)?;
        let a = basis[along];
        let mut out = SectionBox { min: origin, max: origin, turn: a.y.atan2(a.x), pivot: origin };
        for j in 0..3 {
            let e = out.axis(j);
            let (k, dot) = (0..3)
                .map(|k| (k, basis[k].normalize_or_zero().dot(e)))
                .max_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))?;
            if dot.abs() < 0.999 {
                return None;
            }
            let (lo, hi) = if dot > 0.0 { (min[k], max[k]) } else { (-max[k], -min[k]) };
            out.min[j] = origin[j] + lo;
            out.max[j] = origin[j] + hi;
        }
        Some(out)
    }

    /// The box round the model (world bounds) with one face on `plane`, when the box can make the
    /// same cut: a level plane, or an upright one (the box turned to it by at most 45°, so an X or
    /// Y plane leaves it straight). A sloped plane gives `None`.
    pub fn from_plane(plane: &SectionPlane, world: &Bounds) -> Option<Self> {
        let n = plane.normal;
        let turn = if n.x.abs() < 1e-6 && n.y.abs() < 1e-6 {
            0.0
        } else if n.z.abs() < 1e-6 {
            let quarter = std::f32::consts::FRAC_PI_2;
            let angle = n.y.atan2(n.x);
            angle - (angle / quarter).round() * quarter
        } else {
            return None;
        };
        let middle = Vec3::from_array(world.center());
        let mut out = SectionBox { min: middle, max: middle, turn, pivot: middle };
        let r = out.local_bounds(world);
        let (min, max) = (Vec3::from_array(r.min), Vec3::from_array(r.max));
        let pad = (max - min).max(Vec3::splat(MIN_SIZE)) * 0.01;
        out.min = min - pad;
        out.max = max + pad;
        // the box axis along the normal: its face on that side goes where the plane is
        let (axis, along) = (0..3).map(|j| (j, out.axis(j).dot(n))).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
        let at = out.pivot[axis] + along.signum() * plane.offset - out.pivot.dot(out.axis(axis));
        if along > 0.0 {
            out.max[axis] = at.min(out.max[axis]).max(out.min[axis] + MIN_SIZE);
        } else {
            out.min[axis] = at.max(out.min[axis]).min(out.max[axis] - MIN_SIZE);
        }
        Some(out)
    }

    /// The box limited to the model (world bounds) plus a margin: views store far planes of
    /// thousands of metres.
    pub fn clamped_to(mut self, world: &Bounds) -> Self {
        let r = self.local_bounds(world);
        for i in 0..3 {
            let pad = ((r.max[i] - r.min[i]) * 0.02).max(MIN_SIZE);
            let (lo, hi) = (self.min[i].max(r.min[i] - pad), self.max[i].min(r.max[i] + pad));
            if hi - lo >= MIN_SIZE {
                self.min[i] = lo;
                self.max[i] = hi;
            }
        }
        self
    }

    /// Axis-aligned world bounds of the box.
    pub fn world_bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for c in self.corners() {
            b.extend(c.to_array());
        }
        b
    }

    pub fn to_world(&self, local: Vec3) -> Vec3 {
        let (s, c) = self.turn.sin_cos();
        let d = local - self.pivot;
        self.pivot + Vec3::new(c * d.x - s * d.y, s * d.x + c * d.y, d.z)
    }

    pub fn to_local(&self, world: Vec3) -> Vec3 {
        let (s, c) = self.turn.sin_cos();
        let d = world - self.pivot;
        self.pivot + Vec3::new(c * d.x + s * d.y, -s * d.x + c * d.y, d.z)
    }

    pub fn contains(&self, world: Vec3) -> bool {
        let p = self.to_local(world);
        let eps = (self.max - self.min).max_element() * 1e-6;
        (0..3).all(|i| p[i] >= self.min[i] - eps && p[i] <= self.max[i] + eps)
    }

    /// The world direction of a box axis.
    pub fn axis(&self, axis: usize) -> Vec3 {
        let (s, c) = self.turn.sin_cos();
        match axis {
            0 => Vec3::new(c, s, 0.0),
            1 => Vec3::new(-s, c, 0.0),
            _ => Vec3::Z,
        }
    }

    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|i| {
            let pick = |bit: usize, axis: usize| if i >> bit & 1 == 1 { self.max[axis] } else { self.min[axis] };
            self.to_world(Vec3::new(pick(0, 0), pick(1, 1), pick(2, 2)))
        })
    }

    /// The middle of a face: where its handle is.
    pub fn handle(&self, axis: usize, is_min: bool) -> Vec3 {
        let mut p = (self.min + self.max) * 0.5;
        p[axis] = if is_min { self.min[axis] } else { self.max[axis] };
        self.to_world(p)
    }

    /// The turning handles: the middle of the top edge of each side face.
    pub fn turn_handles(&self) -> [Vec3; 4] {
        let at = |axis: usize, is_min: bool| {
            let mut p = (self.min + self.max) * 0.5;
            p[axis] = if is_min { self.min[axis] } else { self.max[axis] };
            p.z = self.max.z;
            self.to_world(p)
        };
        [at(0, true), at(0, false), at(1, true), at(1, false)]
    }

    /// Moves a face along its axis by `delta` (world units), keeping the box at least
    /// `MIN_SIZE` deep.
    pub fn move_face(&mut self, axis: usize, is_min: bool, delta: f32) {
        if axis > 2 || !delta.is_finite() {
            return;
        }
        if is_min {
            self.min[axis] = (self.min[axis] + delta).min(self.max[axis] - MIN_SIZE);
        } else {
            self.max[axis] = (self.max[axis] + delta).max(self.min[axis] + MIN_SIZE);
        }
    }

    /// Turns the box about the vertical through its middle.
    pub fn turn_by(&mut self, radians: f32) {
        if !radians.is_finite() {
            return;
        }
        let middle = self.to_world((self.min + self.max) * 0.5);
        self.turn = (self.turn + radians + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let shift = self.to_local(middle) - (self.min + self.max) * 0.5;
        self.min += shift;
        self.max += shift;
    }

    /// The bounds of `world` in the frame of the box (for the sliders).
    pub fn local_bounds(&self, world: &Bounds) -> Bounds {
        let b = SectionBox { min: Vec3::from_array(world.min), max: Vec3::from_array(world.max), turn: 0.0, pivot: Vec3::ZERO };
        let mut out = Bounds::empty();
        for c in b.corners() {
            out.extend(self.to_local(c).to_array());
        }
        out
    }

    /// For the shader: pivot + cos(turn), min + sin(turn), max + 1 (on).
    pub fn uniform(&self) -> [[f32; 4]; 3] {
        let (s, c) = self.turn.sin_cos();
        [
            [self.pivot.x, self.pivot.y, self.pivot.z, c],
            [self.min.x, self.min.y, self.min.z, s],
            [self.max.x, self.max.y, self.max.z, 1.0],
        ]
    }
}

/// Components of a picked normal smaller than this (about 0.5°) snap to zero, so walls and floors
/// give clean orthogonal sections.
const SNAP: f32 = 0.0087;

/// A single cutting plane, placed by clicking a face of the model (any orientation, e.g. a sloped
/// roof): everything on the side the face points to is cut away. As the web viewer's
/// `SectionPlane`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    /// Unit normal pointing to the side that is cut away (world).
    pub normal: Vec3,
    /// Position along the normal: points with `normal · p > offset` are cut away.
    pub offset: f32,
}

impl SectionPlane {
    /// The plane on a face at `point` whose normal faces the viewer, just in front of the face so
    /// that the face itself stays.
    pub fn on_face(point: Vec3, normal: Vec3, world: &Bounds) -> Option<Self> {
        let mut n = normal.normalize_or_zero();
        for c in [&mut n.x, &mut n.y, &mut n.z] {
            if c.abs() < SNAP {
                *c = 0.0;
            }
        }
        let normal = n.try_normalize()?;
        let diag = (Vec3::from_array(world.max) - Vec3::from_array(world.min)).length();
        Some(Self { normal, offset: normal.dot(point) + (diag * 1e-4).max(1e-4) })
    }

    /// Cut away the other side instead.
    pub fn flip(&mut self) {
        self.normal = -self.normal;
        self.offset = -self.offset;
    }

    pub fn cuts(&self, world: Vec3) -> bool {
        self.normal.dot(world) > self.offset
    }

    /// The offsets at which the plane just touches the model (world bounds), lowest first.
    pub fn range(&self, world: &Bounds) -> (f32, f32) {
        SectionBox::around(world).corners().iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| {
            let d = self.normal.dot(*c);
            (lo.min(d), hi.max(d))
        })
    }

    /// Move along the normal, kept within (just outside) the model.
    pub fn move_by(&mut self, delta: f32, world: &Bounds) {
        let (lo, hi) = self.range(world);
        self.offset = (self.offset + delta).clamp(lo, hi);
    }

    /// Axes in the plane: `u` horizontal and `v` upwards for vertical and sloped planes.
    pub fn basis(&self) -> (Vec3, Vec3) {
        let up = if self.normal.z.abs() < 0.99 { Vec3::Z } else { Vec3::X };
        let u = up.cross(self.normal).normalize();
        (u, self.normal.cross(u).normalize())
    }

    /// The rectangle on the plane round the model (world bounds), as its four corners.
    pub fn rectangle(&self, world: &Bounds) -> [Vec3; 4] {
        let (u, v) = self.basis();
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for c in SectionBox::around(world).corners() {
            let (a, b) = (u.dot(c), v.dot(c));
            lo = (lo.0.min(a), lo.1.min(b));
            hi = (hi.0.max(a), hi.1.max(b));
        }
        let pad = ((hi.0 - lo.0).max(hi.1 - lo.1) * 0.05).max(0.05);
        let base = self.normal * self.offset;
        let at = |a: f32, b: f32| base + u * a + v * b;
        [at(lo.0 - pad, lo.1 - pad), at(hi.0 + pad, lo.1 - pad), at(hi.0 + pad, hi.1 + pad), at(lo.0 - pad, hi.1 + pad)]
    }

    /// The middle of the rectangle, where the handle sits.
    pub fn handle(&self, world: &Bounds) -> Vec3 {
        let r = self.rectangle(world);
        (r[0] + r[2]) * 0.5
    }

    /// For the shader: normal + offset, rectangle middle + half size (the cut fill is drawn as a
    /// square on the plane).
    pub fn uniform(&self, world: &Bounds) -> [[f32; 4]; 2] {
        let r = self.rectangle(world);
        let middle = (r[0] + r[2]) * 0.5;
        let half = (r[1] - r[0]).length().max((r[3] - r[0]).length()) * 0.5;
        [[self.normal.x, self.normal.y, self.normal.z, self.offset], [middle.x, middle.y, middle.z, half]]
    }
}

