//! Where each desktop tile, app cell and widget goes. Sizes are logical pixels (before Windows
//! display scaling). App cells are relative to their tile; tiles and widgets to the panel.

pub const PAD: f32 = 14.0; // around the panel edge
pub const GAP: f32 = 10.0; // between tiles
const HEAD: f32 = 34.0; // number pill row
const INSET: f32 = 10.0; // inside a tile
const HINT: f32 = 30.0; // bottom line (search text, edit button)
const PLUS_W: f32 = 64.0; // "+" tile in a row
const PLUS_H: f32 = 44.0; // "+" tile in a column
/// Toolbar shown at the top of the panel in edit mode.
pub const EDIT_BAR: f32 = 46.0;
/// Room for the search capsule at the top while searching.
pub const SEARCH_BAR: f32 = 42.0;

// Icons view
const CELL: f32 = 52.0;
const ICON_CELL: f32 = 46.0;

// Live previews view (at 100% size; shrunk when the panel would be too big)
const SLOT_W: f32 = 184.0;
const SLOT_H: f32 = 128.0;
const SLOT_GAP: f32 = 8.0;
/// Up to 6 × 6 = 36 previews per desktop; past that the last slot shows "+N".
const MAX_COLS: usize = 6;
/// Up to this many columns the previews keep their full size; with more they shrink so the
/// tile stays about as wide as a 3-column one.
const FULL_SIZE_COLS: usize = 3;
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
    fn shifted(mut self, dx: f32, dy: f32) -> Rect {
        self.x += dx;
        self.y += dy;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Icons,
    Previews,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Orient {
    /// Desktops side by side, widgets in a row under them
    Horizontal,
    /// Desktops stacked in a column, widgets in a column beside them
    Vertical,
}

/// The widgets, in the order they're shown. The number is what the UI uses (WidgetSlot.kind).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Widget {
    Notes = 0,
    Music = 1,
    Timer = 2,
    Weather = 3,
    Calendar = 4,
}

impl Widget {
    pub const ALL: [Widget; 5] = [Widget::Notes, Widget::Music, Widget::Timer, Widget::Weather, Widget::Calendar];

    pub fn key(self) -> &'static str {
        match self {
            Widget::Notes => "notes",
            Widget::Music => "music",
            Widget::Timer => "timer",
            Widget::Weather => "weather",
            Widget::Calendar => "calendar",
        }
    }

    pub fn from_key(k: &str) -> Option<Widget> {
        Widget::ALL.into_iter().find(|w| w.key().eq_ignore_ascii_case(k.trim()))
    }
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
    pub widgets: Vec<(Widget, Rect)>,
    pub width: f32,
    pub height: f32,
}

pub struct Options<'a> {
    /// `apps[i]` = number of apps on desktop i
    pub apps: &'a [usize],
    pub per_row: usize,
    pub mode: Mode,
    pub orient: Orient,
    pub widgets: &'a [Widget],
    /// Widgets before the desktops (above them, or to their left)
    pub widgets_first: bool,
    /// Room the panel may take (the screen minus a margin)
    pub max_w: f32,
    pub max_h: f32,
    /// Extra room at the top (the edit-mode toolbar)
    pub top: f32,
    /// Widget card size
    pub widget_w: f32,
    pub widget_h: f32,
}

pub fn compute(o: &Options) -> Layout {
    let scales: &[f32] = match o.mode {
        Mode::Icons => &[1.0],
        Mode::Previews => &[1.0, 0.85, 0.72, 0.6],
    };
    let per_row = o.per_row.clamp(1, 10);
    // Try the requested number per row at full size, then smaller previews, then fewer per row
    let mut last = None;
    for row in (1..=per_row).rev() {
        for &s in scales {
            let l = arrange(o, row, s);
            if l.width <= o.max_w && l.height <= o.max_h {
                return l;
            }
            last = Some(l);
        }
        if o.orient == Orient::Vertical {
            break; // per_row doesn't apply to a column
        }
    }
    last.unwrap_or_else(|| arrange(o, 1, 0.6))
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
            let max_slots = MAX_COLS * MAX_COLS;
            let shown = if n > max_slots { max_slots - 1 } else { n };
            let more = n - shown;
            let slots = (shown + usize::from(more > 0)).max(1);
            // Smallest square-ish grid that fits: 1, 2×2, 3×3, 4×4, 5×5, 6×6
            let cols = match slots {
                1 => 1,
                2..=4 => 2,
                _ => ((slots as f32).sqrt().ceil() as usize).clamp(3, MAX_COLS),
            };
            let rows = slots.div_ceil(cols);
            let shrink = if cols > FULL_SIZE_COLS { FULL_SIZE_COLS as f32 / cols as f32 } else { 1.0 };
            let g = if cols > 4 { SLOT_GAP * 0.75 } else { SLOT_GAP };
            let (sw, sh) = (SLOT_W * scale * shrink, (SLOT_H * scale * shrink).max(PREVIEW_HEADER + 34.0));
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

/// Desktops (+ the "+" tile) placed from (0, 0). Returns them and the block's size.
fn desk_block(o: &Options, per_row: usize, scale: f32) -> (Vec<Desk>, Rect, f32, f32) {
    let apps = o.apps;
    // Icons view: wider tiles once a desktop gets crowded (same width for all, so they line up)
    let most = apps.iter().copied().max().unwrap_or(0);
    let icon_cols = match most {
        0..=16 => 4,
        17..=30 => 5,
        _ => 6,
    };
    let tiles: Vec<_> = apps.iter().map(|&n| tile(n, o.mode, scale, icon_cols)).collect();
    let mut desks = Vec::with_capacity(apps.len());
    let mut plus = Rect::default();

    match o.orient {
        Orient::Horizontal => {
            let per_row = per_row.min(apps.len().max(1));
            let (mut y, mut width) = (0.0f32, PLUS_W);
            for row_start in (0..apps.len()).step_by(per_row) {
                let row_end = (row_start + per_row).min(apps.len());
                let row_h = tiles[row_start..row_end].iter().map(|t| t.1).fold(0.0, f32::max);
                let mut x = 0.0;
                for t in &tiles[row_start..row_end] {
                    let (w, _, cells, more, more_cell) = t.clone();
                    desks.push(Desk { rect: Rect { x, y, w, h: row_h }, cells, more, more_cell });
                    x += w + GAP;
                }
                if row_end == apps.len() {
                    // The narrow "+" tile goes at the end of the last row
                    plus = Rect { x, y, w: PLUS_W, h: row_h };
                    x += PLUS_W + GAP;
                }
                width = width.max(x - GAP);
                y += row_h + GAP;
            }
            if apps.is_empty() {
                plus = Rect { x: 0.0, y: 0.0, w: PLUS_W, h: 80.0 };
                y = 80.0 + GAP;
            }
            (desks, plus, width, y - GAP)
        }
        Orient::Vertical => {
            // Top to bottom; start another column when the screen runs out
            let limit = (o.max_h - PAD * 2.0 - HINT - o.top).max(200.0);
            let (mut x, mut y, mut col_w, mut height) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            let mut column: Vec<usize> = Vec::new();
            let finish = |column: &mut Vec<usize>, desks: &mut Vec<Desk>, col_w: f32| {
                for &i in column.iter() {
                    desks[i].rect.w = col_w; // same width down a column
                }
                column.clear();
            };
            for t in &tiles {
                let (w, h, cells, more, more_cell) = t.clone();
                if y > 0.0 && y + h > limit {
                    finish(&mut column, &mut desks, col_w);
                    x += col_w + GAP;
                    y = 0.0;
                    col_w = 0.0;
                }
                column.push(desks.len());
                desks.push(Desk { rect: Rect { x, y, w, h }, cells, more, more_cell });
                col_w = col_w.max(w);
                y += h + GAP;
                height = height.max(y - GAP);
            }
            if desks.is_empty() {
                col_w = tiles.first().map_or(200.0, |t| t.0).max(200.0);
            }
            if y > 0.0 && y + PLUS_H > limit {
                finish(&mut column, &mut desks, col_w);
                x += col_w + GAP;
                y = 0.0;
            }
            col_w = col_w.max(160.0);
            finish(&mut column, &mut desks, col_w);
            plus = Rect { x, y, w: col_w, h: PLUS_H };
            height = height.max(y + PLUS_H);
            (desks, plus, x + col_w, height)
        }
    }
}

fn arrange(o: &Options, per_row: usize, scale: f32) -> Layout {
    let (mut desks, mut plus, dw, dh) = desk_block(o, per_row, scale);

    // Widgets: a row (horizontal) or a column (vertical), wrapping if they don't fit
    let mut widgets: Vec<(Widget, Rect)> = Vec::new();
    let (mut ww, mut wh) = (0.0f32, 0.0f32);
    if !o.widgets.is_empty() {
        let (room_w, room_h) = (o.max_w - PAD * 2.0, o.max_h - PAD * 2.0 - HINT - o.top);
        let (widget_w, widget_h) = (o.widget_w, o.widget_h);
        let (mut x, mut y) = (0.0f32, 0.0f32);
        for &kind in o.widgets {
            match o.orient {
                Orient::Horizontal => {
                    if x > 0.0 && x + widget_w > room_w.max(dw) {
                        x = 0.0;
                        y += widget_h + GAP;
                    }
                }
                Orient::Vertical => {
                    if y > 0.0 && y + widget_h > room_h.max(dh) {
                        y = 0.0;
                        x += widget_w + GAP;
                    }
                }
            }
            widgets.push((kind, Rect { x, y, w: widget_w, h: widget_h }));
            ww = ww.max(x + widget_w);
            wh = wh.max(y + widget_h);
            match o.orient {
                Orient::Horizontal => x += widget_w + GAP,
                Orient::Vertical => y += widget_h + GAP,
            }
        }
    }

    // Put the two blocks together
    let has_w = !widgets.is_empty();
    let (desk_off, widget_off) = match (o.orient, o.widgets_first && has_w) {
        (Orient::Horizontal, false) => ((0.0, 0.0), (0.0, dh + GAP)),
        (Orient::Horizontal, true) => ((0.0, wh + GAP), (0.0, 0.0)),
        (Orient::Vertical, false) => ((0.0, 0.0), (dw + GAP, 0.0)),
        (Orient::Vertical, true) => ((ww + GAP, 0.0), (0.0, 0.0)),
    };
    let (ox, oy) = (PAD, PAD + o.top);
    for d in &mut desks {
        d.rect = d.rect.shifted(ox + desk_off.0, oy + desk_off.1);
    }
    plus = plus.shifted(ox + desk_off.0, oy + desk_off.1);
    for (_, r) in &mut widgets {
        *r = r.shifted(ox + widget_off.0, oy + widget_off.1);
    }

    let (cw, ch) = match (o.orient, has_w) {
        (_, false) => (dw, dh),
        (Orient::Horizontal, true) => (dw.max(ww), dh + GAP + wh),
        (Orient::Vertical, true) => (dw + GAP + ww, dh.max(wh)),
    };
    // Room for the edit toolbar's buttons even on a tiny panel
    let min_w = if o.top > 0.0 { 300.0 } else { 0.0 };
    Layout {
        desks,
        plus,
        widgets,
        width: (PAD * 2.0 + cw).max(min_w),
        height: PAD * 2.0 + o.top + ch + HINT - PAD,
    }
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
