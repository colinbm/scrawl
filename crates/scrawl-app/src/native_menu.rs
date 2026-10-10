//! The macOS menu bar, built with muda from the same menu model as the in-window menu bar.
//!
//! Items keep their shortcuts as key equivalents, so macOS shows them and handles the keys; each
//! selection arrives here as a [`Command`] for the workspace to route to the focused window.
//! The menus are rebuilt only when their structure changes (recent files, symbols, plugins);
//! otherwise each frame just updates labels, enabled and checked states.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, channel};

use egui::{Key, KeyboardShortcut};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, IsMenuItem, Menu as MudaMenu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::commands::{Command, Entry, Item, Menu};

/// A built item, to update in place.
enum Handle {
    Plain(MenuItem),
    Check(CheckMenuItem),
    Sub(Submenu),
}

pub struct NativeMenu {
    bar: MudaMenu,
    /// The structure the bar was built from: menu titles and each entry's command or kind.
    built: Vec<String>,
    commands: HashMap<MenuId, Command>,
    handles: Vec<Handle>,
    events: Receiver<MenuId>,
}

impl NativeMenu {
    /// Installs the menu bar. Selections wake the event loop.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            let _ = tx.send(e.id);
            ctx.request_repaint();
        }));
        Some(Self { bar: MudaMenu::new(), built: vec![], commands: HashMap::new(), handles: vec![], events: rx })
    }

    /// Commands chosen since the last call.
    pub fn poll(&self) -> Vec<Command> {
        self.events.try_iter().filter_map(|id| self.commands.get(&id).cloned()).collect()
    }

    /// Brings the bar in line with `menus`.
    pub fn update(&mut self, menus: &[Menu]) {
        let structure = signature(menus);
        if structure != self.built {
            self.rebuild(menus);
            self.built = structure;
            return;
        }
        let mut handles = self.handles.iter();
        for m in menus {
            sync(&m.entries, &mut handles);
        }
    }

    fn rebuild(&mut self, menus: &[Menu]) {
        let bar = MudaMenu::new();
        self.commands.clear();
        self.handles.clear();
        let app = Submenu::new("Scrawl", true);
        let about = AboutMetadata { name: Some("Scrawl".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() };
        let quit = MenuItem::new("Quit Scrawl", true, accelerator(Command::Quit.shortcut()));
        self.commands.insert(quit.id().clone(), Command::Quit);
        let _ = app.append_items(&[
            &PredefinedMenuItem::about(None, Some(about)),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &quit,
        ]);
        let _ = bar.append(&app);
        let mut help = None;
        for m in menus {
            let sub = Submenu::new(m.title, true);
            self.fill(&sub, &m.entries);
            if m.title == "Help" {
                help = Some(sub);
                continue;
            }
            let _ = bar.append(&sub);
        }
        // The window menu lists open windows by itself.
        let window = Submenu::new("Window", true);
        let _ = window.append_items(&[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(Some("Zoom")),
            &PredefinedMenuItem::fullscreen(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ]);
        let _ = bar.append(&window);
        window.set_as_windows_menu_for_nsapp();
        if let Some(help) = help {
            let _ = bar.append(&help);
            help.set_as_help_menu_for_nsapp();
        }
        bar.init_for_nsapp();
        self.bar = bar;
    }

    fn fill(&mut self, parent: &Submenu, entries: &[Entry]) {
        for e in entries {
            let item: Box<dyn IsMenuItem> = match e {
                Entry::Separator => Box::new(PredefinedMenuItem::separator()),
                Entry::Submenu { label, entries, enabled } => {
                    let sub = Submenu::new(label, *enabled);
                    self.handles.push(Handle::Sub(sub.clone()));
                    self.fill(&sub, entries);
                    Box::new(sub)
                }
                Entry::Item(item) => {
                    let label = label(item);
                    let acc = accelerator(item.shortcut);
                    match item.checked {
                        Some(on) => {
                            let check = CheckMenuItem::new(label, item.enabled, on, acc);
                            self.commands.insert(check.id().clone(), item.command.clone());
                            self.handles.push(Handle::Check(check.clone()));
                            Box::new(check)
                        }
                        None => {
                            let plain = MenuItem::new(label, item.enabled, acc);
                            self.commands.insert(plain.id().clone(), item.command.clone());
                            self.handles.push(Handle::Plain(plain.clone()));
                            Box::new(plain)
                        }
                    }
                }
            };
            let _ = parent.append(item.as_ref());
        }
    }
}

/// Single-key shortcuts aren't key equivalents: they'd swallow typing in text fields.
fn label(item: &Item) -> String {
    item.label.clone()
}

/// Updates built items from `entries`, in the order [`NativeMenu::fill`] built them.
fn sync<'a>(entries: &[Entry], handles: &mut impl Iterator<Item = &'a Handle>) {
    for e in entries {
        match e {
            Entry::Separator => {}
            Entry::Submenu { label, entries, enabled } => {
                if let Some(Handle::Sub(sub)) = handles.next() {
                    if sub.text() != *label {
                        sub.set_text(label);
                    }
                    if sub.is_enabled() != *enabled {
                        sub.set_enabled(*enabled);
                    }
                }
                sync(entries, handles);
            }
            Entry::Item(item) => match handles.next() {
                Some(Handle::Plain(m)) => {
                    let text = label(item);
                    if m.text() != text {
                        m.set_text(text);
                    }
                    if m.is_enabled() != item.enabled {
                        m.set_enabled(item.enabled);
                    }
                }
                Some(Handle::Check(c)) => {
                    if c.is_enabled() != item.enabled {
                        c.set_enabled(item.enabled);
                    }
                    if let Some(on) = item.checked
                        && c.is_checked() != on
                    {
                        c.set_checked(on);
                    }
                }
                _ => {}
            },
        }
    }
}

/// What has to match for the built bar to be updated in place rather than rebuilt.
fn signature(menus: &[Menu]) -> Vec<String> {
    fn walk(entries: &[Entry], out: &mut Vec<String>) {
        for e in entries {
            match e {
                Entry::Separator => out.push("-".into()),
                Entry::Submenu { entries, .. } => {
                    out.push(">".into());
                    walk(entries, out);
                    out.push("<".into());
                }
                Entry::Item(i) => out.push(format!("{:?}{}", i.command, if i.checked.is_some() { "✓" } else { "" })),
            }
        }
    }
    let mut out = vec![];
    for m in menus {
        out.push(m.title.to_owned());
        walk(&m.entries, &mut out);
    }
    out
}

fn accelerator(shortcut: Option<KeyboardShortcut>) -> Option<Accelerator> {
    let s = shortcut?;
    let mut mods = Modifiers::empty();
    if s.modifiers.command || s.modifiers.mac_cmd {
        mods |= Modifiers::META;
    }
    if s.modifiers.ctrl && !s.modifiers.command {
        mods |= Modifiers::CONTROL;
    }
    if s.modifiers.alt {
        mods |= Modifiers::ALT;
    }
    if s.modifiers.shift {
        mods |= Modifiers::SHIFT;
    }
    Some(Accelerator::new(mods, code(s.logical_key)?))
}

fn code(key: Key) -> Option<Code> {
    use Code::*;
    Some(match key {
        Key::A => KeyA,
        Key::B => KeyB,
        Key::C => KeyC,
        Key::D => KeyD,
        Key::E => KeyE,
        Key::F => KeyF,
        Key::G => KeyG,
        Key::H => KeyH,
        Key::I => KeyI,
        Key::J => KeyJ,
        Key::K => KeyK,
        Key::L => KeyL,
        Key::M => KeyM,
        Key::N => KeyN,
        Key::O => KeyO,
        Key::P => KeyP,
        Key::Q => KeyQ,
        Key::R => KeyR,
        Key::S => KeyS,
        Key::T => KeyT,
        Key::U => KeyU,
        Key::V => KeyV,
        Key::W => KeyW,
        Key::X => KeyX,
        Key::Y => KeyY,
        Key::Z => KeyZ,
        Key::Num0 => Digit0,
        Key::Num1 => Digit1,
        Key::Num2 => Digit2,
        Key::Num3 => Digit3,
        Key::Num4 => Digit4,
        Key::Num5 => Digit5,
        Key::Num6 => Digit6,
        Key::Num7 => Digit7,
        Key::Num8 => Digit8,
        Key::Num9 => Digit9,
        Key::OpenBracket => BracketLeft,
        Key::CloseBracket => BracketRight,
        Key::Equals => Equal,
        Key::Minus => Minus,
        _ => return None,
    })
}
