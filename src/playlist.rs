use std::collections::HashSet;

use rand::seq::SliceRandom;
use rand::{Rng, RngExt};

use crate::library::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    /// Off -> All -> One -> Off
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::All => "Repeat: All",
            Self::One => "Repeat: One",
        }
    }
}

/// The loaded tracks plus the order they play in.
///
/// `tracks` is display order (sorted, filtered only at render time).
/// `order` is play order: a permutation of track indices — identity when
/// shuffle is off. `cursor` points into `order`, not into `tracks`.
pub struct Playlist {
    pub tracks: Vec<Track>,
    order: Vec<usize>,
    cursor: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            order: Vec::new(),
            cursor: None,
            shuffle: false,
            repeat: RepeatMode::Off,
        }
    }

    /// Replace the track list. Preserves the current track if it is still in
    /// the list (matched by path), so rescanning doesn't kill playback.
    pub fn set_tracks(&mut self, tracks: Vec<Track>, rng: &mut impl Rng) {
        let current_path = self.current().map(|t| t.path.clone());
        self.tracks = tracks;
        self.rebuild_order(rng);
        self.cursor = current_path.and_then(|p| {
            self.tracks
                .iter()
                .position(|t| t.path == p)
                .and_then(|idx| self.order.iter().position(|&i| i == idx))
        });
    }

    fn rebuild_order(&mut self, rng: &mut impl Rng) {
        self.order = (0..self.tracks.len()).collect();
        if self.shuffle {
            self.order.shuffle(rng);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Index into `tracks` of the currently loaded track.
    pub fn current_index(&self) -> Option<usize> {
        self.cursor.map(|c| self.order[c])
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index().map(|i| &self.tracks[i])
    }

    /// Jump to a specific track (e.g. user double-clicked a row).
    /// Returns the index into `tracks` that should be played.
    pub fn select(&mut self, track_index: usize) -> Option<usize> {
        self.cursor = self.order.iter().position(|&i| i == track_index);
        self.current_index()
    }

    /// Track finished on its own. Honors repeat mode: One replays the track,
    /// All wraps at the end, Off yields None at the end (stop).
    pub fn advance_auto(&mut self) -> Option<usize> {
        if self.repeat == RepeatMode::One {
            return self.current_index();
        }
        self.step(true, self.repeat == RepeatMode::All)
    }

    /// User pressed next/previous. Always cyclic regardless of repeat mode.
    pub fn step_manual(&mut self, forward: bool) -> Option<usize> {
        self.step(forward, true)
    }

    fn step(&mut self, forward: bool, wrap: bool) -> Option<usize> {
        let cursor = self.cursor?;
        let next = if forward {
            if cursor + 1 < self.order.len() {
                cursor + 1
            } else if wrap {
                0
            } else {
                return None;
            }
        } else if cursor > 0 {
            cursor - 1
        } else if wrap {
            self.order.len() - 1
        } else {
            return None;
        };
        self.cursor = Some(next);
        self.current_index()
    }

    pub fn set_shuffle(&mut self, shuffle: bool, rng: &mut impl Rng) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let current = self.current_index();
        self.rebuild_order(rng);
        // Keep the currently playing track under the cursor.
        if let Some(idx) = current {
            self.cursor = self.order.iter().position(|&i| i == idx);
        }
    }

    /// Append tracks that aren't already in the playlist (matched by path).
    /// New tracks join the play order at the end, or at random positions
    /// when shuffle is on. Returns how many were added.
    pub fn add_tracks(&mut self, new_tracks: Vec<Track>, rng: &mut impl Rng) -> usize {
        let mut existing: HashSet<_> = self.tracks.iter().map(|t| t.path.clone()).collect();
        let mut added = 0;
        for track in new_tracks {
            if !existing.insert(track.path.clone()) {
                continue;
            }
            let idx = self.tracks.len();
            self.tracks.push(track);
            added += 1;
            if self.shuffle {
                let pos = rng.random_range(0..=self.order.len());
                self.order.insert(pos, idx);
                // Inserting at or before the cursor shifts the current
                // track right — keep the cursor on it.
                if let Some(c) = self.cursor
                    && pos <= c
                {
                    self.cursor = Some(c + 1);
                }
            } else {
                self.order.push(idx);
            }
        }
        added
    }

    /// Update a track's duration once the decoder reports the real value.
    pub fn set_duration(&mut self, track_index: usize, duration: std::time::Duration) {
        if let Some(track) = self.tracks.get_mut(track_index) {
            track.duration = Some(duration);
        }
    }
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::path::PathBuf;

    fn track(name: &str) -> Track {
        Track {
            path: PathBuf::from(format!("/music/{name}.mp3")),
            title: Some(name.to_string()),
            artist: None,
            album: None,
            duration: None,
        }
    }

    fn playlist(n: usize) -> Playlist {
        let mut p = Playlist::new();
        let mut rng = StdRng::seed_from_u64(1);
        p.set_tracks((0..n).map(|i| track(&format!("t{i}"))).collect(), &mut rng);
        p
    }

    #[test]
    fn repeat_mode_cycles() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::Off);
    }

    #[test]
    fn select_sets_current() {
        let mut p = playlist(3);
        assert_eq!(p.select(2), Some(2));
        assert_eq!(p.current_index(), Some(2));
        assert_eq!(p.current().unwrap().title.as_deref(), Some("t2"));
    }

    #[test]
    fn auto_advance_stops_at_end_when_repeat_off() {
        let mut p = playlist(3);
        p.select(0);
        assert_eq!(p.advance_auto(), Some(1));
        assert_eq!(p.advance_auto(), Some(2));
        assert_eq!(p.advance_auto(), None);
    }

    #[test]
    fn auto_advance_wraps_when_repeat_all() {
        let mut p = playlist(2);
        p.repeat = RepeatMode::All;
        p.select(1);
        assert_eq!(p.advance_auto(), Some(0));
    }

    #[test]
    fn auto_advance_repeats_track_when_repeat_one() {
        let mut p = playlist(2);
        p.repeat = RepeatMode::One;
        p.select(0);
        assert_eq!(p.advance_auto(), Some(0));
        assert_eq!(p.advance_auto(), Some(0));
    }

    #[test]
    fn manual_next_wraps_even_when_repeat_off() {
        let mut p = playlist(2);
        p.select(1);
        assert_eq!(p.step_manual(true), Some(0));
    }

    #[test]
    fn manual_prev_wraps() {
        let mut p = playlist(3);
        p.select(0);
        assert_eq!(p.step_manual(false), Some(2));
        assert_eq!(p.step_manual(false), Some(1));
    }

    #[test]
    fn shuffle_keeps_current_track_under_cursor() {
        let mut p = playlist(10);
        let mut rng = StdRng::seed_from_u64(42);
        p.select(4);
        p.set_shuffle(true, &mut rng);
        assert!(p.shuffle);
        assert_eq!(p.current_index(), Some(4));
        // order is a permutation covering every track exactly once
        let mut sorted = p.order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn unshuffle_restores_sequential_order() {
        let mut p = playlist(5);
        let mut rng = StdRng::seed_from_u64(7);
        p.select(2);
        p.set_shuffle(true, &mut rng);
        p.set_shuffle(false, &mut rng);
        assert_eq!(p.order, vec![0, 1, 2, 3, 4]);
        assert_eq!(p.current_index(), Some(2));
        assert_eq!(p.cursor, Some(2));
    }

    #[test]
    fn set_tracks_preserves_current_by_path() {
        let mut p = playlist(3);
        let mut rng = StdRng::seed_from_u64(1);
        p.select(1);
        // Rescan finds same files plus one new, order may shift.
        let tracks = vec![track("new"), track("t0"), track("t1"), track("t2")];
        p.set_tracks(tracks, &mut rng);
        assert_eq!(p.current().unwrap().title.as_deref(), Some("t1"));
    }

    #[test]
    fn add_tracks_appends_to_order() {
        let mut p = playlist(2);
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(p.add_tracks(vec![track("extra")], &mut rng), 1);
        assert_eq!(p.tracks.len(), 3);
        assert_eq!(p.order, vec![0, 1, 2]);
    }

    #[test]
    fn add_tracks_deduplicates_by_path() {
        let mut p = playlist(2);
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(p.add_tracks(vec![track("t0"), track("new")], &mut rng), 1);
        assert_eq!(p.tracks.len(), 3);
    }

    #[test]
    fn add_tracks_keeps_cursor_on_current_track_when_shuffled() {
        let mut p = playlist(5);
        let mut rng = StdRng::seed_from_u64(9);
        p.select(2);
        p.set_shuffle(true, &mut rng);
        let current = p.current_index();
        p.add_tracks(vec![track("x"), track("y")], &mut rng);
        assert_eq!(p.current_index(), current);
        let mut sorted = p.order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..7).collect::<Vec<_>>());
    }

    #[test]
    fn empty_playlist_yields_none() {
        let mut p = playlist(0);
        assert_eq!(p.current_index(), None);
        assert_eq!(p.advance_auto(), None);
        assert_eq!(p.step_manual(true), None);
    }
}
