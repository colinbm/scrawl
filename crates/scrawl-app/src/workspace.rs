//! The application: every open document window, and what they share (recent files, the native
//! menu bar, autosave, plugins).
//!
//! The first window lives in eframe's root viewport; the others in viewports of their own. When
//! the root window closes while others are open, the next one moves into the root viewport, so
//! the app only quits with its last window.

use std::path::{Path, PathBuf};

use crate::app::ScrawlApp;
use crate::commands::{Command, MenuContext, Request};
use crate::prefs::Prefs;

pub struct Workspace {
    /// `windows[0]` is shown in the root viewport.
    pub windows: Vec<ScrawlApp>,
    render_state: Option<egui_wgpu::RenderState>,
    /// Scrawl's own folder; `None` keeps nothing between runs (as in tests).
    support: Option<PathBuf>,
    pub prefs: Prefs,
    /// The window menu commands go to.
    focused: u64,
    /// Each window's focus last frame, to notice when one gains focus.
    was_focused: Vec<(u64, bool)>,
    /// Quitting: every window has agreed to close.
    quitting: bool,
    pub plugins: crate::plugins::Plugins,
    /// Stands in for dialogs and the network in plugins; `None` uses the real ones.
    pub plugin_host: Option<std::rc::Rc<dyn crate::plugins::Host>>,
    /// Each window's selection when plugins last saw it, for "selection" handlers.
    seen_selection: Vec<(u64, Vec<scrawl_core::LayerId>)>,
    /// What each window's title bar last showed of its document.
    #[cfg(target_os = "macos")]
    title_bars: Vec<(u64, crate::macos::TitleBar)>,
    #[cfg(target_os = "macos")]
    menu: Option<crate::native_menu::NativeMenu>,
}

impl Workspace {
    /// Opens `paths` (or an empty document), recovering any documents left unsaved by a crash.
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>, support: Option<PathBuf>) -> Self {
        crate::style::apply(&cc.egui_ctx);
        let prefs = support.as_deref().map(Prefs::load).unwrap_or_default();
        let plugins = crate::plugins::Plugins::load(support.as_deref());
        let mut ws = Self {
            windows: vec![],
            render_state: cc.wgpu_render_state.clone(),
            support,
            prefs,
            focused: 0,
            was_focused: vec![],
            quitting: false,
            plugins,
            plugin_host: None,
            seen_selection: vec![],
            #[cfg(target_os = "macos")]
            title_bars: vec![],
            #[cfg(target_os = "macos")]
            menu: None,
        };
        ws.recover();
        #[cfg(target_os = "macos")]
        crate::macos::set_context(&cc.egui_ctx);
        for path in paths {
            ws.open(&path, None);
        }
        if ws.windows.is_empty() {
            ws.add_window(ScrawlApp::new(ws.render_state.clone()));
        }
        ws.windows[0].dev = crate::dev::DevHooks::from_env();
        ws
    }

    /// Puts the menus in the macOS menu bar instead of the window. Must be called on the main
    /// thread, once the app is running.
    pub fn install_native_menu(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        if !self.windows[0].dev.active() {
            self.menu = crate::native_menu::NativeMenu::new(ctx);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }

    pub fn support(&self) -> Option<&Path> {
        self.support.as_deref()
    }

    /// The window that has (or last had) focus.
    pub fn front(&self) -> &ScrawlApp {
        self.windows.iter().find(|w| w.uid == self.focused).unwrap_or(&self.windows[0])
    }

    pub fn front_mut(&mut self) -> &mut ScrawlApp {
        let i = self.index_of(self.focused).unwrap_or(0);
        &mut self.windows[i]
    }

    fn index_of(&self, uid: u64) -> Option<usize> {
        self.windows.iter().position(|w| w.uid == uid)
    }

    fn add_window(&mut self, mut app: ScrawlApp) -> usize {
        app.support = self.support.clone();
        self.focused = app.uid;
        self.windows.push(app);
        self.windows.len() - 1
    }

    fn new_window(&mut self) -> usize {
        self.add_window(ScrawlApp::new(self.render_state.clone()))
    }

    /// Opens `path`: brings its window forward if it's open already; otherwise loads it into
    /// window `into` if that one is empty and untouched, or into a new window.
    pub fn open(&mut self, path: &Path, into: Option<usize>) {
        let same = |w: &ScrawlApp| w.file_path.as_deref().is_some_and(|p| same_file(p, path));
        if let Some(i) = self.windows.iter().position(same) {
            self.focus(i);
            return;
        }
        let i = match into.filter(|&i| self.windows[i].is_pristine()) {
            Some(i) => i,
            None => self.new_window(),
        };
        let w = &mut self.windows[i];
        w.open_path(path);
        let opened = w.error.is_none();
        if opened {
            self.prefs.add_recent(path);
            self.save_prefs();
            #[cfg(target_os = "macos")]
            crate::macos::note_recent(path);
        }
        self.focused = self.windows[i].uid;
        if opened {
            crate::plugins::run_handlers(self, i, "open");
        }
    }

    /// Opens files macOS asked us to (from Finder or the Dock), and shows each window's file and
    /// edited state in its title bar.
    #[cfg(target_os = "macos")]
    fn macos_documents(&mut self) {
        for path in crate::macos::take_opened() {
            let into = self.index_of(self.focused);
            self.open(&path, into);
        }
        if self.windows[0].dev.active() {
            return;
        }
        for w in &self.windows {
            let bar = crate::macos::TitleBar { title: w.title(), file: w.file_path.clone(), edited: w.is_dirty() };
            let shown = self.title_bars.iter().find(|(u, _)| *u == w.uid).map(|(_, b)| b);
            // A window's new title may take a frame to arrive; until then it isn't found.
            if shown != Some(&bar) && crate::macos::set_title_bar(&bar) {
                self.title_bars.retain(|(u, _)| *u != w.uid);
                self.title_bars.push((w.uid, bar));
            }
        }
        let open: Vec<u64> = self.windows.iter().map(|w| w.uid).collect();
        self.title_bars.retain(|(u, _)| open.contains(u));
    }

    /// Runs "selection" handlers for windows whose selection changed, once the pointer is up
    /// (not every frame of a drag).
    fn selection_events(&mut self, ctx: &egui::Context) {
        if self.plugins.handlers("selection").is_empty() || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        for i in 0..self.windows.len() {
            let (uid, selection) = (self.windows[i].uid, self.windows[i].selection.clone());
            let before = self.seen_selection.iter().find(|(u, _)| *u == uid).map(|(_, s)| s);
            if before.is_some_and(|s| *s != selection) {
                crate::plugins::run_handlers(self, i, "selection");
            }
            // A handler may change the selection itself; that doesn't count as a new change.
            let now = self.windows[i].selection.clone();
            self.seen_selection.retain(|(u, _)| *u != uid);
            self.seen_selection.push((uid, now));
        }
        let open: Vec<u64> = self.windows.iter().map(|w| w.uid).collect();
        self.seen_selection.retain(|(u, _)| open.contains(u));
    }

    fn focus(&mut self, i: usize) {
        self.focused = self.windows[i].uid;
        self.windows[i].requests.push(Request::FocusWindow(self.focused));
    }

    fn save_prefs(&self) {
        if let Some(dir) = &self.support {
            self.prefs.save(dir);
        }
    }

    /// Reopens documents a crash left unsaved.
    fn recover(&mut self) {
        let Some(support) = self.support.clone() else { return };
        for r in crate::autosave::recover(&support) {
            let i = self.new_window();
            let w = &mut self.windows[i];
            let path = r.meta.path.filter(|p| p.exists());
            w.set_document(r.doc, path);
            // Unsaved: compare against the file on disk, or nothing for an untitled document.
            w.mark_unsaved();
            w.autosave_name = r.name;
            w.notify("Recovered unsaved changes");
        }
    }

    /// Asks every window with unsaved changes whether to save. Returns false if one cancelled.
    fn confirm_all(&mut self) -> bool {
        self.windows.iter_mut().all(|w| w.dev.active() || w.confirm_discard())
    }

    /// Closes window `i` after asking to save its changes. The root window hands its viewport
    /// over to the next window.
    fn close(&mut self, ctx: &egui::Context, i: usize) {
        if !self.windows[i].dev.active() && !self.windows[i].confirm_discard() {
            return;
        }
        if self.windows.len() == 1 {
            self.quitting = true;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            return;
        }
        let closed = self.windows.remove(i);
        if let Some(support) = &self.support {
            crate::autosave::remove(support, &closed.autosave_name);
        }
        if i == 0 {
            // The next window takes over the root viewport, where it was on screen.
            let id = viewport_id(&self.windows[0]);
            let rect = ctx.input_for(id, |input| (input.viewport().outer_rect, input.viewport().inner_rect));
            if let (Some(outer), Some(inner)) = rect {
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::OuterPosition(outer.min));
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::InnerSize(inner.size()));
            }
            self.windows[0].window_title_changed();
        }
        if self.index_of(self.focused).is_none() {
            self.focused = self.windows[i.min(self.windows.len() - 1)].uid;
        }
    }

    /// Runs what windows asked of the workspace.
    fn handle_requests(&mut self, ctx: &egui::Context) {
        // What requests change (window titles, menus, notices) shows next frame.
        if self.windows.iter().any(|w| !w.requests.is_empty()) {
            ctx.request_repaint();
        }
        let mut i = 0;
        while i < self.windows.len() {
            let requests = std::mem::take(&mut self.windows[i].requests);
            let uid = self.windows[i].uid;
            for r in requests {
                // Earlier requests may have closed windows.
                let Some(i) = self.index_of(uid) else { break };
                self.handle(ctx, i, r);
            }
            i += 1;
        }
    }

    fn handle(&mut self, ctx: &egui::Context, i: usize, request: Request) {
        match request {
            Request::NewWindow => {
                self.new_window();
            }
            Request::Open(path) => self.open(&path, Some(i)),
            Request::OpenDocument(doc) => {
                let j = self.new_window();
                self.windows[j].set_document(*doc, None);
                self.windows[j].mark_unsaved();
            }
            Request::OpenSample => {
                let j = if self.windows[i].is_pristine() { i } else { self.new_window() };
                self.windows[j].set_document(scrawl_core::sample::sample_document(), None);
                self.focused = self.windows[j].uid;
            }
            Request::Saved(path) => {
                #[cfg(target_os = "macos")]
                crate::macos::note_recent(&path);
                self.prefs.add_recent(&path);
                self.save_prefs();
                let w = &mut self.windows[i];
                if let Some(support) = &self.support {
                    crate::autosave::remove(support, &w.autosave_name);
                }
                w.autosaved = None;
                crate::plugins::run_handlers(self, i, "save");
            }
            Request::CloseWindow => self.close(ctx, i),
            Request::Quit => {
                if self.confirm_all() {
                    self.quitting = true;
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
            }
            Request::ClearRecent => {
                self.prefs.recent.clear();
                self.save_prefs();
            }
            Request::FocusWindow(uid) => {
                if let Some(j) = self.index_of(uid) {
                    self.focused = uid;
                    ctx.send_viewport_cmd_to(viewport_id(&self.windows[j]), egui::ViewportCommand::Focus);
                }
            }
            Request::RunPlugin(id) => crate::plugins::run_command(self, i, &id),
            Request::RunScript => crate::plugins::open_console(&mut self.windows[i]),
            Request::ReloadPlugins => {
                self.plugins = crate::plugins::Plugins::load(self.support.as_deref());
                let n = self.plugins.commands().len();
                self.windows[i].notify(format!("Loaded {n} plugin command{}", if n == 1 { "" } else { "s" }));
            }
            Request::RevealPlugins => crate::plugins::reveal_folder(self, i),
        }
    }

    /// Routes a command from the native menu bar to the focused window: commands a text field
    /// may want arrive as key presses, the rest run at the start of its next frame.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn menu_command(&mut self, command: Command) {
        let w = self.front_mut();
        if command.edits_text() {
            let events = crate::commands::as_events(&command, read_clipboard).unwrap_or_default();
            w.injected.extend(events);
        } else {
            w.pending.push(command);
        }
    }

    fn menu_context(&self, native: bool) -> MenuContext {
        MenuContext {
            recent: self.prefs.recent.clone(),
            plugins: self.plugins.commands(),
            windows: self.windows.iter().map(|w| (w.uid, w.title(), w.uid == self.focused)).collect(),
            native,
        }
    }

    /// Notes which window has focus, from the viewports' focus changes.
    fn track_focus(&mut self, uid: u64, focused: Option<bool>) {
        let now = focused.unwrap_or(false);
        let before = self.was_focused.iter().find(|(u, _)| *u == uid).map(|&(_, f)| f);
        if now && before == Some(false) {
            self.focused = uid;
        }
        self.was_focused.retain(|(u, _)| *u != uid);
        self.was_focused.push((uid, now));
    }
}

/// The viewport of a window other than the root one.
fn viewport_id(w: &ScrawlApp) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("scrawl-window", w.uid))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn read_clipboard() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

impl eframe::App for Workspace {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        crate::dev::DevHooks::inject(&mut self.windows[0], raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(target_os = "macos")]
        let native = self.menu.is_some();
        #[cfg(not(target_os = "macos"))]
        let native = false;
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            for command in menu.poll() {
                self.menu_command(command);
            }
        }
        let cx = self.menu_context(native);
        for w in &mut self.windows {
            w.menu_context = cx.clone();
        }

        // The root window.
        let root_focus = ctx.input(|i| i.viewport().focused);
        self.track_focus(self.windows[0].uid, root_focus);
        self.windows[0].show(ui);
        crate::dev::DevHooks::on_frame(&mut self.windows[0], &ctx);
        let root_close = ctx.input(|i| i.viewport().close_requested());

        // The others, each in its own viewport.
        let mut closing = vec![];
        for i in 1..self.windows.len() {
            let w = &mut self.windows[i];
            let builder = egui::ViewportBuilder::default()
                .with_title(w.title())
                .with_inner_size([1280.0, 820.0])
                .with_min_inner_size([900.0, 560.0]);
            let (focused, close) = ctx.show_viewport_immediate(viewport_id(w), builder, |ui, _| {
                w.show(ui);
                ui.input(|i| (i.viewport().focused, i.viewport().close_requested()))
            });
            let uid = self.windows[i].uid;
            self.track_focus(uid, focused);
            if close {
                closing.push(uid);
            }
        }
        for uid in closing {
            if let Some(i) = self.index_of(uid) {
                self.close(&ctx, i);
            }
        }

        // Closing the root window closes its document; the app quits with the last one.
        if root_close && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close(&ctx, 0);
        }

        self.handle_requests(&ctx);
        self.selection_events(&ctx);
        #[cfg(target_os = "macos")]
        self.macos_documents();

        if let Some(support) = self.support.clone() {
            let now = ctx.input(|i| i.time);
            for w in &mut self.windows {
                crate::autosave::tick(w, &support, now);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(crate::autosave::INTERVAL));
        }

        #[cfg(target_os = "macos")]
        if self.menu.is_some() {
            let menus = crate::commands::menus(self.front(), &cx);
            if let Some(menu) = &mut self.menu {
                menu.update(&menus);
            }
        }
    }

    fn on_exit(&mut self) {
        // Every window either saved or chose to discard its changes.
        if let Some(support) = &self.support {
            for w in &self.windows {
                crate::autosave::remove(support, &w.autosave_name);
            }
        }
    }
}

/// A new document for tests and examples: the sample document in a single window.
#[cfg(test)]
pub fn sample(cc: &eframe::CreationContext<'_>) -> Workspace {
    let mut ws = Workspace::new(cc, vec![], None);
    ws.windows[0].set_document(scrawl_core::sample::sample_document(), None);
    ws
}
