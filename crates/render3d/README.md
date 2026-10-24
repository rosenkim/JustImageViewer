[English](README.md) | [한국어](README_KR.md)

# render3d

A small viewport engine that shares the application's wgpu device and queue.
It is a separate workspace crate. It has no dependency on the app state, image
loader, window, or UI library.

Dependencies are kept to what rendering needs: `wgpu`, `gltf`, `glam`,
`bytemuck`, `image` (PNG and JPEG only), and `anyhow`.

| File (in `src/`) | Responsibility |
| --- | --- |
| `lib.rs` | Declare modules and the public API |
| `viewport.rs` | Combine a renderer, scene, and camera for simple use |
| `renderer.rs` | Reuse pipelines and record render passes |
| `scene.rs` | Own uploaded meshes, materials, and world bounds |
| `camera.rs` | Frame bounds and build a perspective view |
| `asset.rs` | Decode GLB geometry, node transforms, and the initial pose |
| `material.rs` | Upload PBR textures and material bindings |
| `offscreen.rs` | Own square render targets and read RGBA pixels |
| `shaders/model.wgsl` | Shade model surfaces |
| `tests.rs` | Check loading, camera validation, and native GPU rendering |
| `../tests/fixtures/` | Small GLB files used by the tests |

## Render an image

```rust,ignore
use render3d::{OffscreenTarget, Viewport3D};

let mut viewport = Viewport3D::from_glb(&device, &queue, path)?;
let image = viewport.render_offscreen(256)?;
// image.width, image.height, and image.pixels contain straight-alpha sRGB RGBA.

let mut target = OffscreenTarget::new(&device, 256)?;
viewport.camera_mut().eye.x += 1.0;
let image = viewport.render_to_target(&mut target)?;

// Loading another scene reuses the pipelines and frames the new bounds.
viewport.load_glb(other_path)?;
```

The size is a side length in pixels. It does not need to be a power of two.
Pixel readback waits on the calling thread, so background image generation can
run on a blocking worker.

## Share a renderer across scenes

```rust,ignore
use render3d::{Camera, Renderer};

let renderer = Renderer::new(&device, &queue);
let scene = renderer.load_scene(path)?;
let camera = Camera::frame(scene.bounds())?;
let mut target = renderer.create_target(256)?;
let image = renderer.render_to_target(&scene, &camera, &mut target)?;
```

## Display a texture without reading pixels

```rust,ignore
let mut encoder = device.create_command_encoder(&Default::default());
viewport.encode(&target, &mut encoder)?;
queue.submit([encoder.finish()]);
// Sample target.color_view() in a later UI pass. Its format is Rgba8UnormSrgb.
```

Use the same GPU device for the renderer, scenes, targets, and command encoder.
Each recorded pass gets its own camera uniform snapshot.

## Current scope

The loader supports self-contained GLB files with embedded buffers and textures,
triangle meshes, node transforms, default morph weights, and the initial skin
pose with up to four joint influences per vertex. Materials support the five
PBR texture slots, two UV sets, unlit shading, alpha masking and blending, and
double-sided faces. Transparent primitives are sorted by their centers.

Animation playback, external resources, compressed geometry, and additional
required glTF extensions are outside the current scope. The target is square.

Run the checks, including native GPU readback, with:

```sh
cargo test --locked --offline -p render3d -- --include-ignored
```
