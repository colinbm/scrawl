//! Core document model for Scrawl.
//!
//! Everything here is UI-agnostic: the layer tree, geometry, editing operations,
//! hit testing, undo history and the on-disk file format.

pub mod color;
pub mod document;
pub mod geometry;
pub mod hit;
pub mod history;
pub mod io;
pub mod layer;
pub mod ops;
pub mod path;
pub mod sample;
pub mod style;
pub mod text;

pub use color::Color;
pub use document::{Document, Page, PageId};
pub use geometry::Frame;
pub use history::History;
pub use layer::{Artboard, Group, Layer, LayerId, LayerKind, Polygon, Rectangle, Star};
pub use text::{TextAlign, TextLayer, TextSizing, TextStyle, TextTransform, VerticalAlign};
pub use path::{CurvePoint, FillRule, PathShape, PointMode, PointRef, Subpath};
pub use style::{
    BlendMode, Border, BorderOptions, BorderPosition, Fill, FillType, Gradient, GradientKind, GradientStop, LineCap,
    LineJoin, Marker, Paint, Shadow, Style,
};

pub use kurbo;
