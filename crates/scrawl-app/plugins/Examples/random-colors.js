// Gives each selected layer a random solid fill from a small palette.
const palette = ["#1F8FFF", "#7C5CFF", "#FF8A3D", "#2EC4B6", "#FFD23F", "#FF3D7F"];
const layers = scrawl.selectedLayers.filter((l) => l.style && l.type !== "artboard" && l.type !== "group");
if (!layers.length) {
  scrawl.message("Select some shapes first");
  return;
}
for (const layer of layers) {
  const hex = palette[Math.floor(Math.random() * palette.length)];
  layer.style.fills = [scrawl.fill(hex)];
}
scrawl.message(`Recolored ${layers.length} layer${layers.length === 1 ? "" : "s"}`);
