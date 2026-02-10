use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub display_name: String,
    pub livekit_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display_name: "You".to_string(),
            livekit_url: String::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, content).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_settings_have_reasonable_values() {
        let settings = Settings::default();
        assert_eq!(settings.display_name, "You");
        assert_eq!(settings.livekit_url, "");
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let settings = Settings {
            display_name: "Alice".to_string(),
            livekit_url: "wss://example.livekit.cloud".to_string(),
        };

        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, settings);
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let path = PathBuf::from("/tmp/nonexistent_gezellig_test/settings.json");
        let loaded = Settings::load(&path);
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn load_returns_default_when_file_is_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "not json").unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded, Settings::default());
    }
}
