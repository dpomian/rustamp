#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rustamp")
            .with_inner_size([520.0, 640.0])
            .with_resizable(false)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png"))
                    .expect("assets/logo.png must be a valid PNG"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "Rustamp",
        options,
        Box::new(|cc| Ok(Box::new(rustamp::ui::RustampApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_logo_decodes_as_icon() {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png"))
            .expect("logo.png should decode");
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(
            icon.rgba.len() as u64,
            u64::from(icon.width) * u64::from(icon.height) * 4
        );
    }
}
