use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_volume() -> f32 {
    1.0
}

/// Persisted application state: watched folders, volume, and the playback
/// position to resume from on the next launch.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub folders: Vec<PathBuf>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Track that was loaded at the last checkpoint, if any.
    #[serde(default)]
    pub last_track: Option<PathBuf>,
    /// How far into `last_track` playback had reached, in seconds.
    #[serde(default)]
    pub last_position_secs: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            volume: 1.0,
            last_track: None,
            last_position_secs: None,
        }
    }
}

impl Config {
    /// Load from the platform config directory. Missing or unreadable config
    /// yields an empty default rather than an error — first run is fine.
    pub fn load() -> Self {
        Self::load_from(&config_file_path()).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&config_file_path())
    }

    pub fn load_from(path: &PathBuf) -> io::Result<Self> {
        let data = fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save_to(&self, path: &PathBuf) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, data)
    }

    /// Add a folder if not already watched. Returns true if it was added.
    pub fn add_folder(&mut self, folder: PathBuf) -> bool {
        if self.folders.contains(&folder) {
            return false;
        }
        self.folders.push(folder);
        self.folders.sort();
        true
    }

    pub fn remove_folder(&mut self, folder: &PathBuf) {
        self.folders.retain(|f| f != folder);
    }
}

fn config_file_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rustamp")
        .join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrips() {
        let dir = std::env::temp_dir().join(format!("rustamp-cfg-{}", std::process::id()));
        let path = dir.join("config.json");

        let mut config = Config::default();
        config.add_folder(PathBuf::from("/music/a"));
        config.add_folder(PathBuf::from("/music/b"));
        config.save_to(&path).unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(
            loaded.folders,
            vec![PathBuf::from("/music/a"), PathBuf::from("/music/b")]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_from_missing_file_returns_err_not_panic() {
        let path = PathBuf::from("/nonexistent/rustamp/config.json");
        assert!(Config::load_from(&path).is_err());
    }

    #[test]
    fn add_folder_deduplicates_and_sorts() {
        let mut config = Config::default();
        assert!(config.add_folder(PathBuf::from("/b")));
        assert!(config.add_folder(PathBuf::from("/a")));
        assert!(!config.add_folder(PathBuf::from("/b")));
        assert_eq!(
            config.folders,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn remove_folder_works() {
        let mut config = Config::default();
        config.add_folder(PathBuf::from("/a"));
        config.add_folder(PathBuf::from("/b"));
        config.remove_folder(&PathBuf::from("/a"));
        assert_eq!(config.folders, vec![PathBuf::from("/b")]);
    }
}
