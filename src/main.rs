#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod constants;
mod core;
mod infra;
mod main_app;
mod math;
mod render;
mod ui;

use anyhow::{bail, Context};
use std::path::PathBuf;
use winit::event_loop::EventLoop;

use main_app::MainApp;

#[derive(Debug, Default)]
struct AppArgs {
    reset_config: bool,
    open_path: Option<PathBuf>,
}

fn parse_args() -> anyhow::Result<AppArgs> {
    const EXECUTE_FILE_NAME: &str = "justImageViewer";

    let mut args = AppArgs::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--reset-config" => args.reset_config = true,
            "-h" | "--help" => {
                println!("Usage: {EXECUTE_FILE_NAME} [--reset-config] [PATH]");
                println!("  --reset-config  overwrite saved settings with default_settings.toml");
                println!("  PATH            image file path (single-file mode) or directory path");
                std::process::exit(0);
            }
            _ => {
                if arg.starts_with('-') {
                    bail!("unknown argument: {arg}\nUsage: {EXECUTE_FILE_NAME} [--reset-config] [PATH]");
                }
                if args.open_path.is_some() {
                    bail!(
                        "only one PATH argument is supported\nUsage: {EXECUTE_FILE_NAME} [--reset-config] [PATH]"
                    );
                }
                args.open_path = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(args)
}

/// App entrypoint: parse arguments, then hand everything to MainApp.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args().context("failed to parse command line arguments")?;

    let event_loop = EventLoop::new().map_err(anyhow::Error::msg)?;
    let app = MainApp::initialize(&event_loop, args.open_path, args.reset_config).await?;
    app.run(event_loop)
}
