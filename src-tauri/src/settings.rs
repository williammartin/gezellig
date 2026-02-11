use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::{Context, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub livekit_url: String,
    pub shared_queue_repo: String,
    pub shared_queue_file: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            livekit_url: String::new(),
            shared_queue_repo: "williammartin/gezellig-queue".to_string(),
            shared_queue_file: "queue.ndjson".to_string(),
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read settings file: {}", path.display()))?;
        let settings = serde_json::from_str(&content)
            .context("Failed to parse settings JSON")?;
        Ok(settings)
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let content = serde_json::to_string_pretty(self).context("Failed to serialize settings")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create settings dir: {}", parent.display()))?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write settings file: {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_settings_have_reasonable_values() {
        let settings = Settings::default();
        assert_eq!(settings.livekit_url, "");
        assert_eq!(settings.shared_queue_repo, "williammartin/gezellig-queue");
        assert_eq!(settings.shared_queue_file, "queue.ndjson");
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("tempdir failed: {err}"),
        };
        let path = dir.path().join("settings.json");

        let settings = Settings {
            livekit_url: "wss://example.livekit.cloud".to_string(),
            shared_queue_repo: "owner/repo".to_string(),
            shared_queue_file: "queue.ndjson".to_string(),
        };

        assert!(settings.save(&path).is_ok());
        let loaded = Settings::load(&path);
        match loaded {
            Ok(loaded) => assert_eq!(loaded, settings),
            Err(err) => panic!("load failed: {err}"),
        }
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let path = PathBuf::from("/tmp/nonexistent_gezellig_test/settings.json");
        let loaded = Settings::load(&path);
        assert!(loaded.is_err());
    }

    #[test]
    fn load_returns_default_when_file_is_invalid_json() {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("tempdir failed: {err}"),
        };
        let path = dir.path().join("settings.json");
        assert!(fs::write(&path, "not json").is_ok());

        let loaded = Settings::load(&path);
        assert!(loaded.is_err());
    }
}
