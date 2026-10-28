//! Zoom, visible range and cursor (spec §14 "Zoom").

use crate::time::Tick;

pub const MAX_COLS_PER_TICK: u64 = 1024;

#[derive(Clone)]
pub struct View {
    /// `ticks` ticks span `cols` columns; one of the two is always 1.
    pub ticks: u64,
    pub cols: u64,
    /// Tick at the left edge.
    pub start: Tick,
    pub cursor: Tick,
    /// Waveform area width in columns, set by the renderer.
    pub width: u64,
    pub end: Tick,
    /// Refit on width changes until the user zooms, scrolls or moves the cursor.
    pub pending_fit: bool,
    /// Likewise keep `start..` this tick on screen (`show_until`).
    pub pending_to: Option<Tick>,
}

impl View {
    pub fn new(end: Tick, width: u64) -> Self {
        let mut v = View { ticks: 1, cols: 1, start: 0, cursor: 0, width: width.max(1), end, pending_fit: true, pending_to: None };
        v.fit();
        v
    }

    /// First tick of column c: [col_start(c), col_start(c+1)) is the column's tick range.
    /// Below one tick per column only the first column of a tick holds it; the rest are empty.
    pub fn col_start(&self, c: u64) -> Tick {
        self.start + (c * self.ticks).div_ceil(self.cols)
    }

    pub fn col_of(&self, t: Tick) -> Option<u64> {
        let c = t.checked_sub(self.start)? * self.cols / self.ticks;
        (c < self.width).then_some(c)
    }

    pub fn last_visible(&self) -> Tick {
        self.col_start(self.width).saturating_sub(1)
    }

    /// Ticks one column moves the cursor.
    // shortcut: below one tick per column the cursor still moves a whole tick (no sub-tick events exist).
    pub fn step(&self) -> u64 {
        self.ticks
    }

    /// A jump (spec §9): the cursor to `t`, and if that is off screen, the screen centered on it.
    pub fn jump(&mut self, t: Tick) {
        let t = t.min(self.end);
        if self.col_of(t).is_some() {
            return self.set_cursor(t);
        }
        self.center(t);
    }

    /// The cursor to `t`, the screen centered on it even if it was on screen (a jump to a mark).
    pub fn center(&mut self, t: Tick) {
        let t = t.min(self.end);
        self.pending_fit = false;
        self.pending_to = None;
        self.cursor = t;
        self.start = t.saturating_sub(self.width / 2 * self.ticks / self.cols);
    }

    pub fn set_cursor(&mut self, t: Tick) {
        self.pending_fit = false;
        self.pending_to = None;
        self.cursor = t.min(self.end);
        if self.cursor < self.start {
            self.start = self.cursor;
        } else if self.col_of(self.cursor).is_none() {
            self.start = self.cursor - (self.width - 1) * self.ticks / self.cols;
        }
    }

    /// Move the screen `cols` columns right (negative: left), at most to show the end at the left
    /// edge; the cursor stays, even off screen.
    pub fn scroll(&mut self, cols: i64) {
        self.pending_fit = false;
        self.pending_to = None;
        let d = cols.unsigned_abs().saturating_mul(self.ticks) / self.cols;
        self.start = if cols < 0 { self.start.saturating_sub(d) } else { self.start.saturating_add(d).min(self.end.max(self.start)) };
    }

    /// Keep the cursor on its time and on its screen column; near 0 it moves on screen instead.
    fn rezoom(&mut self, ticks: u64, cols: u64) {
        let col = self.col_of(self.cursor).unwrap_or(0);
        self.pending_fit = false;
        self.pending_to = None;
        self.ticks = ticks;
        self.cols = cols;
        self.start = self.cursor.saturating_sub(col * ticks / cols);
    }

    pub fn zoom_in(&mut self) {
        if self.ticks > 1 {
            self.rezoom(self.ticks.div_ceil(2), 1);
        } else if self.cols < MAX_COLS_PER_TICK {
            self.rezoom(1, self.cols * 2);
        }
    }

    /// Stops once the whole simulation fits the width (or, before the width is known, once one
    /// column spans it).
    pub fn zoom_out(&mut self) {
        let shown = if self.cols > 1 { self.width / self.cols } else { self.ticks.saturating_mul(self.width) };
        // as `fit` has it
        if shown >= self.end {
            return;
        }
        if self.cols > 1 {
            self.rezoom(1, self.cols / 2);
        } else if self.ticks < self.end.max(1) {
            self.rezoom(self.ticks * 2, 1);
        }
    }

    /// Set ticks per column, rounding finer zooms to a power-of-two columns per tick.
    pub fn set_zoom(&mut self, ticks_per_col: f64) {
        if ticks_per_col >= 1.0 {
            self.rezoom((ticks_per_col.round() as u64).min(self.end.max(1)), 1);
        } else {
            let cols = (1.0 / ticks_per_col.max(1e-9)).log2().round().exp2() as u64;
            self.rezoom(1, cols.clamp(1, MAX_COLS_PER_TICK));
        }
    }

    /// Show `start..to` across the width, now and again when the width changes (it isn't known
    /// before the first draw), until the user zooms, scrolls or moves the cursor. wtf zooms in
    /// steps, so a little more can show, never less.
    pub fn show_until(&mut self, to: Tick) {
        self.zoom_to_fit(to.saturating_sub(self.start).max(1));
        self.pending_fit = false;
        self.pending_to = Some(to);
    }

    /// The closest zoom at which `span` ticks fit in the width.
    fn zoom_to_fit(&mut self, span: u64) {
        if span >= self.width {
            self.ticks = span.div_ceil(self.width);
            self.cols = 1;
        } else {
            self.ticks = 1;
            self.cols = (self.width / span).clamp(1, MAX_COLS_PER_TICK);
            self.cols = 1 << self.cols.ilog2();
        }
    }

    /// Whole simulation on screen. The cursor keeps its time.
    pub fn fit(&mut self) {
        self.pending_to = None;
        self.zoom_to_fit(self.end.max(1));
        self.start = 0;
    }

    pub fn set_width(&mut self, w: u64) {
        self.width = w.max(1);
        if let Some(to) = self.pending_to {
            self.show_until(to);
        } else if self.pending_fit {
            self.fit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_until_shows_the_whole_range() {
        // zoom goes in steps: what shows may be wider than asked, never narrower; the screen's
        // right edge (where the column after the last one would start) reaches `to`
        for width in [10, 33, 66, 80, 117] {
            for (start, to) in [(20, 60), (0, 7), (100, 101), (5, 990), (0, 80)] {
                let mut v = View::new(1000, width);
                v.start = start;
                v.show_until(to);
                assert_eq!(v.start, start, "width {width}, {start}..{to}");
                assert!(v.last_visible() + 1 >= to, "width {width}, {start}..{to}: shows to {}", v.last_visible());
                // and keeps doing so when the width changes
                v.set_width(width + 7);
                assert!(v.start == start && v.last_visible() + 1 >= to, "width {width} + 7, {start}..{to}");
            }
        }
    }

    #[test]
    fn columns() {
        let mut v = View::new(80, 40);
        assert_eq!((v.ticks, v.cols), (2, 1));
        assert_eq!(v.col_start(3), 6);
        assert_eq!(v.col_of(7), Some(3));
        v.rezoom(1, 4);
        assert_eq!((v.col_start(0), v.col_start(1), v.col_start(4)), (0, 1, 1));
        assert_eq!(v.col_of(1), Some(4));
    }

    #[test]
    fn zoom_keeps_cursor() {
        let mut v = View::new(1000, 100);
        v.set_cursor(500);
        let col = v.col_of(500).unwrap();
        v.zoom_in();
        assert_eq!(v.col_of(500), Some(col));
        v.zoom_out();
        v.zoom_out();
        assert_eq!(v.cursor, 500);
        // near 0 the view can't scroll left, so the cursor moves on screen
        v.set_zoom(1.0);
        v.start = 0;
        v.set_cursor(50);
        v.zoom_out();
        assert_eq!(v.start, 0);
        assert_eq!(v.col_of(50), Some(25));
    }

    #[test]
    fn zoom_limits() {
        let mut v = View::new(10, 20);
        for _ in 0..20 {
            v.zoom_in();
        }
        assert_eq!((v.ticks, v.cols), (1, MAX_COLS_PER_TICK));
        for _ in 0..40 {
            v.zoom_out();
        }
        assert_eq!((v.ticks, v.cols), (1, 2)); // 20 columns show all 10 ticks, as :fit
        let mut v = View::new(1000, 100);
        for _ in 0..40 {
            v.zoom_out();
        }
        v.set_zoom(1.0);
        for _ in 0..40 {
            v.zoom_out();
        }
        assert_eq!(v.ticks, 16); // 8 would show 800 of them
        v.set_zoom(0.3);
        assert_eq!((v.ticks, v.cols), (1, 4));
    }

    #[test]
    fn cursor_scrolls_view() {
        let mut v = View::new(1000, 10);
        v.set_zoom(1.0);
        v.set_cursor(50);
        assert_eq!(v.col_of(50), Some(9));
        v.set_cursor(5);
        assert_eq!(v.start, 5);
        v.set_cursor(5000);
        assert_eq!(v.cursor, 1000);
    }
}
