use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use walkdir::WalkDir;

use super::Track;

/// Extensions we hand to the decoder — rodio (symphonia) supports all of
/// these, and lofty can read tags from all of them.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "aiff", "aif", "wv",
];

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// Read whatever metadata is cheaply available from the file's tags.
/// Missing/invalid tags are not fatal — the track is still usable.
fn read_tags(
    path: &Path,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Duration>,
) {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return (None, None, None, None);
    };
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let duration = tagged.properties().duration();
    (
        tag.and_then(|t| t.title().map(|s| s.into_owned())),
        tag.and_then(|t| t.artist().map(|s| s.into_owned())),
        tag.and_then(|t| t.album().map(|s| s.into_owned())),
        (!duration.is_zero()).then_some(duration),
    )
}

/// Recursively collect every supported audio file under `folder`.
pub fn scan_folder(folder: &Path) -> Vec<Track> {
    WalkDir::new(folder)
        .follow_links(true)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_audio(path))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn is_audio_checks_extension_case_insensitively() {
        assert!(is_audio(Path::new("song.mp3")));
        assert!(is_audio(Path::new("song.MP3")));
        assert!(is_audio(Path::new("song.Flac")));
        assert!(is_audio(Path::new("song.m4a")));
        assert!(is_audio(Path::new("song.wav")));
        assert!(!is_audio(Path::new("song.txt")));
        assert!(!is_audio(Path::new("song")));
    }

    #[test]
    fn scan_folder_finds_audio_recursively_and_ignores_others() {
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

        assert_eq!(found, vec!["a.mp3", "b.MP3", "c.flac"]);
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
}
