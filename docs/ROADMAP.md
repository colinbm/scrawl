# Feature roadmap

✅ done · 🟡 partial · ⬜ not started

## Milestone 1 — Core editor
- ✅ Infinite canvas: scroll pan, pinch / ⌘-scroll zoom, space-drag & middle-drag pan, zoom presets (⌘0/1/2, ⌘±)
- ✅ Pages (add, rename, delete, switch)
- ✅ Artboards: draw, presets, background, clip content, drag-into/out-of artboard, title click-select
- ✅ Shapes: rectangle (corner radius), oval, line (endpoint handles, shift 45° snap)
- ✅ Select, shift/⌘ multi-select, marquee, hover outline, group drill-in (double-click, ⌘-click, Esc to parent)
- ✅ Move (shift axis lock, ⌥-drag duplicate), rotation-aware resize (shift aspect, ⌥ from center), arrow nudge
- ✅ Group / ungroup, z-order, align (6 ways), duplicate, delete, copy/cut/paste (system clipboard JSON)
- ✅ Layer list: tree, expand, rename, visibility, lock, drag-and-drop reorder/reparent
- ✅ Inspector: X/Y/W/H/rotation, aspect lock, flip, opacity, fills, borders (center/inside/outside)
- ✅ Undo/redo, `.scrawl` save/open, unsaved-changes prompts

## Milestone 2 — Vector editing
- ✅ Multi-subpath paths with fill rule; Point types (straight/mirrored/disconnected/asymmetric, keys 1–4)
- ✅ Pen tool (P): click for corners, drag for curves, click first point to close, Enter/Esc to finish
- ✅ Point editing (double-click / Enter): drag points & handles, click segment to add, double-click to toggle curve, delete, marquee, nudge, inspector X/Y & type
- ✅ Boolean ops via `linesweeper`: union ⌥⌘U, subtract ⌥⌘S, intersect ⌥⌘I, difference ⌥⌘X (destructive)
- ✅ Convert to path ⌥⌘F, convert to outlines ⇧⌘O (respects border position), reverse path
- ✅ Triangle / polygon / star shapes with editable sides, points, radius; independent corner radii
- ✅ ⌘-drag corner to rotate (⇧ snaps 15°); multi-selection resize
- ⬜ Non-destructive boolean groups (live booleans)
- ⬜ Scissors, rotate copies, per-point corner radius, vector point snapping

## Milestone 3 — Styling
- ✅ Linear, radial and angular gradients on fills and borders, with a stops editor and on-canvas handles
- ✅ Paint popover: HSV picker, hex/alpha, per-fill blend modes, document color swatches
- ✅ Drop shadows and inner shadows (offset, blur, spread). Exact Gaussian for rects/circles; banded approximation for other shapes
- ✅ Border options: dash/gap, caps, joins; start/end markers (arrows, circles, squares, line) on open paths
- ✅ Layer blend modes (16 modes); "Use as Mask" ⌃⌘M (outline masks clip layers above in the same group)
- ✅ Headless GPU exporter (`scrawl_render::export`) with pixel tests; whole-app UI tests in egui_kittest
- ⬜ Image fills and image layers (needs asset storage in the file format)
- ⬜ True GPU blur pass (replace banded shadows; enables layer and background blur)
- ⬜ Alpha/luminance masks, "ignore underlying mask"
- ⬜ Noise fills, pattern fills, gradient interpolation options

## Milestone 4 — Text
- ⬜ Text layers via `parley` (shaping, font fallback), Vello glyph rendering
- ⬜ Fixed/auto width, alignment, line height, letter spacing, decorations, text on path

## Milestone 5 — Productivity
- ⬜ Smart guides & snapping, distance measurements (⌥ hover), rulers & guides, grids/layouts
- ⬜ Smart distribute, tidy, Smart Layout / stacks
- ⬜ Resizing constraints (pin edges, fix size)
- ⬜ Find/filter layers, layer search

## Milestone 6 — Components
- ⬜ Shared layer & text styles
- ⬜ Symbols: masters, instances, overrides, nested symbols, swaps
- ⬜ Libraries (linked documents), color variables

## Milestone 7 — Export & interop
- ⬜ Exportable slices / layer export: PNG, JPG, WEBP, SVG, PDF at multiple scales
- ⬜ Copy as SVG/CSS; Import SVG and images (drag-drop)
- ⬜ Design-file import (open format spec), then export

## Milestone 8 — Prototyping & platform
- ⬜ Prototype links, hotspots, flows, animations, preview window
- ⬜ Native menus (muda), multi-window documents, recent files, autosave/versions
- ⬜ Plugins/scripting (likely WASM or JS)
- ⬜ Collaboration (CRDT doc model) — out of scope until the model stabilises
