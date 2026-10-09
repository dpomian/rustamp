use std::fmt;

/// Errors from the YouTube download pipeline (yt-dlp + ffmpeg subprocesses).
#[derive(Debug)]
pub enum DownloadError {
    Download(String),
    Conversion(String),
    InfoExtraction(String),
    YtDlpNotFound,
    FfmpegNotFound,
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Download(msg) => write!(f, "Failed to download video: {msg}"),
            Self::Conversion(msg) => write!(f, "Failed to convert video: {msg}"),
            Self::InfoExtraction(msg) => write!(f, "Failed to extract video info: {msg}"),
            Self::YtDlpNotFound => {
                write!(
                    f,
                    "yt-dlp not found. Please install it first (e.g. brew install yt-dlp)"
                )
            }
            Self::FfmpegNotFound => {
                write!(
                    f,
                    "ffmpeg not found. Please install it first (e.g. brew install ffmpeg)"
                )
            }
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Json(e) => write!(f, "JSON parsing error: {e}"),
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for DownloadError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, DownloadError>;
