//! Plugins: JavaScript run by an embedded QuickJS engine, against the document as plain objects
//! (the same JSON as the file format). A script edits `scrawl.document` and `scrawl.selection`
//! freely; when it finishes, the result replaces the document as one undoable step.
//!
//! Plugins live in the `Plugins` folder: either a loose `.js` file (one command, named after the
//! file or a `// @name` comment in its first lines) or a folder with a `plugin.json` listing its
//! commands. See docs/PLUGINS.md for the API.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Function, Runtime};
use scrawl_core::{Document, Frame, Layer, LayerId};
use serde::Deserialize;

use crate::app::ScrawlApp;
use crate::workspace::Workspace;

/// Scripts are stopped after this long.
const TIME_LIMIT: Duration = Duration::from_secs(5);
const MEMORY_LIMIT: usize = 512 << 20;

/// The `scrawl` and `console` globals scripts see.
const PRELUDE: &str = include_str!("plugin_prelude.js");

/// A menu command a plugin provides.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginCommand {
    /// Unique across plugins: "<plugin>/<command>".
    pub id: String,
    /// Shown in the Plugins menu.
    pub label: String,
    pub script: PathBuf,
    /// Events that run it by themselves: "open", "save", "selection".
    pub on: Vec<String>,
}

/// What scripts may reach outside the document, through the app. Tests use a fake.
pub trait Host {
    /// Asks for a file to read, of one of `extensions` (any if empty). `None` if cancelled.
    fn choose_file(&self, extensions: &[String]) -> Option<PathBuf>;
    /// Asks where to save a file, suggesting `name`.
    fn choose_save(&self, name: &str) -> Option<PathBuf>;
    /// Asks whether `plugin` may fetch from `host`.
    fn allow_network(&self, plugin: &str, host: &str) -> bool;
    fn fetch(&self, url: &str) -> Result<String, String>;
}

/// The real thing: native dialogs and HTTP.
pub struct DesktopHost;

impl Host for DesktopHost {
    fn choose_file(&self, extensions: &[String]) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new();
        if !extensions.is_empty() {
            d = d.add_filter("Files", extensions);
        }
        d.pick_file()
    }

    fn choose_save(&self, name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_file_name(name).save_file()
    }

    fn allow_network(&self, plugin: &str, host: &str) -> bool {
        let answer = rfd::MessageDialog::new()
            .set_title("Allow network access?")
            .set_description(format!("The plugin “{plugin}” wants to connect to {host}. Allow it from now on?"))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        answer == rfd::MessageDialogResult::Yes
    }

    fn fetch(&self, url: &str) -> Result<String, String> {
        let mut response = ureq::get(url).call().map_err(|e| e.to_string())?;
        response.body_mut().with_config().limit(32 << 20).read_to_string().map_err(|e| e.to_string())
    }
}

/// Who a script runs as and what it may reach.
#[derive(Clone)]
pub struct Access {
    pub host: Rc<dyn Host>,
    /// Names the plugin in permission prompts and records.
    pub plugin: String,
    /// The plugin's own folder, which it may always read.
    pub folder: Option<PathBuf>,
    /// Where granted network hosts are remembered.
    pub permissions: Option<PathBuf>,
    /// The event that ran the script, if any: `scrawl.event`.
    pub event: Option<String>,
}

impl Access {
    /// The console's access: dialogs and prompts, no plugin folder.
    pub fn console(support: Option<&Path>) -> Self {
        Self {
            host: Rc::new(DesktopHost),
            plugin: "Script console".into(),
            folder: None,
            permissions: support.map(|s| folder(s).join("permissions.json")),
            event: None,
        }
    }

    fn granted(&self, host: &str) -> bool {
        let Some(file) = &self.permissions else { return false };
        let all: std::collections::BTreeMap<String, Vec<String>> =
            std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        all.get(&self.plugin).is_some_and(|hosts| hosts.iter().any(|h| h == host))
    }

    fn grant(&self, host: &str) {
        let Some(file) = &self.permissions else { return };
        let mut all: std::collections::BTreeMap<String, Vec<String>> =
            std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        all.entry(self.plugin.clone()).or_default().push(host.into());
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, serde_json::to_vec_pretty(&all).unwrap_or_default());
    }
}

/// The host part of a URL, for permissions.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    commands: Vec<ManifestCommand>,
}

#[derive(Deserialize)]
struct ManifestCommand {
    name: String,
    script: String,
    #[serde(default)]
    on: Vec<String>,
}

#[derive(Default)]
pub struct Plugins {
    commands: Vec<PluginCommand>,
    /// Problems found while loading, e.g. a malformed plugin.json.
    pub problems: Vec<String>,
}

pub fn folder(support: &Path) -> PathBuf {
    support.join("Plugins")
}

impl Plugins {
    /// Finds the plugins in `support`'s Plugins folder.
    pub fn load(support: Option<&Path>) -> Self {
        let mut out = Self::default();
        let Some(dir) = support.map(folder) else { return out };
        let Ok(entries) = std::fs::read_dir(&dir) else { return out };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            if path.extension().is_some_and(|e| e == "js") {
                let source = std::fs::read_to_string(&path).unwrap_or_default();
                let tag = |name: &str| source.lines().take(5).find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim().to_owned())).filter(|v| !v.is_empty());
                let label = tag("// @name").unwrap_or(stem.clone());
                let on = tag("// @on").map(|v| v.split([',', ' ']).filter(|s| !s.is_empty()).map(str::to_owned).collect()).unwrap_or_default();
                out.commands.push(PluginCommand { id: stem, label, script: path, on });
            } else if path.join("plugin.json").is_file() {
                let manifest = std::fs::read(path.join("plugin.json"))
                    .map_err(|e| e.to_string())
                    .and_then(|b| serde_json::from_slice::<Manifest>(&b).map_err(|e| e.to_string()));
                match manifest {
                    Ok(m) => {
                        for c in m.commands {
                            let label = format!("{} › {}", m.name, c.name);
                            out.commands.push(PluginCommand { id: format!("{stem}/{}", c.name), label, script: path.join(c.script), on: c.on });
                        }
                    }
                    Err(e) => out.problems.push(format!("{}: {e}", path.display())),
                }
            }
        }
        out
    }

    /// (id, label) of every command, for the Plugins menu.
    pub fn commands(&self) -> Vec<(String, String)> {
        self.commands.iter().map(|c| (c.id.clone(), c.label.clone())).collect()
    }

    pub fn find(&self, id: &str) -> Option<&PluginCommand> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// Commands that run on `event`.
    pub fn handlers(&self, event: &str) -> Vec<PluginCommand> {
        self.commands.iter().filter(|c| c.on.iter().any(|e| e == event)).cloned().collect()
    }
}

/// What a script printed and asked to show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    /// `console.log` lines.
    pub log: Vec<String>,
    /// `scrawl.message` texts, shown as notices.
    pub messages: Vec<String>,
    /// Whether the document changed.
    pub changed: bool,
}

#[derive(Deserialize)]
struct Result_ {
    document: serde_json::Value,
    selection: Vec<serde_json::Value>,
    messages: Vec<String>,
}

/// Runs `source` against `app`'s document. On success the script's edits are applied (as one
/// undo step); on failure nothing changes and the error says why, with what it logged so far.
pub fn run(app: &mut ScrawlApp, source: &str, name: &str, access: &Access) -> Result<Output, (String, Vec<String>)> {
    let input = serde_json::json!({
        "document": serde_json::to_string(&app.doc).map_err(|e| (e.to_string(), vec![]))?,
        "selection": serde_json::to_string(&app.selection).map_err(|e| (e.to_string(), vec![]))?,
        "page": app.page.0.to_string(),
        "event": access.event,
    });
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let result = evaluate(source, name, &input, log.clone(), access);
    let log = log.take();
    let json = result.map_err(|e| (e, log.clone()))?;
    let out: Result_ = serde_json::from_str(&json).map_err(|e| (format!("unexpected script result: {e}"), log.clone()))?;
    let mut doc: Document = serde_json::from_value(out.document)
        .map_err(|e| (format!("the script left the document in a shape Scrawl can't read: {e}"), log.clone()))?;
    if doc.pages.is_empty() {
        return Err(("the script removed every page".into(), log));
    }
    doc.assets = app.doc.assets.clone();
    dedupe_ids(&mut doc);
    let changed = doc != app.doc;
    if changed {
        app.replace_document(doc);
    }
    let page = app.page();
    let selection: Vec<LayerId> = out
        .selection
        .into_iter()
        .filter_map(|v| serde_json::from_value::<LayerId>(v).ok())
        .filter(|&id| page.contains(id))
        .collect();
    app.selection = selection;
    Ok(Output { log, messages: out.messages, changed })
}

/// Evaluates the prelude and the script in a fresh engine, returning the JSON of the result.
fn evaluate(source: &str, name: &str, input: &serde_json::Value, log: Rc<RefCell<Vec<String>>>, access: &Access) -> Result<String, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(MEMORY_LIMIT);
    let start = Instant::now();
    // Time spent in dialogs and downloads doesn't count against the script.
    let waited = Rc::new(std::cell::Cell::new(Duration::ZERO));
    let w = waited.clone();
    rt.set_interrupt_handler(Some(Box::new(move || start.elapsed().saturating_sub(w.get()) > TIME_LIMIT)));
    let timed = {
        let waited = waited.clone();
        move |f: &mut dyn FnMut()| {
            let t = Instant::now();
            f();
            waited.set(waited.get() + t.elapsed());
        }
    };
    let timed = Rc::new(timed);
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    ctx.with(|ctx| {
        let globals = ctx.globals();
        let fail = |e: rquickjs::Error| e.to_string();
        globals.set("__input", input.to_string()).map_err(fail)?;
        let logger = log.clone();
        globals.set("__log", Function::new(ctx.clone(), move |line: String| logger.borrow_mut().push(line)).map_err(fail)?).map_err(fail)?;
        globals.set("__make", Function::new(ctx.clone(), make_layer).map_err(fail)?).map_err(fail)?;
        globals.set("__uuid", Function::new(ctx.clone(), || uuid::Uuid::new_v4().to_string()).map_err(fail)?).map_err(fail)?;
        // Files the user chose this run, which the script may read.
        let chosen: Rc<RefCell<HashSet<PathBuf>>> = Default::default();
        let err = |m: String| rquickjs::Error::new_from_js_message("host", "value", m);
        {
            let (access, chosen, timed) = (access.clone(), chosen.clone(), timed.clone());
            globals
                .set(
                    "__chooseFile",
                    Function::new(ctx.clone(), move |exts: String| {
                        let exts: Vec<String> = serde_json::from_str(&exts).unwrap_or_default();
                        let mut picked = None;
                        timed(&mut || picked = access.host.choose_file(&exts));
                        picked.map(|p| {
                            chosen.borrow_mut().insert(p.clone());
                            p.to_string_lossy().into_owned()
                        })
                    })
                    .map_err(fail)?,
                )
                .map_err(fail)?;
        }
        {
            let (access, chosen) = (access.clone(), chosen.clone());
            globals
                .set(
                    "__readText",
                    Function::new(ctx.clone(), move |path: String| -> rquickjs::Result<String> {
                        let p = PathBuf::from(&path);
                        let own = access.folder.as_ref().is_some_and(|f| {
                            std::fs::canonicalize(&p).ok().zip(std::fs::canonicalize(f).ok()).is_some_and(|(p, f)| p.starts_with(f))
                        });
                        if !own && !chosen.borrow().contains(&p) {
                            return Err(err(format!("{path} wasn't chosen with scrawl.chooseFile()")));
                        }
                        std::fs::read_to_string(&p).map_err(|e| err(e.to_string()))
                    })
                    .map_err(fail)?,
                )
                .map_err(fail)?;
        }
        {
            let (access, timed) = (access.clone(), timed.clone());
            globals
                .set(
                    "__saveText",
                    Function::new(ctx.clone(), move |name: String, text: String| -> rquickjs::Result<Option<String>> {
                        let mut picked = None;
                        timed(&mut || picked = access.host.choose_save(&name));
                        let Some(p) = picked else { return Ok(None) };
                        std::fs::write(&p, text).map_err(|e| err(e.to_string()))?;
                        Ok(Some(p.to_string_lossy().into_owned()))
                    })
                    .map_err(fail)?,
                )
                .map_err(fail)?;
        }
        {
            let (access, timed) = (access.clone(), timed.clone());
            globals
                .set(
                    "__fetch",
                    Function::new(ctx.clone(), move |url: String| -> rquickjs::Result<String> {
                        let host = url_host(&url).ok_or_else(|| err(format!("not a URL: {url}")))?.to_owned();
                        let mut result = Err(String::new());
                        timed(&mut || {
                            let allowed = access.granted(&host) || {
                                let yes = access.host.allow_network(&access.plugin, &host);
                                if yes {
                                    access.grant(&host);
                                }
                                yes
                            };
                            result = if allowed { access.host.fetch(&url) } else { Err(format!("not allowed to reach {host}")) };
                        });
                        result.map_err(err)
                    })
                    .map_err(fail)?,
                )
                .map_err(fail)?;
        }
        let report = |e: rquickjs::CaughtError<'_>| {
            let msg = e.to_string();
            if start.elapsed().saturating_sub(waited.get()) > TIME_LIMIT {
                format!("the script took longer than {} seconds and was stopped", TIME_LIMIT.as_secs())
            } else {
                msg
            }
        };
        ctx.eval::<(), _>(PRELUDE).catch(&ctx).map_err(report)?;
        // Wrapped in a function so scripts can `return` early.
        let wrapped = format!("(function() {{\n{source}\n}})();");
        let mut opts = rquickjs::context::EvalOptions::default();
        opts.global = true;
        opts.strict = false;
        opts.filename = Some(name.to_owned());
        ctx.eval_with_options::<(), _>(wrapped, opts).catch(&ctx).map_err(report)?;
        ctx.eval::<String, _>("__result()").catch(&ctx).map_err(report)
    })
}

/// A new layer of `kind`, as JSON, for `scrawl.create`. Frames and names are set by the prelude.
fn make_layer(kind: String, text: rquickjs::prelude::Opt<String>) -> rquickjs::Result<String> {
    let f = Frame::new(0.0, 0.0, 100.0, 100.0);
    let layer = match kind.as_str() {
        "rectangle" => Layer::rectangle(f),
        "oval" => Layer::oval(f),
        "triangle" => Layer::polygon(f, 3),
        "polygon" => Layer::polygon(f, 5),
        "star" => Layer::star(f),
        "line" => Layer::line(scrawl_core::kurbo::Point::ZERO, scrawl_core::kurbo::Point::new(100.0, 0.0)),
        "text" => Layer::text(scrawl_core::kurbo::Point::ZERO, text.0.unwrap_or_else(|| "Text".into())),
        "artboard" => Layer::artboard("Artboard", Frame::new(0.0, 0.0, 375.0, 667.0)),
        "group" => Layer::group(vec![], f),
        "hotspot" => Layer::hotspot(f),
        "slice" => Layer::slice(f),
        other => return Err(rquickjs::Error::new_from_js_message("string", "layer", format!("unknown layer kind `{other}`"))),
    };
    serde_json::to_string(&layer).map_err(|e| rquickjs::Error::new_from_js_message("layer", "string", e.to_string()))
}

/// Gives a fresh id to every layer whose id was already used, e.g. one a script copied with
/// `JSON.parse(JSON.stringify(layer))`.
fn dedupe_ids(doc: &mut Document) {
    let mut seen = HashSet::new();
    for page in &mut doc.pages {
        page.walk_mut(|l| {
            if !seen.insert(l.id) {
                l.id = LayerId::new();
                seen.insert(l.id);
            }
        });
    }
}

/// Runs plugin command `id` in window `i`.
pub fn run_command(ws: &mut Workspace, i: usize, id: &str) {
    let Some(cmd) = ws.plugins.find(id).cloned() else { return };
    run_plugin(ws, i, &cmd, None);
}

/// Runs the plugins that handle `event` ("open", "save", "selection") in window `i`.
pub fn run_handlers(ws: &mut Workspace, i: usize, event: &str) {
    for cmd in ws.plugins.handlers(event) {
        run_plugin(ws, i, &cmd, Some(event));
    }
}

fn run_plugin(ws: &mut Workspace, i: usize, cmd: &PluginCommand, event: Option<&str>) {
    let mut access = Access::console(ws.support());
    access.plugin = cmd.id.split('/').next().unwrap_or(&cmd.id).to_owned();
    access.folder = cmd.script.parent().filter(|p| p.join("plugin.json").exists()).map(Path::to_path_buf);
    access.event = event.map(str::to_owned);
    if let Some(host) = &ws.plugin_host {
        access.host = host.clone();
    }
    let app = &mut ws.windows[i];
    let source = match std::fs::read_to_string(&cmd.script) {
        Ok(s) => s,
        Err(e) => {
            app.error = Some(format!("Couldn't read {}: {e}", cmd.script.display()));
            return;
        }
    };
    let name = cmd.script.file_name().map_or_else(|| cmd.label.clone(), |n| n.to_string_lossy().into_owned());
    match run(app, &source, &name, &access) {
        Ok(out) => {
            for line in &out.log {
                log::info!("{}: {line}", cmd.label);
            }
            if let Some(m) = out.messages.last() {
                app.notify(m.clone());
            }
            if let Some(console) = &mut app.console {
                console.output.extend(out.log);
            }
        }
        Err((e, log)) => {
            let tail = log.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join("\n");
            app.error = Some(if tail.is_empty() { format!("{}: {e}", cmd.label) } else { format!("{}: {e}\n\n{tail}", cmd.label) });
        }
    }
}

/// Plugins › Run Script…: a window to type and run a script.
#[derive(Clone, Debug)]
pub struct Console {
    pub code: String,
    pub output: Vec<String>,
}

const STARTER: &str = "// Edit scrawl.document and scrawl.selection; the result replaces the document.\n\
for (const layer of scrawl.selectedLayers) {\n  layer.name = layer.name.toUpperCase();\n}\n\
scrawl.message(`Renamed ${scrawl.selection.length} layers`);\n";

pub fn open_console(app: &mut ScrawlApp) {
    if app.console.is_none() {
        app.console = Some(Console { code: STARTER.into(), output: vec![] });
    }
}

/// Shows the script console, if open.
pub fn show_console(app: &mut ScrawlApp, ctx: &egui::Context) {
    let id = app.id("script");
    let Some(console) = &mut app.console else { return };
    let mut open = true;
    let mut run_now = false;
    egui::Window::new("Script").id(id).open(&mut open).default_size([520.0, 420.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            run_now = ui.button("Run").on_hover_text("⌘↩").clicked();
            if ui.button("Clear Output").clicked() {
                console.output.clear();
            }
            ui.weak("scrawl.document, scrawl.selection, scrawl.create(), console.log()");
        });
        let editor = egui::TextEdit::multiline(&mut console.code).code_editor().desired_rows(14).desired_width(f32::INFINITY);
        let response = ui.add(editor);
        if response.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            run_now = true;
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(120.0).stick_to_bottom(true).show(ui, |ui| {
            for line in &console.output {
                ui.monospace(line);
            }
        });
    });
    if !open {
        app.console = None;
        return;
    }
    if run_now {
        let code = console.code.clone();
        let access = Access::console(app.support.as_deref());
        let result = run(app, &code, "script.js", &access);
        let Some(console) = &mut app.console else { return };
        match result {
            Ok(out) => {
                console.output.extend(out.log);
                console.output.extend(out.messages.iter().map(|m| format!("» {m}")));
                if !out.changed {
                    console.output.push("(no changes to the document)".into());
                }
            }
            Err((e, log)) => {
                console.output.extend(log);
                console.output.push(format!("Error: {e}"));
            }
        }
    }
}

/// Opens the Plugins folder, creating it (with an example plugin) the first time.
pub fn reveal_folder(ws: &mut Workspace, i: usize) {
    let Some(support) = ws.support().map(Path::to_path_buf) else { return };
    let dir = folder(&support);
    if !dir.exists()
        && let Err(e) = install_examples(&dir)
    {
        ws.windows[i].error = Some(format!("Couldn't create {}: {e}", dir.display()));
        return;
    }
    let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
    if let Err(e) = std::process::Command::new(opener).arg(&dir).spawn() {
        ws.windows[i].error = Some(format!("Couldn't open {}: {e}", dir.display()));
    }
    ws.plugins = Plugins::load(Some(&support));
}

/// Example plugins, written into a new Plugins folder.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("Examples/plugin.json", include_str!("../plugins/Examples/plugin.json")),
    ("Examples/rename-by-type.js", include_str!("../plugins/Examples/rename-by-type.js")),
    ("Examples/random-colors.js", include_str!("../plugins/Examples/random-colors.js")),
    ("Examples/grid-of-copies.js", include_str!("../plugins/Examples/grid-of-copies.js")),
];

fn install_examples(dir: &Path) -> std::io::Result<()> {
    for (path, contents) in EXAMPLES {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().expect("has a folder"))?;
        std::fs::write(p, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers dialogs and requests from fixed values, and records what was asked.
    #[derive(Default)]
    struct FakeHost {
        file: Option<PathBuf>,
        save: Option<PathBuf>,
        allow: bool,
        asked: RefCell<Vec<String>>,
    }

    impl Host for FakeHost {
        fn choose_file(&self, _: &[String]) -> Option<PathBuf> {
            self.file.clone()
        }
        fn choose_save(&self, _: &str) -> Option<PathBuf> {
            self.save.clone()
        }
        fn allow_network(&self, _: &str, host: &str) -> bool {
            self.asked.borrow_mut().push(host.into());
            self.allow
        }
        fn fetch(&self, url: &str) -> Result<String, String> {
            Ok(format!("{{\"url\": \"{url}\"}}"))
        }
    }

    fn access(host: impl Into<Rc<FakeHost>>, permissions: Option<PathBuf>) -> Access {
        Access { host: host.into(), plugin: "test".into(), folder: None, permissions, event: None }
    }

    fn eval_js(source: &str, name: &str, input: &serde_json::Value, log: Rc<RefCell<Vec<String>>>) -> Result<String, String> {
        evaluate(source, name, input, log, &access(FakeHost::default(), None))
    }

    fn empty_input() -> serde_json::Value {
        serde_json::json!({ "document": serde_json::to_string(&Document::default()).unwrap(), "selection": "[]", "page": "", "event": "save" })
    }

    #[test]
    fn scripts_read_only_chosen_files_and_save_where_asked() {
        let dir = std::env::temp_dir().join(format!("scrawl-plugin-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (chosen, other, saved) = (dir.join("in.txt"), dir.join("other.txt"), dir.join("out.txt"));
        std::fs::write(&chosen, "hello").unwrap();
        std::fs::write(&other, "secret").unwrap();
        let host = FakeHost { file: Some(chosen.clone()), save: Some(saved.clone()), ..Default::default() };
        let log = Rc::new(RefCell::new(vec![]));
        let source = format!(
            "const p = scrawl.chooseFile(['txt']); console.log(scrawl.readText(p)); scrawl.saveText('x.txt', 'written');\n\
             try {{ scrawl.readText({other:?}); }} catch (e) {{ console.log('refused'); }}\n\
             console.log(scrawl.event);"
        );
        evaluate(&source, "files.js", &empty_input(), log.clone(), &access(host, None)).unwrap();
        assert_eq!(log.borrow().as_slice(), ["hello", "refused", "save"]);
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), "written");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn network_access_is_asked_once_per_host_and_remembered() {
        let dir = std::env::temp_dir().join(format!("scrawl-plugin-net-{}", std::process::id()));
        let permissions = dir.join("permissions.json");
        let log = Rc::new(RefCell::new(vec![]));
        let source = "console.log(scrawl.fetchJSON('https://example.com:8080/a?b').url); scrawl.fetch('https://example.com/c');";

        let host = Rc::new(FakeHost { allow: true, ..Default::default() });
        let a = access(host.clone(), Some(permissions.clone()));
        evaluate(source, "net.js", &empty_input(), log.clone(), &a).unwrap();
        assert_eq!(log.borrow().as_slice(), ["https://example.com:8080/a?b"]);
        assert_eq!(host.asked.borrow().as_slice(), ["example.com"]);

        // Remembered: not asked again, even by a host that would refuse.
        let refusing = Rc::new(FakeHost::default());
        let b = access(refusing.clone(), Some(permissions.clone()));
        evaluate(source, "net.js", &empty_input(), log.clone(), &b).unwrap();
        assert!(refusing.asked.borrow().is_empty());

        // Another host, refused.
        let e = evaluate("scrawl.fetch('https://other.org/')", "net.js", &empty_input(), log, &b).unwrap_err();
        assert!(e.contains("not allowed to reach other.org"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hosts_of_urls() {
        assert_eq!(url_host("https://user@api.example.com:443/x"), Some("api.example.com"));
        assert_eq!(url_host("http://localhost"), Some("localhost"));
        assert_eq!(url_host("example.com"), None);
    }

    fn layer_named<'a>(doc: &'a Document, name: &str) -> Option<&'a Layer> {
        let mut found = None;
        for p in &doc.pages {
            p.walk(|l| {
                if l.name == name && found.is_none() {
                    found = Some(l);
                }
            });
        }
        found
    }

    #[test]
    fn evaluates_with_the_prelude() {
        let doc = scrawl_core::sample::sample_document();
        let input = serde_json::json!({
            "document": serde_json::to_string(&doc).unwrap(),
            "selection": "[]",
            "page": doc.pages[0].id.0.to_string(),
        });
        let log = Rc::new(RefCell::new(vec![]));
        let json = eval_js("console.log('pages', scrawl.pages.length); scrawl.page.name = 'Renamed';", "t.js", &input, log.clone()).unwrap();
        assert_eq!(log.borrow().as_slice(), ["pages 2"]);
        let out: Result_ = serde_json::from_str(&json).unwrap();
        assert_eq!(out.document["pages"][0]["name"], "Renamed");
    }

    #[test]
    fn errors_and_runaway_scripts_are_reported() {
        let input = serde_json::json!({ "document": serde_json::to_string(&Document::default()).unwrap(), "selection": "[]", "page": "" });
        let log = Rc::new(RefCell::new(vec![]));
        let e = eval_js("throw new Error('nope')", "bad.js", &input, log.clone()).unwrap_err();
        assert!(e.contains("nope"), "{e}");
        let e = eval_js("while (true) {}", "loop.js", &input, log).unwrap_err();
        assert!(e.contains("stopped"), "{e}");
    }

    #[test]
    fn layers_made_by_scripts_are_valid() {
        let json = make_layer("text".into(), rquickjs::prelude::Opt(Some("Hi".into()))).unwrap();
        let layer: Layer = serde_json::from_str(&json).unwrap();
        assert_eq!(layer.kind.type_name(), "Text");
        assert!(make_layer("blob".into(), rquickjs::prelude::Opt(None)).is_err());
    }

    #[test]
    fn copied_layers_get_fresh_ids() {
        let mut doc = scrawl_core::sample::sample_document();
        let card = layer_named(&doc, "Card").unwrap().clone();
        doc.pages[0].layers.push(card.clone());
        dedupe_ids(&mut doc);
        let mut ids = HashSet::new();
        doc.pages[0].walk(|l| assert!(ids.insert(l.id), "duplicate id"));
        assert_eq!(layer_named(&doc, "Card").unwrap().id, card.id, "the original keeps its id");
    }

    #[test]
    fn the_documented_example_runs() {
        let docs = include_str!("../../../docs/PLUGINS.md");
        let start = docs.rfind("```js\n").unwrap() + 6;
        let source = &docs[start..start + docs[start..].find("```").unwrap()];
        let doc = scrawl_core::sample::sample_document();
        let input = serde_json::json!({
            "document": serde_json::to_string(&doc).unwrap(),
            "selection": "[]",
            "page": doc.pages[0].id.0.to_string(),
        });
        let json = eval_js(source, "example.js", &input, Rc::new(RefCell::new(vec![]))).unwrap();
        let out: Result_ = serde_json::from_str(&json).unwrap();
        let back: Document = serde_json::from_value(out.document).unwrap();
        let names: Vec<&str> = back.pages[0].layers.iter().filter(|l| l.is_artboard()).map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["01 iPhone 16", "02 Buttons"]);
        assert_eq!(out.messages, ["Numbered 2 artboards"]);
    }

    #[test]
    fn example_plugins_run() {
        let doc = scrawl_core::sample::sample_document();
        let card = layer_named(&doc, "Card").unwrap().id;
        let input = serde_json::json!({
            "document": serde_json::to_string(&doc).unwrap(),
            "selection": serde_json::to_string(&[card]).unwrap(),
            "page": doc.pages[0].id.0.to_string(),
        });
        for (path, source) in EXAMPLES.iter().filter(|(p, _)| p.ends_with(".js")) {
            let json = eval_js(source, path, &input, Rc::new(RefCell::new(vec![]))).unwrap_or_else(|e| panic!("{path}: {e}"));
            let out: Result_ = serde_json::from_str(&json).unwrap();
            let mut back: Document = serde_json::from_value(out.document).unwrap_or_else(|e| panic!("{path}: {e}"));
            dedupe_ids(&mut back);
            assert_eq!(out.messages.len(), 1, "{path}");
            if path.contains("grid") {
                assert_eq!(out.selection.len(), 12);
                let mut n = 0;
                back.pages[0].walk(|l| n += usize::from(l.name == "Card"));
                assert_eq!(n, 12);
            }
            if path.contains("rename") {
                assert_eq!(layer_named(&back, "Rectangle 1").map(|l| l.id), Some(card));
            }
        }
    }

    #[test]
    fn example_plugins_load_and_parse() {
        let support = std::env::temp_dir().join(format!("scrawl-plugins-{}", std::process::id()));
        install_examples(&folder(&support)).unwrap();
        std::fs::write(folder(&support).join("hello.js"), "// @name Say Hello\nscrawl.message('hi')").unwrap();
        let plugins = Plugins::load(Some(&support));
        assert!(plugins.problems.is_empty(), "{:?}", plugins.problems);
        let labels: Vec<String> = plugins.commands().into_iter().map(|(_, l)| l).collect();
        assert!(labels.contains(&"Say Hello".to_owned()), "{labels:?}");
        assert_eq!(labels.iter().filter(|l| l.starts_with("Examples › ")).count(), 3, "{labels:?}");
        std::fs::remove_dir_all(&support).ok();
    }
}
