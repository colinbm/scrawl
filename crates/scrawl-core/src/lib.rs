//! Core document model for Scrawl.
//!
//! Everything here is UI-agnostic: the layer tree, geometry, editing operations,
//! hit testing, undo history and the on-disk file format.

pub mod assets;
pub mod color;
pub mod components;
pub mod constraints;
pub mod css;
pub mod document;
pub mod exportable;
pub mod geometry;
pub mod grid;
pub mod hit;
pub mod history;
pub mod io;
pub mod layout;
pub mod layer;
pub mod library;
pub mod ops;
pub mod path;
pub mod sample;
pub mod snap;
pub mod style;
pub mod symbols;
pub mod text;

pub use color::Color;
pub use components::{SharedStyle, StyleId, Swatch, SwatchId};
pub use symbols::{Override, OverrideValue, SymbolId, SymbolInstance};
pub use document::{Document, Page, PageId};
pub use geometry::Frame;
pub use history::History;
pub use layer::{Artboard, Group, ImageLayer, Layer, LayerId, LayerKind, Polygon, Rectangle, Star};
pub use text::{TextAlign, TextLayer, TextSizing, TextStyle, TextTransform, VerticalAlign};
pub use path::{CurvePoint, FillRule, PathShape, PointMode, PointRef, Subpath};
pub use style::{
    BlendMode, Border, BorderOptions, BorderPosition, Fill, FillType, Gradient, GradientKind, GradientStop, ImageMode, LineCap,
    LineJoin, Marker, Paint, Shadow, Style,
};

pub use kurbo;
