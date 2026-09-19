use std::path::PathBuf;
use std::time::Duration;

/// A single audio file discovered in a watched folder.
#[derive(Debug, Clone)]
pub struct Track {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Duration read from the ID3 tag (TLEN). Filled in exactly by the decoder
    /// once the track is loaded for playback.
    pub duration: Option<Duration>,
}

impl Track {
    /// Human-readable label used in the playlist: "Artist - Title", falling
    /// back to title alone, then to the file name.
    pub fn display_title(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ => self
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string()),
        }
    }
}

/// Case-insensitive substring match used by the playlist filter box.
pub fn matches_filter(track: &Track, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let needle = filter.to_lowercase();
    track.display_title().to_lowercase().contains(&needle)
        || track
            .album
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(&needle)
        || track
            .path
            .to_string_lossy()
            .to_lowercase()
            .contains(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_title_falls_back_to_file_stem() {
        let track = Track {
            path: PathBuf::from("/music/01 - Cool Song.mp3"),
            title: None,
            artist: None,
            album: None,
            duration: None,
        };
        assert_eq!(track.display_title(), "01 - Cool Song");
    }

    #[test]
    fn display_title_prefers_artist_and_title() {
        let track = Track {
            path: PathBuf::from("/music/track.mp3"),
            title: Some("Song".to_string()),
            artist: Some("Band".to_string()),
            album: None,
            duration: None,
        };
        assert_eq!(track.display_title(), "Band - Song");
    }

    #[test]
    fn matches_filter_is_case_insensitive() {
        let track = Track {
            path: PathBuf::from("/music/song.mp3"),
            title: Some("Yellow Submarine".to_string()),
            artist: Some("The Beatles".to_string()),
            album: Some("Revolver".to_string()),
            duration: None,
        };
        assert!(matches_filter(&track, "beatles"));
        assert!(matches_filter(&track, "SUBMARINE"));
        assert!(matches_filter(&track, "revolver"));
        assert!(matches_filter(&track, ""));
        assert!(!matches_filter(&track, "zeppelin"));
    }
}
