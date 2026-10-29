use std::path::{Path, PathBuf};
use thiserror::Error;

pub const NMH_HOST_NAME: &str = "org.oxide.nmh";
pub const EXTENSION_ID_FIREFOX: &str = "oxide-dl@shariarunix";
pub const EXTENSION_ORIGIN_CHROME: &str = "chrome-extension://*";

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Platform registry error: {0}")]
    Registry(String),
}

/// Generates the Native Messaging JSON manifest for Chromium-based browsers.
pub fn generate_chromium_manifest(binary_path: &Path, allowed_origins: &[&str]) -> String {
    let origins: Vec<String> = if allowed_origins.is_empty() {
        vec!["chrome-extension://*".into()]
    } else {
        allowed_origins.iter().map(|s| s.to_string()).collect()
    };

    let manifest = serde_json::json!({
        "name": NMH_HOST_NAME,
        "description": "Oxide Download Manager Native Messaging Host",
        "path": binary_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": origins
    });

    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

/// Generates the Native Messaging JSON manifest for Gecko-based browsers (Firefox, Zen, LibreWolf).
pub fn generate_firefox_manifest(binary_path: &Path, allowed_extensions: &[&str]) -> String {
    let extensions: Vec<String> = if allowed_extensions.is_empty() {
        vec![EXTENSION_ID_FIREFOX.into()]
    } else {
        allowed_extensions.iter().map(|s| s.to_string()).collect()
    };

    let manifest = serde_json::json!({
        "name": NMH_HOST_NAME,
        "description": "Oxide Download Manager Native Messaging Host",
        "path": binary_path.to_string_lossy(),
        "type": "stdio",
        "allowed_extensions": extensions
    });

    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

/// Automatically registers the Native Messaging manifest for all installed browsers.
pub fn install_browser_manifests(binary_path: &Path) -> Result<Vec<PathBuf>, ManifestError> {
    let mut installed = Vec::new();
    let chromium_content = generate_chromium_manifest(binary_path, &[]);
    let firefox_content = generate_firefox_manifest(binary_path, &[]);

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let home_path = PathBuf::from(home);

        // Chromium browsers paths
        let chromium_dirs = [
            home_path.join(".config/google-chrome/NativeMessagingHosts"),
            home_path.join(".config/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
            home_path.join(".config/microsoft-edge/NativeMessagingHosts"),
            home_path.join(".config/chromium/NativeMessagingHosts"),
            home_path.join(".config/vivaldi/NativeMessagingHosts"),
            home_path.join(".config/opera/NativeMessagingHosts"),
        ];

        for dir in &chromium_dirs {
            let manifest_path = dir.join(format!("{}.json", NMH_HOST_NAME));
            if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&manifest_path, &chromium_content).is_ok() {
                installed.push(manifest_path);
            }
        }

        // Firefox & Zen paths
        let gecko_dirs = [
            home_path.join(".mozilla/native-messaging-hosts"),
            home_path.join(".zen/native-messaging-hosts"),
        ];

        for dir in &gecko_dirs {
            let manifest_path = dir.join(format!("{}.json", NMH_HOST_NAME));
            if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&manifest_path, &firefox_content).is_ok() {
                installed.push(manifest_path);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let app_support = PathBuf::from(home).join("Library/Application Support");

        let chromium_dirs = [
            app_support.join("Google/Chrome/NativeMessagingHosts"),
            app_support.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"),
            app_support.join("Microsoft Edge/NativeMessagingHosts"),
            app_support.join("Chromium/NativeMessagingHosts"),
            app_support.join("Vivaldi/NativeMessagingHosts"),
        ];

        for dir in &chromium_dirs {
            let manifest_path = dir.join(format!("{}.json", NMH_HOST_NAME));
            if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&manifest_path, &chromium_content).is_ok() {
                installed.push(manifest_path);
            }
        }

        let gecko_dirs = [
            app_support.join("Mozilla/NativeMessagingHosts"),
        ];

        for dir in &gecko_dirs {
            let manifest_path = dir.join(format!("{}.json", NMH_HOST_NAME));
            if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&manifest_path, &firefox_content).is_ok() {
                installed.push(manifest_path);
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        let manifest_dir = PathBuf::from(app_data).join("Oxide\\NativeMessagingHosts");
        std::fs::create_dir_all(&manifest_dir)?;

        let chrome_manifest_path = manifest_dir.join(format!("{}-chrome.json", NMH_HOST_NAME));
        let firefox_manifest_path = manifest_dir.join(format!("{}-firefox.json", NMH_HOST_NAME));

        std::fs::write(&chrome_manifest_path, &chromium_content)?;
        std::fs::write(&firefox_manifest_path, &firefox_content)?;

        installed.push(chrome_manifest_path.clone());
        installed.push(firefox_manifest_path.clone());

        // Register Registry Keys for Chrome, Brave, Edge, and Firefox
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let registry_targets = [
            (r"Software\Google\Chrome\NativeMessagingHosts", &chrome_manifest_path),
            (r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts", &chrome_manifest_path),
            (r"Software\Microsoft\Edge\NativeMessagingHosts", &chrome_manifest_path),
            (r"Software\Mozilla\NativeMessagingHosts", &firefox_manifest_path),
        ];

        for (reg_subpath, path) in &registry_targets {
            if let Ok((key, _)) = hkcu.create_subkey(format!(r"{}\{}", reg_subpath, NMH_HOST_NAME)) {
                let _ = key.set_value("", &path.to_string_lossy().to_string());
            }
        }
    }

    Ok(installed)
}
