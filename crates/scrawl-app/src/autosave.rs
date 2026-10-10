//! Crash recovery: every few seconds, documents with unsaved changes are written to the
//! `Autosave` folder. Saving, closing or quitting normally removes them; whatever is left at
//! startup (from a crash) reopens with its changes.

use std::path::{Path, PathBuf};

use scrawl_core::{Document, io};
use serde::{Deserialize, Serialize};

use crate::app::ScrawlApp;

/// Seconds between autosaves of a changed document.
pub const INTERVAL: f64 = 10.0;

/// Written next to each autosaved document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// The file the document belongs to, if it has been saved before.
    pub path: Option<PathBuf>,
    /// The process that wrote it, so a second running copy of the app doesn't take it over.
    pub pid: u32,
}

pub fn dir(support: &Path) -> PathBuf {
    support.join("Autosave")
}

fn files(support: &Path, name: &str) -> (PathBuf, PathBuf) {
    let d = dir(support);
    (d.join(format!("{name}.{}", io::FILE_EXTENSION)), d.join(format!("{name}.json")))
}

/// Writes `app`'s document if it changed since its last autosave, on a background thread.
/// Removes the autosave once there's nothing unsaved.
pub fn tick(app: &mut ScrawlApp, support: &Path, now: f64) {
    if now - app.autosave_time < INTERVAL {
        return;
    }
    app.autosave_time = now;
    if !app.is_dirty() {
        if app.autosaved.take().is_some() {
            remove(support, &app.autosave_name);
        }
        return;
    }
    if app.autosaved.as_ref() == Some(&app.doc) {
        return;
    }
    app.autosaved = Some(app.doc.clone());
    let doc = app.doc.clone();
    let meta = Meta { path: app.file_path.clone(), pid: std::process::id() };
    let (file, meta_file) = files(support, &app.autosave_name);
    std::thread::spawn(move || {
        if let Err(e) = write(&doc, &meta, &file, &meta_file) {
            log::warn!("autosave failed: {e}");
        }
    });
}

fn write(doc: &Document, meta: &Meta, file: &Path, meta_file: &Path) -> Result<(), String> {
    std::fs::create_dir_all(file.parent().ok_or("no folder")?).map_err(|e| e.to_string())?;
    // Write beside and rename, so a crash mid-write leaves the previous autosave intact.
    let tmp = file.with_extension("tmp");
    io::save(doc, &tmp).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    std::fs::write(meta_file, serde_json::to_vec(meta).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Deletes the autosave named `name`.
pub fn remove(support: &Path, name: &str) {
    let (file, meta) = files(support, name);
    std::fs::remove_file(file).ok();
    std::fs::remove_file(meta).ok();
}

/// A document left behind by a run that didn't quit normally.
pub struct Recovered {
    pub name: String,
    pub doc: Document,
    pub meta: Meta,
}

/// Autosaves left by processes that are no longer running.
pub fn recover(support: &Path) -> Vec<Recovered> {
    let Ok(entries) = std::fs::read_dir(dir(support)) else { return vec![] };
    let mut out = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        let Some(meta) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Meta>(&b).ok()) else { continue };
        if meta.pid != std::process::id() && process_alive(meta.pid) {
            continue;
        }
        match io::load(&files(support, &name).0) {
            Ok(doc) => out.push(Recovered { name, doc, meta }),
            Err(e) => {
                log::warn!("couldn't recover {name}: {e}");
                remove(support, &name);
            }
        }
    }
    out
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // Signal 0 checks for the process without disturbing it.
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autosaves_survive_until_removed() {
        let support = std::env::temp_dir().join(format!("scrawl-autosave-{}", std::process::id()));
        let doc = scrawl_core::sample::sample_document();
        // No process has this pid, so its autosave is up for recovery.
        let meta = Meta { path: Some("/tmp/Design.scrawl".into()), pid: u32::MAX };
        let (file, meta_file) = files(&support, "abc");
        write(&doc, &meta, &file, &meta_file).unwrap();
        let found = recover(&support);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meta, meta);
        assert_eq!(found[0].doc.pages.len(), doc.pages.len());
        remove(&support, "abc");
        assert!(recover(&support).is_empty());
        std::fs::remove_dir_all(&support).ok();
    }

    #[test]
    fn running_processes_keep_their_autosaves() {
        let support = std::env::temp_dir().join(format!("scrawl-autosave-live-{}", std::process::id()));
        let meta = Meta { path: None, pid: std::process::id() };
        let (file, meta_file) = files(&support, "mine");
        write(&Document::default(), &meta, &file, &meta_file).unwrap();
        // Our own autosaves count as recoverable (we're starting up); another live process's don't.
        assert_eq!(recover(&support).len(), 1);
        if cfg!(unix) {
            let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
            let other = Meta { path: None, pid: child.id() };
            write(&Document::default(), &other, &file, &meta_file).unwrap();
            assert!(recover(&support).is_empty());
            child.kill().ok();
            child.wait().ok();
        }
        std::fs::remove_dir_all(&support).ok();
    }
}
