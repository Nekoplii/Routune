use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use routune_core::config::RoutuneConfig;
use routune_core::graph::VirtualChannel;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

pub fn resolve_config_path() -> PathBuf {
    if let Ok(custom) = std::env::var("ROUTUNE_CONFIG_PATH") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    if let Some(proj_dirs) = directories::ProjectDirs::from("com", "routune", "routune") {
        proj_dirs.config_dir().join("config.json")
    } else if let Some(base_dirs) = directories::BaseDirs::new() {
        base_dirs.config_dir().join("routune").join("config.json")
    } else {
        PathBuf::from("config.json")
    }
}

pub struct ConfigManager {
    config: Arc<Mutex<RoutuneConfig>>,
    save_tx: mpsc::Sender<()>,
    path: PathBuf,
}

impl ConfigManager {
    pub fn init() -> Result<Self> {
        let path = resolve_config_path();
        Self::with_path(path)
    }

    pub fn with_path(path: PathBuf) -> Result<Self> {
        let config = RoutuneConfig::load_or_default(&path);
        // Ensure default file exists on disk if not already present
        if !path.exists() {
            if let Err(e) = config.save_to_file(&path) {
                tracing::warn!("Failed to create initial config file at {:?}: {}", path, e);
            } else {
                info!("Initialized default config template at {:?}", path);
            }
        } else {
            info!("Loaded existing Routune config from {:?}", path);
        }

        let config = Arc::new(Mutex::new(config));
        let (save_tx, mut save_rx) = mpsc::channel::<()>(32);

        let config_clone = config.clone();
        let path_clone = path.clone();

        tokio::spawn(async move {
            while let Some(()) = save_rx.recv().await {
                // Debounce window (150ms) to coalesce rapid slider adjustments
                tokio::time::sleep(Duration::from_millis(150)).await;
                while save_rx.try_recv().is_ok() {}

                let snapshot = {
                    let guard = config_clone.lock().await;
                    guard.clone()
                };

                if let Err(e) = snapshot.save_to_file(&path_clone) {
                    error!("Failed to persist config to {:?}: {}", path_clone, e);
                } else {
                    debug!("Asynchronously saved config to {:?}", path_clone);
                }
            }
        });

        Ok(Self {
            config,
            save_tx,
            path,
        })
    }

    pub async fn get_config(&self) -> RoutuneConfig {
        let guard = self.config.lock().await;
        guard.clone()
    }

    pub async fn set_route(&self, channel: VirtualChannel, device: impl Into<String>) {
        {
            let mut guard = self.config.lock().await;
            guard.set_route(channel, device);
        }
        let _ = self.save_tx.try_send(());
    }

    pub async fn set_eq_band(&self, channel: VirtualChannel, band_index: usize, gain_db: f32) {
        {
            let mut guard = self.config.lock().await;
            guard.set_eq_band(channel, band_index, gain_db);
        }
        let _ = self.save_tx.try_send(());
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_manager_lifecycle_and_debounce() {
        let temp_dir = std::env::temp_dir().join("routune_daemon_cfg_test");
        let cfg_file = temp_dir.join("test_config.json");
        let _ = std::fs::remove_file(&cfg_file);

        let mgr = ConfigManager::with_path(cfg_file.clone()).expect("Failed to init ConfigManager");
        assert!(cfg_file.exists());

        // Update routing and EQ
        mgr.set_route(VirtualChannel::Game, "USB Dac").await;
        for i in 0..10 {
            mgr.set_eq_band(VirtualChannel::Game, i, (i as f32) * 0.5).await;
        }

        // Wait for debounced write
        tokio::time::sleep(Duration::from_millis(300)).await;

        let loaded = RoutuneConfig::load_or_default(&cfg_file);
        assert_eq!(loaded.get_route(VirtualChannel::Game), "USB Dac");
        assert_eq!(loaded.get_eq(VirtualChannel::Game)[5], 2.5);

        let _ = std::fs::remove_file(&cfg_file);
        let _ = std::fs::remove_dir(&temp_dir);
    }
}
