//! Developer hooks driven by environment variables, for checking the app without a human at the
//! keyboard (and without OS screen-recording permission):
//!
//! - `SCRAWL_SCREENSHOT=out.png` saves the window's own framebuffer once everything else is done,
//!   then quits.
//! - `SCRAWL_SELECT=name[,name…]` selects the layers with these names at startup.
//! - `SCRAWL_SCRIPT="cmd; cmd; …"` injects real input events. Coordinates are in page space.
//!   Commands: `click x y [mods]`, `dblclick x y [mods]`, `drag x1 y1 x2 y2 [mods]`,
//!   `key NAME [mods]` (egui key names, e.g. `P`, `Enter`, `Num1`), `wait FRAMES`,
//!   `shot PATH`. Modifiers are a `+`-joined list of `shift`, `alt`, `cmd`.

use std::collections::VecDeque;
use std::path::PathBuf;

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use scrawl_core::kurbo::Point;

use crate::app::ScrawlApp;

const SETTLE_FRAMES: u32 = 10;
const DRAG_STEPS: usize = 8;

#[derive(Debug)]
enum Step {
    Move(Point),
    Button { pressed: bool, mods: Modifiers },
    Key { key: Key, mods: Modifiers },
    Wait(u32),
    Shot(PathBuf),
}

#[derive(Default)]
pub struct DevHooks {
    screenshot: Option<PathBuf>,
    select: Vec<String>,
    script: VecDeque<Step>,
    has_script: bool,
    frame: u32,
    /// Screenshot requested and not yet received, with where to save it.
    awaiting: Option<PathBuf>,
    /// Whether the screenshot for `awaiting` has been requested.
    shot_requested: bool,
    final_requested: bool,
    pointer: Pos2,
    mods: Modifiers,
}

fn parse_mods(s: Option<&str>) -> Modifiers {
    let mut m = Modifiers::NONE;
    for part in s.unwrap_or("").split('+') {
        match part {
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            "cmd" => {
                m.command = true;
                if cfg!(target_os = "macos") { m.mac_cmd = true } else { m.ctrl = true }
            }
            _ => {}
        }
    }
    m
}

fn parse_script(src: &str) -> Result<VecDeque<Step>, String> {
    let mut steps = VecDeque::new();
    for cmd in src.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let num = |i: usize| -> Result<f64, String> {
            parts.get(i).ok_or(format!("missing argument in `{cmd}`"))?.parse().map_err(|_| format!("bad number in `{cmd}`"))
        };
        let up = |m| Step::Button { pressed: false, mods: m };
        let down = |m| Step::Button { pressed: true, mods: m };
        match parts[0] {
            "click" | "dblclick" => {
                let mods = parse_mods(parts.get(3).copied());
                steps.push_back(Step::Move(Point::new(num(1)?, num(2)?)));
                let times = if parts[0] == "click" { 1 } else { 2 };
                for _ in 0..times {
                    steps.extend([down(mods), up(mods)]);
                }
                steps.push_back(Step::Wait(2));
            }
            "drag" => {
                let mods = parse_mods(parts.get(5).copied());
                let (a, b) = (Point::new(num(1)?, num(2)?), Point::new(num(3)?, num(4)?));
                steps.extend([Step::Move(a), down(mods)]);
                for i in 1..=DRAG_STEPS {
                    steps.push_back(Step::Move(a.lerp(b, i as f64 / DRAG_STEPS as f64)));
                }
                steps.extend([up(mods), Step::Wait(2)]);
            }
            "key" => {
                let name = parts.get(1).ok_or(format!("missing key in `{cmd}`"))?;
                let key = Key::from_name(name).ok_or(format!("unknown key `{name}`"))?;
                steps.push_back(Step::Key { key, mods: parse_mods(parts.get(2).copied()) });
                steps.push_back(Step::Wait(1));
            }
            "wait" => steps.push_back(Step::Wait(num(1)? as u32)),
            "shot" => steps.push_back(Step::Shot(PathBuf::from(parts.get(1).ok_or("missing path")?))),
            other => return Err(format!("unknown command `{other}`")),
        }
    }
    Ok(steps)
}

impl DevHooks {
    pub fn from_env() -> Self {
        let script = match std::env::var("SCRAWL_SCRIPT") {
            Ok(src) => parse_script(&src).unwrap_or_else(|e| {
                eprintln!("SCRAWL_SCRIPT: {e}");
                std::process::exit(2);
            }),
            Err(_) => VecDeque::new(),
        };
        Self {
            screenshot: std::env::var_os("SCRAWL_SCREENSHOT").map(PathBuf::from),
            select: std::env::var("SCRAWL_SELECT").map(|s| s.split(',').map(str::to_owned).collect()).unwrap_or_default(),
            has_script: !script.is_empty(),
            script,
            ..Default::default()
        }
    }

    pub fn active(&self) -> bool {
        self.screenshot.is_some() || self.has_script
    }

    /// Injects the next scripted input step. Called before each frame.
    pub fn inject(app: &mut ScrawlApp, raw: &mut egui::RawInput) {
        let hooks = &mut app.dev;
        if !hooks.active() || hooks.frame < SETTLE_FRAMES || hooks.awaiting.is_some() {
            return;
        }
        let Some(step) = hooks.script.pop_front() else { return };
        let (view, origin) = (app.viewport().transform(), app.canvas.rect.min);
        let to_screen = move |p: Point| {
            let s = view * p;
            origin + egui::vec2(s.x as f32, s.y as f32)
        };
        let hooks = &mut app.dev;
        // Modifiers persist from a button press through its release.
        raw.events.push(Event::ModifiersChanged(hooks.mods));
        match step {
            Step::Move(p) => {
                hooks.pointer = to_screen(p);
                raw.events.push(Event::PointerMoved(hooks.pointer));
            }
            Step::Button { pressed, mods } => {
                raw.events.push(Event::ModifiersChanged(mods));
                raw.events.push(Event::PointerButton { pos: hooks.pointer, button: PointerButton::Primary, pressed, modifiers: mods });
                hooks.mods = if pressed { mods } else { Modifiers::NONE };
            }
            Step::Key { key, mods } => {
                raw.events.push(Event::ModifiersChanged(mods));
                for pressed in [true, false] {
                    raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: mods });
                }
                raw.events.push(Event::ModifiersChanged(hooks.mods));
            }
            Step::Wait(n) => {
                if n > 1 {
                    hooks.script.push_front(Step::Wait(n - 1));
                }
            }
            Step::Shot(path) => {
                hooks.awaiting = Some(path);
            }
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
        let hooks = &mut app.dev;
        if !hooks.active() {
            return;
        }
        ctx.request_repaint();

        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Some(path) = hooks.awaiting.take() {
                hooks.shot_requested = false;
                if let Err(e) = write_png(&path, &image) {
                    eprintln!("screenshot {} failed: {e}", path.display());
                }
            } else if hooks.final_requested {
                if let Some(path) = &hooks.screenshot
                    && let Err(e) = write_png(path, &image)
                {
                    eprintln!("screenshot failed: {e}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        // A pending mid-script shot: request it once, then wait for it.
        if hooks.awaiting.is_some() {
            if !hooks.shot_requested {
                hooks.shot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            return;
        }
        if hooks.frame >= SETTLE_FRAMES && hooks.script.is_empty() && !hooks.final_requested {
            hooks.final_requested = true;
            if hooks.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
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
