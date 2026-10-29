use directories::ProjectDirs;
use std::path::PathBuf;

pub struct OxidePaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    downloads_dir: PathBuf,
}

impl OxidePaths {
    pub fn new() -> Self {
        let proj = ProjectDirs::from("org", "oxide", "oxide-dl");
        let (config_dir, data_dir) = if let Some(ref p) = proj {
            (p.config_dir().to_path_buf(), p.data_dir().to_path_buf())
        } else {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into());
            let base = PathBuf::from(home).join(".oxide-dl");
            (base.join("config"), base.join("data"))
        };

        let downloads_dir = dirs::download_dir().unwrap_or_else(|| {
            if let Some(ref p) = proj {
                p.data_dir().join("Downloads")
            } else {
                PathBuf::from("Downloads")
            }
        });

        Self {
            config_dir,
            data_dir,
            downloads_dir,
        }
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    pub fn downloads_dir(&self) -> &PathBuf {
        &self.downloads_dir
    }

    /// Default path for the Unix socket or Named Pipe
    pub fn ipc_endpoint(&self) -> String {
        #[cfg(unix)]
        {
            if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                format!("{}/oxide-dl.sock", runtime_dir)
            } else {
                format!("{}/oxide-dl.sock", self.data_dir.display())
            }
        }

        #[cfg(windows)]
        {
            r"\\.\pipe\oxide-dl".to_string()
        }
    }

    /// Ensure config, data, and download directories exist on disk
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.downloads_dir)?;
        Ok(())
    }
}

impl Default for OxidePaths {
    fn default() -> Self {
        Self::new()
    }
}
