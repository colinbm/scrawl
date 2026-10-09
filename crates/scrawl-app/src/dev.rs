//! Developer hooks driven by environment variables, for visual checks without a human at the
//! keyboard (and without OS screen-recording permission):
//!
//! - `SCRAWL_SCREENSHOT=out.png` saves the window's own framebuffer after a few frames, then quits.
//! - `SCRAWL_SELECT=name[,name…]` selects the layers with these names on the first page at startup.

use std::path::PathBuf;

use crate::app::ScrawlApp;

const SETTLE_FRAMES: u32 = 10;

#[derive(Default)]
pub struct DevHooks {
    screenshot: Option<PathBuf>,
    select: Vec<String>,
    frame: u32,
    requested: bool,
}

impl DevHooks {
    pub fn from_env() -> Self {
        Self {
            screenshot: std::env::var_os("SCRAWL_SCREENSHOT").map(PathBuf::from),
            select: std::env::var("SCRAWL_SELECT")
                .map(|s| s.split(',').map(str::to_owned).collect())
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    pub fn on_frame(app: &mut ScrawlApp, ctx: &egui::Context) {
        let hooks = &mut app.dev;
        hooks.frame += 1;
        if hooks.frame == 2 && !hooks.select.is_empty() {
            let names = std::mem::take(&mut hooks.select);
            let mut ids = vec![];
            app.page().walk(|l| {
                if names.contains(&l.name) {
                    ids.push(l.id);
                }
            });
            app.selection = ids;
        }
        let Some(path) = app.dev.screenshot.clone() else { return };
        if app.dev.frame >= SETTLE_FRAMES && !app.dev.requested {
            app.dev.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(e) = write_png(&path, &image) {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn write_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    let [w, h] = image.size;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    enc.write_header()?.write_image_data(&bytes)?;
    Ok(())
}
