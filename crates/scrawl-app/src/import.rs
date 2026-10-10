//! Bringing files into the document: dropped on the canvas or chosen with Insert › Image….

use std::path::Path;

use scrawl_core::kurbo::{Point, Vec2};
use scrawl_core::{Frame, Layer, LayerId, ops};

use crate::app::ScrawlApp;

/// File types offered by the Insert › Image… dialog.
pub const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// The pixel size of an encoded image.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// Adds an image file as an image layer centered on `at` (page space), inside the artboard
/// there. Returns the new layer.
pub fn import_image(app: &mut ScrawlApp, name: &str, bytes: Vec<u8>, at: Point) -> Result<LayerId, String> {
    let (w, h) = image_size(&bytes).ok_or("not a PNG, JPEG, GIF or WebP image")?;
    let id = app.doc.assets.add_image(bytes).ok_or("not a PNG, JPEG, GIF or WebP image")?;
    let frame = Frame::new((at.x - w as f64 / 2.0).round(), (at.y - h as f64 / 2.0).round(), w as f64, h as f64);
    let mut layer = Layer::image(id, frame);
    if !name.is_empty() {
        layer.name = name.to_owned();
    }
    Ok(ops::insert_drawn(app.page_mut(), layer))
}

/// Imports files, cascading them from `at`, and selects what was added. Problems are reported
/// in the error dialog.
pub fn import_files(app: &mut ScrawlApp, files: Vec<(String, Vec<u8>)>, at: Point) {
    let mut added = vec![];
    let mut errors = vec![];
    for (i, (name, bytes)) in files.into_iter().enumerate() {
        let stem = Path::new(&name).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let at = at + Vec2::splat(20.0 * i as f64);
        match import_image(app, &stem, bytes, at) {
            Ok(id) => added.push(id),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    if !added.is_empty() {
        app.selection = added;
        app.tool = crate::canvas::Tool::Select;
    }
    if !errors.is_empty() {
        app.error = Some(format!("Couldn't import:\n{}", errors.join("\n")));
    }
}

/// The page-space point at the center of the canvas.
fn view_center(app: &ScrawlApp) -> Point {
    app.viewport().to_page(app.canvas.rect, app.canvas.rect.center())
}

/// Insert › Image…: asks for files and places them at the center of the view.
pub fn choose_images(app: &mut ScrawlApp) {
    let Some(paths) = rfd::FileDialog::new().add_filter("Images", &IMAGE_EXTENSIONS).pick_files() else { return };
    let files = read_paths(app, &paths);
    let at = view_center(app);
    import_files(app, files, at);
}

fn read_paths(app: &mut ScrawlApp, paths: &[std::path::PathBuf]) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![];
    for p in paths {
        match std::fs::read(p) {
            Ok(bytes) => files.push((p.to_string_lossy().into_owned(), bytes)),
            Err(e) => app.error = Some(format!("Couldn't read {}: {e}", p.display())),
        }
    }
    files
}

/// Imports files dropped on the window at the pointer (or the view's center).
pub fn handle_drops(app: &mut ScrawlApp, ctx: &egui::Context) {
    let (dropped, pointer) = ctx.input(|i| (i.raw.dropped_files.clone(), i.pointer.latest_pos()));
    if dropped.is_empty() {
        return;
    }
    let mut files = vec![];
    for f in dropped {
        let name = f.path().to_string_lossy().into_owned();
        match f.bytes() {
            Ok(bytes) => files.push((name, bytes)),
            Err(e) => app.error = Some(format!("Couldn't read {name}: {e}")),
        }
    }
    let at = match pointer.filter(|p| app.canvas.rect.contains(*p)) {
        Some(p) => app.viewport().to_page(app.canvas.rect, p),
        None => view_center(app),
    };
    import_files(app, files, at);
}
