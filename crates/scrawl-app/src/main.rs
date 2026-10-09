mod app;
mod canvas;
mod dev;
mod gpu;
mod style;
mod text_edit;
mod ui;
#[cfg(test)]
mod ui_tests;
mod vector;
mod viewport;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scrawl")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Scrawl", options, Box::new(|cc| Ok(Box::new(app::ScrawlApp::new(cc, path)))))
}
