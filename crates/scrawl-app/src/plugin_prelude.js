// The globals plugin scripts see. `__input`, `__log`, `__make`, `__uuid`, `__chooseFile`,
// `__readText`, `__saveText` and `__fetch` come from the host.
"use strict";

globalThis.console = (() => {
  const show = (v) => {
    if (typeof v === "string") return v;
    try {
      return JSON.stringify(v);
    } catch (e) {
      return String(v);
    }
  };
  const line = (...args) => __log(args.map(show).join(" "));
  return { log: line, info: line, warn: line, error: line };
})();

globalThis.scrawl = (() => {
  const input = JSON.parse(__input);
  const document = JSON.parse(input.document);
  const pageId = input.page;
  let selection = JSON.parse(input.selection);
  const messages = [];

  const childrenOf = (layer) => (Array.isArray(layer.children) ? layer.children : null);

  // Calls fn(layer, parent) depth-first; returning false stops the walk.
  const walk = (layers, fn, parent) => {
    for (const layer of layers) {
      if (fn(layer, parent) === false) return false;
      const kids = childrenOf(layer);
      if (kids && walk(kids, fn, layer) === false) return false;
    }
    return true;
  };

  const find = (id) => {
    let found = null;
    for (const page of document.pages) {
      walk(page.layers, (layer) => {
        if (layer.id === id) {
          found = layer;
          return false;
        }
      });
      if (found) break;
    }
    return found;
  };

  // The child list holding the layer with `id`: a page's layers or a container's children.
  const siblingsOf = (id) => {
    for (const page of document.pages) {
      if (page.layers.some((l) => l.id === id)) return page.layers;
      let list = null;
      walk(page.layers, (layer) => {
        const kids = childrenOf(layer);
        if (kids && kids.some((l) => l.id === id)) {
          list = kids;
          return false;
        }
      });
      if (list) return list;
    }
    return null;
  };

  const idOf = (x) => (typeof x === "string" ? x : x && x.id);

  const api = {
    document,
    get pages() {
      return document.pages;
    },
    /** The page shown in the window. */
    get page() {
      return document.pages.find((p) => p.id === pageId) || document.pages[0];
    },
    /** Ids of the selected layers; assign ids or layers to change it. */
    get selection() {
      return selection;
    },
    set selection(items) {
      selection = Array.from(items, idOf).filter(Boolean);
    },
    get selectedLayers() {
      return selection.map(find).filter(Boolean);
    },
    find,
    /** The layer containing the one with `id`, or null at a page's root. */
    parentOf(id) {
      let parent = null;
      for (const page of document.pages) {
        walk(page.layers, (layer, p) => {
          if (layer.id === idOf(id)) {
            parent = p || null;
            return false;
          }
        });
      }
      return parent;
    },
    /** Visits every layer of the current page (or of `layers`) depth-first. */
    walk(fn, layers) {
      walk(layers || api.page.layers, fn, null);
    },
    /** All layers of the current page matching `test`. */
    filter(test, layers) {
      const out = [];
      walk(layers || api.page.layers, (l, p) => {
        if (test(l, p)) out.push(l);
      });
      return out;
    },
    /**
     * A new layer: rectangle, oval, triangle, polygon, star, line, text, artboard, group,
     * hotspot or slice. `props` may set name, text, x, y, width, height, and any other field.
     */
    create(kind, props = {}) {
      const layer = JSON.parse(props.text === undefined ? __make(kind) : __make(kind, String(props.text)));
      const { x, y, width, height, text, ...rest } = props;
      if (x !== undefined) layer.frame.x = x;
      if (y !== undefined) layer.frame.y = y;
      if (width !== undefined) layer.frame.width = width;
      if (height !== undefined) layer.frame.height = height;
      return Object.assign(layer, rest);
    },
    /** Adds `layer` on top of `parent`'s children (a layer or id), or the current page. */
    insert(layer, parent) {
      const target = parent ? (typeof parent === "string" ? find(parent) : parent) : null;
      const list = target ? childrenOf(target) : api.page.layers;
      if (!list) throw new Error(`${target.name} can't hold other layers`);
      list.push(layer);
      return layer;
    },
    /** Removes the layer with `id` (or the layer itself). Returns it, or null. */
    remove(id) {
      const list = siblingsOf(idOf(id));
      if (!list) return null;
      const i = list.findIndex((l) => l.id === idOf(id));
      selection = selection.filter((s) => s !== idOf(id));
      return list.splice(i, 1)[0];
    },
    /** A deep copy of `layer` with fresh ids, not yet inserted. */
    duplicate(layer) {
      const copy = JSON.parse(JSON.stringify(layer));
      walk([copy], (l) => {
        l.id = __uuid();
      });
      return copy;
    },
    /** `#RRGGBB` or `#RRGGBBAA` as a color. */
    color(hex) {
      const h = hex.replace("#", "");
      const c = (i) => parseInt(h.slice(i, i + 2), 16) / 255;
      return { r: c(0), g: c(2), b: c(4), a: h.length >= 8 ? c(6) : 1 };
    },
    /** A solid fill of `hex`. */
    fill(hex) {
      return {
        enabled: true,
        fill_type: "color",
        color: api.color(hex),
        gradient: { kind: "linear", from: { x: 0.5, y: 0 }, to: { x: 0.5, y: 1 }, stops: [] },
        blend_mode: "normal",
      };
    },
    uuid: () => __uuid(),
    /** What ran the script: "open", "save", "selection", or null from a menu or the console. */
    event: input.event ?? null,
    /** Asks the user for a file, e.g. `chooseFile(["csv", "txt"])`. Its path, or null if cancelled. */
    chooseFile(extensions = []) {
      return __chooseFile(JSON.stringify(extensions)) ?? null;
    },
    /** The text of a file chosen with chooseFile(), or one in the plugin's own folder. */
    readText(path) {
      return __readText(String(path));
    },
    /** Asks where to save `text`, suggesting `name`. The path saved to, or null if cancelled. */
    saveText(name, text) {
      return __saveText(String(name), String(text)) ?? null;
    },
    /** GETs `url` as text. The first request to each host asks the user's permission. */
    fetch(url) {
      return __fetch(String(url));
    },
    /** fetch() parsed as JSON. */
    fetchJSON(url) {
      return JSON.parse(__fetch(String(url)));
    },
    /** Shows `text` in the window when the script finishes. */
    message(text) {
      messages.push(String(text));
    },
  };

  globalThis.__result = () => JSON.stringify({ document, selection, messages });
  return api;
})();
