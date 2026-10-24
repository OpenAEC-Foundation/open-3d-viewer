//! Orbit camera for a Z-up scene.

use crate::model::Bounds;
use glam::{Mat4, Vec3};

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
}

/// Standard views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    Top,
    Front,
    Right,
    Iso,
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
        // Near plane relative to the distance keeps depth precision for both small parts and
        // whole buildings.
        let near = (self.distance * 0.002).max(1e-4);
        let far = self.distance + self.scene_radius * 4.0 + 1.0;
        Mat4::perspective_rh(self.fov_y, aspect.max(1e-3), near, far)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    pub fn set_preset(&mut self, preset: ViewPreset) {
        use std::f32::consts::FRAC_PI_2;
        let (yaw, pitch) = match preset {
            ViewPreset::Top => (-FRAC_PI_2, MAX_PITCH),
            ViewPreset::Front => (-FRAC_PI_2, 0.0),
            ViewPreset::Right => (0.0, 0.0),
            ViewPreset::Iso => (-std::f32::consts::FRAC_PI_4, (1.0f32 / 2f32.sqrt()).atan()),
        };
        self.yaw = yaw;
        self.pitch = pitch;
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
        let max = self.scene_radius * 100.0 + 10.0;
        self.distance = (self.distance * 0.85f32.powf(steps)).clamp(min.max(1e-4), max);
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
    fn fit_centres_on_bounds() {
        let mut c = Camera::default();
        c.fit(&Bounds { min: [0.0, 0.0, 0.0], max: [10.0, 20.0, 5.0] }, 1.5);
        assert_eq!(c.target, Vec3::new(5.0, 10.0, 2.5));
        assert!(c.distance > 11.0);
    }
}
