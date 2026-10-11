# Plugins

Scrawl runs plugins written in JavaScript with an embedded [QuickJS](https://bellard.org/quickjs/)
engine. A script sees the open document as plain objects (the same JSON as the `.scrawl` file
format), changes them however it likes, and when it finishes Scrawl reads the result back. The
whole run is a single undo step. If the script throws, or leaves the document in a shape Scrawl
can't read, nothing changes and the error is shown.

Scripts get 5 seconds and 512 MB, then they're stopped.

## Running scripts

- **Plugins › Run Script…** (⌥⌘K) opens a console: type a script, press **Run** (⌘↩).
  `console.log` output appears under the editor.
- **Plugins › Reveal Plugins Folder** opens the folder plugins live in, creating it with a few
  examples the first time:
  - macOS: `~/Library/Application Support/Scrawl/Plugins`
  - Windows: `%APPDATA%\Scrawl\Plugins`
  - Linux: `$XDG_CONFIG_HOME/scrawl/Plugins` (usually `~/.config/scrawl/Plugins`)
- **Plugins › Reload Plugins** picks up new or changed plugins.

## Writing a plugin

A plugin is either:

- a loose `.js` file: one command, named after the file, or after a line near the top like
  `// @name Make It Pop`; or
- a folder with a `plugin.json` naming its commands:

  ```json
  {
    "name": "Examples",
    "commands": [
      { "name": "Rename Layers by Type", "script": "rename-by-type.js" }
    ]
  }
  ```

  These appear in the Plugins menu as "Examples › Rename Layers by Type".

A script's top level runs inside a function, so it can `return` early.

### Running on events

A command can also run by itself when something happens: add `"on": ["save"]` to its entry in
`plugin.json`, or a line `// @on save` near the top of a loose script. The events are:

- `open`: a document was opened;
- `save`: it was saved (changes the script makes are left unsaved);
- `selection`: the selection changed (once the mouse is released, not during a drag).

`scrawl.event` says which event ran the script, or is `null` when it was run from the menu.

### Files and the network

Scripts can't touch files or the network directly; they ask the user through the app:

| | |
|---|---|
| `scrawl.chooseFile(extensions?)` | Asks the user to pick a file (e.g. `["csv"]`). Its path, or `null` if cancelled. |
| `scrawl.readText(path)` | The text of a file picked with `chooseFile()` this run, or of one in the plugin's own folder. Other paths throw. |
| `scrawl.saveText(name, text)` | Asks where to save `text`, suggesting `name`. The path, or `null` if cancelled. |
| `scrawl.fetch(url)` | GETs `url` and returns the body as text. |
| `scrawl.fetchJSON(url)` | `fetch()` parsed as JSON. |

The first time a plugin fetches from a host, Scrawl asks whether to allow it; the answer "yes" is
remembered in `Plugins/permissions.json` (delete a line there to revoke it). Time spent in
dialogs and downloads doesn't count towards the 5-second limit.

## The `scrawl` object

| | |
|---|---|
| `scrawl.document` | The whole document: `pages`, `colors`, `layer_styles`, `text_styles`, `swatches`… |
| `scrawl.pages` | `scrawl.document.pages`. |
| `scrawl.page` | The page shown in the window. |
| `scrawl.selection` | Ids of the selected layers. Assign ids or layers to change the selection. |
| `scrawl.selectedLayers` | The selected layer objects. |
| `scrawl.find(id)` | The layer with `id` on any page, or `null`. |
| `scrawl.parentOf(id)` | The group or artboard holding a layer, or `null` at a page's root. |
| `scrawl.walk(fn, layers?)` | Calls `fn(layer, parent)` for every layer of the page (or of `layers`), depth first. Return `false` to stop. |
| `scrawl.filter(test, layers?)` | The layers for which `test(layer, parent)` is true. |
| `scrawl.create(kind, props?)` | A new layer, not yet inserted: `rectangle`, `oval`, `triangle`, `polygon`, `star`, `line`, `text`, `artboard`, `group`, `hotspot` or `slice`. `props` can set `x`, `y`, `width`, `height`, `text` and any other field, e.g. `name`. |
| `scrawl.insert(layer, parent?)` | Adds a layer on top of `parent`'s children (a layer or id), or of the page. |
| `scrawl.remove(layerOrId)` | Removes a layer and returns it. |
| `scrawl.duplicate(layer)` | A deep copy with fresh ids, not yet inserted. |
| `scrawl.color(hex)` | `"#RRGGBB"` or `"#RRGGBBAA"` as `{ r, g, b, a }` (0–1). |
| `scrawl.fill(hex)` | A solid fill, for `layer.style.fills`. |
| `scrawl.uuid()` | A fresh id. |
| `scrawl.message(text)` | Shown in the window when the script finishes. |
| `scrawl.event` | The event that ran the script (`"open"`, `"save"`, `"selection"`), or `null`. |
| `scrawl.chooseFile()`, `readText()`, `saveText()`, `fetch()` | See [Files and the network](#files-and-the-network). |
| `console.log(...)` | Printed in the script console (and the log). |

## Layers

Layers are the file format's JSON. The fields every layer has:

```js
{
  id, name, visible, locked,
  frame: { x, y, width, height },   // in the parent's coordinates
  rotation,                          // degrees, clockwise
  flip_horizontal, flip_vertical, opacity, blend_mode, is_mask,
  style: { fills, borders, shadows, inner_shadows, border_options },
  flow,                              // prototyping link, or absent
  type,                              // "rectangle", "oval", "text", "group", "artboard", …
}
```

plus what their `type` adds: `children` for groups and artboards, `text` and `style` for text,
`corner_radius` for rectangles, `subpaths` for paths, and so on. The quickest way to learn a
shape is to log one: `console.log(scrawl.selectedLayers[0])`.

Path points are normalized to the frame (0–1), so changing a frame's size scales its path. Group
frames always fit their children; Scrawl refits them after the script runs, as it does text
frames and symbol instances.

Layers copied with `JSON.parse(JSON.stringify(layer))` keep their ids; Scrawl gives any repeated
ids fresh ones, but `scrawl.duplicate` is clearer.

## Example

```js
// @name Number the Artboards
const artboards = scrawl.page.layers.filter((l) => l.type === "artboard");
artboards
  .sort((a, b) => a.frame.y - b.frame.y || a.frame.x - b.frame.x)
  .forEach((ab, i) => (ab.name = `${String(i + 1).padStart(2, "0")} ${ab.name.replace(/^\d+ /, "")}`));
scrawl.message(`Numbered ${artboards.length} artboards`);
```
