[English](README.md) | [한국어](README_KR.md)

# Just Image Viewer

I started creating this because I needed an image viewer that I could use on both my Windows PC and Mac, and that I could easily extend with new features. I also wanted to test the capabilities of Codex.

An image viewer built using Rust, winit, wgpu, and imgui-rs.
It supports cross-platform.
It has a limitation dependent on the GPU's maximum texture size.

![screenshot](screenshot.jpg)

Tested and in use on Windows 11 and Apple Silicon Mac.

## Development Tools

- Codex and Windsurf

## Features

- Directory (folder) based image browsing
- Open a single image or directory from the command line
- Open images via drag and drop
- Display thumbnail list
- Display image information
- Image selection area and copy to clipboard
- Save and restore settings
- Imgui theme support ('Dark', 'Light', 'Classic')
- Custom font support

## Build and Run

Requires Rust stable toolchain.

To run:

```sh
git clone
cargo run
```

Package build

```sh
cargo install cargo-packager
cargo packager
```

## Command-line Options

**`--reset-config`**

Discards the existing configuration file and regenerates it with the bundled default settings.

**`PATH` or `File`**

You can pass an image file or a directory path as an argument.

- If a file path is given: It opens in single file mode.
- If a directory path is given: It scans the images in the directory and shows a list.

## Controls

- **Arrow Left / Arrow Up / Page Up**: Previous image
- **Arrow Right / Arrow Down / Page Down**: Next image
- **Home / End**: First image, Last image
- **Ctrl/Cmd + O**: Open folder
- **Esc**: Deselect

You can also drag and drop a file or folder directly onto the window.

## Configuration File Location

Settings are stored under the user's home directory.

- macOS / Linux: `~/.justImageViewer/settings.toml`
- Windows: `%USERPROFILE%/.justImageViewer/settings.toml`

## Custom Font File Location

Custom font files must be located at the settings file location or in a fonts directory under that location.

## Notes

- Large images exceeding the GPU's maximum texture size will not be displayed.

## License

MIT License
