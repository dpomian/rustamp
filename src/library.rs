use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use id3::TagLike;
use walkdir::WalkDir;

/// A single MP3 file discovered in a watched folder.
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

pub fn is_mp3(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
}

/// Read whatever metadata is cheaply available from the ID3 tag.
/// Missing/invalid tags are not fatal — the track is still usable.
fn read_tags(
    path: &Path,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Duration>,
) {
    match id3::Tag::read_from_path(path) {
        Ok(tag) => (
            tag.title().map(str::to_owned),
            tag.artist().map(str::to_owned),
            tag.album().map(str::to_owned),
            tag.duration()
                .map(|ms| Duration::from_millis(u64::from(ms))),
        ),
        Err(_) => (None, None, None, None),
    }
}

/// Recursively collect every MP3 under `folder`.
pub fn scan_folder(folder: &Path) -> Vec<Track> {
    WalkDir::new(folder)
        .follow_links(true)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_mp3(path))
        .map(|path| {
            let (title, artist, album, duration) = read_tags(&path);
            Track {
                path,
                title,
                artist,
                album,
                duration,
            }
        })
        .collect()
}

/// Scan several folders, deduplicating tracks that appear in more than one
/// (e.g. a folder and its parent are both watched). Sorted by artist/title.
pub fn scan_folders(folders: &[PathBuf]) -> Vec<Track> {
    let mut seen = HashSet::new();
    let mut tracks = Vec::new();
    for folder in folders {
        for track in scan_folder(folder) {
            if seen.insert(track.path.clone()) {
                tracks.push(track);
            }
        }
    }
    tracks.sort_by_key(|t| {
        (
            t.artist.clone().unwrap_or_default().to_lowercase(),
            t.title.clone().unwrap_or_default().to_lowercase(),
            t.path.to_string_lossy().to_lowercase(),
        )
    });
    tracks
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
    use std::fs;

    #[test]
    fn is_mp3_checks_extension_case_insensitively() {
        assert!(is_mp3(Path::new("song.mp3")));
        assert!(is_mp3(Path::new("song.MP3")));
        assert!(is_mp3(Path::new("song.Mp3")));
        assert!(!is_mp3(Path::new("song.flac")));
        assert!(!is_mp3(Path::new("song")));
    }

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
    fn scan_folder_finds_mp3s_recursively_and_ignores_others() {
        let root = std::env::temp_dir().join(format!("rustamp-test-{}", std::process::id()));
        let nested = root.join("nested").join("deep");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("a.mp3"), b"not really mp3").unwrap();
        fs::write(nested.join("b.MP3"), b"fake").unwrap();
        fs::write(nested.join("c.flac"), b"fake").unwrap();
        fs::write(root.join("d.txt"), b"fake").unwrap();

        let mut found: Vec<String> = scan_folder(&root)
            .iter()
            .map(|t| t.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();

        assert_eq!(found, vec!["a.mp3", "b.MP3"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scan_folders_deduplicates_shared_files() {
        let root = std::env::temp_dir().join(format!("rustamp-dedup-{}", std::process::id()));
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("x.mp3"), b"fake").unwrap();

        let tracks = scan_folders(&[root.clone(), sub]);
        assert_eq!(tracks.len(), 1);
        fs::remove_dir_all(&root).unwrap();
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
