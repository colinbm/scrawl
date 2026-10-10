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
- ✅ Live boolean groups: Combine makes a shape group whose shapes stay editable, each with its own operation (inspector, layer list icons); Flatten / Convert to Path bakes it. Shape groups in design files import live
- ✅ Scissors (C in point editing, or Layer › Path), Rotate Copies with a live draggable pivot, per-point corner radius (round-trips through design files), point drags snap to layers, guides and the path's own points

## Milestone 3 — Styling
- ✅ Linear, radial and angular gradients on fills and borders, with a stops editor and on-canvas handles
- ✅ Paint popover: HSV picker, hex/alpha, per-fill blend modes, document color swatches
- ✅ Drop shadows and inner shadows (offset, blur, spread). Exact Gaussian for rects/circles; banded approximation for other shapes
- ✅ Border options: dash/gap, caps, joins; start/end markers (arrows, circles, squares, line) on open paths
- ✅ Layer blend modes (16 modes); "Use as Mask" ⌃⌘M (outline masks clip layers above in the same group)
- ✅ Headless GPU exporter (`scrawl_render::export`) with pixel tests; whole-app UI tests in egui_kittest
- ✅ Image layers and image fills (fill, fit, stretch, tile); images stored in the `.scrawl` archive
- ⬜ True GPU blur pass (replace banded shadows; enables layer and background blur)
- ✅ Alpha and luminance masks alongside outline masks, "Ignore underlying mask"; in SVG as `<mask>`, in design files as alpha masks
- ✅ Noise fills (original, black, white, color; intensity), pattern fills (tiled image fills), gradients blended in sRGB, linear, OKLab or OKLCH (sampled stops for SVG/design files, `in oklab` in CSS)

## Milestone 4 — Text
- ✅ Text layers (T): parley layout with system fonts and fallback, Vello glyph rendering
- ✅ Auto / fixed-width / fixed sizing; side handles fix the width, other handles fix the frame
- ✅ On-canvas editing with parley's `PlainEditor`: caret, selection, word/line navigation, clipboard, IME, click/drag/double-click
- ✅ Typeface picker with search, weight, italic, size, line height, letter spacing, alignment, vertical alignment
- ✅ Underline, strikethrough, case transform; fills (incl. gradients), borders, shadows on text
- ✅ Convert text to outlines (⇧⌘O) via skrifa glyph outlines
- ✅ Rich text: style runs (font, size, weight, italic, spacing, decorations, color) on parts of a layer, from the inspector or ⌘B/⌘I/⌘U while editing; the editor works on the rendered layout. Runs round-trip through design files and keep their colors in SVG
- ✅ OpenType features (ligatures, figures, small caps…), paragraph spacing, bulleted and numbered lists (shared text styles: see Milestone 6)
- ✅ Text on path: Layer › Attach Text to Path, with a start offset; lies flat while editing
- ⬜ Decorations along paths; lists and features per style run; lists and features in design files

## Milestone 5 — Productivity
- ✅ Smart guides: edge/center alignment and equal-spacing snaps while moving, resizing and drawing (⌃ suspends; View > Snap to Layers)
- ✅ ⌥ hover distance measurements to the hovered layer or parent container
- ✅ Rulers (⌃R) with per-artboard and page guides; square grid (⌃G) and column/row layout (⌃L); all snappable
- ✅ Resizing constraints (pin edges, fix size); artboards can adjust content on resize
- ✅ Stacks (auto-flowing groups, reorder by dragging), distribute (⌃⌘H/⌃⌘V), tidy (⌃⌘T)
- ✅ Layer search (⌘F) by name or text, with a type filter and Select All
- ✅ Smart distribute handles: drag the gap between evenly spaced layers on the canvas
- ✅ Stack padding, wrapping, fixed length; children fill across or grow; constraints for rotated layers use their rotated bounds

## Milestone 6 — Components
- ✅ Shared layer & text styles: link, detect drift, update or reset, apply from the Components tab
- ✅ Color variables on fills, borders and shadows; editing one recolors every linked color
- ✅ Symbols: masters (artboards), instances resized by constraints, text / nested-swap / style overrides, nested symbols, Create Symbol ⌥⌘Y, Detach ⌥⌘B, Edit Master
- ✅ Libraries: link other `.scrawl` files, use their symbols, styles and variables (copied in with dependencies), update when the library changes, unlink to keep them locally
- ✅ Masters choose which overrides instances may use; image overrides and color overrides (for fills following a color variable); color overrides and master settings round-trip through design files (image overrides don't yet)
- ⬜ Watch library files for changes; relative library paths; gradient stops linked to variables

## Milestone 7 — Export & interop
- ✅ Export formats per layer (scale, suffix, type) and slices (S): PNG, JPG, WEBP via the headless renderer; SVG via a structure-preserving writer; PDF from that SVG (svg2pdf)
- ✅ Copy as SVG / CSS; import images and SVG files by drag-and-drop, Insert › Image… (⇧⌘K) or pasting SVG markup
- ✅ Design-file import (File › Open) and export (File › Save as…): pages, artboards, shapes, paths, text, images, symbols with overrides, shared styles, swatches, library components, export formats. Exports validate against the published file-format schema; imports checked against real saved files and their previews
- ⬜ SVG: angular gradients, line-end markers and shadow spread are approximated or dropped; SVG text import needs fonts
- ⬜ Design files: blurs, noise fills (prototyping links, hotspots and start points round-trip since Milestone 8)

## Milestone 8 — Prototyping & platform
- ✅ Prototyping: links on any layer (W, or pick in the inspector) to an artboard or Back, with slide animations; hotspots (H); flow start points; links drawn on the canvas (⌃P shows all); fix position when scrolling
- ✅ Preview window (⌘P): follows links with slide transitions, Back and Restart, artboard picker, scrolling for tall artboards, hotspot flash on missed clicks
- ✅ Native macOS menu bar (muda) from the same command model as the in-window menu bar and shortcuts
- ✅ Multi-window documents (⌘N, ⌘W, Window menu), Open Recent, autosave with crash recovery, versions kept on each save (File › Browse Versions…, Revert to Saved)
- ✅ Plugins: JavaScript (QuickJS) against the document's JSON, as one undo step; Plugins menu, script console (⌥⌘K), example plugins. See [PLUGINS.md](PLUGINS.md)
- ⬜ Prototyping: overlays, fixed headers inside groups, more animations (dissolve, push), device frames, flows across pages
- ⬜ Native menus on Windows/Linux (in-window menu bar there for now); macOS document browser and Versions integration
- ⬜ Plugin API: events/actions, UI panels, network and file access with permissions
- ⬜ Collaboration (CRDT doc model) — out of scope until the model stabilises
