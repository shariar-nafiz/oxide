mod app;
mod theme;
mod tray;

use app::OxideApp;
use eframe::egui::Vec2;
use oxide_core::{DownloadManager, QueueScheduler, SqliteStore};
use oxide_ipc::{start_http_rpc_server, IpcServer};
use oxide_os::OxidePaths;
use tray::TrayManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let paths = OxidePaths::new();
    paths.ensure_directories()?;

    let db_path = paths.data_dir().join("oxide_state.db");
    let db = SqliteStore::open(&db_path)?;
    let default_save_dir = paths.downloads_dir().to_string_lossy().to_string();
    let manager = DownloadManager::new(db, paths.downloads_dir().clone())?;

    // 1. Spawn Queue & Retry Scheduler
    QueueScheduler::new(manager.clone()).spawn();

    // 2. Spawn Local IPC Socket Server (for oxide-nmh / oxide-cli)
    let ipc_mgr = manager.clone();
    tokio::spawn(async move {
        let server = IpcServer::new();
        let _ = server.start_with_manager(ipc_mgr).await;
    });

    // 3. Spawn Local JSON/WS RPC Server on 127.0.0.1:46890 (for Browser Extension)
    let rpc_mgr = manager.clone();
    tokio::spawn(async move {
        start_http_rpc_server(rpc_mgr, 46890).await;
    });

    // On Linux, initialize GTK for system tray (tray-icon/libayatana-appindicator)
    #[cfg(target_os = "linux")]
    {
        let _ = gtk::init();
    }

    let _tray = match TrayManager::new() {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!(
                "Tray initialization warning: {}. Running in standard window mode.",
                e
            );
            None
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Oxide Download Manager — Rust Native IDM Engine")
            .with_inner_size(Vec2::new(1240.0, 760.0))
            .with_min_inner_size(Vec2::new(960.0, 560.0)),
        ..Default::default()
    };

    let gui_manager = manager.clone();
    eframe::run_native(
        "Oxide Download Manager",
        native_options,
        Box::new(move |_cc| Ok(Box::new(OxideApp::new(gui_manager, default_save_dir)))),
    )?;

    Ok(())
}
