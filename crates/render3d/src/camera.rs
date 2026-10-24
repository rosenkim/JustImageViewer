use anyhow::{Result, ensure};
use glam::{Mat4, Vec3};

use crate::scene::Bounds;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CameraUniform {
    matrix: [[f32; 4]; 4],
    eye: [f32; 4],
}

/// A right-handed perspective camera. Angles are in radians.
#[derive(Debug, Clone)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    /// Fit a square viewport around the scene with a small margin.
    pub fn frame(bounds: Bounds) -> Result<Self> {
        let center = (bounds.min + bounds.max) * 0.5;
        let radius = ((bounds.max - bounds.min).length() * 0.5).max(0.001);
        ensure!(
            bounds.min.is_finite()
                && bounds.max.is_finite()
                && bounds.min.cmple(bounds.max).all()
                && center.is_finite()
                && radius.is_finite(),
            "scene bounds cannot be framed"
        );
        let fov_y = 45.0_f32.to_radians();
        let distance = radius / (fov_y * 0.5).sin() * 1.1;
        Ok(Self {
            eye: center + Vec3::new(1.3, 0.9, 2.0).normalize() * distance,
            target: center,
            up: Vec3::Y,
            fov_y,
            near: radius * 0.01,
            far: distance + radius * 3.0,
        })
    }

    pub(crate) fn uniform(&self, aspect: f32) -> Result<CameraUniform> {
        let direction = self.target - self.eye;
        ensure!(
            self.eye.is_finite()
                && self.target.is_finite()
                && self.up.is_finite()
                && direction.length_squared() > 0.0
                && direction.cross(self.up).length_squared() > 0.0,
            "camera must have a finite position and a valid view direction"
        );
        ensure!(
            aspect.is_finite()
                && aspect > 0.0
                && self.fov_y.is_finite()
                && self.fov_y > 0.0
                && self.fov_y < std::f32::consts::PI
                && self.near.is_finite()
                && self.far.is_finite()
                && self.near > 0.0
                && self.far > self.near,
            "invalid camera projection"
        );
        let matrix = Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far)
            * Mat4::look_at_rh(self.eye, self.target, self.up.normalize());
        ensure!(matrix.is_finite(), "camera matrix is not finite");
        Ok(CameraUniform {
            matrix: matrix.to_cols_array_2d(),
            eye: [self.eye.x, self.eye.y, self.eye.z, 1.0],
        })
    }
}
