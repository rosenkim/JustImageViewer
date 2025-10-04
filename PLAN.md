# Port Plan: eframe → imgui-rs (SDL backend)

## Goals
- Recreate the existing Vibe Image Viewer experience (see screenshot in request) using `imgui-rs` with an SDL2 windowing/backend stack.
- Preserve core functionality: folder selection, drag-and-drop folders/files, sortable library list, image preview with metadata, status reporting, and configuration persistence.
- Maintain portability across macOS, Windows, and Linux while matching the app's dark theme aesthetics.

## Current State Recap (eframe/egui)
- `src/main.rs` launches an `eframe` window with WGPU renderer and mounts `ViewerApp`.
- `ViewerApp` in `src/app/mod.rs` orchestrates state (`AppState`) and renders via egui panels:
  - Top menu bar with File/View/Help menus and quit handling.
  - Left `SidePanel` showing the "Library" list and folder info.
  - Central image viewer area with zoom-to-fit, metadata labels, and drop handling.
  - Bottom status bar displaying configuration info and status messages.
- `core::media` scans directories for PNG/JPEG images, while `core::image_loader` decodes images into RGBA buffers.
- File dialogs rely on `rfd`; logging/config layers live under `infra`.

## Target Stack & Crates
- Push renderer/window stack to SDL2 + OpenGL for `imgui-rs`.
  - `sdl2` for window, GL context, input, drag-and-drop events.
  - `imgui`, `imgui-sdl2` for event integration, and `imgui-opengl-renderer` (or `imgui-winit-support` alternative if GL unsuitable) for drawing.
  - Retain `image`, `rfd`, `anyhow`, logging crates already in use.
- Introduce a texture manager abstraction to upload `DecodedImage` data into OpenGL textures and recycle them when switching images.

## Implementation Phases
1. **Bootstrap SDL + imgui runtime**
   - Extend `Cargo.toml` with SDL2/imgui dependencies and feature flags (ensure `bundled` feature for SDL on macOS if needed).
   - Initialize SDL2 (video, events) and create an OpenGL-enabled window sized similar to current app (min 1024×768).
   - Create an `imgui::Context`, configure style to mimic the dark look from the screenshot, and install `imgui-sdl2` input pump + `imgui_opengl_renderer::Renderer`.
   - Port logging/config bootstrap code into the new `main` while honoring `windows_subsystem` attribute behavior.

2. **Restructure application state**
   - Extract `ViewerApp` state logic into a backend-agnostic core module (reusing `AppState`, `format_file_size`, loaders, navigation helpers).
   - Expose methods for UI layer to query state (`status_message`, `media_items`, `current_texture_path`, etc.) and to trigger actions (`load_folder`, `advance_selection`, `handle_drop`, menu actions).
   - Replace egui texture management with hooks to the new OpenGL texture store (store `Option<TextureId>` instead of `TextureHandle`).

3. **Implement SDL event loop & interaction glue**
   - Translate SDL events (keyboard, mouse wheel, window resize, drop) into imgui inputs via `imgui_sdl2::ImguiSdl2`.
   - Map shortcuts: Arrow keys, page navigation, ⌘/Ctrl+O for open folder, file-drop handlers to call existing folder/image loaders.
   - Forward quit requests (`SDL_QUIT`) to terminate cleanly; replicate menu-triggered quit by setting loop flag.

4. **Rebuild UI with imgui windows**
   - Construct a main dockspace or manual layout mirroring the screenshot: top menu bar, left child window (`Window::new("Library")` + `ChildWindow` for scroll list), central viewer, bottom status bar (`imgui::WindowFlags::NoTitleBar` etc.).
   - Implement menu bar with File/View/Help entries; wire actions to state methods and placeholder overlays.
   - In the viewer area, calculate image fit scaling manually and render with `imgui::Image` using OpenGL texture id; display metadata stack below (text, separators) to mimic current layout.

5. **OpenGL texture pipeline**
   - Build a `TextureManager` that loads `DecodedImage` into GL textures (glTexImage2D); reuse textures when reloading same path; free textures when image changes or on shutdown.
   - Handle hi-DPI scaling (SDL2 `dpi` queries + imgui `fonts.tex_filters`) to ensure crisp rendering on macOS Retina like the screenshot.

6. **Persistence & status updates**
   - Keep `infra::config` for restoring last folder preference; persist changed settings when closing.
   - Ensure status messages update across the SDL loop (e.g., when folder dialog canceled, load success/fail, decode errors) and render in bottom bar.

7. **Testing & polish**
   - Manual test flows: startup without folder, open folder via dialog, drag/drop folder, navigate with keyboard, error handling for unsupported files.
   - Validate hot reload of textures when switching images quickly to avoid leaks.
   - Confirm graceful shutdown (destroy GL textures, drop SDL/ImGui contexts).

## Risk Notes & Mitigations
- **SDL & imgui dependency setup**: GL loader differences per platform; gate with cfgs or use `glow` as abstraction if issues arise.
- **Hi-DPI handling**: need correct framebuffer scale on macOS; query `window.drawable_size()` and feed to renderer each frame.
- **Drag-and-drop events**: SDL returns raw C strings; ensure UTF-8 conversion and proper path normalization.
- **Texture lifetime**: avoid stale `TextureId` reuse by clearing manager when folder changes.
- **Menu shortcuts**: Confirm ImGui handles platform modifiers (Command vs Control) and adapt accordingly.

## Deliverables
- Updated Cargo dependencies & `main.rs` entry using SDL/imgui loop.
- New renderer/texture modules and refactored `ViewerApp` UI built with `imgui` widgets.
- Documentation in README/PLAN follow-up summarizing build steps for SDL requirements.
