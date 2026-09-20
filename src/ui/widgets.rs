use std::time::Duration;

use eframe::egui::{self, Color32, FontId, Sense, Ui, vec2};

use crate::audio::NUM_BANDS;

pub fn format_time(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Winamp-style scrolling title for the "now playing" line.
pub fn marquee(ui: &mut Ui, text: &str, color: Color32) {
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
    use super::*;

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
