//! Orbit camera for a Z-up scene, navigated about the point under the cursor (issue #324).

use crate::model::Bounds;
use glam::{Mat4, Quat, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub distance: f32,
    /// Rotation around Z (radians); 0 looks from +X towards the target.
    pub yaw: f32,
    /// Elevation above the XY plane (radians).
    pub pitch: f32,
    /// Vertical field of view (radians).
    pub fov_y: f32,
    /// Scene radius, used for near/far planes.
    pub scene_radius: f32,
    /// Orthographic instead of perspective projection.
    pub ortho: bool,
}

/// The least a wheel step moves the eye, as a share of how far the point under the cursor was
/// when it was found: about a dozen steps from there take the eye through it, whatever its size.
pub const MIN_STEP: f32 = 0.03;

/// Standard views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    Top,
    Bottom,
    Front,
    Back,
    Right,
    Left,
    Iso,
}

impl ViewPreset {
    /// Yaw and pitch of the preset (top and bottom just short of the pole).
    pub fn orientation(self) -> (f32, f32) {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        match self {
            ViewPreset::Top => (-FRAC_PI_2, MAX_PITCH),
            ViewPreset::Bottom => (-FRAC_PI_2, -MAX_PITCH),
            ViewPreset::Front => (-FRAC_PI_2, 0.0),
            ViewPreset::Back => (FRAC_PI_2, 0.0),
            ViewPreset::Right => (0.0, 0.0),
            ViewPreset::Left => (PI, 0.0),
            ViewPreset::Iso => (-FRAC_PI_4, (1.0f32 / 2f32.sqrt()).atan()),
        }
    }
}

const MAX_PITCH: f32 = 89.9 * std::f32::consts::PI / 180.0;

impl Default for Camera {
    fn default() -> Self {
        let mut c = Self {
            target: Vec3::ZERO,
            distance: 20.0,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 45f32.to_radians(),
            scene_radius: 10.0,
            ortho: false,
        };
        c.set_preset(ViewPreset::Iso);
        c
    }
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + self.distance * Vec3::new(cp * cy, cp * sy, sp)
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Z)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-3);
        if self.ortho {
            // Same framing as the perspective view at the target distance
            let h = self.distance * (self.fov_y * 0.5).tan();
            let depth = self.distance + self.scene_radius * 4.0 + 1.0;
            return Mat4::orthographic_rh(-h * aspect, h * aspect, -h, h, -depth, depth);
        }
        // Near plane relative to the distance keeps depth precision for both small parts and
        // whole buildings.
        let near = self.near();
        let far = self.distance + self.scene_radius * 4.0 + 1.0;
        Mat4::perspective_rh(self.fov_y, aspect, near, far)
    }

    /// The near plane of the perspective view: a fixed share of the distance to the target,
    /// so a part of a centimetre is seen as closely as a building.
    pub fn near(&self) -> f32 {
        (self.distance * 0.002).max(f32::MIN_POSITIVE)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    pub fn set_preset(&mut self, preset: ViewPreset) {
        let (yaw, pitch) = preset.orientation();
        self.yaw = yaw;
        self.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Frames the bounds (assumed in world space) for the given viewport aspect ratio.
    pub fn fit(&mut self, bounds: &Bounds, aspect: f32) {
        if !bounds.is_valid() {
            return;
        }
        let radius = bounds.radius().max(1e-3);
        self.target = Vec3::from_array(bounds.center());
        self.scene_radius = radius;
        let half_v = self.fov_y * 0.5;
        let half_h = (half_v.tan() * aspect.max(0.1)).atan();
        let half = half_v.min(half_h);
        self.distance = radius / half.sin() * 1.15;
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Pans by a screen offset in logical pixels for a viewport of the given height.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
        let world_per_pixel = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        let forward = (self.target - self.eye()).normalize_or(Vec3::Y);
        let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
        let up = right.cross(forward);
        self.target += (-dx * right + dy * up) * world_per_pixel;
    }

    /// Zooms by wheel steps (positive = closer).
    pub fn zoom(&mut self, steps: f32) {
        let min = self.scene_radius * 1e-4;
        let max = self.scene_radius * 100.0;
        self.distance = (self.distance * 0.85f32.powf(steps)).clamp(min.max(f32::MIN_POSITIVE), max.max(min));
    }

    /// Zooms towards the point under the cursor (as Revit): `ndc` in -1..1, y up. The view scales
    /// about the cursor's point on the plane through the target, so that point stays put.
    pub fn zoom_at(&mut self, steps: f32, ndc: (f32, f32), aspect: f32) {
        let before = self.distance;
        self.zoom(steps);
        let f = self.distance / before;
        let forward = (self.target - self.eye()).normalize_or(Vec3::Y);
        let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
        let up = right.cross(forward);
        let half_h = before * (self.fov_y * 0.5).tan();
        let p = self.target + right * (ndc.0 * half_h * aspect.max(1e-3)) + up * (ndc.1 * half_h);
        self.target = p + (self.target - p) * f;
    }

    /// The unit direction the camera looks in.
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        -Vec3::new(cp * cy, cp * sy, sp)
    }

    /// The ray (world) through `ndc` (-1..1, y up): where it starts (on the near plane) and its
    /// unit direction.
    pub fn ray(&self, ndc: (f32, f32), aspect: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect).inverse();
        let a = inv.project_point3(Vec3::new(ndc.0, ndc.1, 0.0));
        let b = inv.project_point3(Vec3::new(ndc.0, ndc.1, 1.0));
        (a, (b - a).normalize_or(self.forward()))
    }

    /// Orbits as [`Camera::orbit`], about `pivot` instead of the target: the eye and the target
    /// turn about it by the same rotation, so the view never jumps towards the pivot when it lies
    /// off the middle of the screen (issue #324, A).
    pub fn orbit_about(&mut self, pivot: Vec3, dx: f32, dy: f32) {
        let before = *self;
        self.orbit(dx, dy);
        // turned up about the camera's right, then about the world's Z
        let d0 = -before.forward();
        let tilt = Quat::from_axis_angle(d0.cross(Vec3::Z).normalize_or(Vec3::X), self.pitch - before.pitch);
        let turn = Quat::from_rotation_z(self.yaw - before.yaw) * tilt;
        self.target = pivot + turn * (before.target - pivot);
    }

    /// Turns the view about the eye by a drag of (dx, dy) logical pixels, the eye staying where it
    /// is: looking around from where one stands, as in a game: the mouse to the right looks to
    /// the right, up looks up.
    pub fn look_around(&mut self, dx: f32, dy: f32) {
        let eye = self.eye();
        self.yaw -= dx * 0.005;
        self.pitch = (self.pitch + dy * 0.005).clamp(-MAX_PITCH, MAX_PITCH);
        self.target = eye + self.forward() * self.distance;
    }

    /// Pans as [`Camera::pan`], with the screen moved as at `depth` in front of the eye (the
    /// depth of the point grabbed), so what was grabbed follows the cursor (issue #324, B).
    pub fn pan_at(&mut self, dx: f32, dy: f32, viewport_height: f32, depth: f32) {
        if self.ortho {
            return self.pan(dx, dy, viewport_height);
        }
        let world_per_pixel = 2.0 * depth.max(self.near()) * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        let forward = self.forward();
        let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
        let up = right.cross(forward);
        self.target += (-dx * right + dy * up) * world_per_pixel;
    }

    /// Moves the eye by wheel steps (positive = closer) along the ray towards `anchor`, the point
    /// under the cursor: each step a fifth of the way there (back out a quarter), and in at least
    /// `min` a step, so a surface close by is passed through instead of crept up to, as BIM Vision
    /// and xeokit (`dollyMinSpeed`) do. `min` is a share of how far the anchor was when it was
    /// found ([`MIN_STEP`]): nothing here is a fixed length, so a part of a centimetre zooms as a
    /// district does. The orientation stays; the target moves along, then lies at the depth of
    /// the anchor (the near plane follows what is looked at). Returns whether the eye passed the
    /// anchor.
    pub fn dolly_to(&mut self, anchor: Vec3, steps: f32, min: f32) -> bool {
        if self.ortho || steps == 0.0 {
            return false;
        }
        let eye = self.eye();
        let to = anchor - eye;
        let d = to.length();
        // a point behind the eye (passed already) is no way to go: in goes ahead
        let dir = if d > 0.0 && to.dot(self.forward()) > 0.0 { to / d } else { self.forward() };
        // the way back out is the way in reversed
        let mut by = d.max(min) * (1.0 - 0.8f32.powf(steps));
        if steps > 0.0 {
            by = by.max(min * steps);
        } else if d > self.scene_radius * 100.0 {
            return false;
        }
        let eye = eye + dir * by;
        let forward = self.forward();
        let depth = (anchor - eye).dot(forward);
        if depth > min {
            self.distance = depth;
        }
        self.target = eye + forward * self.distance;
        by >= d
    }

    /// The camera between `self` (t = 0) and `to` (t = 1): yaw the short way round, distance
    /// geometric.
    pub fn lerp(&self, to: &Camera, t: f32) -> Camera {
        let tau = std::f32::consts::TAU;
        let dyaw = (to.yaw - self.yaw + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
        Camera {
            target: self.target.lerp(to.target, t),
            distance: self.distance * (to.distance / self.distance.max(1e-6)).powf(t),
            yaw: self.yaw + dyaw * t,
            pitch: self.pitch + (to.pitch - self.pitch) * t,
            scene_radius: to.scene_radius,
            ..*to
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_view_looks_down() {
        let mut c = Camera::default();
        c.set_preset(ViewPreset::Top);
        assert!(c.eye().z > c.target.z + c.distance * 0.99);
    }

    #[test]
    fn zoom_at_keeps_the_point_under_the_cursor() {
        let mut c = Camera::default();
        c.set_preset(ViewPreset::Top);
        let aspect = 1.5;
        let ndc = (0.5, -0.25);
        let world = |c: &Camera| c.view_proj(aspect).inverse().project_point3(Vec3::new(ndc.0, ndc.1, 0.5));
        let ray = |c: &Camera| {
            let inv = c.view_proj(aspect).inverse();
            let a = inv.project_point3(Vec3::new(ndc.0, ndc.1, 0.0));
            let b = inv.project_point3(Vec3::new(ndc.0, ndc.1, 1.0));
            // where the cursor ray meets the plane z = target.z
            a + (b - a) * ((c.target.z - a.z) / (b.z - a.z))
        };
        let before = ray(&c);
        let _ = world(&c);
        c.zoom_at(2.0, ndc, aspect);
        let after = ray(&c);
        assert!((before - after).length() < 1e-3 * c.distance.max(1.0), "{before} vs {after}");
    }

    /// Where `p` shows on the screen (NDC).
    fn on_screen(c: &Camera, p: Vec3) -> glam::Vec2 {
        c.view_proj(1.5).project_point3(p).truncate()
    }

    #[test]
    fn orbiting_about_a_pivot_keeps_it_where_it_shows() {
        let mut c = Camera { scene_radius: 500.0, distance: 400.0, ..Camera::default() };
        // a pivot off the middle of the screen, well before the target
        let pivot = c.eye() + (c.target - c.eye()).normalize() * 50.0 + Vec3::new(3.0, -2.0, 1.0);
        let before = on_screen(&c, pivot);
        for _ in 0..5 {
            c.orbit_about(pivot, 25.0, -12.0);
        }
        assert!((on_screen(&c, pivot) - before).length() < 1e-3, "{before} vs {}", on_screen(&c, pivot));
        // the eye kept its distance to the pivot, the target its distance to the eye
        assert!(((c.eye() - pivot).length() - (Camera { scene_radius: 500.0, distance: 400.0, ..Camera::default() }.eye() - pivot).length()).abs() < 1e-2);
    }

    #[test]
    fn the_wheel_goes_by_the_distance_to_the_point_not_the_size_of_the_model() {
        // a whole district framed, a house wall 10 m ahead of the eye
        let mut c = Camera { scene_radius: 1000.0, distance: 2000.0, ..Camera::default() };
        let wall = c.eye() + c.forward() * 10.0;
        let eye = c.eye();
        let min = 10.0 * MIN_STEP;
        assert!(!c.dolly_to(wall, 1.0, min));
        let step = (c.eye() - eye).length();
        assert!((step - 2.0).abs() < 1e-2, "{step}");
        // the target now lies at the wall's depth, the near plane with it
        assert!((c.distance - 8.0).abs() < 1e-2, "{}", c.distance);
        // the wall stays under the cursor, the view turned not
        assert!(on_screen(&c, wall).length() < 1e-4);
        // close by, each step at least 30 cm: the wall is passed, not crept up to
        let passed = (0..40).any(|_| c.dolly_to(wall, 1.0, min));
        assert!(passed && (c.eye() - wall).dot(c.forward()) > 0.0);
        // a step more goes on ahead, not back to the wall
        let eye = c.eye();
        c.dolly_to(wall, 1.0, min);
        assert!((c.eye() - eye).dot(c.forward()) > 0.0);
        // out again by a quarter of the way per step
        let mut c = Camera { scene_radius: 1000.0, distance: 2000.0, ..Camera::default() };
        let eye = c.eye();
        c.dolly_to(eye + c.forward() * 10.0, -1.0, min);
        assert!(((c.eye() - eye).length() - 2.5).abs() < 1e-2);
    }

    #[test]
    fn a_part_of_a_centimetre_zooms_as_a_district_does() {
        // the same view at every scale: as many steps to the point and through it
        let steps_through = |scale: f32| {
            let mut c = Camera { scene_radius: 100.0 * scale, distance: 200.0 * scale, ..Camera::default() };
            let point = c.eye() + c.forward() * 3.0 * scale;
            let min = 3.0 * scale * MIN_STEP;
            let mut steps = 0;
            while !c.dolly_to(point, 1.0, min) {
                steps += 1;
                assert!(steps < 100);
            }
            steps
        };
        let district = steps_through(10.0);
        assert_eq!(steps_through(0.0001), district);
        assert_eq!(steps_through(1.0), district);
        assert!((8..=20).contains(&district), "{district}");
    }

    #[test]
    fn looking_around_keeps_the_eye_and_turns_as_in_a_game() {
        let mut c = Camera { scene_radius: 100.0, distance: 50.0, ..Camera::default() };
        let (eye, ahead) = (c.eye(), c.target);
        c.look_around(40.0, -20.0);
        assert!((c.eye() - eye).length() < 1e-3);
        // looking to the right and up: what was ahead now shows left and below
        let moved = on_screen(&c, ahead);
        assert!(moved.x < -0.05 && moved.y < -0.05, "{moved}");
    }

    #[test]
    fn panning_at_a_depth_moves_what_lies_there_with_the_cursor() {
        let mut c = Camera { scene_radius: 1000.0, distance: 1000.0, ..Camera::default() };
        let near = c.eye() + c.forward() * 20.0;
        let before = on_screen(&c, near);
        // 100 pixels to the right in a viewport 800 high (and 1200 wide)
        c.pan_at(100.0, 0.0, 800.0, 20.0);
        let moved = on_screen(&c, near) - before;
        assert!((moved.x - 100.0 / 600.0).abs() < 1e-3 && moved.y.abs() < 1e-3, "{moved}");
    }

    #[test]
    fn lerp_turns_the_short_way() {
        let mut a = Camera::default();
        a.yaw = 3.0;
        let mut b = a;
        b.yaw = -3.0;
        let half = a.lerp(&b, 0.5);
        assert!((half.yaw.rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs() < 1e-3, "{}", half.yaw);
    }

    #[test]
    fn fit_centres_on_bounds() {
        let mut c = Camera::default();
        c.fit(&Bounds { min: [0.0, 0.0, 0.0], max: [10.0, 20.0, 5.0] }, 1.5);
        assert_eq!(c.target, Vec3::new(5.0, 10.0, 2.5));
        assert!(c.distance > 11.0);
    }
}
