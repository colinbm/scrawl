use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::Key;
use scrawl_core::kurbo::Vec2;
use scrawl_core::ops::{self, Align, BooleanOp, Reorder};
use scrawl_core::kurbo::Point;
use scrawl_core::library::{Component, LibraryId};
use scrawl_core::symbols::{self, SymbolId};
use scrawl_core::{Document, Frame, History, Layer, LayerId, LayerKind, Page, PageId, PointMode, io};
use serde_json::json;

use crate::canvas::{CanvasState, Tool};
use crate::commands::{Command, MenuContext, Request};
use crate::gpu::VelloCanvas;
use crate::ui;
use crate::viewport::Viewport;

/// Prefix identifying Scrawl layers on the system clipboard.
const CLIPBOARD_TAG: &str = "scrawl-layers";

pub struct ScrawlApp {
    /// Distinguishes this document's windows from other documents'.
    pub uid: u64,
    pub doc: Document,
    pub history: History,
    pub page: PageId,
    pub selection: Vec<LayerId>,
    pub tool: Tool,
    pub viewports: HashMap<PageId, Viewport>,
    pub canvas: CanvasState,
    pub gpu: Option<VelloCanvas>,
    pub gpu_error: Option<String>,
    /// The wgpu device and egui renderer shared by every window.
    pub render_state: Option<egui_wgpu::RenderState>,
    /// The prototype preview window, when open.
    pub preview: Option<crate::preview::Preview>,
    /// Show every prototyping link on the canvas, not just the selection's.
    pub show_links: bool,
    pub scene: vello::Scene,
    pub text: scrawl_text::TextEngine,
    /// Decoded image assets for the canvas.
    pub images: scrawl_render::ImageCache,
    /// Containers expanded in the layer list.
    pub expanded: HashSet<LayerId>,
    /// Layer or page currently being renamed, with the edit buffer.
    pub renaming: Option<(RenameTarget, String)>,
    pub lock_aspect: bool,
    /// Snap drags to other layers, guides and grids.
    pub snap_to_layers: bool,
    pub show_rulers: bool,
    pub show_grid: bool,
    pub show_layout: bool,
    pub layer_filter: crate::ui::layers::LayerFilter,
    /// What the left sidebar shows.
    pub sidebar: Sidebar,
    /// Offscreen renderer for exports, created on first use.
    exporter: Option<scrawl_render::export::Exporter>,
    /// A short message shown over the canvas, with when it was posted (egui time).
    pub notice: Option<(String, f64)>,
    /// Linked libraries' documents as last read from disk, or why they couldn't be.
    pub libraries: HashMap<LibraryId, Result<Document, String>>,
    /// The gradient whose handles are shown on the canvas.
    pub gradient_edit: Option<crate::canvas::GradientTarget>,
    pub file_path: Option<PathBuf>,
    /// Document as last saved/loaded, to detect unsaved changes.
    saved: Document,
    /// Layers copied most recently, used when pasting from the menu (which can't read the system
    /// clipboard through egui).
    clipboard: Vec<Layer>,
    pub error: Option<String>,
    /// Zoom to fit on the next frame (after opening a file, once the canvas size is known).
    pub fit_pending: bool,
    window_title: String,
    pub dev: crate::dev::DevHooks,
    /// Things for the workspace to do after this window's frame.
    pub requests: Vec<Request>,
    /// What the menus show beyond this window, kept up to date by the workspace.
    pub menu_context: MenuContext,
    /// Input events from native menu commands, delivered at the start of the next frame.
    pub injected: Vec<egui::Event>,
    /// Commands from native menus to run at the start of the next frame.
    pub pending: Vec<Command>,
    /// The document as of the last autosave, to skip unchanged ones.
    pub autosaved: Option<Document>,
    /// When this window's document was last autosaved (egui time).
    pub autosave_time: f64,
    /// The version browser, when open.
    pub versions: Option<crate::versions::Browser>,
    /// Scrawl's own folder (preferences, autosaves, versions), if there is one.
    pub support: Option<PathBuf>,
    /// Names this window's autosave file.
    pub autosave_name: String,
    /// The script console, when open.
    pub console: Option<crate::plugins::Console>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sidebar {
    Layers,
    Components,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Layer(LayerId),
    Page(PageId),
}

impl ScrawlApp {
    /// A window with an empty document. Every window shares the workspace's `render_state`.
    pub fn new(render_state: Option<egui_wgpu::RenderState>) -> Self {
        let doc = Document::default();
        static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            page: doc.pages[0].id,
            history: History::new(&doc),
            saved: doc.clone(),
            doc,
            selection: vec![],
            tool: Tool::Select,
            viewports: HashMap::new(),
            canvas: CanvasState::default(),
            gpu: None,
            gpu_error: None,
            render_state,
            preview: None,
            show_links: false,
            scene: vello::Scene::new(),
            text: scrawl_text::TextEngine::new(),
            images: Default::default(),
            expanded: HashSet::new(),
            renaming: None,
            lock_aspect: false,
            snap_to_layers: true,
            show_rulers: false,
            show_grid: false,
            show_layout: false,
            layer_filter: Default::default(),
            sidebar: Sidebar::Layers,
            libraries: HashMap::new(),
            exporter: None,
            notice: None,
            gradient_edit: None,
            file_path: None,
            clipboard: vec![],
            error: None,
            fit_pending: false,
            window_title: String::new(),
            dev: crate::dev::DevHooks::default(),
            requests: vec![],
            menu_context: MenuContext::default(),
            injected: vec![],
            pending: vec![],
            autosaved: None,
            autosave_time: 0.0,
            versions: None,
            support: None,
            autosave_name: uuid::Uuid::new_v4().to_string(),
            console: None,
        }
    }

    /// The window title: the file name, "Untitled", and whether it has unsaved changes.
    pub fn title(&self) -> String {
        let name = self.file_path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned());
        if self.is_dirty() { format!("{name} — Edited") } else { name }
    }

    /// Treats the document as having unsaved changes: unlike the file it belongs to (if any).
    pub fn mark_unsaved(&mut self) {
        self.saved = match self.file_path.as_ref().and_then(|p| io::load(p).ok()) {
            Some(mut doc) => {
                self.text.fit_document(&mut doc);
                doc
            }
            None => Document::default(),
        };
    }

    /// An id for one of this window's panels or popups, distinct from other windows' (which may
    /// share a viewport when windows are embedded).
    pub fn id(&self, name: &str) -> egui::Id {
        egui::Id::new(name).with(self.uid)
    }

    /// Makes the next frame set the window title, e.g. after moving to another viewport.
    pub fn window_title_changed(&mut self) {
        self.window_title.clear();
    }

    /// An untitled, unedited, empty document: one that opening a file may replace.
    pub fn is_pristine(&self) -> bool {
        self.file_path.is_none() && !self.is_dirty() && !self.history.can_undo() && self.doc.pages.iter().all(|p| p.layers.is_empty())
    }

    /// The render state, creating the Vello renderer on first use. `None` (with `gpu_error` set)
    /// if there's no GPU.
    pub fn ensure_gpu(&mut self) -> Option<egui_wgpu::RenderState> {
        let Some(rs) = self.render_state.clone() else {
            self.gpu_error.get_or_insert_with(|| "wgpu is not available".into());
            return None;
        };
        if self.gpu.is_none() && self.gpu_error.is_none() {
            match VelloCanvas::new(&rs) {
                Ok(c) => self.gpu = Some(c),
                Err(e) => self.gpu_error = Some(e),
            }
        }
        self.gpu.is_some().then_some(rs)
    }

    pub fn page(&self) -> &Page {
        self.doc.page(self.page).unwrap_or(&self.doc.pages[0])
    }

    pub fn page_mut(&mut self) -> &mut Page {
        let id = self.page;
        let i = self.doc.pages.iter().position(|p| p.id == id).unwrap_or(0);
        &mut self.doc.pages[i]
    }

    pub fn viewport(&self) -> Viewport {
        self.viewports.get(&self.page).copied().unwrap_or_default()
    }

    pub fn viewport_mut(&mut self) -> &mut Viewport {
        self.viewports.entry(self.page).or_default()
    }

    pub fn is_dirty(&self) -> bool {
        self.doc != self.saved
    }

    pub fn set_page(&mut self, id: PageId) {
        if self.page != id {
            self.page = id;
            self.selection.clear();
            self.canvas.cancel();
        }
    }

    pub fn set_document(&mut self, mut doc: Document, path: Option<PathBuf>) {
        // Text frames depend on installed fonts, so refit them before anything else sees them.
        self.text.fit_document(&mut doc);
        self.page = doc.pages[0].id;
        self.history = History::new(&doc);
        self.saved = doc.clone();
        self.doc = doc;
        self.file_path = path;
        self.selection.clear();
        self.viewports.clear();
        self.expanded.clear();
        self.renaming = None;
        self.preview = None;
        self.reset_modes();
        self.fit_pending = true;
        self.reload_libraries();
    }

    // ---- File --------------------------------------------------------------------------------

    /// Asks whether to save unsaved changes. Returns false if the user cancelled.
    pub fn confirm_discard(&mut self) -> bool {
        if !self.is_dirty() {
            return true;
        }
        let answer = rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Do you want to save the changes you made to this document?")
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom("Save".into(), "Don't Save".into(), "Cancel".into()))
            .show();
        match answer {
            rfd::MessageDialogResult::Custom(s) if s == "Save" => self.save(),
            rfd::MessageDialogResult::Custom(s) if s == "Don't Save" => true,
            rfd::MessageDialogResult::Yes => self.save(),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    /// File › Open…: asks for a file, which the workspace opens (here if this window is empty).
    pub fn open(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Documents", &[io::FILE_EXTENSION, scrawl_core::sketch::FILE_EXTENSION])
            .add_filter("Scrawl document", &[io::FILE_EXTENSION])
            .add_filter("Sketch document", &[scrawl_core::sketch::FILE_EXTENSION]);
        if let Some(path) = dialog.pick_file() {
            self.requests.push(Request::Open(path));
        }
    }

    /// File › Revert to Saved: reloads the file, as an undoable change.
    pub fn revert_to_saved(&mut self) {
        let Some(path) = self.file_path.clone() else { return };
        match io::load(&path) {
            Ok(mut doc) => {
                self.text.fit_document(&mut doc);
                self.reset_modes();
                self.history.commit(&self.doc);
                self.saved = doc.clone();
                self.doc = doc;
                self.after_history_change();
            }
            Err(e) => self.error = Some(format!("Couldn't open {}: {e}", path.display())),
        }
    }

    /// Replaces the document with `doc` as one undoable step, keeping the file it's saved to.
    pub fn replace_document(&mut self, mut doc: Document) {
        self.text.fit_document(&mut doc);
        self.reset_modes();
        self.history.commit(&self.doc);
        self.doc = doc;
        self.after_history_change();
        self.reload_libraries();
    }

    pub fn open_path(&mut self, path: &Path) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(scrawl_core::sketch::FILE_EXTENSION)) {
            self.import_sketch(path);
            return;
        }
        match io::load(path) {
            Ok(doc) => self.set_document(doc, Some(path.to_path_buf())),
            Err(e) => self.error = Some(format!("Couldn't open {}: {e}", path.display())),
        }
    }

    /// Opens a design file as a new, untitled document (saving asks where to put the .scrawl).
    pub fn import_sketch(&mut self, path: &Path) {
        let result = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| scrawl_core::sketch::import(&b));
        match result {
            Ok(doc) => {
                self.set_document(doc, None);
                // Unsaved, since it isn't a Scrawl file yet.
                self.saved = Document::default();
                self.notify(format!("Imported {}", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())));
            }
            Err(e) => self.error = Some(format!("Couldn't open {}: {e}", path.display())),
        }
    }

    /// File › Save as…: writes the document as a design file.
    pub fn save_as_sketch(&mut self) {
        let stem = self.file_path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned());
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Sketch document", &[scrawl_core::sketch::FILE_EXTENSION])
            .set_file_name(format!("{stem}.{}", scrawl_core::sketch::FILE_EXTENSION))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension(scrawl_core::sketch::FILE_EXTENSION);
        }
        let result = scrawl_core::sketch::export(&self.doc).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()));
        match result {
            Ok(()) => self.notify(format!("Saved {}", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))),
            Err(e) => self.error = Some(format!("Couldn't save {}: {e}", path.display())),
        }
    }

    /// Returns whether the document was saved.
    pub fn save(&mut self) -> bool {
        match self.file_path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let name = self
            .file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| format!("Untitled.{}", io::FILE_EXTENSION), |n| n.to_string_lossy().into_owned());
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Scrawl document", &[io::FILE_EXTENSION])
            .set_file_name(name)
            .save_file()
        else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension(io::FILE_EXTENSION);
        }
        self.save_to(&path)
    }

    fn save_to(&mut self, path: &Path) -> bool {
        self.history.commit(&self.doc);
        // Keep what's being overwritten as a version.
        if let Some(support) = &self.support {
            crate::versions::keep(support, path);
        }
        match io::save(&self.doc, path) {
            Ok(()) => {
                self.saved = self.doc.clone();
                self.file_path = Some(path.to_path_buf());
                self.requests.push(Request::Saved(path.to_path_buf()));
                true
            }
            Err(e) => {
                self.error = Some(format!("Couldn't save {}: {e}", path.display()));
                false
            }
        }
    }

    // ---- Edit --------------------------------------------------------------------------------

    /// Leaves point editing and abandons any pen path, e.g. before the document is replaced.
    fn reset_modes(&mut self) {
        self.canvas.cancel();
        self.canvas.edit = None;
        self.canvas.pen = None;
        self.canvas.text_edit = None;
        self.canvas.linking = None;
        self.text.raw_case = None;
    }

    pub fn undo(&mut self) {
        self.reset_modes();
        if self.history.undo(&mut self.doc) {
            self.after_history_change();
        }
    }

    pub fn redo(&mut self) {
        self.reset_modes();
        if self.history.redo(&mut self.doc) {
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        if self.doc.page(self.page).is_none() {
            self.page = self.doc.pages[0].id;
        }
        self.sanitize_selection();
    }

    pub fn sanitize_selection(&mut self) {
        let page = self.doc.page(self.page).unwrap_or(&self.doc.pages[0]);
        let valid: Vec<_> = self.selection.iter().copied().filter(|&id| page.contains(id)).collect();
        self.selection = valid;
    }

    pub fn delete_selection(&mut self) {
        let sel = std::mem::take(&mut self.selection);
        // Deleting a master detaches its instances first, so they keep their look.
        let masters: Vec<_> = sel.iter().filter_map(|&id| self.page().find(id).and_then(Layer::master_of)).collect();
        for symbol in masters {
            symbols::detach_all(&mut self.doc, symbol);
        }
        ops::delete(self.page_mut(), &sel);
    }

    // ---- Export ------------------------------------------------------------------------------

    /// Writes every export format of each layer (1× PNG for layers without any) into a folder
    /// the user picks.
    pub fn export_layers(&mut self, ids: &[LayerId]) {
        if ids.is_empty() {
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("Export to Folder").pick_folder() else { return };
        let page = self.page;
        let jobs: Vec<(PageId, LayerId)> = ids.iter().map(|&id| (page, id)).collect();
        self.export_to(&dir, &jobs, true);
    }

    /// File › Export All…: every layer with export formats, on every page.
    pub fn export_all(&mut self) {
        let jobs: Vec<(PageId, LayerId)> = self
            .doc
            .pages
            .iter()
            .flat_map(|p| {
                let mut ids = vec![];
                p.walk(|l| {
                    if !l.exports.is_empty() {
                        ids.push((p.id, l.id));
                    }
                });
                ids
            })
            .collect();
        if jobs.is_empty() {
            self.error = Some("Nothing is exportable yet. Add export formats in the inspector, or draw a slice (S).".into());
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("Export All to Folder").pick_folder() else { return };
        self.export_to(&dir, &jobs, false);
    }

    /// Exports `jobs` into `dir`. Layers without formats get 1× PNG when `default_png` is set.
    /// Returns the files written.
    /// The offscreen renderer, started on first use.
    pub fn exporter(&mut self) -> Result<&mut scrawl_render::export::Exporter, String> {
        if self.exporter.is_none() {
            self.exporter = Some(scrawl_render::export::Exporter::new()?);
        }
        Ok(self.exporter.as_mut().expect("created above"))
    }

    pub fn export_to(&mut self, dir: &Path, jobs: &[(PageId, LayerId)], default_png: bool) -> Vec<PathBuf> {
        if let Err(e) = self.exporter() {
            self.error = Some(format!("Couldn't start the exporter: {e}"));
            return vec![];
        }
        let ex = self.exporter.as_mut().expect("started above");
        let (mut written, mut errors) = (vec![], vec![]);
        for &(page_id, id) in jobs {
            let Some(page) = self.doc.page(page_id) else { continue };
            let Some(layer) = page.find(id) else { continue };
            let formats = match (&layer.exports[..], default_png) {
                ([], true) => vec![scrawl_core::exportable::ExportFormat::default()],
                (f, _) => f.to_vec(),
            };
            for f in formats {
                let path = dir.join(f.file_name(&layer.name));
                let result = scrawl_render::output::export_layer(ex, &self.doc, page, id, &f)
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
                match result {
                    Ok(()) => written.push(path),
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        if !errors.is_empty() {
            self.error = Some(format!("Some exports failed:\n{}", errors.join("\n")));
        }
        if !written.is_empty() {
            let msg = match written.len() {
                1 => format!("Exported {}", written[0].file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())),
                n => format!("Exported {n} files"),
            };
            self.notify(msg);
        }
        written
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), f64::NAN));
    }

    // ---- Libraries ---------------------------------------------------------------------------

    /// Re-reads every linked library from disk.
    pub fn reload_libraries(&mut self) {
        self.libraries.clear();
        for lib in self.doc.libraries.clone() {
            let loaded = io::load(Path::new(&lib.path)).map_err(|e| e.to_string());
            self.libraries.insert(lib.id, loaded);
        }
    }

    /// Asks for a library file to link.
    pub fn add_library(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Scrawl document", &[io::FILE_EXTENSION]).pick_file() {
            self.add_library_path(&path);
        }
    }

    pub fn add_library_path(&mut self, path: &Path) {
        if self.file_path.as_deref() == Some(path) {
            self.error = Some("A document can't be a library of itself.".into());
            return;
        }
        let name = path.file_stem().map_or_else(|| "Library".to_owned(), |s| s.to_string_lossy().into_owned());
        self.doc.add_library(name, path.to_string_lossy());
        self.reload_libraries();
    }

    /// The loaded document of library `lib`, if it could be read.
    pub fn library_doc(&self, lib: LibraryId) -> Option<&Document> {
        self.libraries.get(&lib).and_then(|r| r.as_ref().ok())
    }

    /// Copies a component from a library into the document. Returns whether it's now here.
    pub fn import_component(&mut self, lib: LibraryId, c: Component) -> bool {
        let Some(source) = self.libraries.get(&lib).and_then(|r| r.as_ref().ok()) else { return false };
        self.doc.import(lib, source, c)
    }

    /// Brings library `lib`'s copies up to date with its file.
    pub fn update_library(&mut self, lib: LibraryId) {
        if let Some(Ok(source)) = self.libraries.get(&lib) {
            self.doc.update_library(lib, source);
        }
    }

    // ---- Symbols -----------------------------------------------------------------------------

    /// Turns the selection into a symbol, replacing it with an instance.
    pub fn create_symbol(&mut self) {
        let sel = self.selection.clone();
        match symbols::create_symbol(&mut self.doc, self.page, &sel) {
            Some(created) => self.selection = vec![created.instance.unwrap_or(created.master)],
            None => self.error = Some("Select layers or a single artboard to create a symbol.".into()),
        }
    }

    /// Replaces selected instances with groups of their contents.
    pub fn detach_selection(&mut self) {
        let sel = self.selection.clone();
        let page = self.page_mut();
        let out: Vec<LayerId> = sel.iter().map(|&id| symbols::detach(page, id).unwrap_or(id)).collect();
        self.selection = out;
    }

    /// Shows the master of `symbol`, selected, on its page.
    pub fn edit_master(&mut self, symbol: SymbolId) {
        let Some((page, id)) = self.doc.find_master(symbol) else {
            let library = self.doc.foreign_symbols.iter().find(|f| f.item.master_of() == Some(symbol)).and_then(|f| self.doc.library(f.library));
            self.error = Some(match library {
                Some(lib) => format!("This symbol comes from the library “{}”. Open {} to edit it.", lib.name, lib.path),
                None => "This symbol's master is missing.".into(),
            });
            return;
        };
        self.set_page(page);
        self.selection = vec![id];
        self.tool = Tool::Select;
        self.zoom_to_selection(1.0);
    }

    /// Places an instance of `symbol` at the center of the view, inside the artboard there.
    pub fn insert_symbol(&mut self, symbol: SymbolId) {
        let Some(master) = symbols::Symbols::new(&self.doc).master(symbol).cloned() else { return };
        let center = self.viewport().to_page(self.canvas.rect, self.canvas.rect.center());
        let origin = Point::new(
            (center.x - master.frame.width / 2.0).round(),
            (center.y - master.frame.height / 2.0).round(),
        );
        let Some(instance) = Layer::instance_of(&master, origin) else { return };
        let id = ops::insert_drawn(self.page_mut(), instance);
        self.selection = vec![id];
        self.tool = Tool::Select;
    }

    pub fn duplicate_selection(&mut self) {
        let sel = self.selection.clone();
        self.selection = ops::duplicate(self.page_mut(), &sel);
    }

    pub fn select_all(&mut self) {
        let page = self.page();
        // Within the current container if something is selected, otherwise the whole page.
        let parent = self.selection.first().and_then(|&id| page.parent(id)).flatten();
        let ids = page.siblings(parent).map(|s| s.iter().filter(|l| l.visible && !l.locked).map(|l| l.id).collect());
        self.selection = ids.unwrap_or_default();
    }

    pub fn group_selection(&mut self) {
        let sel = self.selection.clone();
        if let Some(g) = ops::group(self.page_mut(), &sel) {
            self.selection = vec![g];
        }
    }

    pub fn ungroup_selection(&mut self) {
        let sel = self.selection.clone();
        let mut released = vec![];
        for id in sel {
            let out = ops::ungroup(self.page_mut(), id);
            if out.is_empty() {
                released.push(id);
            } else {
                released.extend(out);
            }
        }
        self.selection = released;
    }

    pub fn reorder(&mut self, how: Reorder) {
        let sel = self.selection.clone();
        ops::reorder(self.page_mut(), &sel, how);
    }

    pub fn boolean(&mut self, op: BooleanOp) {
        let sel = self.selection.clone();
        match ops::boolean(self.page_mut(), &sel, op) {
            Ok(id) => self.selection = vec![id],
            Err(e) => self.error = Some(format!("Couldn't {}: {e}", op.label().to_lowercase())),
        }
    }

    pub fn use_as_mask(&mut self) {
        let sel = self.selection.clone();
        self.selection = ops::use_as_mask(self.page_mut(), &sel);
    }

    pub fn convert_to_paths(&mut self) {
        let sel = self.selection.clone();
        ops::convert_to_paths(self.page_mut(), &sel);
    }

    pub fn outline_strokes(&mut self) {
        let sel = self.selection.clone();
        // Text becomes its glyph outlines; other shapes have their borders outlined.
        for &id in &sel {
            let Some(layer) = self.page().find(id).filter(|l| l.is_text()).cloned() else { continue };
            let Some(outline) = self.text.outlines(&layer) else { continue };
            let mut shape = scrawl_core::PathShape::from_bez_path(&outline);
            shape.fill_rule = scrawl_core::FillRule::NonZero;
            if let Some(l) = self.page_mut().find_mut(id) {
                l.set_local_path(&shape);
            }
        }
        if let Err(e) = ops::outline_strokes(self.page_mut(), &sel) {
            self.error = Some(format!("Couldn't convert to outlines: {e}"));
        }
    }

    pub fn reverse_paths(&mut self) {
        let sel = self.selection.clone();
        ops::reverse_paths(self.page_mut(), &sel);
    }

    /// Starts editing a layer: text in place, or the points of a shape (converting it to a path
    /// first if needed).
    pub fn enter_vector_edit(&mut self, id: LayerId) {
        // Double-clicking an instance opens its master.
        if let Some(LayerKind::Symbol(inst)) = self.page().find(id).map(|l| &l.kind) {
            let symbol = inst.symbol_id;
            self.edit_master(symbol);
            return;
        }
        if self.page().find(id).is_some_and(Layer::is_text) {
            crate::text_edit::begin(self, id, true);
            return;
        }
        let page = self.page_mut();
        let Some(layer) = page.find_mut(id) else { return };
        if layer.locked || !layer.convert_to_path() {
            return;
        }
        page.refit_ancestor_groups(id);
        self.selection = vec![id];
        self.tool = Tool::Select;
        self.canvas.cancel();
        self.canvas.edit = Some(crate::vector::VectorEdit::new(id));
    }

    pub fn exit_vector_edit(&mut self) {
        self.canvas.edit = None;
    }

    pub fn align(&mut self, how: Align) {
        let sel = self.selection.clone();
        ops::align(self.page_mut(), &sel, how);
    }

    pub fn distribute(&mut self, axis: scrawl_core::snap::Axis) {
        let sel = self.selection.clone();
        scrawl_core::layout::distribute(self.page_mut(), &sel, axis);
    }

    pub fn tidy(&mut self) {
        let sel = self.selection.clone();
        scrawl_core::layout::tidy(self.page_mut(), &sel, None);
    }

    /// Groups the selection into a stack.
    pub fn make_stack(&mut self) {
        let sel = self.selection.clone();
        if let Some(g) = scrawl_core::layout::make_stack(self.page_mut(), &sel) {
            self.selection = vec![g];
        }
    }

    pub fn nudge(&mut self, d: Vec2) {
        let sel = self.selection.clone();
        ops::move_by(self.page_mut(), &sel, d);
    }

    /// Copies the selection (in page space) to the internal and system clipboards.
    pub fn copy(&mut self, ctx: &egui::Context) {
        let page = self.page();
        let layers: Vec<Layer> = ops::normalize_selection(page, &self.selection)
            .into_iter()
            .filter_map(|id| ops::page_space_copy(page, id))
            .collect();
        if layers.is_empty() {
            return;
        }
        ctx.copy_text(json!({ CLIPBOARD_TAG: layers }).to_string());
        self.clipboard = layers;
    }

    pub fn cut(&mut self, ctx: &egui::Context) {
        self.copy(ctx);
        self.delete_selection();
    }

    /// Puts the selection on the clipboard as an SVG document.
    pub fn copy_as_svg(&mut self, ctx: &egui::Context) {
        let page = self.page();
        let layers: Vec<Layer> = ops::normalize_selection(page, &self.selection)
            .into_iter()
            .filter_map(|id| ops::page_space_copy(page, id))
            .collect();
        let pad = layers.iter().map(scrawl_core::exportable::paint_outset).fold(0.0, f64::max);
        let Some(area) = layers.iter().filter_map(|l| l.bounds_in(scrawl_core::kurbo::Affine::IDENTITY)).reduce(|a, b| a.union(b)) else {
            return;
        };
        let mut temp = Page::new("Copy");
        temp.layers = layers;
        let svg = scrawl_render::svg::page_svg(&temp, area.inflate(pad, pad), 1.0, &mut self.text, &self.doc.assets);
        ctx.copy_text(svg);
        self.notify("Copied SVG");
    }

    /// Puts CSS for the selected layers on the clipboard.
    pub fn copy_as_css(&mut self, ctx: &egui::Context) {
        let page = self.page();
        let css: Vec<String> = ops::normalize_selection(page, &self.selection)
            .into_iter()
            .filter_map(|id| page.find(id))
            .map(scrawl_core::css::layer_css)
            .collect();
        if css.is_empty() {
            return;
        }
        ctx.copy_text(css.join("\n"));
        self.notify("Copied CSS");
    }

    /// Pastes clipboard text if it holds Scrawl layers or an SVG, otherwise the internal clipboard.
    pub fn paste(&mut self, text: Option<&str>) {
        if let Some(svg) = text.filter(|t| crate::import::is_svg(t.as_bytes()))
            && !svg.contains(CLIPBOARD_TAG)
        {
            let at = crate::import::view_center(self);
            match crate::import::import_svg(self, "SVG", svg.as_bytes(), at) {
                Ok(id) => self.selection = vec![id],
                Err(e) => self.error = Some(format!("Couldn't paste the SVG: {e}")),
            }
            return;
        }
        let from_text = text.and_then(|t| {
            let mut v: serde_json::Value = serde_json::from_str(t).ok()?;
            serde_json::from_value::<Vec<Layer>>(v.get_mut(CLIPBOARD_TAG)?.take()).ok()
        });
        let layers = from_text.unwrap_or_else(|| self.clipboard.clone());
        if layers.is_empty() {
            return;
        }
        let page = self.page_mut();
        let ids: Vec<LayerId> = layers.iter().map(|l| ops::insert_root(page, l.duplicate())).collect();
        ops::reparent_to_artboards(page, &ids);
        self.selection = ids;
    }

    pub fn add_page(&mut self) {
        let page = Page::new(self.doc.next_page_name());
        let id = page.id;
        let index = self.doc.pages.iter().position(|p| p.id == self.page).map_or(self.doc.pages.len(), |i| i + 1);
        self.doc.pages.insert(index, page);
        self.set_page(id);
    }

    pub fn delete_page(&mut self, id: PageId) {
        if self.doc.pages.len() <= 1 {
            return;
        }
        let Some(i) = self.doc.pages.iter().position(|p| p.id == id) else { return };
        self.doc.pages.remove(i);
        if self.page == id {
            let next = self.doc.pages[i.min(self.doc.pages.len() - 1)].id;
            self.set_page(next);
        }
    }

    pub fn insert_artboard_preset(&mut self, name: &str, width: f64, height: f64) {
        let page = self.page();
        let x = page.content_bounds().map_or(0.0, |b| (b.x1 + 100.0).round());
        let y = page.content_bounds().map_or(0.0, |b| b.y0.round());
        let id = ops::insert_drawn(self.page_mut(), Layer::artboard(name, Frame::new(x, y, width, height)));
        self.selection = vec![id];
        self.tool = Tool::Select;
        self.zoom_to_selection(1.0);
    }

    // ---- View --------------------------------------------------------------------------------

    pub fn zoom_to_fit(&mut self) {
        let canvas = self.canvas.rect;
        match self.page().content_bounds() {
            Some(b) => self.viewport_mut().fit(canvas, b, 1.0),
            None => *self.viewport_mut() = Viewport::default(),
        }
    }

    pub fn zoom_to_selection(&mut self, max_zoom: f64) {
        let canvas = self.canvas.rect;
        if let Some(b) = self.page().selection_bounds(&self.selection) {
            self.viewport_mut().fit(canvas, b, max_zoom);
        }
    }

    pub fn actual_size(&mut self) {
        let canvas = self.canvas.rect;
        self.viewport_mut().set_zoom_about(canvas, canvas.center(), 1.0);
    }

    pub fn step_zoom(&mut self, zoom_in: bool) {
        let canvas = self.canvas.rect;
        self.viewport_mut().step_zoom(canvas, zoom_in);
    }

    // ---- Frame loop --------------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // File and history shortcuts work everywhere except in text fields (which have their own
        // undo); editing shortcuts only when the canvas has keyboard focus.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |sc: egui::KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&sc));
        // While editing text, keys go to the text (undo ends editing first).
        if self.canvas.text_edit.is_some() {
            if pressed(Command::Undo.shortcut().expect("undo has a shortcut")) {
                crate::commands::execute(self, Command::Undo, ctx);
            } else if pressed(Command::Save.shortcut().expect("save has a shortcut")) {
                self.save();
            } else {
                crate::text_edit::handle_keys(self, ctx);
            }
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            match event {
                egui::Event::Copy => self.copy(ctx),
                egui::Event::Cut => self.cut(ctx),
                egui::Event::Paste(text) => self.paste(Some(text)),
                _ => {}
            }
        }
        for (command, shortcut) in crate::commands::keyboard_commands() {
            if pressed(shortcut) {
                crate::commands::execute(self, command, ctx);
            }
        }

        // Single-key shortcuts: no command modifier.
        let (mods, keys) = ctx.input(|i| {
            let keys: Vec<Key> =
                i.events.iter().filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, .. } => Some(*key),
                    _ => None,
                }).collect();
            (i.modifiers, keys)
        });
        if mods.command {
            return;
        }
        // ⌃R, ⌃G, ⌃L and ⌃P are commands (above); no single-key shortcuts with ⌃.
        if mods.ctrl {
            return;
        }
        let step = if mods.shift { 10.0 } else { 1.0 };
        if self.canvas.edit.is_some() {
            for key in keys {
                match key {
                    Key::Escape | Key::Enter => self.exit_vector_edit(),
                    Key::Backspace | Key::Delete => crate::vector::delete_points(self),
                    Key::ArrowLeft => crate::vector::nudge_points(self, Vec2::new(-step, 0.0)),
                    Key::ArrowRight => crate::vector::nudge_points(self, Vec2::new(step, 0.0)),
                    Key::ArrowUp => crate::vector::nudge_points(self, Vec2::new(0.0, -step)),
                    Key::ArrowDown => crate::vector::nudge_points(self, Vec2::new(0.0, step)),
                    // Point-type keys.
                    Key::Num1 => crate::vector::set_point_mode(self, PointMode::Straight),
                    Key::Num2 => crate::vector::set_point_mode(self, PointMode::Mirrored),
                    Key::Num3 => crate::vector::set_point_mode(self, PointMode::Disconnected),
                    Key::Num4 => crate::vector::set_point_mode(self, PointMode::Asymmetric),
                    _ => {}
                }
            }
            return;
        }
        if self.canvas.linking.is_some() {
            if keys.contains(&Key::Escape) {
                self.canvas.linking = None;
            }
            return;
        }
        if crate::vector::pen_active(self) {
            if keys.iter().any(|k| matches!(k, Key::Escape | Key::Enter)) {
                crate::vector::finish_pen(self, false);
            }
            return;
        }
        for key in keys {
            match key {
                Key::V => self.tool = Tool::Select,
                Key::A => self.tool = Tool::Artboard,
                Key::R => self.tool = Tool::Rectangle,
                Key::O => self.tool = Tool::Oval,
                Key::L => self.tool = Tool::Line,
                Key::P => self.tool = Tool::Pen,
                Key::T => self.tool = Tool::Text,
                Key::S => self.tool = Tool::Slice,
                Key::H => self.tool = Tool::Hotspot,
                Key::W => crate::prototyping::start_linking(self),
                Key::Enter => {
                    if let [id] = self.selection[..] {
                        self.enter_vector_edit(id);
                    }
                }
                Key::Escape => {
                    if self.gradient_edit.take().is_some() {
                        // Escape first dismisses the gradient handles.
                    } else if self.tool != Tool::Select {
                        self.tool = Tool::Select;
                    } else if !self.canvas.cancel() {
                        // Escape walks up to the parent group.
                        let page = self.page();
                        let parents: Vec<_> = self.selection.iter().filter_map(|&id| page.parent(id).flatten()).filter(|&p| page.find(p).is_some_and(Layer::is_group)).collect();
                        self.selection = if parents.is_empty() { vec![] } else { ops::normalize_selection(page, &parents) };
                    }
                }
                Key::Backspace | Key::Delete => self.delete_selection(),
                Key::ArrowLeft => self.nudge(Vec2::new(-step, 0.0)),
                Key::ArrowRight => self.nudge(Vec2::new(step, 0.0)),
                Key::ArrowUp => self.nudge(Vec2::new(0.0, -step)),
                Key::ArrowDown => self.nudge(Vec2::new(0.0, step)),
                _ => {}
            }
        }
    }

    fn end_frame(&mut self, ctx: &egui::Context) {
        self.sanitize_selection();
        // Switching tools ends a pen path; changing the selection ends point editing.
        if crate::vector::pen_active(self) && self.tool != Tool::Pen {
            let tool = self.tool;
            crate::vector::finish_pen(self, false);
            self.tool = tool;
        }
        if let Some(edit) = &self.canvas.edit
            && (self.selection != [edit.id] || self.tool != Tool::Select)
        {
            self.canvas.edit = None;
        }
        // Picking a link destination ends with another tool, or if its layers went away.
        if let Some(sources) = &self.canvas.linking
            && (self.tool != Tool::Select || sources.iter().any(|&id| !self.page().contains(id)))
        {
            self.canvas.linking = None;
        }
        if let Some(target) = self.gradient_edit
            && (self.selection != [target.layer] || self.page().find(target.layer).and_then(|l| target.gradient(l)).is_none())
        {
            self.gradient_edit = None;
        }
        // Commit an undo step once an interaction is finished: no pointer button held and no text
        // field being edited. A whole drag or a run of typing becomes a single step.
        // Keep text frames fitted to their content.
        let page_id = self.page;
        let dragging = self.canvas.dragging();
        // Instances follow their masters and overrides.
        let text = &mut self.text;
        if symbols::sync_instances(&mut self.doc, &mut |layers| {
            text.fit_all(layers);
        }) {
            ctx.request_repaint();
        }
        if let Some(page) = self.doc.page_mut(page_id) {
            let fitted = self.text.fit_all(&mut page.layers);
            // Stacks re-flow after their contents change, except while a child is dragged.
            let flowed = scrawl_core::layout::apply_stacks(page, &dragging);
            // This frame was drawn before these changes; draw another.
            if fitted || flowed {
                ctx.request_repaint();
            }
        }
        if let Some(editing) = &self.canvas.text_edit
            && self.selection != [editing.id]
        {
            crate::text_edit::end(self);
        }
        // A whole text-editing session is one undo step.
        let interacting = ctx.input(|i| i.pointer.any_down())
            || ctx.egui_wants_keyboard_input()
            || self.canvas.text_edit.is_some();
        if !interacting {
            self.history.commit(&self.doc);
        }

        let title = self.title();
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

impl ScrawlApp {
    /// Shows the latest notice at the bottom of the canvas for a few seconds.
    fn show_notice(&mut self, ctx: &egui::Context) {
        let id = self.id("notice");
        let Some((msg, posted)) = &mut self.notice else { return };
        let now = ctx.input(|i| i.time);
        if posted.is_nan() {
            *posted = now;
        }
        if now - *posted > 3.0 {
            self.notice = None;
            return;
        }
        let pos = egui::pos2(self.canvas.rect.center().x, self.canvas.rect.bottom() - 24.0);
        egui::Area::new(id).fixed_pos(pos).pivot(egui::Align2::CENTER_BOTTOM).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(msg.as_str()));
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl ScrawlApp {
    /// Draws this document's window into `ui` (the root viewport, or its own).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Native menu commands arrive between frames: as key presses (so text fields get theirs)
        // or to run directly.
        if !self.injected.is_empty() {
            let events = std::mem::take(&mut self.injected);
            ctx.input_mut(|i| i.events.extend(events));
        }
        for command in std::mem::take(&mut self.pending) {
            crate::commands::execute(self, command, &ctx);
        }
        self.handle_shortcuts(&ctx);
        crate::import::handle_drops(self, &ctx);

        if !self.menu_context.native {
            egui::Panel::top(self.id("menu_bar")).show(ui, |ui| ui::toolbar::menu_bar(self, ui));
        }
        egui::Panel::top(self.id("toolbar")).show(ui, |ui| ui::toolbar::toolbar(self, ui));
        egui::Panel::left(self.id("layers")).resizable(true).default_size(240.0).min_size(180.0).show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.sidebar, Sidebar::Layers, "Layers");
                ui.selectable_value(&mut self.sidebar, Sidebar::Components, "Components");
            });
            ui.separator();
            match self.sidebar {
                Sidebar::Layers => ui::layers::show(self, ui),
                Sidebar::Components => ui::components::show(self, ui),
            }
        });
        egui::Panel::right(self.id("inspector")).resizable(false).exact_size(264.0).show(ui, |ui| {
            ui::inspector::show(self, ui);
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            crate::canvas::show(self, ui);
        });

        if let Some(msg) = self.error.clone() {
            egui::Modal::new(self.id("error")).show(&ctx, |ui| {
                ui.set_max_width(360.0);
                ui.heading("Something went wrong");
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }

        self.show_notice(&ctx);
        crate::preview::show(self, &ctx);
        crate::versions::show(self, &ctx);
        crate::plugins::show_console(self, &ctx);
        self.end_frame(&ctx);
    }
}
