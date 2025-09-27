#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod infra;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    infra::logging::init();

    let config_handle =
        infra::config::load_or_create().context("unable to prepare application configuration")?;
    log::info!("Loaded configuration from {}", config_handle.path.display());

    let config_handle_for_app = config_handle.clone();

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Vibe Image Viewer",
        native_options,
        Box::new(move |cc| {
            Ok::<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>>(Box::new(
                app::ViewerApp::new(cc, config_handle_for_app.clone()),
            ))
        }),
    )
    .context("failed to start Vibe Image Viewer")?;

    Ok(())
}
