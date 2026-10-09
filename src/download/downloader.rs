use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crate::download::error::{DownloadError, Result};
use crate::download::models::{DownloadEvent, DownloadOptions, VideoInfo};
use crate::download::utils::{ensure_output_dir, is_playlist_url, sanitize_filename};

/// Number of times a video download is attempted before giving up.
/// Transient failures (e.g. HTTP 403 from googlevideo) are common when
/// downloading playlists, so each track is retried with backoff.
const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;

/// stderr markers that indicate a permanent failure where retrying is
/// pointless (unavailable/private/deleted videos, bad URLs, etc.).
const FATAL_DOWNLOAD_MARKERS: &[&str] = &[
    "Video unavailable",
    "Private video",
    "This video is not available",
    "HTTP Error 404",
    "HTTP Error 410",
    "Unsupported URL",
    "is not a valid URL",
    "members-only content",
    "Premieres in",
    "age-restricted",
];

/// Returns true if a yt-dlp failure looks transient and worth retrying.
fn is_retryable_download_error(stderr: &str) -> bool {
    !FATAL_DOWNLOAD_MARKERS
        .iter()
        .any(|marker| stderr.contains(marker))
}

/// Extract the "NN.N%" out of a `[download]  45.3% of ~10.0MiB …` line.
fn parse_download_percent(line: &str) -> Option<f32> {
    let rest = line.trim_start().strip_prefix("[download]")?.trim_start();
    let token = rest.split_whitespace().next()?;
    token.strip_suffix('%')?.parse().ok()
}

/// Drain a child pipe to its end, splitting on '\r' and '\n' — yt-dlp's
/// progress meter is carriage-return-separated and lands on *stdout* when
/// `--progress` forces it onto a non-TTY pipe. `[download] N%` lines are
/// forwarded as [`DownloadEvent::Percent`]; everything is collected into
/// one string for later parsing (filepath on stdout, errors on stderr).
fn drain_stream(reader: impl Read, progress: Option<Sender<DownloadEvent>>) -> String {
    let mut collected = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut reader = BufReader::new(reader);
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                for &byte in &chunk[..n] {
                    if byte == b'\n' || byte == b'\r' {
                        if pending.is_empty() {
                            continue;
                        }
                        let line = String::from_utf8_lossy(&pending).into_owned();
                        pending.clear();
                        if let Some(tx) = &progress
                            && let Some(pct) = parse_download_percent(&line)
                        {
                            let _ = tx.send(DownloadEvent::Percent(pct));
                        }
                        collected.push_str(&line);
                        collected.push('\n');
                    } else {
                        pending.push(byte);
                    }
                }
            }
        }
    }
    if !pending.is_empty() {
        collected.push_str(&String::from_utf8_lossy(&pending));
    }
    collected
}

/// Spawn `program` with the given args, draining stdout and stderr on
/// separate threads so full pipes can't deadlock the child.
fn run_tool(
    program: &str,
    args: &[String],
    missing_tool: DownloadError,
    progress: Option<&Sender<DownloadEvent>>,
) -> Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing_tool
            } else {
                DownloadError::Io(e)
            }
        })?;

    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr_progress = progress.cloned();
    let stdout_progress = progress.cloned();
    let stderr_thread = thread::spawn(move || drain_stream(stderr, stderr_progress));
    let stdout_thread = thread::spawn(move || drain_stream(stdout, stdout_progress));

    let status = child.wait()?;
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Ok(Output {
        status,
        stdout: stdout.into_bytes(),
        stderr: stderr.into_bytes(),
    })
}

/// YouTube downloader using yt-dlp (+ ffmpeg for chapter splitting).
/// Synchronous — meant to run on a worker thread, reporting progress to
/// the UI over an mpsc channel.
pub struct Downloader {
    options: DownloadOptions,
    progress: Option<Sender<DownloadEvent>>,
}

impl Downloader {
    pub fn new(options: DownloadOptions) -> Self {
        Self {
            options,
            progress: None,
        }
    }

    pub fn with_progress(mut self, tx: Sender<DownloadEvent>) -> Self {
        self.progress = Some(tx);
        self
    }

    fn report(&self, msg: impl Into<String>) {
        if let Some(tx) = &self.progress {
            let _ = tx.send(DownloadEvent::Status(msg.into()));
        }
    }

    fn report_file(&self, path: &Path) {
        if let Some(tx) = &self.progress {
            let _ = tx.send(DownloadEvent::FileDone(path.to_path_buf()));
        }
    }

    fn run_ytdlp(&self, args: &[String]) -> Result<Output> {
        run_tool(
            "yt-dlp",
            args,
            DownloadError::YtDlpNotFound,
            self.progress.as_ref(),
        )
    }

    /// Extract video/playlist information without downloading.
    pub fn get_info(&self) -> Result<Vec<VideoInfo>> {
        self.report(format!("Fetching info: {}", self.options.url));

        let output = self.run_ytdlp(&[
            "--dump-json".to_string(),
            "--flat-playlist".to_string(),
            "--no-warnings".to_string(),
            self.options.url.clone(),
        ])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DownloadError::InfoExtraction(stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut videos = Vec::new();

        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<VideoInfo>(line) {
                Ok(info) => videos.push(info),
                Err(e) => {
                    self.report(format!("Skipping unparseable video info: {e}"));
                }
            }
        }

        if videos.is_empty() {
            return Err(DownloadError::InfoExtraction("No videos found".to_string()));
        }

        self.report(format!("Found {} video(s)", videos.len()));
        Ok(videos)
    }

    /// Get detailed info for a single video (including chapters).
    pub fn get_detailed_info(&self, url: &str) -> Result<VideoInfo> {
        let output = self.run_ytdlp(&[
            "--dump-json".to_string(),
            "--no-warnings".to_string(),
            url.to_string(),
        ])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DownloadError::InfoExtraction(stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(serde_json::from_str(&stdout)?)
    }

    /// Download a single video's audio track in the configured format,
    /// optionally prefixing the output filename (e.g. "01 - " for playlist
    /// track ordering).
    fn download_audio_with_prefix(&self, url: &str, name_prefix: Option<&str>) -> Result<PathBuf> {
        ensure_output_dir(&self.options.output_dir)?;

        let filename_template = match name_prefix {
            Some(prefix) => format!("{prefix}%(title)s.%(ext)s"),
            None => "%(title)s.%(ext)s".to_string(),
        };
        let output_template = self
            .options
            .output_dir
            .join(filename_template)
            .to_string_lossy()
            .to_string();

        let args = vec![
            "--no-warnings".to_string(),
            // stderr isn't a TTY in the worker thread — force the progress
            // meter so the UI can show a percentage.
            "--progress".to_string(),
            "--extractor-args".to_string(),
            "youtube:player_client=default,android".to_string(),
            "--retries".to_string(),
            "10".to_string(),
            "--fragment-retries".to_string(),
            "10".to_string(),
            "--extractor-retries".to_string(),
            "3".to_string(),
            "--file-access-retries".to_string(),
            "3".to_string(),
            "--print".to_string(),
            "after_move:filepath".to_string(),
            "-o".to_string(),
            output_template,
            "-x".to_string(),
            "--audio-format".to_string(),
            self.options.format.yt_dlp_name().to_string(),
            "--audio-quality".to_string(),
            "0".to_string(),
            url.to_string(),
        ];

        let mut attempt = 0;
        let output = loop {
            attempt += 1;
            let output = self.run_ytdlp(&args)?;

            if output.status.success() {
                break output;
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("unknown error")
                .trim();

            if attempt >= MAX_DOWNLOAD_ATTEMPTS || !is_retryable_download_error(&stderr) {
                return Err(DownloadError::Download(format!(
                    "yt-dlp exited with {}: {}",
                    output.status, reason
                )));
            }

            let delay = Duration::from_secs(2u64.pow(attempt));
            self.report(format!(
                "Attempt {attempt}/{MAX_DOWNLOAD_ATTEMPTS} failed ({reason}); retrying in {}s",
                delay.as_secs()
            ));
            thread::sleep(delay);
        };

        // yt-dlp prints the final file path on stdout (--print after_move:filepath)
        let stdout = String::from_utf8_lossy(&output.stdout);
        let output_path = stdout
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .ok_or_else(|| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let detail = stderr
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim().to_string())
                    .unwrap_or_else(|| "yt-dlp produced no output".to_string());
                DownloadError::Download(format!(
                    "Could not determine output filename for {url}: {detail}"
                ))
            })?;

        self.report(format!("Downloaded {}", output_path.display()));
        Ok(output_path)
    }

    /// Download all videos from a playlist. Per-item failures are reported
    /// and skipped rather than aborting the whole playlist.
    pub fn download_playlist(&self) -> Result<Vec<PathBuf>> {
        let videos = self.get_info()?;
        let mut downloaded = Vec::new();

        for (i, video) in videos.iter().enumerate() {
            self.report(format!(
                "Downloading {}/{}: {}",
                i + 1,
                videos.len(),
                video.title
            ));

            let url = video
                .webpage_url
                .clone()
                .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", video.id));

            let track_number = video.playlist_index.unwrap_or((i + 1) as u32);
            let prefix = format!("{track_number:02} - ");

            match self.download_audio_with_prefix(&url, Some(&prefix)) {
                Ok(path) => {
                    self.report_file(&path);
                    downloaded.push(path);
                }
                Err(e) => {
                    self.report(format!("Failed to download {}: {e}", video.title));
                }
            }
        }

        if downloaded.is_empty() {
            return Err(DownloadError::Download(
                "All playlist downloads failed".to_string(),
            ));
        }

        Ok(downloaded)
    }

    /// Download with track splitting for long videos: videos with chapters
    /// become one audio file per chapter.
    pub fn download_with_split(&self, url: &str) -> Result<Vec<PathBuf>> {
        let info = self.get_detailed_info(url)?;

        if let Some(chapters) = &info.chapters
            && !chapters.is_empty()
        {
            self.report(format!(
                "Found {} chapters, will split into separate tracks",
                chapters.len()
            ));
            return self.download_and_split_by_chapters(url, &info);
        }

        self.report("No chapters found, downloading as single file");
        let path = self.download_audio_with_prefix(url, None)?;
        self.report_file(&path);
        Ok(vec![path])
    }

    /// Download the full audio, then cut it into one file per chapter with
    /// ffmpeg (re-encoded into the configured format).
    fn download_and_split_by_chapters(&self, url: &str, info: &VideoInfo) -> Result<Vec<PathBuf>> {
        ensure_output_dir(&self.options.output_dir)?;

        let chapters = info.chapters.as_ref().expect("checked by caller");
        let format = self.options.format;
        let base_title = sanitize_filename(&info.title);

        // First, download the full audio in the target format. The template
        // must use %(ext)s rather than a literal extension: yt-dlp only
        // treats a literal ext as the output slot when it matches the
        // --audio-format name, so a literal ".ogg" would produce
        // "…_full.ogg.ogg" and the exists() check below would fail.
        let temp_template = self
            .options
            .output_dir
            .join(format!("{base_title}_full.%(ext)s"));
        let temp_output = self
            .options
            .output_dir
            .join(format!("{base_title}_full.{}", format.extension()));
        let temp_output_str = temp_output.to_string_lossy().to_string();

        self.report("Downloading full audio for splitting…");

        let output = self.run_ytdlp(&[
            "--no-warnings".to_string(),
            "--progress".to_string(),
            "--extractor-args".to_string(),
            "youtube:player_client=default,android".to_string(),
            "--retries".to_string(),
            "10".to_string(),
            "--fragment-retries".to_string(),
            "10".to_string(),
            "--extractor-retries".to_string(),
            "3".to_string(),
            "--file-access-retries".to_string(),
            "3".to_string(),
            "-x".to_string(),
            "--audio-format".to_string(),
            format.yt_dlp_name().to_string(),
            "--audio-quality".to_string(),
            "0".to_string(),
            "-o".to_string(),
            temp_template.to_string_lossy().to_string(),
            url.to_string(),
        ])?;

        if !output.status.success() || !temp_output.exists() {
            return Err(DownloadError::Download(
                "Failed to download audio for splitting".to_string(),
            ));
        }

        // Split into chapters using ffmpeg.
        let mut output_files = Vec::new();
        let codec_args = format.ffmpeg_args();

        for (i, chapter) in chapters.iter().enumerate() {
            let chapter_title = sanitize_filename(&chapter.title);
            let output_file = self.options.output_dir.join(format!(
                "{:02} - {}.{}",
                i + 1,
                chapter_title,
                format.extension()
            ));
            let output_file_str = output_file.to_string_lossy().to_string();

            self.report(format!(
                "Extracting chapter {}/{}: {}",
                i + 1,
                chapters.len(),
                chapter.title
            ));

            let duration = chapter.end_time - chapter.start_time;
            let mut args = vec![
                "-i".to_string(),
                temp_output_str.clone(),
                "-ss".to_string(),
                chapter.start_time.to_string(),
                "-t".to_string(),
                duration.to_string(),
            ];
            args.extend(codec_args.iter().map(|s| s.to_string()));
            args.extend(["-y".to_string(), output_file_str]);

            let output = run_tool("ffmpeg", &args, DownloadError::FfmpegNotFound, None)?;

            if output.status.success() {
                self.report_file(&output_file);
                output_files.push(output_file);
            } else {
                self.report(format!("Failed to extract chapter: {}", chapter.title));
            }
        }

        // Clean up temp file.
        if temp_output.exists() {
            std::fs::remove_file(&temp_output)?;
        }

        if output_files.is_empty() {
            return Err(DownloadError::Conversion(
                "Chapter splitting produced no files".to_string(),
            ));
        }

        Ok(output_files)
    }

    /// Main download entry point.
    pub fn download(&self) -> Result<Vec<PathBuf>> {
        if is_playlist_url(&self.options.url) {
            self.download_playlist()
        } else if self.options.split_chapters {
            self.download_with_split(&self.options.url)
        } else {
            let path = self.download_audio_with_prefix(&self.options.url, None)?;
            self.report_file(&path);
            Ok(vec![path])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_403_is_retryable() {
        let stderr = "ERROR: unable to download video data: HTTP Error 403: Forbidden";
        assert!(is_retryable_download_error(stderr));
    }

    #[test]
    fn network_errors_are_retryable() {
        assert!(is_retryable_download_error(
            "ERROR: unable to download video data: <urlopen error timed out>"
        ));
        assert!(is_retryable_download_error(
            "ERROR: unable to download video data: HTTP Error 503: Service Unavailable"
        ));
    }

    #[test]
    fn permanent_failures_are_not_retryable() {
        assert!(!is_retryable_download_error(
            "ERROR: [youtube] abc123: Video unavailable"
        ));
        assert!(!is_retryable_download_error(
            "ERROR: [youtube] abc123: Private video. Sign in if you've been granted access"
        ));
        assert!(!is_retryable_download_error(
            "ERROR: unable to download video data: HTTP Error 404: Not Found"
        ));
        assert!(!is_retryable_download_error(
            "ERROR: [youtube] abc123: This video is age-restricted"
        ));
        assert!(!is_retryable_download_error(
            "ERROR: Unsupported URL: https://example.com/foo"
        ));
    }

    #[test]
    fn parses_download_percent_lines() {
        assert_eq!(
            parse_download_percent("[download]  45.3% of ~  10.02MiB at    1.23MiB/s"),
            Some(45.3)
        );
        assert_eq!(
            parse_download_percent("[download] 100% of   10.02MiB"),
            Some(100.0)
        );
        assert_eq!(parse_download_percent("[youtube] Extracting URL"), None);
        assert_eq!(parse_download_percent(""), None);
    }
}
