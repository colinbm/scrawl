# Feature roadmap

✅ done · 🟡 partial · ⬜ not started

## Milestone 1 — Core editor (current)
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
- ⬜ Pen tool & vector point editing (corner/mirrored/asymmetric/disconnected points)
- ⬜ Boolean ops (union, subtract, intersect, difference) — e.g. `linesweeper` / `flo_curves`
- ⬜ Flatten, outline stroke, reverse path, scissors, rotate-copies
- ⬜ Per-corner radius, polygon/star/triangle shapes
- ⬜ Rotate handles on canvas; multi-selection resize

## Milestone 3 — Styling
- ⬜ Gradients (linear, radial, angular), image fills, fill blend modes
- ⬜ Shadows & inner shadows, layer/background blur (Vello lacks general blur — needs a filter pass)
- ⬜ Border options: dash, caps, joins, arrowheads
- ⬜ Layer blend modes, masks (alpha/outline)
- ⬜ Color picker with document/global swatches

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
