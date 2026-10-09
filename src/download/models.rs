use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Represents a YouTube video's metadata (subset of `yt-dlp --dump-json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub chapters: Option<Vec<Chapter>>,
    #[serde(default)]
    pub playlist_index: Option<u32>,
    #[serde(default)]
    pub webpage_url: Option<String>,
}

/// Represents a chapter/timestamp in a video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start_time: f64,
    pub end_time: f64,
}

/// Audio container/codec the downloaded stream is converted to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    #[default]
    Mp3,
    /// Ogg Vorbis — yt-dlp calls the encoder "vorbis", the file ext is .ogg.
    Ogg,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 2] = [AudioFormat::Mp3, AudioFormat::Ogg];

    /// Label shown in the format picker.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Ogg => "OGG (Vorbis)",
        }
    }

    /// Value passed to `yt-dlp --audio-format`.
    pub fn yt_dlp_name(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Ogg => "vorbis",
        }
    }

    /// File extension of the produced audio file.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
        }
    }

    /// ffmpeg arguments re-encoding a segment into this format
    /// (`-acodec <codec> -q:a <quality>`), used when splitting chapters.
    pub fn ffmpeg_args(self) -> [&'static str; 4] {
        match self {
            // lame V0 — best VBR quality
            Self::Mp3 => ["-acodec", "libmp3lame", "-q:a", "0"],
            // vorbis q7 ≈ 224kbps VBR, transparent for music
            Self::Ogg => ["-acodec", "libvorbis", "-q:a", "7"],
        }
    }
}

/// What to download and where to put it.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub url: String,
    pub output_dir: PathBuf,
    pub format: AudioFormat,
    /// Split a long video into separate tracks using its chapters.
    pub split_chapters: bool,
}

/// Progress events sent from the download worker thread to the UI.
#[derive(Debug)]
pub enum DownloadEvent {
    /// Human-readable progress line for the dialog log.
    Status(String),
    /// Download progress of the current item, 0.0–100.0.
    Percent(f32),
    /// One file finished downloading/converting.
    FileDone(PathBuf),
    /// All work finished; carries every produced file.
    Finished(Vec<PathBuf>),
    /// Fatal failure — nothing more will be produced.
    Failed(String),
}
