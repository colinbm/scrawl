# Scrawl

A cross-platform vector design tool written in Rust, aiming for feature parity with the established professional design tools.

## Running

```sh
cargo run -p scrawl-app --release                     # empty document
cargo run -p scrawl-core --example sample demo.scrawl  # generate a sample file
cargo run -p scrawl-app --release -- demo.scrawl       # open it
cargo test --workspace
```

## Architecture

| Crate | Role |
|---|---|
| `scrawl-core` | UI-agnostic document model: layer tree, geometry (kurbo), editing ops, hit testing, snapshot undo, `.scrawl` file format (zip + JSON). |
| `scrawl-render` | Turns a page into a [Vello](https://github.com/linebender/vello) scene (GPU compute 2D renderer). |
| `scrawl-app` | Desktop shell: eframe/egui (winit + wgpu) for panels; the canvas is Vello rendered to a wgpu texture shown by egui, with selection overlays painted by egui. |

Key conventions:
- A layer's `frame` is in its **parent's** space; rotation/flips apply about the frame center.
- Path points are **normalized** to the frame (0..1), so resizing scales paths.
- Group frames always tightly fit their children (`Page::refit_ancestor_groups`).
- Undo: the editor mutates freely; `History::commit` runs when no interaction is in progress and records one step per drag/edit.

## Dev hooks

- `SCRAWL_SCREENSHOT=out.png` saves the window's framebuffer after a few frames, then quits.
- `SCRAWL_SELECT=Name1,Name2` selects layers by name at startup.

See [docs/ROADMAP.md](docs/ROADMAP.md) for the feature map.
