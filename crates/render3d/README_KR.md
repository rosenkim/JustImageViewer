[English](README.md) | [한국어](README_KR.md)

# render3d

애플리케이션의 wgpu device와 queue를 함께 사용하는 작은 뷰포트 엔진.
별도의 workspace crate이며, 앱 상태, 이미지 로더, 윈도우, UI 라이브러리에
의존하지 않는다.

의존성은 렌더링에 필요한 것만 둔다: `wgpu`, `gltf`, `glam`, `bytemuck`,
`image` (PNG와 JPEG만), `anyhow`.

| 파일 (`src/` 기준) | 역할 |
| --- | --- |
| `lib.rs` | 모듈과 공개 API 선언 |
| `viewport.rs` | 간단한 사용을 위해 renderer, scene, camera를 묶음 |
| `renderer.rs` | 파이프라인 재사용과 render pass 기록 |
| `scene.rs` | 업로드된 mesh, material, 월드 bounds 소유 |
| `camera.rs` | bounds에 맞춰 화면 구성, 원근 뷰 생성 |
| `asset.rs` | GLB geometry, node transform, 초기 pose 디코딩 |
| `material.rs` | PBR 텍스처와 material binding 업로드 |
| `offscreen.rs` | 정사각형 render target 소유와 RGBA 픽셀 읽기 |
| `shaders/model.wgsl` | 모델 표면 셰이딩 |
| `tests.rs` | 로딩, 카메라 검증, 네이티브 GPU 렌더링 확인 |
| `../tests/fixtures/` | 테스트에 쓰는 작은 GLB 파일 |

## 이미지 렌더링

```rust,ignore
use render3d::{OffscreenTarget, Viewport3D};

let mut viewport = Viewport3D::from_glb(&device, &queue, path)?;
let image = viewport.render_offscreen(256)?;
// image.width, image.height, image.pixels 에는 straight-alpha sRGB RGBA 가 들어 있다.

let mut target = OffscreenTarget::new(&device, 256)?;
viewport.camera_mut().eye.x += 1.0;
let image = viewport.render_to_target(&mut target)?;

// 다른 scene을 불러와도 파이프라인은 재사용하고, 새 bounds에 맞춰 화면을 구성한다.
viewport.load_glb(other_path)?;
```

크기는 한 변의 픽셀 길이이며, 2의 거듭제곱일 필요는 없다.
픽셀 읽기(readback)는 호출한 스레드에서 기다리므로, 백그라운드 이미지 생성은
blocking worker에서 실행하면 된다.

## 여러 scene에서 renderer 공유

```rust,ignore
use render3d::{Camera, Renderer};

let renderer = Renderer::new(&device, &queue);
let scene = renderer.load_scene(path)?;
let camera = Camera::frame(scene.bounds())?;
let mut target = renderer.create_target(256)?;
let image = renderer.render_to_target(&scene, &camera, &mut target)?;
```

## 픽셀을 읽지 않고 텍스처 표시

```rust,ignore
let mut encoder = device.create_command_encoder(&Default::default());
viewport.encode(&target, &mut encoder)?;
queue.submit([encoder.finish()]);
// 이후 UI pass에서 target.color_view()를 샘플링한다. 포맷은 Rgba8UnormSrgb.
```

renderer, scene, target, command encoder는 모두 같은 GPU device를 사용해야 한다.
기록되는 pass마다 각자의 camera uniform 스냅샷을 가진다.

## 현재 지원 범위

로더는 버퍼와 텍스처가 내장된 독립형 GLB 파일, 삼각형 mesh, node transform,
기본 morph weight, 정점당 최대 4개 joint 영향을 받는 초기 skin pose를 지원한다.
Material은 5개의 PBR 텍스처 슬롯, 2개의 UV 세트, unlit 셰이딩, 알파 마스킹과
블렌딩, 양면 렌더링을 지원한다. 투명한 primitive는 중심점 기준으로 정렬한다.

애니메이션 재생, 외부 리소스, 압축된 geometry, 그 밖의 필수 glTF 확장은
현재 범위에 포함되지 않는다. Target은 정사각형이다.

네이티브 GPU readback을 포함한 검사 실행:

```sh
cargo test --locked --offline -p render3d -- --include-ignored
```
