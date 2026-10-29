use anyhow::Result;
use render3d::{Bounds, Camera};

pub struct ModelCamera {
    pub camera: Camera,
    initial: Camera,
    radius: f32,
}

impl ModelCamera {
    pub fn new(bounds: Bounds) -> Result<Self> {
        let camera = Camera::frame(bounds)?;
        Ok(Self {
            initial: camera.clone(),
            camera,
            radius: ((bounds.max - bounds.min).length() * 0.5).max(0.001),
        })
    }

    pub fn reset(&mut self) {
        self.camera = self.initial.clone();
    }

    pub fn orbit(&mut self, delta: [f32; 2]) {
        let mut offset = self.camera.eye - self.camera.target;
        let distance = offset.length();
        let yaw = offset.x.atan2(offset.z) - delta[0] * 0.01;
        let pitch =
            ((offset.y / distance).clamp(-1.0, 1.0).asin() + delta[1] * 0.01).clamp(-1.55, 1.55);
        offset.x = yaw.sin() * pitch.cos() * distance;
        offset.y = pitch.sin() * distance;
        offset.z = yaw.cos() * pitch.cos() * distance;
        self.camera.eye = self.camera.target + offset;
    }

    pub fn pan(&mut self, delta: [f32; 2], side: f32) {
        let offset = self.camera.eye - self.camera.target;
        let right = (-offset).normalize().cross(self.camera.up).normalize();
        let up = right.cross((-offset).normalize());
        let scale = 2.0 * offset.length() * (self.camera.fov_y * 0.5).tan() / side.max(1.0);
        let shift = (right * -delta[0] + up * delta[1]) * scale;
        self.camera.eye += shift;
        self.camera.target += shift;
    }

    pub fn zoom(&mut self, wheel: f32) {
        let offset = self.camera.eye - self.camera.target;
        let distance = (offset.length() * (-wheel * 0.15).exp())
            .clamp(self.radius * 0.05, self.radius * 100.0);
        self.camera.eye = self.camera.target + offset.normalize() * distance;
        self.camera.far = distance + self.radius * 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_controls_stay_valid_and_reset() {
        let mut view = ModelCamera::new(Bounds {
            min: [-1.0; 3].into(),
            max: [1.0; 3].into(),
        })
        .unwrap();
        let initial = view.camera.clone();
        view.orbit([10000.0, 10000.0]);
        let direction = view.camera.target - view.camera.eye;
        assert!(direction.cross(view.camera.up).length() > 0.0);
        let distance = direction.length();
        view.pan([80.0, 30.0], 500.0);
        assert!((view.camera.eye.distance(view.camera.target) - distance).abs() < 0.001);
        assert_ne!(view.camera.target, initial.target);
        for wheel in [-10000.0, 10000.0] {
            view.zoom(wheel);
            assert!(view.camera.eye.is_finite());
            assert!(view.camera.eye.distance(view.camera.target) > 0.0);
            assert!(view.camera.far > view.camera.near);
        }
        view.reset();
        assert_eq!(view.camera.eye, initial.eye);
        assert_eq!(view.camera.target, initial.target);
        assert_eq!(view.camera.far, initial.far);
    }
}
