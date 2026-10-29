use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use oxide_core::{DownloadRequest, DownloadTask};
use oxide_os::{install_browser_manifests, OxidePaths};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "oxide-cli",
    about = "Oxide - High-performance dynamic multi-segment download manager",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Download a file using dynamic multi-segment work-stealing
    Download {
        /// Target URL to download
        url: String,

        /// Output filename (optional, defaults to server provided name)
        #[arg(short, long)]
        output: Option<String>,

        /// Directory to save the file
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Maximum concurrent connections (default: 16)
        #[arg(short, long, default_value = "16")]
        connections: usize,

        /// Bandwidth limit (bytes/sec, 0 for unlimited)
        #[arg(short, long, default_value = "0")]
        limit: u64,
    },

    /// Register Native Messaging manifests for installed web browsers
    InstallBrowsers {
        /// Path to the oxide-nmh binary (optional, defaults to current binary directory)
        #[arg(long)]
        nmh_path: Option<PathBuf>,
    },

    /// Show Oxide default paths and system configuration
    Info,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::WARN.into()))
        .init();

    let cli = Cli::parse();
    let paths = OxidePaths::new();
    paths.ensure_directories()?;

    match cli.command {
        Commands::Download {
            url,
            output,
            dir,
            connections,
            limit,
        } => {
            println!("🚀 Oxide Dynamic Downloader");
            println!("Target URL: {}", url);

            let save_dir = dir.unwrap_or_else(|| paths.downloads_dir().clone());
            let request = DownloadRequest {
                url,
                filename: output,
                save_dir: Some(save_dir.to_string_lossy().to_string()),
                headers: HashMap::new(),
                max_segments: Some(connections),
                speed_limit_bps: if limit > 0 { Some(limit) } else { None },
            };

            let id = format!("dl-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis());
            let task = DownloadTask::create(id, request, save_dir).await?;

            println!("File Name: {}", task.probe.filename);
            if let Some(total) = task.probe.content_length {
                println!("File Size: {:.2} MB ({} bytes)", total as f64 / 1_048_576.0, total);
            } else {
                println!("File Size: Unknown (Streaming)");
            }
            println!("Multi-Segment Support: {}", if task.probe.supports_ranges { "Yes" } else { "No" });
            println!("Destination: {}\n", task.target_path.display());

            let pb = if let Some(total) = task.probe.content_length {
                let pb = ProgressBar::new(total);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) @ {bytes_per_sec}")?
                        .progress_chars("#>-"),
                );
                pb
            } else {
                let pb = ProgressBar::new_spinner();
                pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} [{elapsed_precise}] {bytes} downloaded @ {bytes_per_sec}")?);
                pb
            };

            // Progress monitor task
            let coord = task.coordinator.clone();
            let pb_clone = pb.clone();
            let monitor_handle = tokio::spawn(async move {
                let mut last_bytes = 0u64;
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let current = coord.total_downloaded();
                    if current > last_bytes {
                        pb_clone.set_position(current);
                        last_bytes = current;
                    }
                    if pb_clone.is_finished() {
                        break;
                    }
                }
            });

            // Run download
            let start_instant = std::time::Instant::now();
            task.start().await?;

            pb.finish_with_message("Download Complete!");
            let _ = monitor_handle.await;

            let elapsed = start_instant.elapsed().as_secs_f64();
            let downloaded = task.coordinator.total_downloaded();
            let avg_speed_mb = (downloaded as f64 / 1_048_576.0) / elapsed.max(0.001);

            println!("\n✨ Download finished successfully in {:.2}s (Avg Speed: {:.2} MB/s)", elapsed, avg_speed_mb);
            println!("Saved to: {}", task.target_path.display());
        }

        Commands::InstallBrowsers { nmh_path } => {
            let binary = if let Some(p) = nmh_path {
                p
            } else {
                let exe = std::env::current_exe()?;
                let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
                #[cfg(windows)]
                { dir.join("oxide-nmh.exe") }
                #[cfg(not(windows))]
                { dir.join("oxide-nmh") }
            };

            println!("Installing Oxide Native Messaging Host for browsers...");
            println!("Host binary: {}", binary.display());

            let installed = install_browser_manifests(&binary)?;
            println!("Registered manifests for {} browser configuration paths:", installed.len());
            for p in installed {
                println!("  ✓ {}", p.display());
            }
            println!("\nBrowser integration setup complete!");
        }

        Commands::Info => {
            println!("Oxide Download Manager Configuration");
            println!("  Config Dir:     {}", paths.config_dir().display());
            println!("  Data Dir:       {}", paths.data_dir().display());
            println!("  Downloads Dir:  {}", paths.downloads_dir().display());
            println!("  IPC Endpoint:   {}", paths.ipc_endpoint());
        }
    }

    Ok(())
}
