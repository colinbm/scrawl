//! Every menu command: its shortcut, where it sits in the menus, and what it does. One model
//! drives the in-window menu bar, keyboard shortcuts and the native macOS menu bar.

use std::path::PathBuf;

use egui::{Key, KeyboardShortcut, Modifiers};
use scrawl_core::ops::{Align, BooleanOp, Reorder};
use scrawl_core::prototype::Destination;
use scrawl_core::snap::Axis;
use scrawl_core::{LayerKind, SymbolId};

use crate::app::ScrawlApp;
use crate::canvas::Tool;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    // File
    New,
    Open,
    OpenRecent(PathBuf),
    ClearRecent,
    CloseWindow,
    Save,
    SaveAs,
    SaveAsSketch,
    RevertToSaved,
    BrowseVersions,
    ExportSelection,
    ExportAll,
    Quit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    CopySvg,
    CopyCss,
    Duplicate,
    Delete,
    SelectAll,
    FindLayer,
    // Insert
    Tool(Tool),
    PlaceImage,
    InsertSymbol(SymbolId),
    // Arrange
    Reorder(Reorder),
    Group,
    Ungroup,
    Align(Align),
    Distribute(Axis),
    Tidy,
    CreateStack,
    // Layer
    Boolean(BooleanOp),
    Mask,
    CreateSymbol,
    DetachSymbol,
    EditMaster,
    EditPoints,
    ConvertToPath,
    ConvertToOutlines,
    ReversePath,
    // Prototype
    Preview,
    AddLink,
    LinkBack,
    RemoveLink,
    ToggleStartPoint,
    ToggleLinks,
    // View
    ZoomIn,
    ZoomOut,
    ActualSize,
    ZoomToFit,
    ZoomToSelection,
    ToggleRulers,
    ToggleGrid,
    ToggleLayout,
    ToggleSnap,
    // Window and Help
    FocusWindow(u64),
    OpenSample,
}

/// What only the workspace, which owns every window, can do. Windows queue these.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    NewWindow,
    /// Open a file: in this window if it's empty and untouched, else in a new one.
    Open(PathBuf),
    /// Open a document (e.g. an earlier version) as a new, untitled window.
    OpenDocument(Box<scrawl_core::Document>),
    /// The window saved its document to this file.
    Saved(PathBuf),
    OpenSample,
    CloseWindow,
    Quit,
    ClearRecent,
    FocusWindow(u64),
}

const fn sc(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CMD_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);
const CMD_CTRL: Modifiers = Modifiers::COMMAND.plus(Modifiers::CTRL);
const CTRL: Modifiers = Modifiers::CTRL;

impl Command {
    /// The keyboard shortcut, if it has one with a modifier. (Single-key tool shortcuts are
    /// handled separately, since they'd swallow typing if they were menu key equivalents.)
    pub fn shortcut(&self) -> Option<KeyboardShortcut> {
        use Command::*;
        match self {
            New => sc(CMD, Key::N),
            Open => sc(CMD, Key::O),
            CloseWindow => sc(CMD, Key::W),
            Save => sc(CMD, Key::S),
            SaveAs => sc(CMD_SHIFT, Key::S),
            ExportSelection => sc(CMD_SHIFT, Key::E),
            Quit => sc(CMD, Key::Q),
            Undo => sc(CMD, Key::Z),
            Redo => sc(CMD_SHIFT, Key::Z),
            Cut => sc(CMD, Key::X),
            Copy => sc(CMD, Key::C),
            Paste => sc(CMD, Key::V),
            Duplicate => sc(CMD, Key::D),
            SelectAll => sc(CMD, Key::A),
            FindLayer => sc(CMD, Key::F),
            PlaceImage => sc(CMD_SHIFT, Key::K),
            Reorder(scrawl_core::ops::Reorder::Forward) => sc(CMD, Key::CloseBracket),
            Reorder(scrawl_core::ops::Reorder::Backward) => sc(CMD, Key::OpenBracket),
            Reorder(scrawl_core::ops::Reorder::ToFront) => sc(CMD_ALT, Key::CloseBracket),
            Reorder(scrawl_core::ops::Reorder::ToBack) => sc(CMD_ALT, Key::OpenBracket),
            Group => sc(CMD, Key::G),
            Ungroup => sc(CMD_SHIFT, Key::G),
            Distribute(Axis::X) => sc(CMD_CTRL, Key::H),
            Distribute(Axis::Y) => sc(CMD_CTRL, Key::V),
            Tidy => sc(CMD_CTRL, Key::T),
            Boolean(BooleanOp::Union) => sc(CMD_ALT, Key::U),
            Boolean(BooleanOp::Subtract) => sc(CMD_ALT, Key::S),
            Boolean(BooleanOp::Intersect) => sc(CMD_ALT, Key::I),
            Boolean(BooleanOp::Difference) => sc(CMD_ALT, Key::X),
            Mask => sc(CMD_CTRL, Key::M),
            CreateSymbol => sc(CMD_ALT, Key::Y),
            DetachSymbol => sc(CMD_ALT, Key::B),
            ConvertToPath => sc(CMD_ALT, Key::F),
            ConvertToOutlines => sc(CMD_SHIFT, Key::O),
            Preview => sc(CMD, Key::P),
            ToggleLinks => sc(CTRL, Key::P),
            ZoomIn => sc(CMD, Key::Equals),
            ZoomOut => sc(CMD, Key::Minus),
            ActualSize => sc(CMD, Key::Num0),
            ZoomToFit => sc(CMD, Key::Num1),
            ZoomToSelection => sc(CMD, Key::Num2),
            ToggleRulers => sc(CTRL, Key::R),
            ToggleGrid => sc(CTRL, Key::G),
            ToggleLayout => sc(CTRL, Key::L),
            _ => None,
        }
    }

    /// Commands a focused text field handles itself when given the key press.
    pub fn edits_text(&self) -> bool {
        matches!(self, Command::Undo | Command::Redo | Command::Cut | Command::Copy | Command::Paste | Command::SelectAll)
    }
}

/// Commands reachable by keyboard shortcut, more modifiers first so ⇧⌘S isn't taken for ⌘S.
pub fn keyboard_commands() -> Vec<(Command, KeyboardShortcut)> {
    use Command::*;
    let mut all = vec![
        New, Open, CloseWindow, Save, SaveAs, ExportSelection, Quit, Undo, Redo, Duplicate, SelectAll, FindLayer, PlaceImage,
        Group, Ungroup, Tidy, Mask, CreateSymbol, DetachSymbol, ConvertToPath, ConvertToOutlines, Preview, ToggleLinks, ZoomIn,
        ZoomOut, ActualSize, ZoomToFit, ZoomToSelection, ToggleRulers, ToggleGrid, ToggleLayout,
    ];
    all.extend([self::Reorder::Forward, self::Reorder::Backward, self::Reorder::ToFront, self::Reorder::ToBack].map(Command::Reorder));
    all.extend([Axis::X, Axis::Y].map(Command::Distribute));
    all.extend(BooleanOp::ALL.map(Command::Boolean));
    let mut out: Vec<(Command, KeyboardShortcut)> = all.into_iter().filter_map(|c| Some((c.clone(), c.shortcut()?))).collect();
    // ⌘+ zooms in too.
    out.push((ZoomIn, KeyboardShortcut::new(CMD, Key::Plus)));
    let weight = |s: &KeyboardShortcut| s.modifiers.shift as u8 + s.modifiers.alt as u8 + s.modifiers.ctrl as u8 + s.modifiers.command as u8;
    out.sort_by_key(|(_, s)| std::cmp::Reverse(weight(s)));
    out
}

// ---- Menu model -------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub command: Command,
    pub shortcut: Option<KeyboardShortcut>,
    /// A single-key shortcut shown beside the item but not bound by the menu.
    pub hint: &'static str,
    pub enabled: bool,
    /// Some for toggles.
    pub checked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Item(Item),
    Separator,
    Submenu { label: String, entries: Vec<Entry>, enabled: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

/// What the menus show beyond the focused window's own state.
#[derive(Clone, Debug, Default)]
pub struct MenuContext {
    pub recent: Vec<PathBuf>,
    /// Open windows: (uid, title, focused).
    pub windows: Vec<(u64, String, bool)>,
    /// The platform puts Quit and the window list in its own menus.
    pub native: bool,
}

fn item(label: impl Into<String>, command: Command, enabled: bool) -> Entry {
    let shortcut = command.shortcut();
    Entry::Item(Item { label: label.into(), command, shortcut, hint: "", enabled, checked: None })
}

fn toggle(label: &str, command: Command, on: bool) -> Entry {
    let shortcut = command.shortcut();
    Entry::Item(Item { label: label.into(), command, shortcut, hint: "", enabled: true, checked: Some(on) })
}

fn hinted(label: &str, command: Command, hint: &'static str) -> Entry {
    Entry::Item(Item { label: label.into(), command, shortcut: None, hint, enabled: true, checked: None })
}

fn submenu(label: &str, entries: Vec<Entry>, enabled: bool) -> Entry {
    Entry::Submenu { label: label.into(), entries, enabled }
}

pub const ALIGN_LABELS: [(&str, Align); 6] = [
    ("Align Left", Align::Left),
    ("Align Horizontally", Align::HCenter),
    ("Align Right", Align::Right),
    ("Align Top", Align::Top),
    ("Align Vertically", Align::VCenter),
    ("Align Bottom", Align::Bottom),
];

/// The menus for `app` as the focused window.
pub fn menus(app: &ScrawlApp, cx: &MenuContext) -> Vec<Menu> {
    use Command::*;
    let has_sel = !app.selection.is_empty();
    let n = app.selection.len();
    let page = app.page();
    let instance = match app.selection[..] {
        [id] => match page.find(id).map(|l| &l.kind) {
            Some(LayerKind::Symbol(i)) => Some(i.symbol_id),
            _ => None,
        },
        _ => None,
    };
    let artboard = match app.selection[..] {
        [id] => page.find(id).filter(|l| l.is_artboard() && l.master_of().is_none()),
        _ => None,
    };
    let is_start = artboard.is_some_and(|l| matches!(&l.kind, LayerKind::Artboard(a) if a.flow_home));

    let recent: Vec<Entry> = cx
        .recent
        .iter()
        .map(|p| item(p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()), OpenRecent(p.clone()), true))
        .chain([Entry::Separator, item("Clear Menu", ClearRecent, !cx.recent.is_empty())])
        .collect();
    let mut file = vec![
        item("New", New, true),
        item("Open…", Open, true),
        submenu("Open Recent", recent, true),
        Entry::Separator,
        item("Close Window", CloseWindow, true),
        item("Save", Save, true),
        item("Save As…", SaveAs, true),
        item("Save as Sketch…", SaveAsSketch, true),
        item("Revert to Saved", RevertToSaved, app.file_path.is_some() && app.is_dirty()),
        item("Browse Versions…", BrowseVersions, app.file_path.is_some()),
        Entry::Separator,
        item("Export Selection…", ExportSelection, has_sel),
        item("Export All…", ExportAll, true),
    ];
    if !cx.native {
        file.extend([Entry::Separator, item("Quit", Quit, true)]);
    }

    let edit = vec![
        item("Undo", Undo, app.history.can_undo() || app.history.is_dirty(&app.doc)),
        item("Redo", Redo, app.history.can_redo()),
        Entry::Separator,
        item("Cut", Cut, has_sel),
        item("Copy", Copy, has_sel),
        item("Paste", Paste, true),
        submenu("Copy As", vec![item("SVG", CopySvg, has_sel), item("CSS", CopyCss, has_sel)], has_sel),
        item("Duplicate", Duplicate, has_sel),
        item("Delete", Delete, has_sel),
        Entry::Separator,
        item("Select All", SelectAll, true),
        Entry::Separator,
        item("Find Layer…", FindLayer, true),
    ];

    let mut insert: Vec<Entry> = crate::canvas::Tool::INSERT.iter().map(|&t| hinted(t.label(), Tool(t), t.key())).collect();
    let masters: Vec<Entry> = app
        .doc
        .symbol_masters()
        .into_iter()
        .filter_map(|(_, l)| Some(item(l.name.clone(), InsertSymbol(l.master_of()?), true)))
        .collect();
    let any_masters = !masters.is_empty();
    insert.extend([Entry::Separator, item("Image…", PlaceImage, true), submenu("Symbol", masters, any_masters)]);

    let arrange = vec![
        item("Bring Forward", Reorder(scrawl_core::ops::Reorder::Forward), has_sel),
        item("Bring to Front", Reorder(scrawl_core::ops::Reorder::ToFront), has_sel),
        item("Send Backward", Reorder(scrawl_core::ops::Reorder::Backward), has_sel),
        item("Send to Back", Reorder(scrawl_core::ops::Reorder::ToBack), has_sel),
        Entry::Separator,
        item("Group", Group, has_sel),
        item("Ungroup", Ungroup, has_sel),
        Entry::Separator,
        submenu("Align", ALIGN_LABELS.iter().map(|&(l, a)| item(l, Align(a), has_sel)).collect(), has_sel),
        item("Distribute Horizontally", Distribute(Axis::X), n >= 3),
        item("Distribute Vertically", Distribute(Axis::Y), n >= 3),
        item("Tidy", Tidy, n >= 2),
        Entry::Separator,
        item("Create Stack", CreateStack, has_sel),
    ];

    let layer = vec![
        submenu("Combine", BooleanOp::ALL.iter().map(|&op| item(op.label(), Boolean(op), n > 1)).collect(), n > 1),
        item("Use as Mask", Mask, has_sel),
        Entry::Separator,
        item("Create Symbol", CreateSymbol, has_sel),
        item("Detach from Symbol", DetachSymbol, instance.is_some()),
        item("Edit Master", EditMaster, instance.is_some()),
        Entry::Separator,
        submenu(
            "Path",
            vec![
                item("Edit Points", EditPoints, n == 1),
                item("Convert to Path", ConvertToPath, has_sel),
                item("Convert to Outlines", ConvertToOutlines, has_sel),
                item("Reverse Path Direction", ReversePath, has_sel),
            ],
            has_sel,
        ),
    ];

    let prototype = vec![
        item("Preview", Preview, true),
        Entry::Separator,
        hinted("Add Link…", AddLink, "W"),
        item("Link Back", LinkBack, has_sel),
        item("Remove Link", RemoveLink, has_sel),
        hinted("Hotspot", Tool(crate::canvas::Tool::Hotspot), "H"),
        Entry::Separator,
        item(if is_start { "Remove Start Point" } else { "Use Artboard as Start Point" }, ToggleStartPoint, artboard.is_some()),
        toggle("Show All Links", ToggleLinks, app.show_links),
    ];

    let view = vec![
        item("Zoom In", ZoomIn, true),
        item("Zoom Out", ZoomOut, true),
        item("Actual Size", ActualSize, true),
        item("Zoom to Fit", ZoomToFit, true),
        item("Zoom to Selection", ZoomToSelection, has_sel),
        Entry::Separator,
        toggle("Show Rulers", ToggleRulers, app.show_rulers),
        toggle("Show Grid", ToggleGrid, app.show_grid),
        toggle("Show Layout", ToggleLayout, app.show_layout),
        toggle("Snap to Layers", ToggleSnap, app.snap_to_layers),
    ];


    let mut menus = vec![
        Menu { title: "File", entries: file },
        Menu { title: "Edit", entries: edit },
        Menu { title: "Insert", entries: insert },
        Menu { title: "Arrange", entries: arrange },
        Menu { title: "Layer", entries: layer },
        Menu { title: "Prototype", entries: prototype },
        Menu { title: "View", entries: view },
    ];
    if !cx.native {
        let windows = cx
            .windows
            .iter()
            .map(|(uid, title, focused)| Entry::Item(Item {
                label: title.clone(),
                command: FocusWindow(*uid),
                shortcut: None,
                hint: "",
                enabled: true,
                checked: Some(*focused),
            }))
            .collect();
        menus.push(Menu { title: "Window", entries: windows });
    }
    menus.push(Menu { title: "Help", entries: vec![item("Open Sample Document", OpenSample, true)] });
    menus
}

// ---- Running commands -------------------------------------------------------------------------

/// Runs `command` in `app`'s window. Commands that need the workspace are queued as requests.
pub fn execute(app: &mut ScrawlApp, command: Command, ctx: &egui::Context) {
    use Command::*;
    match command {
        New => app.requests.push(Request::NewWindow),
        Open => app.open(),
        OpenRecent(path) => app.requests.push(Request::Open(path)),
        ClearRecent => app.requests.push(Request::ClearRecent),
        CloseWindow => app.requests.push(Request::CloseWindow),
        Save => {
            app.save();
        }
        SaveAs => {
            app.save_as();
        }
        SaveAsSketch => app.save_as_sketch(),
        RevertToSaved => app.revert_to_saved(),
        BrowseVersions => crate::versions::open_browser(app),
        ExportSelection => {
            let sel = app.selection.clone();
            app.export_layers(&sel);
        }
        ExportAll => app.export_all(),
        Quit => app.requests.push(Request::Quit),
        Undo => {
            if app.canvas.text_edit.is_some() {
                crate::text_edit::end(app);
            }
            app.undo();
        }
        Redo => app.redo(),
        Cut => app.cut(ctx),
        Copy => app.copy(ctx),
        Paste => app.paste(None),
        CopySvg => app.copy_as_svg(ctx),
        CopyCss => app.copy_as_css(ctx),
        Duplicate => app.duplicate_selection(),
        Delete => app.delete_selection(),
        SelectAll => {
            if app.canvas.edit.is_some() {
                crate::vector::select_all_points(app);
            } else {
                app.select_all();
            }
        }
        FindLayer => {
            app.sidebar = crate::app::Sidebar::Layers;
            app.layer_filter.focus = true;
        }
        Tool(tool) => app.tool = tool,
        PlaceImage => crate::import::choose_images(app),
        InsertSymbol(symbol) => app.insert_symbol(symbol),
        Reorder(how) => app.reorder(how),
        Group => app.group_selection(),
        Ungroup => app.ungroup_selection(),
        Align(how) => app.align(how),
        Distribute(axis) => app.distribute(axis),
        Tidy => app.tidy(),
        CreateStack => app.make_stack(),
        Boolean(op) => app.boolean(op),
        Mask => app.use_as_mask(),
        CreateSymbol => app.create_symbol(),
        DetachSymbol => app.detach_selection(),
        EditMaster => {
            if let [id] = app.selection[..]
                && let Some(LayerKind::Symbol(i)) = app.page().find(id).map(|l| &l.kind)
            {
                let symbol = i.symbol_id;
                app.edit_master(symbol);
            }
        }
        EditPoints => {
            if let [id] = app.selection[..] {
                app.enter_vector_edit(id);
            }
        }
        ConvertToPath => app.convert_to_paths(),
        ConvertToOutlines => app.outline_strokes(),
        ReversePath => app.reverse_paths(),
        Preview => crate::preview::open(app),
        AddLink => crate::prototyping::start_linking(app),
        LinkBack => {
            let sel = app.selection.clone();
            crate::prototyping::set_links(app, &sel, Some(Destination::Back));
        }
        RemoveLink => {
            let sel = app.selection.clone();
            crate::prototyping::set_links(app, &sel, None);
        }
        ToggleStartPoint => {
            if let [id] = app.selection[..]
                && let Some(LayerKind::Artboard(a)) = app.page_mut().find_mut(id).map(|l| &mut l.kind)
                && a.symbol_id.is_none()
            {
                a.flow_home = !a.flow_home;
            }
        }
        ToggleLinks => app.show_links ^= true,
        ZoomIn => app.step_zoom(true),
        ZoomOut => app.step_zoom(false),
        ActualSize => app.actual_size(),
        ZoomToFit => app.zoom_to_fit(),
        ZoomToSelection => app.zoom_to_selection(crate::viewport::MAX_ZOOM),
        ToggleRulers => app.show_rulers ^= true,
        ToggleGrid => app.show_grid ^= true,
        ToggleLayout => app.show_layout ^= true,
        ToggleSnap => app.snap_to_layers ^= true,
        FocusWindow(uid) => app.requests.push(Request::FocusWindow(uid)),
        OpenSample => app.requests.push(Request::OpenSample),
    }
}

/// The input events that stand for `command` when it comes from a native menu, so the window
/// handles it exactly as if its shortcut had been typed (text fields included). `None` for
/// commands without a shortcut, which run directly. Pastes carry `clipboard`.
pub fn as_events(command: &Command, clipboard: impl FnOnce() -> Option<String>) -> Option<Vec<egui::Event>> {
    match command {
        Command::Copy => Some(vec![egui::Event::Copy]),
        Command::Cut => Some(vec![egui::Event::Cut]),
        Command::Paste => Some(clipboard().map(|t| vec![egui::Event::Paste(t)]).unwrap_or_default()),
        _ => {
            let s = command.shortcut()?;
            let key = |pressed| egui::Event::Key { key: s.logical_key, physical_key: None, pressed, repeat: false, modifiers: s.modifiers };
            Some(vec![key(true), key(false)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique() {
        let all = keyboard_commands();
        for (i, (a, sa)) in all.iter().enumerate() {
            for (b, sb) in &all[i + 1..] {
                assert!(sa != sb || a == b, "{a:?} and {b:?} share a shortcut");
            }
        }
    }

    #[test]
    fn more_modifiers_come_first() {
        let all = keyboard_commands();
        let pos = |c: &Command| all.iter().position(|(x, _)| x == c).unwrap();
        assert!(pos(&Command::SaveAs) < pos(&Command::Save));
        assert!(pos(&Command::Redo) < pos(&Command::Undo));
        assert!(pos(&Command::Ungroup) < pos(&Command::Group));
    }

    #[test]
    fn menu_shortcuts_become_key_events() {
        let events = as_events(&Command::Group, || None).unwrap();
        assert!(matches!(events[0], egui::Event::Key { key: Key::G, pressed: true, modifiers, .. } if modifiers.command));
        assert_eq!(as_events(&Command::Paste, || Some("x".into())), Some(vec![egui::Event::Paste("x".into())]));
        assert_eq!(as_events(&Command::ExportAll, || None), None);
    }
}
