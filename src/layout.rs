//! Where each desktop tile and app cell goes. Sizes are logical pixels (before Windows display
//! scaling). App cells are relative to their tile; tiles are relative to the panel.

pub const PAD: f32 = 14.0; // around the panel edge
pub const GAP: f32 = 10.0; // between tiles
const HEAD: f32 = 34.0; // number pill row
const INSET: f32 = 10.0; // inside a tile
const HINT: f32 = 32.0; // bottom line
const PLUS_W: f32 = 64.0;

// Icons view
const CELL: f32 = 52.0;
const ICON_CELL: f32 = 46.0;

// Live previews view (at 100% size; shrunk when the panel would be too wide)
const SLOT_W: f32 = 184.0;
const SLOT_H: f32 = 128.0;
const SLOT_GAP: f32 = 8.0;
const MAX_COLS: usize = 3;
const MAX_ROWS: usize = 3;
/// Title strip at the top of each preview cell (the live preview goes below it).
pub const PREVIEW_HEADER: f32 = 22.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Icons,
    Previews,
}

pub struct Desk {
    pub rect: Rect,
    /// One per shown app, in order (in previews view at most 9; the rest are counted in `more`).
    pub cells: Vec<Rect>,
    pub more: usize,
    pub more_cell: Rect,
}

pub struct Layout {
    pub desks: Vec<Desk>,
    pub plus: Rect,
    pub width: f32,
    pub height: f32,
}

/// `apps[i]` = number of apps on desktop i. `max_width` = screen width, so the panel always fits.
pub fn compute(apps: &[usize], per_row: usize, mode: Mode, max_width: f32) -> Layout {
    let per_row = per_row.clamp(1, 10);
    let scales: &[f32] = match mode {
        Mode::Icons => &[1.0],
        Mode::Previews => &[1.0, 0.85, 0.72, 0.6],
    };
    // Try the requested number per row at full size, then smaller previews, then fewer per row
    let mut last = None;
    for row in (1..=per_row).rev() {
        for &s in scales {
            let l = arrange(apps, row, mode, s);
            if l.width <= max_width {
                return l;
            }
            last = Some(l);
        }
    }
    last.unwrap_or_else(|| arrange(apps, 1, mode, 0.6))
}

/// A desktop tile's size and its app cells (relative to the tile).
fn tile(n: usize, mode: Mode, scale: f32, icon_cols: usize) -> (f32, f32, Vec<Rect>, usize, Rect) {
    match mode {
        Mode::Icons => {
            let cols = icon_cols;
            let w = INSET * 2.0 + cols as f32 * CELL - 6.0;
            let h = HEAD + n.div_ceil(cols).max(1) as f32 * CELL + 6.0;
            let cells = (0..n)
                .map(|i| Rect {
                    x: INSET + (i % cols) as f32 * CELL,
                    y: HEAD + (i / cols) as f32 * CELL,
                    w: ICON_CELL,
                    h: ICON_CELL,
                })
                .collect();
            (w, h, cells, 0, Rect::default())
        }
        Mode::Previews => {
            let max_slots = MAX_COLS * MAX_ROWS;
            let shown = if n > max_slots { max_slots - 1 } else { n };
            let more = n - shown;
            let slots = (shown + usize::from(more > 0)).max(1);
            let cols = match slots {
                1 => 1,
                2..=4 => 2,
                _ => 3,
            };
            let rows = slots.div_ceil(cols);
            let (sw, sh, g) = (SLOT_W * scale, SLOT_H * scale, SLOT_GAP);
            let at = |i: usize| Rect {
                x: INSET + (i % cols) as f32 * (sw + g),
                y: HEAD + (i / cols) as f32 * (sh + g),
                w: sw,
                h: sh,
            };
            let cells = (0..shown).map(at).collect();
            let more_cell = if more > 0 { at(shown) } else { Rect::default() };
            let w = INSET * 2.0 + cols as f32 * sw + (cols - 1) as f32 * g;
            let h = HEAD + rows as f32 * sh + (rows - 1) as f32 * g + INSET;
            (w, h, cells, more, more_cell)
        }
    }
}

fn arrange(apps: &[usize], per_row: usize, mode: Mode, scale: f32) -> Layout {
    // Icons view: wider tiles once a desktop gets crowded (same width for all, so they line up)
    let most = apps.iter().copied().max().unwrap_or(0);
    let icon_cols = match most {
        0..=16 => 4,
        17..=30 => 5,
        _ => 6,
    };

    let per_row = per_row.min(apps.len().max(1));
    let tiles: Vec<_> = apps.iter().map(|&n| tile(n, mode, scale, icon_cols)).collect();
    let mut desks = Vec::with_capacity(apps.len());
    let mut plus = Rect::default();
    let mut y = PAD;
    let mut width: f32 = PAD * 2.0 + PLUS_W;

    for row_start in (0..apps.len()).step_by(per_row) {
        let row_end = (row_start + per_row).min(apps.len());
        let row_h = tiles[row_start..row_end].iter().map(|t| t.1).fold(0.0, f32::max);
        let mut x = PAD;
        for t in &tiles[row_start..row_end] {
            let (w, _, cells, more, more_cell) = t.clone();
            desks.push(Desk { rect: Rect { x, y, w, h: row_h }, cells, more, more_cell });
            x += w + GAP;
        }
        // The narrow "+" tile goes at the end of the last row
        if row_end == apps.len() {
            plus = Rect { x, y, w: PLUS_W, h: row_h };
            x += PLUS_W + GAP;
        }
        width = width.max(x - GAP + PAD);
        y += row_h + GAP;
    }
    if apps.is_empty() {
        plus = Rect { x: PAD, y: PAD, w: PLUS_W, h: 80.0 };
        y += 80.0 + GAP;
    }

    Layout { desks, plus, width, height: y - GAP + HINT }
}

// ===================== snap layouts =====================

/// Zones as fractions of the screen (x, y, w, h). The dragged app takes the zone you pick;
/// the app you were using before takes the first other zone.
pub const SNAP_LAYOUTS: &[&[(f32, f32, f32, f32)]] = &[
    &[(0.0, 0.0, 0.5, 1.0), (0.5, 0.0, 0.5, 1.0)],
    &[(0.0, 0.0, 2.0 / 3.0, 1.0), (2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0)],
    &[(0.0, 0.0, 0.5, 1.0), (0.5, 0.0, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)],
    &[(0.0, 0.0, 0.5, 0.5), (0.5, 0.0, 0.5, 0.5), (0.0, 0.5, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)],
    &[(0.0, 0.0, 1.0 / 3.0, 1.0), (1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0), (2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0)],
    &[(0.0, 0.0, 0.25, 1.0), (0.25, 0.0, 0.5, 1.0), (0.75, 0.0, 0.25, 1.0)],
    &[(0.0, 0.0, 0.5, 0.5), (0.0, 0.5, 0.5, 0.5), (0.5, 0.0, 0.5, 1.0)],
    &[(0.0, 0.0, 1.0, 0.5), (0.0, 0.5, 1.0, 0.5)],
    &[(0.0, 0.0, 1.0, 1.0)], // full screen
];

/// Which zone the previous app takes: the first one that isn't the chosen one.
pub fn partner_zone(zones: usize, chosen: usize) -> Option<usize> {
    match (zones, chosen) {
        (0..=1, _) => None,
        (_, 0) => Some(1),
        _ => Some(0),
    }
}
