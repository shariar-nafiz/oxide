mod app;
mod tray;

use app::{OxideApp, UiDownloadItem};
use eframe::egui::Vec2;
use oxide_os::OxidePaths;
use parking_lot::RwLock;
use std::sync::Arc;
use tray::TrayManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // On Linux, initialize GTK for system tray (tray-icon/libayatana-appindicator)
    #[cfg(target_os = "linux")]
    {
        let _ = gtk::init();
    }

    let paths = OxidePaths::new();
    paths.ensure_directories()?;

    let items = Arc::new(RwLock::new(Vec::<UiDownloadItem>::new()));

    // Launch cross-platform tray controller
    let _tray = match TrayManager::new() {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("Tray initialization warning: {}. Running in standard window mode.", e);
            None
        }
    };

    println!("Oxide Download Manager Daemon started.");
    println!("Running background event loop. Tray icon active.");

    // Options for clean minimal native window
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Oxide Download Manager")
            .with_inner_size(Vec2::new(760.0, 520.0))
            .with_min_inner_size(Vec2::new(500.0, 350.0)),
        ..Default::default()
    };

    // Run minimal eframe UI
    let items_clone = items.clone();
    eframe::run_native(
        "Oxide Download Manager",
        native_options,
        Box::new(|_cc| Ok(Box::new(OxideApp::new(items_clone)))),
    )?;

    Ok(())
}
