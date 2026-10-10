// Repeats the selected layer in a 4 × 3 grid, spaced by its size plus 16.
const [layer] = scrawl.selectedLayers;
if (!layer) {
  scrawl.message("Select a layer to repeat");
  return;
}
const parent = scrawl.parentOf(layer.id);
const gap = 16;
const copies = [];
for (let row = 0; row < 3; row++) {
  for (let col = 0; col < 4; col++) {
    if (row === 0 && col === 0) continue;
    const copy = scrawl.duplicate(layer);
    copy.frame.x += col * (layer.frame.width + gap);
    copy.frame.y += row * (layer.frame.height + gap);
    scrawl.insert(copy, parent);
    copies.push(copy);
  }
}
scrawl.selection = [layer, ...copies];
scrawl.message(`Made ${copies.length} copies`);
