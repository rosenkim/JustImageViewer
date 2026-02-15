# Vibe Image Viewer

`imgui-rs` + `SDL2` + `OpenGL` based image viewer prototype.

## Build Requirements

- Rust toolchain (stable)
- C/C++ toolchain for native crates (`cc`, `cmake`)
- OpenGL 3.3 compatible driver

## Run

```bash
cargo run
```

## Notes

- The project uses `sdl2` with the `bundled` feature, so SDL2 is built as part of the Rust build.
- Configuration is stored in the platform config directory (`dev/Vibe/ImageViewer/settings.toml`).
- When `restore_last_directory = true`, the app attempts to restore the most recently opened directory on startup.
