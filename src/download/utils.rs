use std::path::Path;
use std::process::Command;

use crate::download::error::Result;

/// Which external tools the download pipeline needs, and whether each was
/// found on PATH. yt-dlp does the downloading; ffmpeg is required for the
/// audio conversion and chapter splitting.
#[derive(Debug, Clone, Copy)]
pub struct ToolsStatus {
    pub yt_dlp: bool,
    pub ffmpeg: bool,
}

/// Probe yt-dlp and ffmpeg once (runs `--version` on each).
pub fn tools_status() -> ToolsStatus {
    ToolsStatus {
        yt_dlp: tool_works("yt-dlp", "--version"),
        ffmpeg: tool_works("ffmpeg", "-version"),
    }
}

fn tool_works(program: &str, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Check if a URL is a YouTube playlist.
pub fn is_playlist_url(url: &str) -> bool {
    url.contains("playlist?list=") || url.contains("&list=")
}

/// Check if a URL looks like a YouTube video or playlist URL. Loose on
/// purpose — yt-dlp itself is the real validator, this just catches
/// obviously-wrong input before spawning it.
pub fn is_valid_youtube_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.strip_prefix("www.").unwrap_or(host);
    match host {
        "youtube.com" | "music.youtube.com" => {
            path.starts_with("watch?v=")
                || path.starts_with("playlist?list=")
                || path.starts_with("shorts/")
        }
        "youtu.be" => !path.is_empty(),
        _ => false,
    }
}

/// Sanitize a filename to remove invalid characters.
pub fn sanitize_filename(name: &str) -> String {
    let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let mut sanitized = name.to_string();

    for c in invalid_chars {
        sanitized = sanitized.replace(c, "_");
    }

    // Trim whitespace and dots from the end
    sanitized = sanitized.trim().trim_end_matches('.').to_string();

    // Limit length
    if sanitized.len() > 200 {
        sanitized = sanitized[..200].to_string();
    }

    sanitized
}

/// Ensure output directory exists.
pub fn ensure_output_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_playlist_url() {
        assert!(is_playlist_url(
            "https://www.youtube.com/playlist?list=PLtest123"
        ));
        assert!(is_playlist_url(
            "https://www.youtube.com/watch?v=abc&list=PLtest123"
        ));
        assert!(!is_playlist_url("https://www.youtube.com/watch?v=abc"));
    }

    #[test]
    fn test_is_valid_youtube_url() {
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(is_valid_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(is_valid_youtube_url(
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/playlist?list=PLtest123"
        ));
        assert!(!is_valid_youtube_url("https://example.com/video"));
        assert!(!is_valid_youtube_url("https://youtube.com/"));
        assert!(!is_valid_youtube_url("not a url"));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("test/file:name"), "test_file_name");
        assert_eq!(sanitize_filename("normal_name"), "normal_name");
        assert_eq!(sanitize_filename("trailing.  "), "trailing");
    }
}
