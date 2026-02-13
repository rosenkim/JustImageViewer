# Port Plan: eframe/egui -> imgui-rs (SDL2 + OpenGL)

## Goals
- Recreate the current Vibe Image Viewer UX on `imgui-rs` with an SDL2 + OpenGL runtime.
- Preserve current features: folder open, drag-and-drop (folder/file), library list selection, image preview + metadata, keyboard navigation, status messaging, and config persistence.
- Keep cross-platform behavior on macOS, Windows, Linux.

## Current Baseline (What Exists Today)
- `src/main.rs` starts `eframe` (`Wgpu`) and mounts `ViewerApp`.
- `src/app/mod.rs` currently owns both state and egui rendering:
  - Top menu: File/View/Help
  - Left library panel
  - Center image panel with fit-to-view behavior
  - Bottom status bar
  - Drop handling + keyboard navigation
- `src/core/media.rs` scans folders for PNG/JPEG.
- `src/core/image_loader.rs` decodes into RGBA.
- `src/infra/config.rs`, `src/infra/logging.rs` already provide persistence/logging.

## Technical Decisions
- Runtime stack: `sdl2` + `imgui` + `imgui-sdl2` + `imgui-opengl-renderer`.
- Rendering path: OpenGL textures created from decoded RGBA buffers.
- Keep existing `core` and `infra` modules; move UI-framework-specific code out of core app state.
- Do not mix in `winit` support libraries in this migration.

## Work Plan
1. **Bootstrap new runtime entrypoint**
- Add SDL2/imgui/OpenGL dependencies in `Cargo.toml`.
- Replace eframe startup in `src/main.rs` with:
  - SDL2 init (video/events)
  - OpenGL context + window creation
  - imgui context + fonts/style setup
  - `imgui-sdl2` integration and frame loop
- Preserve current logging/config bootstrap and Windows subsystem behavior.

2. **Split state from UI framework**
- Extract framework-agnostic app state/actions from `ViewerApp` into backend-neutral module(s).
- Keep actions as explicit methods: `load_folder`, `handle_drop_path`, `advance_selection`, `open_folder_dialog`, etc.
- Keep derived read models for UI: selected entry, status text, image metadata.

3. **Introduce texture manager**
- Add OpenGL texture lifecycle module:
  - upload RGBA buffers (`glTexImage2D`)
  - cache/reuse by file path where safe
  - destroy textures on replacement/shutdown
- UI state should reference texture keys/ids from this manager, not egui `TextureHandle`.

4. **Rebuild UI in imgui**
- Recreate existing layout with imgui windows/child regions:
  - top menu bar
  - left library list
  - center image viewport
  - bottom status bar
- Re-implement current interactions:
  - library click selection
  - arrow/page navigation
  - File -> Open Folder
  - drag/drop folder or file
- Maintain fit-to-view image scaling and metadata display.

5. **Wire SDL events + shortcuts**
- Translate SDL event stream into imgui input through `imgui-sdl2`.
- Handle app-level events directly from SDL:
  - `Quit`
  - `DropFile`/`DropText` as needed
  - resize events for viewport/scale correctness
- Implement cross-platform modifier handling (`Cmd` on macOS, `Ctrl` elsewhere) for Open Folder.

6. **Persistence and shutdown**
- Keep existing config semantics (including restore-last-folder behavior).
- Ensure status messages mirror existing behavior for cancel/success/error states.
- On shutdown, release GL textures and exit cleanly without resource leaks.

## Validation Checklist (Definition of Done)
- App starts to an empty state without panic.
- Open folder via menu loads supported images and selects first image.
- Drag folder works; drag file opens parent and focuses dropped file when present.
- Arrow/Page keys cycle images correctly.
- Viewer scales large images to fit viewport and keeps metadata visible.
- Status bar updates for cancel, load success, and decode failure paths.
- Last-folder behavior remains consistent with config settings.
- No obvious GL texture leak during rapid image switching.

## Risks and Mitigations
- **OpenGL setup differences by platform**: isolate GL init code and validate per OS early.
- **HiDPI scaling issues**: use drawable size vs logical size correctly each frame.
- **Drop path encoding**: normalize and validate UTF-8/path conversion before use.
- **State/UI coupling regressions**: enforce state-only unit tests for folder scan/selection transitions.

## Deliverables
- Updated `Cargo.toml` and SDL2/imgui-based `src/main.rs`.
- New backend-neutral app-state module(s).
- New OpenGL texture manager module.
- imgui-based UI rendering layer replacing egui panels.
- Brief README update for SDL2/OpenGL build/runtime prerequisites.
