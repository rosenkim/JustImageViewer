use std::path::Path;

use glam::Vec3;

use crate::asset::ModelAsset;
use crate::camera::CameraUniform;
use crate::material::MaterialUniform;
use crate::*;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn shader_matches_uniforms() {
    let module = naga::front::wgsl::parse_str(include_str!("shaders/model.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
    for (_, ty) in module.types.iter() {
        let expected = match ty.name.as_deref() {
            Some("Camera") => std::mem::size_of::<CameraUniform>(),
            Some("Material") => std::mem::size_of::<MaterialUniform>(),
            _ => continue,
        };
        if let naga::TypeInner::Struct { span, .. } = ty.inner {
            assert_eq!(span as usize, expected);
        }
    }
}

#[test]
fn initial_pose_and_embedded_textures() {
    for (name, min, max) in [
        ("textured.glb", [-1.0, -1.0, 0.0], [1.0, 1.0, 0.0]),
        ("transformed.glb", [0.0, 1.0, 4.0], [4.0, 5.0, 4.0]),
        ("morph.glb", [-1.0, -1.0, 0.5], [1.0, 1.0, 0.5]),
        ("skin.glb", [-1.0, 1.0, 0.0], [1.0, 3.0, 0.0]),
    ] {
        let asset = ModelAsset::load(&fixture(name)).unwrap();
        assert!(asset.min.abs_diff_eq(Vec3::from_array(min), 1e-5), "{name}");
        assert!(asset.max.abs_diff_eq(Vec3::from_array(max), 1e-5), "{name}");
        assert_eq!(asset.primitives[0].indices, [0, 1, 2]);
        let image = asset
            .image(asset.document.images().next().unwrap())
            .unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [220, 70, 40, 255]);
    }
    assert!(ModelAsset::load(&fixture("image.png")).is_err());
    assert!(ModelAsset::load(&fixture("missing.glb")).is_err());
}

#[test]
fn camera_validates_view_and_projection() {
    let bounds = Bounds {
        min: Vec3::splat(-1.0),
        max: Vec3::splat(1.0),
    };
    let camera = Camera::frame(bounds).unwrap();
    assert_eq!(camera.target, Vec3::ZERO);
    assert!(camera.uniform(1.0).is_ok());

    let mut invalid = camera.clone();
    invalid.eye = invalid.target;
    assert!(invalid.uniform(1.0).is_err());
    invalid = camera.clone();
    invalid.up = invalid.target - invalid.eye;
    assert!(invalid.uniform(1.0).is_err());
    invalid = camera.clone();
    invalid.near = invalid.far;
    assert!(invalid.uniform(1.0).is_err());
    assert!(camera.uniform(0.0).is_err());
    assert!(
        Camera::frame(Bounds {
            min: Vec3::ONE,
            max: Vec3::ZERO
        })
        .is_err()
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn offscreen_gpu_readback() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: None,
        force_fallback_adapter: false,
        power_preference: wgpu::PowerPreference::LowPower,
    }))
    .expect("a native GPU adapter is required for this test");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("model render test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        memory_hints: Default::default(),
        trace: wgpu::Trace::Off,
    }))
    .unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut target = OffscreenTarget::new(&device, 65).unwrap();
    assert_eq!(target.size(), 65);
    assert!(OffscreenTarget::new(&device, 0).is_err());
    assert!(OffscreenTarget::new(&device, device.limits().max_texture_dimension_2d + 1).is_err());
    let renderer = Renderer::new(&device, &queue);
    for name in [
        "textured.glb",
        "transformed.glb",
        "morph.glb",
        "skin.glb",
        "transparent.glb",
        "cube.glb",
    ] {
        // One renderer and target serve multiple scenes without rebuilding pipelines.
        let scene = renderer.load_scene(&fixture(name)).unwrap();
        let camera = Camera::frame(scene.bounds()).unwrap();
        let image = renderer
            .render_to_target(&scene, &camera, &mut target)
            .unwrap();
        assert_eq!(
            (image.width, image.height, image.pixels.len()),
            (65, 65, 65 * 65 * 4)
        );
        let visible: Vec<_> = image
            .pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 0)
            .collect();
        assert!(
            visible.len() > 50,
            "{name}: model must cover visible pixels"
        );
        assert!(
            visible.iter().any(|pixel| pixel[0] > 0),
            "{name}: material must contribute color"
        );
        assert_eq!(
            &image.pixels[..4],
            &[0, 0, 0, 0],
            "{name}: corners stay transparent"
        );
        if name == "transparent.glb" {
            assert!(
                visible
                    .iter()
                    .all(|pixel| (i16::from(pixel[3]) - 102).abs() <= 1)
            );
        }
    }
    let mut viewport = Viewport3D::from_glb(&device, &queue, &fixture("cube.glb")).unwrap();
    let small = viewport.render_offscreen(31).unwrap();
    assert_eq!(small.pixels.len(), 31 * 31 * 4);
    let old_bounds = viewport.scene().bounds();
    assert!(viewport.load_glb(&fixture("missing.glb")).is_err());
    assert_eq!(viewport.scene().bounds().min, old_bounds.min);
    viewport.camera_mut().eye.x *= -1.0;
    let rotated = viewport.render_offscreen(31).unwrap();
    assert_ne!(small.pixels, rotated.pixels);
    viewport.load_glb(&fixture("textured.glb")).unwrap();
    let expected = viewport.render_to_target(&mut target).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    viewport.encode(&target, &mut encoder).unwrap();
    let encoded = target.read_pixels(&device, &queue, encoder).unwrap();
    assert_eq!(encoded.pixels, expected.pixels);
    let _view = target.color_view();
    assert!(pollster::block_on(device.pop_error_scope()).is_none());
}
