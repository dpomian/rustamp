use std::time::Duration;

use eframe::egui::{self, Color32, FontId, Sense, Ui, vec2};

use crate::audio::NUM_BANDS;
use crate::library::Track;

pub fn format_time(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Winamp-style sample-rate label: "44.1kHz", "48kHz", "22.05kHz".
fn format_khz(rate: u32) -> String {
    if rate.is_multiple_of(1000) {
        format!("{}kHz", rate / 1000)
    } else {
        let s = format!("{:.2}", rate as f64 / 1000.0);
        format!("{}kHz", s.trim_end_matches('0').trim_end_matches('.'))
    }
}

/// "Now playing" line: "Artist - Title - Album :: 3:45 :: 44.1kHz".
/// Missing parts are skipped along with their separators; the title
/// falls back to the file stem like the playlist does.
pub fn now_playing(track: &Track, duration: Option<Duration>, sample_rate: Option<u32>) -> String {
    let mut names: Vec<String> = Vec::new();
    if let Some(a) = track.artist.as_deref().filter(|s| !s.trim().is_empty()) {
        names.push(a.to_string());
    }
    names.push(track.title_or_stem());
    if let Some(a) = track.album.as_deref().filter(|s| !s.trim().is_empty()) {
        names.push(a.to_string());
    }
    let mut line = names.join(" - ");
    if let Some(d) = duration {
        line += &format!(" :: {}", format_time(d));
    }
    if let Some(rate) = sample_rate {
        line += &format!(" :: {}", format_khz(rate));
    }
    line
}

/// Winamp-style scrolling title for the "now playing" line.
///
/// When the text is wider than the view it slides left until its tail is
/// visible — pausing at the start and the end, then snapping back. The
/// cycle restarts whenever the text changes, so a new track always gets
/// its initial hold.
pub fn marquee(ui: &mut Ui, text: &str, color: Color32) {
    const SPEED: f32 = 30.0; // px/s
    const HOLD_START: f64 = 5.0;
    const HOLD_END: f64 = 3.0;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::monospace(15.0), color);
    let overflow = galley.size().x - (rect.width() - 8.0);
    let mut x = rect.left() + 4.0;
    if overflow > 0.0 {
        let now = ui.input(|i| i.time);
        let start = ui.ctx().data_mut(|d| {
            let state =
                d.get_temp_mut_or_insert_with(ui.id().with("marquee"), || (text.to_string(), now));
            if state.0 != text {
                *state = (text.to_string(), now);
            }
            state.1
        });
        let scroll_time = f64::from(overflow / SPEED);
        let period = HOLD_START + scroll_time + HOLD_END;
        let t = (now - start).max(0.0) % period;
        x -= if t < HOLD_START {
            0.0
        } else if t < HOLD_START + scroll_time {
            ((t - HOLD_START) * f64::from(SPEED)) as f32
        } else {
            overflow
        };
        ui.ctx().request_repaint();
    }
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter()
        .with_clip_rect(rect)
        .galley(egui::pos2(x, y), galley, color);
}

/// Tiny gap between adjacent equalizer bars.
const BAR_GAP: f32 = 2.0;

/// Width of one bar when `NUM_BANDS` bars span `width` edge to edge with
/// `BAR_GAP` between each pair.
fn bar_width(width: f32) -> f32 {
    ((width - 8.0 - BAR_GAP * (NUM_BANDS as f32 - 1.0)) / NUM_BANDS as f32).max(1.0)
}

/// Winamp-style spectrum analyzer: segmented bars with a brighter peak cell
/// that falls more slowly than the bar itself.
pub fn spectrum(ui: &mut Ui, bars: &[f32; NUM_BANDS], peaks: &[f32; NUM_BANDS]) {
    const HEIGHT: f32 = 64.0;
    const SEG_H: f32 = 5.0;
    const SEG_GAP: f32 = 2.0;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(8, 12, 8));

    let bar_w = bar_width(rect.width());
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn track(artist: Option<&str>, title: Option<&str>, album: Option<&str>) -> Track {
        Track {
            path: PathBuf::from("/music/01 - Templars.mp3"),
            title: title.map(String::from),
            artist: artist.map(String::from),
            album: album.map(String::from),
            duration: None,
        }
    }

    #[test]
    fn now_playing_joins_all_parts() {
        let t = track(Some("Sabaton"), Some("Templars"), Some("The Great War"));
        assert_eq!(
            now_playing(&t, Some(Duration::from_secs(225)), Some(44_100)),
            "Sabaton - Templars - The Great War :: 3:45 :: 44.1kHz"
        );
    }

    #[test]
    fn now_playing_skips_missing_parts() {
        let t = track(Some("Sabaton"), Some("Templars"), None);
        assert_eq!(
            now_playing(&t, Some(Duration::from_secs(225)), Some(48_000)),
            "Sabaton - Templars :: 3:45 :: 48kHz"
        );
        let t = track(None, Some("Templars"), None);
        assert_eq!(now_playing(&t, None, None), "Templars");
    }

    #[test]
    fn now_playing_falls_back_to_file_stem_for_title() {
        let t = track(None, None, None);
        assert_eq!(now_playing(&t, None, None), "01 - Templars");
    }

    #[test]
    fn now_playing_ignores_blank_tags() {
        let t = track(Some("  "), Some("Templars"), Some(""));
        assert_eq!(now_playing(&t, None, None), "Templars");
    }

    #[test]
    fn format_khz_trims_whole_rates() {
        assert_eq!(format_khz(44_100), "44.1kHz");
        assert_eq!(format_khz(48_000), "48kHz");
        assert_eq!(format_khz(22_050), "22.05kHz");
        assert_eq!(format_khz(96_000), "96kHz");
    }

    #[test]
    fn bars_span_the_full_width() {
        let width = 512.0;
        let bar_w = bar_width(width);
        let total = NUM_BANDS as f32 * bar_w + (NUM_BANDS as f32 - 1.0) * BAR_GAP;
        assert!((total - (width - 8.0)).abs() < 1.0);
    }

    #[test]
    fn degenerate_width_still_yields_drawable_bars() {
        assert!(bar_width(0.0) >= 1.0);
    }
}
