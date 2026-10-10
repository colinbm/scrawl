mod app;
mod autosave;
mod canvas;
mod commands;
mod dev;
mod gpu;
mod import;
#[cfg(target_os = "macos")]
mod native_menu;
mod plugins;
mod prefs;
mod preview;
mod prototyping;
mod rulers;
mod snapping;
mod style;
mod text_edit;
mod ui;
#[cfg(test)]
mod ui_tests;
mod vector;
mod versions;
mod viewport;
mod workspace;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(std::path::PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scrawl")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Scrawl", options, Box::new(|cc| {
            // Scripted runs (dev hooks) keep their files out of the user's own folder.
            let support = match std::env::var_os("SCRAWL_SUPPORT") {
                Some(dir) => Some(std::path::PathBuf::from(dir)),
                None if dev::DevHooks::from_env().active() => None,
                None => prefs::support_dir(),
            };
            let mut ws = workspace::Workspace::new(cc, paths, support);
            ws.install_native_menu(&cc.egui_ctx);
            Ok(Box::new(ws))
        }),
    )
}
