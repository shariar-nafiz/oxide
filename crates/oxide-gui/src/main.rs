mod app;
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

    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| a == "--headless" || a == "--daemon");

    let paths = OxidePaths::new();
    paths.ensure_directories()?;

    let db_path = paths.data_dir().join("oxide_state.db");
    let db = SqliteStore::open(&db_path)?;
    let manager = DownloadManager::new(db, paths.downloads_dir().clone())?;

    // 1. Spawn Queue & Retry Scheduler
    QueueScheduler::new(manager.clone()).spawn();

    // 2. Spawn Local IPC Socket Server (for oxide-nmh / oxide-cli)
    let ipc_mgr = manager.clone();
    tokio::spawn(async move {
        let server = IpcServer::new();
        let _ = server.start_with_manager(ipc_mgr).await;
    });

    // 3. Spawn Local HTTP + 10Hz WebSocket RPC & Live Preview Server on 127.0.0.1:46890
    let http_mgr = manager.clone();
    tokio::spawn(async move {
        start_http_rpc_server(http_mgr, 46890).await;
    });

    println!("Oxide Download Manager Daemon started.");
    println!("SQLite State DB:      {}", db_path.display());
    println!("Live UI & RPC Server: http://127.0.0.1:46890");

    if headless {
        println!("Running in headless daemon + live web UI mode on http://127.0.0.1:46890 ...");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

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
            .with_title("Oxide Download Manager — Modern IDM Engine")
            .with_inner_size(Vec2::new(1120.0, 680.0))
            .with_min_inner_size(Vec2::new(820.0, 480.0)),
        ..Default::default()
    };

    let gui_manager = manager.clone();
    eframe::run_native(
        "Oxide Download Manager",
        native_options,
        Box::new(move |_cc| Ok(Box::new(OxideApp::new(gui_manager)))),
    )?;

    Ok(())
}
