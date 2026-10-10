// Names each selected layer (or every layer on the page) after its type: "Rectangle 1",
// "Rectangle 2", "Text 1"…
const layers = scrawl.selection.length ? scrawl.selectedLayers : scrawl.filter(() => true);
const counts = {};
for (const layer of layers) {
  const type = layer.type[0].toUpperCase() + layer.type.slice(1);
  counts[type] = (counts[type] || 0) + 1;
  layer.name = `${type} ${counts[type]}`;
}
scrawl.message(`Renamed ${layers.length} layer${layers.length === 1 ? "" : "s"}`);
