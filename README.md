# Scrawl

A cross-platform vector design tool written in Rust, aiming for feature parity with the established professional design tools.

## Running

```sh
cargo run -p scrawl-app --release                     # empty document
cargo run -p scrawl-core --example sample demo.scrawl  # generate a sample file
cargo run -p scrawl-app --release -- demo.scrawl       # open it (several paths open several windows)
cargo test --workspace
cargo run -p scrawl-render --example render -- demo.scrawl out.png 2   # headless PNG render
cargo run -p scrawl-core --example sketch -- demo.scrawl demo.sketch   # convert to/from a design file
```

UI tests run the whole app offscreen with egui_kittest (wgpu); set `SCRAWL_UI_SHOTS=dir` to save
a PNG of each test's final frame. GPU tests skip themselves when no adapter is available.

## Architecture

| Crate | Role |
|---|---|
| `scrawl-core` | UI-agnostic document model: layer tree, geometry (kurbo), editing ops, hit testing, snapshot undo, `.scrawl` file format (zip + JSON + images), design-file import/export, CSS. |
| `scrawl-text` | Text layout (parley) with a layout cache, frame fitting, font listing, glyph outlines (skrifa). |
| `scrawl-render` | Turns a page into a [Vello](https://github.com/linebender/vello) scene (GPU compute 2D renderer); headless export to PNG/JPG/WEBP, SVG writer and import (usvg), PDF (svg2pdf). |
| `scrawl-app` | Desktop shell: eframe/egui (winit + wgpu) for panels; the canvas is Vello rendered to a wgpu texture shown by egui, with selection overlays painted by egui. A `Workspace` owns every document window (one per egui viewport), the native macOS menu bar (muda), recent files, autosave, versions and plugins (QuickJS). |

Key conventions:
- A layer's `frame` is in its **parent's** space; rotation/flips apply about the frame center.
- Path points are **normalized** to the frame (0..1), so resizing scales paths.
- Group frames always tightly fit their children (`Page::refit_ancestor_groups`).
- Undo: the editor mutates freely; `History::commit` runs when no interaction is in progress and records one step per drag/edit.

## Dev hooks

- `SCRAWL_SCREENSHOT=out.png` saves the window's framebuffer after a few frames, then quits.
- `SCRAWL_SELECT=Name1,Name2` selects layers by name at startup.
- `SCRAWL_SCRIPT="key P; click 100 100; drag 300 100 380 160; shot out.png"` injects real input
  events (page coordinates). Commands: `click`, `dblclick`, `drag`, `key NAME [mods]`, `wait N`,
  `shot PATH`; modifiers like `shift+cmd`. See `crates/scrawl-app/src/dev.rs`.

The app keeps preferences, autosaves (crash recovery), versions and plugins in
`~/Library/Application Support/Scrawl` (macOS), `%APPDATA%\Scrawl` (Windows) or `~/.config/scrawl`.
`SCRAWL_SUPPORT=dir` uses another folder; runs with dev hooks use none unless it's set.

Commands live in one model (`commands.rs`): the in-window menu bar, keyboard shortcuts and the
macOS menu bar are all built from it.

See [docs/ROADMAP.md](docs/ROADMAP.md) for the feature map and [docs/PLUGINS.md](docs/PLUGINS.md)
for the plugin API.
