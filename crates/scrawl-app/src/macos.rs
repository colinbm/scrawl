//! macOS document integration: files opened from Finder or the Dock, the title bar's document
//! icon and edited dot, the system's recent documents, and versions kept in the system's version
//! store (where they follow the file when it's moved or renamed).

use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSApplicationWillFinishLaunchingNotification, NSDocumentController};
use objc2_foundation::{NSArray, NSFileVersion, NSFileVersionAddingOptions, NSNotification, NSNotificationCenter, NSString, NSURL};

/// Files macOS asked us to open that the workspace hasn't picked up yet.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// Woken when files arrive, so the workspace opens them straight away.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Arranges for files opened from Finder or dropped on the Dock icon to reach [`take_opened`].
/// Call before the event loop starts: files that launch the app arrive before it has finished
/// launching.
pub fn install_open_handler() {
    // winit sets its own application delegate, without a handler for opening files; this adds
    // one to its class once it's in place, just before launching finishes.
    let block = block2::RcBlock::new(|_: NonNull<NSNotification>| {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else { return };
        let object: &AnyObject = delegate.as_ref();
        let class: &AnyClass = object.class();
        type Imp = unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>);
        let imp: Imp = open_urls;
        // SAFETY: the types string matches `open_urls` (void; self, _cmd, two objects), and the
        // class is a live Objective-C class.
        unsafe {
            objc2::ffi::class_addMethod(
                (class as *const AnyClass).cast_mut(),
                sel!(application:openURLs:),
                std::mem::transmute::<Imp, objc2::runtime::Imp>(imp),
                c"v@:@@".as_ptr(),
            );
        }
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the block only touches the application on the main thread, where the notification
    // is posted.
    let observer = unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSApplicationWillFinishLaunchingNotification), None, None, &block) };
    // Observed for the life of the app.
    std::mem::forget(observer);
}

unsafe extern "C-unwind" fn open_urls(_this: &AnyObject, _cmd: Sel, _app: &AnyObject, urls: &NSArray<NSURL>) {
    let paths: Vec<PathBuf> = urls.iter().filter_map(|u| u.to_file_path()).collect();
    log::info!("asked to open {paths:?}");
    if let Ok(mut pending) = PENDING.lock() {
        pending.extend(paths);
    }
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// Lets file openings wake the app.
pub fn set_context(ctx: &egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}

/// Files macOS has asked to open since the last call.
pub fn take_opened() -> Vec<PathBuf> {
    PENDING.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default()
}

/// Adds `path` to the system's recent documents (the Dock menu, Apple menu › Recent Items).
pub fn note_recent(path: &Path) {
    let (Some(mtm), Some(url)) = (MainThreadMarker::new(), NSURL::from_file_path(path)) else { return };
    NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
}

/// What a window's title bar shows about its document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleBar {
    /// The window title it's found by.
    pub title: String,
    pub file: Option<PathBuf>,
    pub edited: bool,
}

/// Shows `bar`'s file as the title's document icon (⌘-click for its folders) and its edited
/// state as the dot in the close button, in the window titled `bar.title`. Returns false if no
/// single window has that title yet (the title may not have reached the window).
pub fn set_title_bar(bar: &TitleBar) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let title = NSString::from_str(&bar.title);
    let windows = NSApplication::sharedApplication(mtm).windows();
    let mut matching = windows.iter().filter(|w| w.title().isEqualToString(&title));
    let (Some(window), None) = (matching.next(), matching.next()) else { return false };
    let url = bar.file.as_deref().and_then(NSURL::from_file_path);
    window.setRepresentedURL(url.as_deref());
    window.setDocumentEdited(bar.edited);
    true
}

/// A version of a document in the system's version store.
pub struct StoredVersion {
    /// A readable copy of the version.
    pub file: PathBuf,
    pub saved: SystemTime,
}

/// Puts the current contents of `path` in the system's version store, keeping the newest `keep`.
pub fn add_version(path: &Path, keep: usize) -> Result<(), String> {
    let url = NSURL::from_file_path(path).ok_or("not a file path")?;
    NSFileVersion::addVersionOfItemAtURL_withContentsOfURL_options_error(&url, &url, NSFileVersionAddingOptions::empty())
        .map_err(|e| e.localizedDescription().to_string())?;
    let mut all = stored(&url);
    all.sort_by_key(|(saved, _)| std::cmp::Reverse(*saved));
    for (_, old) in all.into_iter().skip(keep) {
        let _ = old.removeAndReturnError();
    }
    Ok(())
}

/// `path`'s versions in the system's version store, in no particular order.
pub fn versions(path: &Path) -> Vec<StoredVersion> {
    let Some(url) = NSURL::from_file_path(path) else { return vec![] };
    stored(&url).into_iter().filter_map(|(saved, v)| Some(StoredVersion { file: v.URL().to_file_path()?, saved })).collect()
}

fn stored(url: &NSURL) -> Vec<(SystemTime, objc2::rc::Retained<NSFileVersion>)> {
    let Some(all) = NSFileVersion::otherVersionsOfItemAtURL(url) else { return vec![] };
    all.iter()
        .map(|v| {
            let secs = v.modificationDate().map_or(0.0, |d| d.timeIntervalSince1970());
            (UNIX_EPOCH + Duration::from_secs_f64(secs.max(0.0)), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_live_in_the_system_store_and_follow_renames() {
        let dir = std::env::temp_dir().join(format!("scrawl-system-versions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("A.scrawl"), dir.join("B.scrawl"));
        std::fs::write(&a, "first").unwrap();
        if let Err(e) = add_version(&a, 2) {
            // Not every volume has a version store.
            eprintln!("skipping: {e}");
            return;
        }
        std::fs::write(&a, "second").unwrap();
        add_version(&a, 2).unwrap();
        std::fs::write(&a, "third").unwrap();
        add_version(&a, 2).unwrap();
        std::fs::rename(&a, &b).unwrap();
        let stored = versions(&b);
        assert!(stored.iter().all(|v| v.saved > UNIX_EPOCH));
        let mut texts: Vec<String> = stored.iter().map(|v| std::fs::read_to_string(&v.file).unwrap()).collect();
        texts.sort();
        assert_eq!(texts, ["second", "third"], "the oldest beyond two was dropped; the rest followed the rename");
        let url = NSURL::from_file_path(&b).unwrap();
        let _ = NSFileVersion::removeOtherVersionsOfItemAtURL_error(&url);
        std::fs::remove_dir_all(&dir).ok();
    }
}
