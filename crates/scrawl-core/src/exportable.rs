//! Export presets ("Make Exportable") and what exporting a layer covers.

use kurbo::{Affine, Rect};
use serde::{Deserialize, Serialize};

use crate::{Layer, LayerId, LayerKind, Page, ops};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileFormat {
    #[default]
    Png,
    Jpg,
    Webp,
    Svg,
    Pdf,
}

impl FileFormat {
    pub const ALL: [Self; 5] = [Self::Png, Self::Jpg, Self::Webp, Self::Svg, Self::Pdf];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpg => "JPG",
            Self::Webp => "WEBP",
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
        }
    }

    /// Vector formats ignore the scale's effect on resolution (they only change the size).
    pub fn is_vector(self) -> bool {
        matches!(self, Self::Svg | Self::Pdf)
    }
}

/// One file to write when a layer is exported.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportFormat {
    pub scale: f64,
    /// Appended to the layer name, e.g. "@2x".
    #[serde(default)]
    pub suffix: String,
    pub format: FileFormat,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self { scale: 1.0, suffix: String::new(), format: FileFormat::Png }
    }
}

impl ExportFormat {
    /// The usual next preset after `previous`: the next scale up, with a matching suffix.
    pub fn after(previous: Option<&ExportFormat>) -> Self {
        let Some(p) = previous else { return Self::default() };
        let scale = p.scale.floor() + 1.0;
        Self { scale, suffix: format!("@{scale}x"), format: p.format }
    }

    /// The file name for a layer called `name`, made safe for file systems.
    pub fn file_name(&self, name: &str) -> String {
        let safe: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c }).collect();
        let safe = safe.trim();
        let base = if safe.is_empty() { "Untitled" } else { safe };
        format!("{base}{}.{}", self.suffix, self.format.extension())
    }
}

/// The furthest a layer or anything in it paints outside its geometry (borders, shadows).
pub fn paint_outset(layer: &Layer) -> f64 {
    let own = layer.style.paint_outset();
    let children = match &layer.kind {
        LayerKind::Symbol(s) => Some(&s.expanded),
        _ => layer.children(),
    };
    children.map_or(own, |c| c.iter().map(paint_outset).fold(own, f64::max))
}

/// What exporting layer `id` renders, as a page in page space, and the area of it (page space)
/// to capture. Artboards export their frame; slices everything under them; other layers export
/// alone, with room for their borders and shadows.
pub fn export_region(page: &Page, id: LayerId) -> Option<(Page, Rect)> {
    let layer = page.find(id)?;
    let mut out = Page::new(&page.name);
    let area = match &layer.kind {
        LayerKind::Slice => {
            out.layers = page.layers.clone();
            page.world_bounds(id)?
        }
        LayerKind::Artboard(_) => {
            out.layers = vec![layer.clone()];
            layer.frame.to_rect()
        }
        _ => {
            let copy = ops::page_space_copy(page, id)?;
            let pad = paint_outset(&copy);
            let bounds = copy.bounds_in(Affine::IDENTITY)?.inflate(pad, pad);
            out.layers = vec![copy];
            bounds
        }
    };
    (area.width() > 0.0 && area.height() > 0.0).then_some((out, area))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Shadow};

    #[test]
    fn file_names_and_presets() {
        let two = ExportFormat::after(Some(&ExportFormat::default()));
        assert_eq!((two.scale, two.suffix.as_str()), (2.0, "@2x"));
        assert_eq!(two.file_name("Icons/Star"), "Icons-Star@2x.png");
        assert_eq!(ExportFormat { format: FileFormat::Svg, ..Default::default() }.file_name(" "), "Untitled.svg");
    }

    #[test]
    fn regions_cover_shadows_and_slices_take_everything() {
        let mut page = Page::new("P");
        let ab = ops::insert_drawn(&mut page, Layer::artboard("A", Frame::new(100.0, 100.0, 200.0, 200.0)));
        let mut r = Layer::rectangle(Frame::new(110.0, 110.0, 20.0, 20.0));
        r.style.borders.clear();
        r.style.shadows = vec![Shadow { offset_x: 0.0, offset_y: 0.0, blur: 2.0, spread: 0.0, ..Shadow::drop() }];
        let r = ops::insert_drawn(&mut page, r);
        let slice = ops::insert_drawn(&mut page, Layer::new("S", Frame::new(0.0, 0.0, 50.0, 50.0), LayerKind::Slice));

        let (p, area) = export_region(&page, r).unwrap();
        assert_eq!(p.layers.len(), 1);
        assert_eq!(area, Rect::new(107.0, 107.0, 133.0, 133.0));
        assert_eq!(export_region(&page, ab).unwrap().1, Rect::new(100.0, 100.0, 300.0, 300.0));
        let (p, area) = export_region(&page, slice).unwrap();
        assert_eq!(p.layers.len(), page.layers.len());
        assert_eq!(area, Rect::new(0.0, 0.0, 50.0, 50.0));
    }
}
