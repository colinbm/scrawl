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
- ✅ Image layers and image fills (fill, fit, stretch, tile); images stored in the `.scrawl` archive
- ⬜ True GPU blur pass (replace banded shadows; enables layer and background blur)
- ⬜ Alpha/luminance masks, "ignore underlying mask"
- ⬜ Noise fills, pattern fills, gradient interpolation options

## Milestone 4 — Text
- ✅ Text layers (T): parley layout with system fonts and fallback, Vello glyph rendering
- ✅ Auto / fixed-width / fixed sizing; side handles fix the width, other handles fix the frame
- ✅ On-canvas editing with parley's `PlainEditor`: caret, selection, word/line navigation, clipboard, IME, click/drag/double-click
- ✅ Typeface picker with search, weight, italic, size, line height, letter spacing, alignment, vertical alignment
- ✅ Underline, strikethrough, case transform; fills (incl. gradients), borders, shadows on text
- ✅ Convert text to outlines (⇧⌘O) via skrifa glyph outlines
- ⬜ Rich text: per-range styles within one layer
- ⬜ Text styles (shared), OpenType features, paragraph spacing, lists
- ⬜ Text on path

## Milestone 5 — Productivity
- ✅ Smart guides: edge/center alignment and equal-spacing snaps while moving, resizing and drawing (⌃ suspends; View > Snap to Layers)
- ✅ ⌥ hover distance measurements to the hovered layer or parent container
- ✅ Rulers (⌃R) with per-artboard and page guides; square grid (⌃G) and column/row layout (⌃L); all snappable
- ✅ Resizing constraints (pin edges, fix size); artboards can adjust content on resize
- ✅ Stacks (auto-flowing groups, reorder by dragging), distribute (⌃⌘H/⌃⌘V), tidy (⌃⌘T)
- ✅ Layer search (⌘F) by name or text, with a type filter and Select All
- ⬜ Smart distribute handles (drag spacing directly on canvas)
- ⬜ Stack padding, wrapping and per-item sizing; constraints for rotated layers

## Milestone 6 — Components
- ✅ Shared layer & text styles: link, detect drift, update or reset, apply from the Components tab
- ✅ Color variables on fills, borders and shadows; editing one recolors every linked color
- ✅ Symbols: masters (artboards), instances resized by constraints, text / nested-swap / style overrides, nested symbols, Create Symbol ⌥⌘Y, Detach ⌥⌘B, Edit Master
- ✅ Libraries: link other `.scrawl` files, use their symbols, styles and variables (copied in with dependencies), update when the library changes, unlink to keep them locally
- ⬜ Per-master override controls (choose which layers are overridable), image and color overrides
- ⬜ Watch library files for changes; relative library paths; gradient stops linked to variables

## Milestone 7 — Export & interop
- ✅ Export formats per layer (scale, suffix, type) and slices (S): PNG, JPG, WEBP via the headless renderer; SVG via a structure-preserving writer; PDF from that SVG (svg2pdf)
- ✅ Copy as SVG / CSS; import images and SVG files by drag-and-drop, Insert › Image… (⇧⌘K) or pasting SVG markup
- ✅ Design-file import (File › Open) and export (File › Save as…): pages, artboards, shapes, paths, text, images, symbols with overrides, shared styles, swatches, library components, export formats. Exports validate against the published file-format schema; imports checked against real saved files and their previews
- ⬜ SVG: angular gradients, line-end markers and shadow spread are approximated or dropped; SVG text import needs fonts
- ⬜ Design files: multi-style text runs, live boolean shape groups (flattened on import), blurs, noise fills, prototyping links

## Milestone 8 — Prototyping & platform
- ⬜ Prototype links, hotspots, flows, animations, preview window
- ⬜ Native menus (muda), multi-window documents, recent files, autosave/versions
- ⬜ Plugins/scripting (likely WASM or JS)
- ⬜ Collaboration (CRDT doc model) — out of scope until the model stabilises
