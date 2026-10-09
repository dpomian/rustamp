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
            _ => self.title_or_stem(),
        }
    }

    /// Title tag, falling back to the file stem when it's missing.
    pub fn title_or_stem(&self) -> String {
        self.title.clone().unwrap_or_else(|| {
            self.path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string())
        })
    }
}

/// Lowercase and treat `-`/`_` as spaces, so a query typed the way a name
/// is written ("battle beast") matches disk names like "battle-beast".
fn normalize_filter_text(s: &str) -> String {
    s.to_lowercase().replace(['-', '_'], " ")
}

/// Case-insensitive substring match used by the playlist filter box.
/// Searches the display title plus the artist, album, and file path —
/// `display_title` alone isn't enough: without a title tag it is just the
/// file stem, which drops a tagged/inferred artist from the search.
pub fn matches_filter(track: &Track, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let needle = normalize_filter_text(filter);
    [
        track.display_title(),
        track.artist.clone().unwrap_or_default(),
        track.album.clone().unwrap_or_default(),
        track.path.to_string_lossy().into_owned(),
    ]
    .iter()
    .any(|haystack| normalize_filter_text(haystack).contains(&needle))
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
    fn title_or_stem_prefers_tag_then_file_name() {
        let tagged = Track {
            path: PathBuf::from("/music/track.mp3"),
            title: Some("Song".to_string()),
            artist: None,
            album: None,
            duration: None,
        };
        assert_eq!(tagged.title_or_stem(), "Song");
        let untagged = Track {
            path: PathBuf::from("/music/01 - Cool Song.mp3"),
            title: None,
            artist: None,
            album: None,
            duration: None,
        };
        assert_eq!(untagged.title_or_stem(), "01 - Cool Song");
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

    /// Issue #15: the artist column displays "Battle Beast" (tag or
    /// `<artist>--<album>` folder inference), but without a title tag the
    /// display title falls back to the file stem — so the artist must be
    /// searched as its own field.
    #[test]
    fn matches_filter_searches_artist_when_title_is_missing() {
        let track = Track {
            path: PathBuf::from("/music/battle-beast--unleash/01-song.mp3"),
            title: None,
            artist: Some("Battle Beast".to_string()),
            album: Some("Unleash".to_string()),
            duration: None,
        };
        assert!(matches_filter(&track, "battle beast"));
    }

    /// Issue #15: for untagged files the name only exists on disk with
    /// hyphens — a natural space-separated query must still match.
    #[test]
    fn matches_filter_treats_hyphens_as_word_separators() {
        let track = Track {
            path: PathBuf::from("/music/battle-beast/01 - song.mp3"),
            title: None,
            artist: None,
            album: None,
            duration: None,
        };
        assert!(matches_filter(&track, "battle beast"));
        assert!(matches_filter(&track, "battle-beast"));
        assert!(!matches_filter(&track, "battle toads"));
    }
}
