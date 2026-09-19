use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontId, RichText, Sense, Ui, vec2};

use crate::config::Config;
use crate::library::{self, Track};
use crate::player::{AudioPlayer, PlayState};
use crate::playlist::Playlist;
use crate::visualizer::{FFT_SIZE, NUM_BANDS, SpectrumAnalyzer};

const ACCENT: Color32 = Color32::from_rgb(0, 255, 128); // winamp-ish green
const ROW_HEIGHT: f32 = 22.0;

pub struct RustampApp {
    config: Config,
    playlist: Playlist,
    player: Option<AudioPlayer>,
    audio_error: Option<String>,
    scan_rx: Option<Receiver<Vec<Track>>>,
    selected: Option<usize>,
    filter: String,
    seek_drag: Option<Duration>,
    status: Option<(String, Instant)>,
    analyzer: SpectrumAnalyzer,
}

impl RustampApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        let player = match AudioPlayer::new() {
            Ok(p) => {
                p.set_volume(config.volume);
                Some(p)
            }
            Err(e) => {
                eprintln!("audio init failed: {e}");
                None
            }
        };
        let audio_error = player
            .is_none()
            .then(|| "No audio output device found — playback disabled.".to_string());
        let mut app = Self {
            config,
            playlist: Playlist::new(),
            player,
            audio_error,
            scan_rx: None,
            selected: None,
            filter: String::new(),
            seek_drag: None,
            status: None,
            analyzer: SpectrumAnalyzer::new(),
        };
        app.rescan();
        app
    }

    fn rescan(&mut self) {
        let (tx, rx) = mpsc::channel();
        let folders = self.config.folders.clone();
        self.scan_rx = Some(rx);
        thread::spawn(move || {
            let _ = tx.send(library::scan_folders(&folders));
        });
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    /// Load and play `track_index` (index into playlist.tracks).
    fn play_index(&mut self, track_index: usize) {
        let Some(player) = &mut self.player else {
            self.set_status("No audio device available");
            return;
        };
        let path = self.playlist.tracks[track_index].path.clone();
        match player.play(&path) {
            Ok(duration) => {
                if let Some(d) = duration {
                    self.playlist.set_duration(track_index, d);
                }
            }
            Err(e) => self.set_status(format!("Cannot play {}: {e}", path.display())),
        }
        self.selected = Some(track_index);
    }

    fn play_selected(&mut self, track_index: usize) {
        if let Some(idx) = self.playlist.select(track_index) {
            self.play_index(idx);
        }
    }

    /// Toggle play/pause. If stopped, (re)start the selected or current track.
    fn toggle_play(&mut self) {
        let Some(player) = &mut self.player else {
            self.set_status("No audio device available");
            return;
        };
        match player.state {
            PlayState::Playing => player.pause(),
            PlayState::Paused => player.resume(),
            PlayState::Stopped => {
                let idx = self.selected.or(self.playlist.current_index());
                if let Some(idx) = idx {
                    self.play_selected(idx);
                } else if !self.playlist.is_empty() {
                    self.play_selected(0);
                } else {
                    self.set_status("Playlist is empty — add a folder first");
                }
            }
        }
    }

    fn stop(&mut self) {
        if let Some(player) = &mut self.player {
            player.stop();
        }
    }

    /// Move to next/previous track (user-initiated, wraps around).
    fn step(&mut self, forward: bool) {
        if self.playlist.is_empty() {
            return;
        }
        if self.playlist.current_index().is_none() {
            self.play_selected(0);
            return;
        }
        if let Some(idx) = self.playlist.step_manual(forward) {
            self.play_index(idx);
        }
    }

    /// Auto-advance when a track finishes. Honors repeat mode via the playlist.
    /// Skips unplayable files rather than dying on the first bad one.
    fn maybe_auto_advance(&mut self) {
        if !self.player.as_ref().is_some_and(|p| p.finished()) {
            return;
        }
        for _ in 0..self.playlist.tracks.len().max(1) {
            let Some(idx) = self.playlist.advance_auto() else {
                break;
            };
            let path = self.playlist.tracks[idx].path.clone();
            match self.player.as_mut().unwrap().play(&path) {
                Ok(duration) => {
                    if let Some(d) = duration {
                        self.playlist.set_duration(idx, d);
                    }
                    self.selected = Some(idx);
                    return;
                }
                Err(e) => {
                    self.set_status(format!("Skipping {}: {e}", path.display()));
                }
            }
        }
        self.stop();
    }

    fn current_position(&self) -> Duration {
        self.seek_drag.unwrap_or_else(|| {
            self.player
                .as_ref()
                .map(|p| p.position())
                .unwrap_or_default()
        })
    }

    fn analyzer_rate(&self) -> u32 {
        self.player
            .as_ref()
            .map(|p| p.sample_rate())
            .unwrap_or(44_100)
    }

    fn current_duration(&self) -> Option<Duration> {
        self.player.as_ref().and_then(|p| p.duration()).or_else(|| {
            self.playlist
                .current_index()
                .and_then(|i| self.playlist.tracks[i].duration)
        })
    }
}

fn format_time(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Winamp-style scrolling title for the "now playing" line.
fn marquee(ui: &mut Ui, text: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::monospace(15.0), color);
    let mut x = rect.left() + 4.0;
    if galley.size().x > rect.width() - 8.0 {
        let period = galley.size().x + 80.0;
        x -= (ui.input(|i| i.time) as f32 * 50.0) % period;
        ui.ctx().request_repaint();
    }
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter()
        .with_clip_rect(rect)
        .galley(egui::pos2(x, y), galley, color);
}

/// Winamp-style spectrum analyzer: segmented bars with a brighter peak cell
/// that falls more slowly than the bar itself.
fn spectrum(ui: &mut Ui, bars: &[f32; NUM_BANDS], peaks: &[f32; NUM_BANDS]) {
    const HEIGHT: f32 = 64.0;
    const SEG_H: f32 = 5.0;
    const SEG_GAP: f32 = 2.0;
    const BAR_GAP: f32 = 3.0;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(8, 12, 8));

    let n = bars.len() as f32;
    let bar_w = ((rect.width() - 8.0 - BAR_GAP * (n - 1.0)) / n).max(1.0);
    let pitch = SEG_H + SEG_GAP;
    let segs = ((rect.height() - 4.0) / pitch).floor().max(1.0) as i32;

    let color_at = |frac: f32, lit: bool| {
        if !lit {
            return Color32::from_rgb(18, 30, 20); // ghost grid
        }
        if frac < 0.62 {
            Color32::from_rgb(0, 210, 90)
        } else if frac < 0.85 {
            Color32::from_rgb(240, 200, 40)
        } else {
            Color32::from_rgb(255, 60, 40)
        }
    };

    for i in 0..NUM_BANDS {
        let x0 = rect.left() + 4.0 + i as f32 * (bar_w + BAR_GAP);
        let lit = (bars[i] * segs as f32).round() as i32;
        let peak = (peaks[i] * segs as f32).round() as i32;
        for s in 0..segs {
            let y1 = rect.bottom() - 2.0 - s as f32 * pitch;
            let seg_rect =
                egui::Rect::from_min_max(egui::pos2(x0, y1 - SEG_H), egui::pos2(x0 + bar_w, y1));
            let frac = s as f32 / segs as f32;
            if s < lit {
                painter.rect_filled(seg_rect, 1.0, color_at(frac, true));
            } else if s == peak - 1 && peak > lit {
                painter.rect_filled(seg_rect, 1.0, Color32::from_rgb(255, 240, 200));
            } else {
                painter.rect_filled(seg_rect, 1.0, color_at(frac, false));
            }
        }
    }
}

impl eframe::App for RustampApp {
    /// Non-UI work: poll the scan thread, advance finished tracks, keys.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.scan_rx {
            match rx.try_recv() {
                Ok(tracks) => {
                    let n = tracks.len();
                    self.playlist.set_tracks(tracks, &mut rand::rng());
                    self.scan_rx = None;
                    self.set_status(format!("Library scan complete: {n} tracks"));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scan_rx = None,
            }
        }

        self.maybe_auto_advance();
        self.handle_keys(ctx);

        let playing = self
            .player
            .as_ref()
            .is_some_and(|p| p.state == PlayState::Playing);

        // Feed the spectrum analyzer: live samples while playing, silence
        // otherwise so the bars fall gracefully on pause/stop.
        let (samples, rate) = if playing {
            let p = self.player.as_ref().unwrap();
            (p.sample_buffer().latest(FFT_SIZE), p.sample_rate())
        } else {
            (vec![0.0; FFT_SIZE], self.analyzer_rate())
        };
        self.analyzer.update(&samples, rate);
        let fresh_status = self
            .status
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(6));
        // Keep animating while the spectrum still has energy to decay —
        // egui only runs frames on input otherwise, freezing the bars.
        let bars_alive = self
            .analyzer
            .bars
            .iter()
            .chain(self.analyzer.peaks.iter())
            .any(|&v| v > 0.002);
        if playing || self.seek_drag.is_some() || fresh_status || bars_alive {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui_top(ui);
        self.ui_bottom(ui);
        self.ui_folders(ui);
        self.ui_playlist(ui);
    }
}

impl RustampApp {
    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return; // don't steal keys from the filter box
        }
        let (space, left, right, up, down, next, prev, enter) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::N),
                i.key_pressed(egui::Key::P),
                i.key_pressed(egui::Key::Enter),
            )
        });
        if space {
            self.toggle_play();
        }
        if next {
            self.step(true);
        }
        if prev {
            self.step(false);
        }
        if enter && let Some(idx) = self.selected {
            self.play_selected(idx);
        }
        if left || right {
            let delta = if right { 5.0 } else { -5.0 };
            if let (Some(player), Some(dur)) = (&self.player, self.current_duration()) {
                let pos = player.position().as_secs_f32() + delta;
                player.seek(Duration::from_secs_f32(pos.clamp(0.0, dur.as_secs_f32())));
            }
        }
        if (up || down)
            && let Some(player) = &self.player
        {
            let new_vol = (self.config.volume + if up { 0.05 } else { -0.05 }).clamp(0.0, 1.0);
            player.set_volume(new_vol);
            self.config.volume = new_vol;
        }
    }

    fn ui_top(&mut self, ui: &mut Ui) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("RUSTAMP")
                        .font(FontId::monospace(18.0))
                        .color(ACCENT)
                        .strong(),
                );
                ui.separator();
                let now_playing = self
                    .playlist
                    .current()
                    .map(|t| t.display_title())
                    .unwrap_or_else(|| "—".to_string());
                let prefix = match self.player.as_ref().map(|p| p.state) {
                    Some(PlayState::Playing) => "> ",
                    Some(PlayState::Paused) => "|| ",
                    _ => "",
                };
                marquee(ui, &format!("{prefix}{now_playing}"), ACCENT);
            });
            let bars = self.analyzer.bars;
            let peaks = self.analyzer.peaks;
            spectrum(ui, &bars, &peaks);
            ui.add_space(2.0);
        });
    }

    fn ui_bottom(&mut self, ui: &mut Ui) {
        egui::Panel::bottom("transport").show(ui, |ui| {
            ui.add_space(6.0);

            // Seek bar with time labels.
            let duration = self.current_duration();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format_time(self.current_position()))
                        .monospace()
                        .color(Color32::LIGHT_GRAY),
                );

                let position = self.current_position();
                let mut shown = duration
                    .map(|d| (position.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0))
                    .unwrap_or(0.0);
                // Reserve ~55px for the trailing duration label.
                let width = (ui.available_width() - 55.0).max(40.0);
                let slider = egui::Slider::new(&mut shown, 0.0..=1.0)
                    .show_value(false)
                    .trailing_fill(true);
                let resp = if duration.is_some() {
                    ui.add_sized([width, 20.0], slider)
                } else {
                    ui.add_enabled(false, slider)
                };
                if resp.dragged()
                    && let Some(d) = duration
                {
                    self.seek_drag = Some(Duration::from_secs_f32(shown * d.as_secs_f32()));
                }
                if resp.drag_stopped()
                    && let Some(target) = self.seek_drag.take()
                    && let Some(player) = &self.player
                {
                    player.seek(target);
                }

                ui.label(
                    RichText::new(duration.map(format_time).unwrap_or_else(|| "--:--".into()))
                        .monospace()
                        .color(Color32::LIGHT_GRAY),
                );
            });

            // Transport buttons + volume + modes.
            ui.horizontal(|ui| {
                if ui.button("<< Prev").clicked() {
                    self.step(false);
                }
                let play_label = match self.player.as_ref().map(|p| p.state) {
                    Some(PlayState::Playing) => "|| Pause",
                    Some(PlayState::Paused) => "> Resume",
                    _ => "> Play",
                };
                if ui
                    .button(RichText::new(play_label).color(ACCENT).strong())
                    .clicked()
                {
                    self.toggle_play();
                }
                if ui.button("[] Stop").clicked() {
                    self.stop();
                }
                if ui.button("Next >>").clicked() {
                    self.step(true);
                }

                ui.separator();

                let shuffle_text = if self.playlist.shuffle {
                    RichText::new("Shuffle").color(ACCENT)
                } else {
                    RichText::new("Shuffle")
                };
                if ui
                    .selectable_label(self.playlist.shuffle, shuffle_text)
                    .clicked()
                {
                    let on = !self.playlist.shuffle;
                    self.playlist.set_shuffle(on, &mut rand::rng());
                }
                if ui.button(self.playlist.repeat.label()).clicked() {
                    self.playlist.repeat = self.playlist.repeat.cycle();
                }

                ui.separator();

                ui.label("Vol");
                let mut volume = self.config.volume;
                let resp = ui.add_sized(
                    [90.0, 20.0],
                    egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false),
                );
                if resp.changed() {
                    self.config.volume = volume;
                    if let Some(player) = &self.player {
                        player.set_volume(volume);
                    }
                }
                if resp.drag_stopped() {
                    let _ = self.config.save();
                }
            });
            ui.add_space(4.0);
        });
    }

    fn ui_folders(&mut self, ui: &mut Ui) {
        egui::Panel::left("folders")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                ui.heading("Watch folders");
                ui.add_space(4.0);
                if ui.button("Add folder…").clicked()
                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                {
                    if self.config.add_folder(dir.clone()) {
                        let _ = self.config.save();
                        self.rescan();
                    } else {
                        self.set_status(format!("{} already watched", dir.display()));
                    }
                }
                if ui.button("Rescan now").clicked() {
                    self.rescan();
                }
                ui.add_space(6.0);
                let mut to_remove = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for folder in &self.config.folders {
                        ui.horizontal(|ui| {
                            if ui.small_button("x").clicked() {
                                to_remove = Some(folder.clone());
                            }
                            ui.label(folder.display().to_string())
                                .on_hover_text(folder.display().to_string());
                        });
                    }
                    if self.config.folders.is_empty() {
                        ui.label(
                            RichText::new("No folders yet.\nAdd one to build your library.").weak(),
                        );
                    }
                });
                if let Some(folder) = to_remove {
                    self.config.remove_folder(&folder);
                    let _ = self.config.save();
                    self.rescan();
                }
            });
    }

    fn ui_playlist(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Playlist ({} tracks)", self.playlist.tracks.len()))
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("Filter:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .desired_width(180.0)
                            .hint_text("artist / title / album"),
                    );
                });
            });
            if self.scan_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Scanning folders…");
                });
            }
            if let Some((msg, at)) = &self.status
                && at.elapsed() < Duration::from_secs(6)
            {
                ui.label(RichText::new(msg.as_str()).weak());
            }
            if let Some(err) = &self.audio_error {
                ui.label(RichText::new(err.as_str()).color(Color32::RED));
            }
            ui.separator();

            let visible: Vec<usize> = self
                .playlist
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| library::matches_filter(t, &self.filter))
                .map(|(i, _)| i)
                .collect();

            let current = self.playlist.current_index();
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show_rows(ui, ROW_HEIGHT, visible.len(), |ui, range| {
                    for row in range {
                        let track_index = visible[row];
                        // Copy display data out so click handlers can mutate self.
                        let (title, track_duration) = {
                            let t = &self.playlist.tracks[track_index];
                            (t.display_title(), t.duration)
                        };
                        let is_current = current == Some(track_index);
                        let is_selected = self.selected == Some(track_index);

                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [40.0, ROW_HEIGHT],
                                egui::Label::new(RichText::new(format!("{}", row + 1)).weak()),
                            );
                            let mut text = RichText::new(title);
                            if is_current {
                                text = text.color(ACCENT).strong();
                            }
                            let resp = ui.selectable_label(is_selected, text);
                            if resp.clicked() {
                                self.selected = Some(track_index);
                            }
                            if resp.double_clicked() {
                                self.play_selected(track_index);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(
                                            track_duration
                                                .map(format_time)
                                                .unwrap_or_else(|| "--:--".into()),
                                        )
                                        .weak()
                                        .monospace(),
                                    );
                                },
                            );
                        });
                    }
                });
        });
    }
}
