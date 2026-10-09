use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use eframe::egui::{self, RichText};

use crate::config::Config;
use crate::download::{
    AudioFormat, DownloadEvent, DownloadOptions, Downloader, ToolsStatus, is_valid_youtube_url,
    tools_status,
};

/// Status lines kept in the dialog's scrollback.
const MAX_LOG_LINES: usize = 12;

/// Floating "Download from YouTube" window. Owns the URL/options form, the
/// worker thread's event channel, and a small log of what the worker is
/// doing. The actual yt-dlp/ffmpeg work lives in `crate::download` — this
/// is only presentation and wiring.
#[derive(Default)]
pub struct DownloadDialog {
    open: bool,
    url: String,
    format: AudioFormat,
    split_chapters: bool,
    tools: Option<ToolsStatus>,
    running: bool,
    rx: Option<Receiver<DownloadEvent>>,
    percent: Option<f32>,
    log: VecDeque<String>,
}

impl DownloadDialog {
    /// Open the window and probe for yt-dlp/ffmpeg so missing tools can be
    /// flagged before the user hits Download.
    pub fn open(&mut self) {
        self.open = true;
        if self.tools.is_none() {
            self.tools = Some(tools_status());
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Drain events from the worker thread; returns paths that finished
    /// downloading and should be added to the playlist.
    pub fn poll_events(&mut self) -> Vec<PathBuf> {
        let mut events = Vec::new();
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok(ev) => events.push(ev),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.rx = None;
                        self.running = false;
                        break;
                    }
                }
            }
        }

        let mut files = Vec::new();
        for ev in events {
            match ev {
                DownloadEvent::Status(msg) => self.push_log(msg),
                DownloadEvent::Percent(p) => self.percent = Some(p),
                DownloadEvent::FileDone(path) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    self.push_log(format!("Done: {name}"));
                    files.push(path);
                }
                DownloadEvent::Finished(all) => {
                    self.running = false;
                    self.percent = None;
                    self.push_log(format!("Download complete: {} file(s)", all.len()));
                    // FileDone events cover the same paths; the app dedups.
                    files.extend(all);
                }
                DownloadEvent::Failed(msg) => {
                    self.running = false;
                    self.percent = None;
                    self.push_log(format!("Failed: {msg}"));
                }
            }
        }
        files
    }

    fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push_back(msg.into());
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
        }
    }

    /// Spawn the worker thread running the download. Events flow back
    /// through `rx` and are drained by [`poll_events`](Self::poll_events).
    fn start(&mut self, output_dir: PathBuf, ctx: &egui::Context) {
        // Strip backslashes from shell-escaped URLs (e.g. "watch\?v\=...")
        let url = self.url.trim().replace('\\', "");
        let options = DownloadOptions {
            url,
            output_dir,
            format: self.format,
            split_chapters: self.split_chapters,
        };

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.running = true;
        self.percent = None;
        self.log.clear();
        self.push_log("Starting download…");

        let ctx = ctx.clone();
        thread::spawn(move || {
            let downloader = Downloader::new(options).with_progress(tx.clone());
            let _ = tx.send(match downloader.download() {
                Ok(files) => DownloadEvent::Finished(files),
                Err(e) => DownloadEvent::Failed(e.to_string()),
            });
            ctx.request_repaint();
        });
    }

    /// Render the window. `config` supplies the download directory (and is
    /// updated + saved when the user picks a different one) and the skin
    /// colors for warnings.
    pub fn show(&mut self, ctx: &egui::Context, config: &mut Config) {
        if !self.open {
            return;
        }
        let error_color = config.skin.error;
        let accent = config.skin.accent;
        let mut open = self.open;
        egui::Window::new("Download from YouTube")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(ctx, |ui| {
                if let Some(tools) = self.tools {
                    if !tools.yt_dlp {
                        ui.label(
                            RichText::new("yt-dlp not found — install it (e.g. brew install yt-dlp)")
                                .color(error_color),
                        );
                    }
                    if !tools.ffmpeg {
                        ui.label(
                            RichText::new(
                                "ffmpeg not found — audio conversion needs it (brew install ffmpeg)",
                            )
                            .color(error_color),
                        );
                    }
                }

                ui.label("Video or playlist URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.url)
                        .desired_width(f32::INFINITY)
                        .hint_text("https://www.youtube.com/watch?v=…"),
                );
                let url_empty = self.url.trim().is_empty();
                if !url_empty && !is_valid_youtube_url(self.url.trim()) {
                    ui.label(
                        RichText::new("This doesn't look like a YouTube URL").color(error_color),
                    );
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Format:");
                    egui::ComboBox::from_id_salt("dl_format")
                        .selected_text(self.format.label())
                        .show_ui(ui, |ui| {
                            for format in AudioFormat::ALL {
                                ui.selectable_value(&mut self.format, format, format.label());
                            }
                        });
                    ui.separator();
                    ui.add_enabled(
                        self.tools.is_none_or(|t| t.ffmpeg),
                        egui::Checkbox::new(&mut self.split_chapters, "Split chapters into tracks"),
                    )
                    .on_hover_text(
                        "Long videos with chapters become one audio file per chapter",
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Save to:");
                    let dir = config.download_dir();
                    ui.add(
                        egui::Label::new(dir.display().to_string())
                            .truncate()
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    if ui.button("Browse…").clicked()
                        && let Some(picked) = rfd::FileDialog::new()
                            .set_directory(&dir)
                            .pick_folder()
                    {
                        config.download_dir = Some(picked);
                        let _ = config.save();
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let can_start =
                        !self.running && !url_empty && self.tools.is_none_or(|t| t.yt_dlp);
                    if ui
                        .add_enabled(can_start, egui::Button::new("Download"))
                        .clicked()
                    {
                        self.start(config.download_dir(), ui.ctx());
                    }
                    if self.running {
                        ui.spinner();
                        if let Some(pct) = self.percent {
                            ui.add(
                                egui::ProgressBar::new(pct / 100.0)
                                    .desired_width(160.0)
                                    .show_percentage(),
                            );
                        }
                    }
                });

                if let Some(last) = self.log.back()
                    && self.running
                {
                    ui.label(RichText::new(last.as_str()).color(accent));
                }

                if !self.log.is_empty() {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("dl_log")
                        .max_height(110.0)
                        .auto_shrink([false, true])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.log {
                                ui.label(RichText::new(line.as_str()).weak().small());
                            }
                        });
                }
            });
        self.open = open;
    }
}
