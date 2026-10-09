//! Ruler guides and grids. Artboards carry their own guides and grid settings (in artboard
//! space); the page has guides for everything outside artboards.

use serde::{Deserialize, Serialize};

use crate::snap::Axis;

/// A ruler guide: a vertical line at x (`Axis::X`) or a horizontal line at y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub axis: Axis,
    pub pos: f64,
}

/// A square grid, shown with View > Show Grid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SquareGrid {
    pub size: f64,
    /// Every nth line is drawn darker; 0 for none.
    pub thick_every: u32,
}

impl Default for SquareGrid {
    fn default() -> Self {
        Self { size: 8.0, thick_every: 10 }
    }
}

impl SquareGrid {
    /// Line positions from 0 to `extent` (exclusive of the edges), with whether each is thick.
    pub fn lines(&self, extent: f64) -> impl Iterator<Item = (f64, bool)> + '_ {
        let n = if self.size > 0.0 { (extent / self.size).ceil() as u32 } else { 0 };
        (1..n).map(move |i| (i as f64 * self.size, self.thick_every > 0 && i % self.thick_every == 0))
    }
}

/// Columns stretched across the container between equal margins.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Columns {
    pub count: u32,
    pub gutter: f64,
    pub margin: f64,
}

impl Default for Columns {
    fn default() -> Self {
        Self { count: 12, gutter: 16.0, margin: 16.0 }
    }
}

impl Columns {
    /// Each column's extent, from the container's left edge.
    pub fn spans(&self, width: f64) -> Vec<(f64, f64)> {
        if self.count == 0 {
            return vec![];
        }
        let n = self.count as f64;
        let col = ((width - 2.0 * self.margin - (n - 1.0) * self.gutter) / n).max(0.0);
        (0..self.count).map(|i| self.margin + i as f64 * (col + self.gutter)).map(|x| (x, x + col)).collect()
    }
}

/// Fixed-height rows repeating down the container.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rows {
    pub height: f64,
    pub gutter: f64,
}

impl Default for Rows {
    fn default() -> Self {
        Self { height: 24.0, gutter: 8.0 }
    }
}

impl Rows {
    /// Each row's extent from the container's top; the last may be cut short.
    pub fn spans(&self, height: f64) -> Vec<(f64, f64)> {
        let step = self.height + self.gutter;
        if self.height <= 0.0 || step <= 0.0 {
            return vec![];
        }
        let mut out = vec![];
        let mut y = 0.0;
        while y < height {
            out.push((y, (y + self.height).min(height)));
            y += step;
        }
        out
    }
}

/// An artboard's layout grid, shown with View > Show Layout.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutGrid {
    pub columns: Option<Columns>,
    pub rows: Option<Rows>,
}

impl Default for LayoutGrid {
    fn default() -> Self {
        Self { columns: Some(Columns::default()), rows: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fill_between_margins() {
        let c = Columns { count: 4, gutter: 20.0, margin: 20.0 };
        assert_eq!(c.spans(400.0), vec![(20.0, 95.0), (115.0, 190.0), (210.0, 285.0), (305.0, 380.0)]);
        assert!(Columns { count: 0, ..c }.spans(400.0).is_empty());
    }

    #[test]
    fn rows_repeat_and_clip() {
        let r = Rows { height: 24.0, gutter: 8.0 };
        assert_eq!(r.spans(100.0), vec![(0.0, 24.0), (32.0, 56.0), (64.0, 88.0), (96.0, 100.0)]);
    }

    #[test]
    fn grid_marks_thick_lines() {
        let g = SquareGrid { size: 10.0, thick_every: 2 };
        let lines: Vec<_> = g.lines(45.0).collect();
        assert_eq!(lines, vec![(10.0, false), (20.0, true), (30.0, false), (40.0, true)]);
    }
}
