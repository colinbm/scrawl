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
pub mod style;

pub use color::Color;
pub use document::{Document, Page, PageId};
pub use geometry::Frame;
pub use history::History;
pub use layer::{Artboard, CurvePoint, Group, Layer, LayerId, LayerKind, PathShape, Rectangle};
pub use style::{Border, BorderPosition, Fill, Style};

pub use kurbo;
