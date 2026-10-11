//! Versions: each save keeps the file it overwrites (the newest [`KEEP`] per document), and
//! File › Browse Versions… shows them with a preview, to restore one or open it as a copy.
//!
//! On macOS versions go in the system's version store, where they follow the file when it's
//! moved or renamed; elsewhere (and if that fails) in the `Versions` folder, by path.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scrawl_core::{Document, io};

use crate::app::ScrawlApp;
use crate::commands::Request;

/// Versions kept per document.
pub const KEEP: usize = 30;

/// Thumbnails are rendered to fit this box, in pixels.
const THUMB: (f64, f64) = (232.0, 160.0);

/// A stable name for `path`'s folder of versions (FNV-1a, so it doesn't change between runs or
/// builds).
fn key(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn dir(support: &Path, path: &Path) -> PathBuf {
    support.join("Versions").join(key(path))
}

/// Copies the current contents of `path` (about to be overwritten) into its versions, and drops
/// the oldest beyond [`KEEP`].
pub fn keep(support: &Path, path: &Path) {
    if !path.exists() {
        return;
    }
    #[cfg(all(target_os = "macos", not(test)))]
    match crate::macos::add_version(path, KEEP) {
        Ok(()) => return,
        Err(e) => log::warn!("couldn't keep a version of {} in the system's store: {e}", path.display()),
    }
    let d = dir(support, path);
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let result = std::fs::create_dir_all(&d)
        .and_then(|()| std::fs::copy(path, d.join(format!("{millis}.{}", io::FILE_EXTENSION))))
        .and_then(|_| std::fs::write(d.join("path.txt"), path.to_string_lossy().as_bytes()));
    if let Err(e) = result {
        log::warn!("couldn't keep a version of {}: {e}", path.display());
        return;
    }
    for old in list(support, path).into_iter().skip(KEEP) {
        std::fs::remove_file(old.file).ok();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub file: PathBuf,
    pub saved: SystemTime,
}

/// `path`'s versions, newest first.
pub fn list(support: &Path, path: &Path) -> Vec<Version> {
    let mut out: Vec<Version> = std::fs::read_dir(dir(support, path))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.path();
            let millis: u64 = file.file_stem()?.to_str()?.parse().ok()?;
            (file.extension()? == io::FILE_EXTENSION).then(|| Version { file, saved: UNIX_EPOCH + Duration::from_millis(millis) })
        })
        .collect();
    #[cfg(all(target_os = "macos", not(test)))]
    out.extend(crate::macos::versions(path).into_iter().map(|v| Version { file: v.file, saved: v.saved }));
    out.sort_by_key(|v| std::cmp::Reverse(v.saved));
    out
}

/// "Just now", "5 minutes ago", "Yesterday", "12 days ago".
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let s = now.duration_since(t).unwrap_or_default().as_secs();
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..60 => "Just now".into(),
        60..3600 => plural(s / 60, "minute"),
        3600..86400 => plural(s / 3600, "hour"),
        86400..172800 => "Yesterday".into(),
        _ => plural(s / 86400, "day"),
    }
}

/// The version browser's state.
pub struct Browser {
    versions: Vec<Version>,
    selected: Option<usize>,
    /// The selected version, loaded, with its preview.
    loaded: Option<(usize, Result<Document, String>, Option<egui::TextureHandle>)>,
}

/// File › Browse Versions…
pub fn open_browser(app: &mut ScrawlApp) {
    let (Some(support), Some(path)) = (&app.support, &app.file_path) else {
        app.error = Some("Versions are kept each time a document is saved. Save this one first.".into());
        return;
    };
    let versions = list(support, path);
    let selected = (!versions.is_empty()).then_some(0);
    app.versions = Some(Browser { versions, selected, loaded: None });
}

/// Shows the version browser, if open.
pub fn show(app: &mut ScrawlApp, ctx: &egui::Context) {
    let Some(browser) = &mut app.versions else { return };
    // Load and preview the selected version.
    if let Some(i) = browser.selected
        && browser.loaded.as_ref().is_none_or(|(j, ..)| *j != i)
    {
        let doc = io::load(&browser.versions[i].file).map_err(|e| e.to_string());
        let thumb = doc.as_ref().ok().and_then(|d| thumbnail(app, d));
        if let Some(b) = &mut app.versions {
            b.loaded = Some((i, doc, None));
        }
        let texture = thumb.map(|img| ctx.load_texture("version-preview", img, egui::TextureOptions::LINEAR));
        if let Some((_, _, t)) = app.versions.as_mut().and_then(|b| b.loaded.as_mut()) {
            *t = texture;
        }
    }
    let id = app.id("versions");
    let Some(browser) = &mut app.versions else { return };
    let mut open = true;
    let mut action = None;
    let now = SystemTime::now();
    egui::Window::new("Versions").id(id).open(&mut open).collapsible(false).resizable(false).default_width(480.0).show(ctx, |ui| {
        if browser.versions.is_empty() {
            ui.label("No earlier versions yet. Each save keeps the version it replaces.");
            return;
        }
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("versions").max_height(240.0).max_width(200.0).show(ui, |ui| {
                ui.set_min_width(200.0);
                for (i, v) in browser.versions.iter().enumerate() {
                    let label = ago(v.saved, now);
                    if ui.selectable_label(browser.selected == Some(i), label).clicked() {
                        browser.selected = Some(i);
                    }
                }
            });
            ui.vertical(|ui| {
                match &browser.loaded {
                    Some((_, Ok(doc), tex)) => {
                        if let Some(tex) = tex {
                            ui.add(egui::Image::new(tex).max_size(egui::vec2(THUMB.0 as f32, THUMB.1 as f32)));
                        }
                        let layers: usize = doc.pages.iter().map(|p| {
                            let mut n = 0;
                            p.walk(|_| n += 1);
                            n
                        }).sum();
                        ui.weak(format!("{} page{}, {layers} layers", doc.pages.len(), if doc.pages.len() == 1 { "" } else { "s" }));
                        ui.horizontal(|ui| {
                            if ui.button("Restore").on_hover_text("Replace the document with this version (undoable)").clicked() {
                                action = Some(true);
                            }
                            if ui.button("Open Copy").on_hover_text("Open this version in a new window").clicked() {
                                action = Some(false);
                            }
                        });
                    }
                    Some((_, Err(e), _)) => {
                        ui.colored_label(ui.visuals().error_fg_color, format!("Couldn't read this version: {e}"));
                    }
                    None => {}
                }
            });
        });
    });
    let doc = match &browser.loaded {
        Some((_, Ok(doc), _)) => Some(doc.clone()),
        _ => None,
    };
    match (action, doc) {
        (Some(true), Some(doc)) => {
            app.versions = None;
            app.replace_document(doc);
            app.notify("Restored an earlier version");
        }
        (Some(false), Some(doc)) => {
            app.versions = None;
            app.requests.push(Request::OpenDocument(Box::new(doc)));
        }
        _ if !open => app.versions = None,
        _ => {}
    }
}

/// A small rendering of `doc`'s first page.
fn thumbnail(app: &mut ScrawlApp, doc: &Document) -> Option<egui::ColorImage> {
    let page = doc.pages.first()?;
    let area = page.content_bounds()?;
    let scale = (THUMB.0 / area.width().max(1.0)).min(THUMB.1 / area.height().max(1.0));
    let ex = app.exporter().ok()?;
    ex.images.load(&doc.assets);
    let bg = vello::peniko::Color::new([0.93, 0.93, 0.94, 1.0]);
    let img = ex.render_page(page, area.inflate(8.0 / scale, 8.0 / scale), scale, bg).ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_keep_bounded_versions() {
        let root = std::env::temp_dir().join(format!("scrawl-versions-{}", std::process::id()));
        let support = root.join("support");
        let file = root.join("Doc.scrawl");
        std::fs::create_dir_all(&root).unwrap();
        keep(&support, &file);
        assert!(list(&support, &file).is_empty(), "nothing to keep before the first save");
        for i in 0..KEEP + 3 {
            std::fs::write(&file, format!("v{i}")).unwrap();
            keep(&support, &file);
            // Versions are named by millisecond.
            std::thread::sleep(Duration::from_millis(2));
        }
        let versions = list(&support, &file);
        assert_eq!(versions.len(), KEEP);
        assert_eq!(std::fs::read_to_string(&versions[0].file).unwrap(), format!("v{}", KEEP + 2));
        assert!(versions.windows(2).all(|w| w[0].saved >= w[1].saved));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_times() {
        let now = SystemTime::now();
        assert_eq!(ago(now, now), "Just now");
        assert_eq!(ago(now - Duration::from_secs(120), now), "2 minutes ago");
        assert_eq!(ago(now - Duration::from_secs(3600), now), "1 hour ago");
        assert_eq!(ago(now - Duration::from_secs(90000), now), "Yesterday");
        assert_eq!(ago(now - Duration::from_secs(86400 * 5), now), "5 days ago");
    }
}
