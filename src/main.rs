mod app;
mod config;
mod library;
mod player;
mod playlist;
mod visualizer;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rustamp")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([680.0, 440.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rustamp",
        options,
        Box::new(|cc| Ok(Box::new(app::RustampApp::new(cc)))),
    )
}
