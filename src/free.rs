//! Free layout: widgets sit in capsules (one or more widgets that move together), each anywhere on
//! screen. A capsule is a small window owned by the panel: it isn't a separate app, has no taskbar
//! button, opens and closes with the panel and always stays in front of it.
//!
//! In edit mode each capsule gets a handle bar: drag it to move, "Join" merges it with the nearest
//! capsule, "Split" breaks it up, "Stack" / "Side by side" turns it.

use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, Timer, VecModel};

use crate::config::WidgetGroup;
use crate::layout::{Widget, GAP};
use crate::{overview, widget_ui, win, Shared, WidgetWindow};

pub struct Capsule {
    pub group: WidgetGroup,
    pub window: WidgetWindow,
    pub hwnd: isize,
}

// Must match WidgetWindow in ui/app.slint
const PAD: f32 = 8.0;
const INNER_GAP: f32 = 8.0;
const HEAD: f32 = 36.0;

/// A capsule's size in logical pixels.
fn size(n: usize, vertical: bool, editing: bool, card: (f32, f32)) -> (f32, f32) {
    let n = n.max(1) as f32;
    let (w, h) = if vertical {
        (card.0, n * card.1 + (n - 1.0) * INNER_GAP)
    } else {
        (n * card.0 + (n - 1.0) * INNER_GAP, card.1)
    };
    (w + PAD * 2.0, h + PAD * 2.0 + if editing { HEAD } else { 0.0 })
}

fn kinds(g: &WidgetGroup) -> Vec<i32> {
    g.widgets.iter().filter_map(|k| Widget::from_key(k)).map(|w| w as i32).collect()
}

/// Close every capsule window.
fn drop_all(app: &Shared) {
    let old: Vec<Capsule> = app.borrow_mut().widget_windows.drain(..).collect();
    for c in old {
        let _ = c.window.hide();
    }
}

/// Make one window per capsule, to match the settings (none unless the layout is Free).
fn build(app: &Shared) {
    drop_all(app);
    let groups = {
        let mut a = app.borrow_mut();
        if !a.config.free() {
            return;
        }
        a.config.groups()
    };
    let card = app.borrow().config.widget_size_px();
    for (index, group) in groups.into_iter().enumerate() {
        let Ok(window) = WidgetWindow::new() else { continue };
        window.set_kinds(ModelRc::new(VecModel::from(kinds(&group))));
        window.set_vertical(group.vertical);
        window.set_card_w(card.0);
        window.set_card_h(card.1);

        let weak = Rc::downgrade(app);
        window.on_edit_drag(move |step| {
            if let Some(app) = weak.upgrade() {
                overview::drag_window(&app, Some(index), step);
            }
        });
        let weak = Rc::downgrade(app);
        window.on_join(move || {
            if let Some(app) = weak.upgrade() {
                join(&app, index);
            }
        });
        let weak = Rc::downgrade(app);
        window.on_split(move || {
            if let Some(app) = weak.upgrade() {
                split(&app, index);
            }
        });
        let weak = Rc::downgrade(app);
        window.on_rotate(move || {
            if let Some(app) = weak.upgrade() {
                rotate(&app, index);
            }
        });
        // Alt+F4 on a capsule just hides it, like the panel
        window.window().on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        widget_ui::wire_window(app, &window);
        app.borrow_mut().widget_windows.push(Capsule { group, window, hwnd: 0 });
    }
}

/// Show the capsules (making their windows only when something changed). Unplaced ones line up
/// under the panel.
pub fn show(app: &Shared) {
    let (wanted, card) = {
        let mut a = app.borrow_mut();
        let wanted = if a.config.free() { a.config.groups() } else { Vec::new() };
        (wanted, a.config.widget_size_px())
    };
    let same = {
        let a = app.borrow();
        a.widget_windows.len() == wanted.len()
            && a.widget_windows.iter().zip(&wanted).all(|(c, g)| {
                c.group.widgets == g.widgets
                    && c.group.vertical == g.vertical
                    && (c.window.get_card_w(), c.window.get_card_h()) == card
            })
    };
    if same {
        // Same capsules: just take the latest positions
        let mut a = app.borrow_mut();
        for (c, g) in a.widget_windows.iter_mut().zip(wanted) {
            c.group = g;
        }
    } else {
        build(app);
    }
    place_and_show(app);
}

fn place_and_show(app: &Shared) {
    if app.borrow().widget_windows.is_empty() {
        return;
    }
    widget_ui::sync_windows(app);

    let (theme, accent, editing, panel_hwnd, card) = {
        let a = app.borrow();
        (a.theme, a.config.accent(), a.editing, a.hwnd, a.config.widget_size_px())
    };
    // The panel's real rectangle right now (it may have a toolbar in edit mode)
    let panel = win::window_rect(panel_hwnd).unwrap_or(app.borrow().panel_rect_ltrb());
    let (work, _) = win::monitor_at((panel.0 + panel.2) / 2, (panel.1 + panel.3) / 2);
    let s = work.scale;
    let gap = (GAP * s) as i32;
    let head = if editing { (HEAD * s) as i32 } else { 0 };

    // Spots for capsules that haven't been placed: a row under the panel, wrapping; above it if
    // there's no room below
    let mut next = (panel.0, panel.3 + gap);
    let mut row_h = 0;

    let count = app.borrow().widget_windows.len();
    let mut spots: Vec<(i32, i32, f32, f32)> = Vec::new();
    for i in 0..count {
        let (window, group, mut hwnd) = {
            let a = app.borrow();
            let c = &a.widget_windows[i];
            (c.window.clone_strong(), c.group.clone(), c.hwnd)
        };
        let (lw, lh) = size(group.widgets.len(), group.vertical, editing, card);
        let (pw, ph) = ((lw * s) as i32, (lh * s) as i32);
        // Position of the capsule's content (below the edit bar)
        let content_h = ph - head;

        let (x, y) = if group.x >= 0.0 && group.y >= 0.0 {
            (
                work.left + ((work.width - pw).max(0) as f64 * group.x.min(1.0)).round() as i32,
                work.top + ((work.height - content_h).max(0) as f64 * group.y.min(1.0)).round() as i32 - head,
            )
        } else {
            if next.0 + pw > work.left + work.width && next.0 > panel.0 {
                next = (panel.0, next.1 + row_h + gap);
                row_h = 0;
            }
            let mut spot = (next.0, next.1 - head);
            if spot.1 + ph > work.top + work.height {
                // no room below the panel: go above it
                spot.1 = panel.1 - gap - ph - (next.1 - (panel.3 + gap));
            }
            next.0 += pw + gap;
            row_h = row_h.max(content_h);
            (
                spot.0.clamp(work.left, (work.left + work.width - pw).max(work.left)),
                spot.1.clamp(work.top, (work.top + work.height - ph).max(work.top)),
            )
        };

        window.set_theme(theme.to_ui());
        window.set_accent(accent);
        window.set_edit_mode(editing);
        window.set_shown(false);
        window.window().set_position(slint::PhysicalPosition::new(x, y));
        window.window().set_size(slint::LogicalSize::new(lw, lh));
        let _ = window.show();
        if hwnd == 0 {
            hwnd = overview::window_handle(window.window());
            if hwnd != 0 {
                win::style_overlay(hwnd, theme.dark, theme.border_bgr());
                win::set_owner(hwnd, panel_hwnd); // part of the panel: no taskbar button, stays in front of it
                win::native_hide(hwnd);
                win::native_show(hwnd);
                app.borrow_mut().widget_windows[i].hwnd = hwnd;
            }
        }
        spots.push((x, y, lw, lh));
        let w2 = window.clone_strong();
        Timer::single_shot(Duration::from_millis(16 + i as u64 * 30), move || w2.set_shown(true));
    }

    // A window made just now may not exist yet in Windows, so the first open can ignore the
    // position and styling. Apply them again a moment later.
    let weak = Rc::downgrade(app);
    Timer::single_shot(Duration::from_millis(60), move || {
        let Some(app) = weak.upgrade() else { return };
        if !app.borrow().visible {
            return;
        }
        let (theme, panel_hwnd) = {
            let a = app.borrow();
            (a.theme, a.hwnd)
        };
        for (i, (x, y, lw, lh)) in spots.into_iter().enumerate() {
            let (window, hwnd) = {
                let a = app.borrow();
                let Some(c) = a.widget_windows.get(i) else { return };
                (c.window.clone_strong(), c.hwnd)
            };
            if hwnd == 0 {
                let h = overview::window_handle(window.window());
                if h != 0 {
                    win::style_overlay(h, theme.dark, theme.border_bgr());
                    win::set_owner(h, panel_hwnd);
                    win::native_hide(h);
                    win::native_show(h);
                    app.borrow_mut().widget_windows[i].hwnd = h;
                }
            }
            window.window().set_position(slint::PhysicalPosition::new(x, y));
            window.window().set_size(slint::LogicalSize::new(lw, lh));
            window.set_shown(true);
        }
        let h = app.borrow().hwnd;
        if h != 0 {
            win::bring_to_front(h); // keyboard back to the panel
        }
    });
}

pub fn hide(app: &Shared) {
    let windows: Vec<WidgetWindow> = app.borrow().widget_windows.iter().map(|c| c.window.clone_strong()).collect();
    for w in windows {
        w.set_shown(false);
        let _ = w.hide();
    }
}

/// Edit mode on/off: the handle bar appears above the widgets, which stay where they are.
pub fn set_edit(app: &Shared, on: bool) {
    let card = app.borrow().config.widget_size_px();
    let a = app.borrow();
    for c in &a.widget_windows {
        let win_ = c.window.window();
        let scale = win_.scale_factor();
        let head = (HEAD * scale) as i32;
        let pos = win_.position();
        c.window.set_edit_mode(on);
        let (lw, lh) = size(c.group.widgets.len(), c.group.vertical, on, card);
        win_.set_position(slint::PhysicalPosition::new(pos.x, if on { pos.y - head } else { pos.y + head }));
        win_.set_size(slint::LogicalSize::new(lw, lh));
    }
}

/// Our capsule windows (to tell them apart from other apps taking focus).
pub fn hwnds(app: &crate::App) -> Vec<isize> {
    app.widget_windows.iter().map(|c| c.hwnd).filter(|h| *h != 0).collect()
}

/// Write where each capsule is now into the settings (not saved to disk here).
pub fn save_positions(app: &mut crate::App) {
    let editing = app.editing;
    let mut groups = Vec::new();
    for c in &app.widget_windows {
        let mut g = c.group.clone();
        if let Some((l, t, r, b)) = win::window_rect(c.hwnd) {
            let scale = c.window.window().scale_factor();
            let head = if c.window.get_edit_mode() || editing { (HEAD * scale) as i32 } else { 0 };
            let (w, h) = (r - l, b - t - head);
            let top = t + head;
            let (work, _) = win::monitor_at(l + w / 2, top + h / 2);
            g.x = if work.width > w { ((l - work.left) as f64 / (work.width - w) as f64).clamp(0.0, 1.0) } else { 0.0 };
            g.y = if work.height > h { ((top - work.top) as f64 / (work.height - h) as f64).clamp(0.0, 1.0) } else { 0.0 };
        }
        groups.push(g);
    }
    if !groups.is_empty() {
        app.config.widget_groups = groups;
    }
}

/// Forget where the capsules were put (they line up under the panel again).
pub fn reset_positions(app: &mut crate::App) {
    for g in &mut app.config.widget_groups {
        g.x = -1.0;
        g.y = -1.0;
    }
}

/// Rebuild after joining / splitting / turning (keeps everything where it is).
fn rebuild(app: &Shared) {
    let _ = app.borrow().config.save();
    show(app);
    set_edit_flags(app);
}

fn set_edit_flags(app: &Shared) {
    let on = app.borrow().editing;
    for c in &app.borrow().widget_windows {
        c.window.set_edit_mode(on);
    }
}

fn centre(hwnd: isize) -> Option<(i32, i32)> {
    win::window_rect(hwnd).map(|(l, t, r, b)| ((l + r) / 2, (t + b) / 2))
}

/// Merge capsule `index` into the nearest other capsule.
fn join(app: &Shared, index: usize) {
    {
        let mut a = app.borrow_mut();
        if a.widget_windows.len() < 2 {
            return;
        }
        save_positions(&mut a);
        let Some(me) = a.widget_windows.get(index).and_then(|c| centre(c.hwnd)) else { return };
        let nearest = a
            .widget_windows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .filter_map(|(i, c)| centre(c.hwnd).map(|p| (i, p)))
            .min_by_key(|(_, p)| {
                let (dx, dy) = ((p.0 - me.0) as i64, (p.1 - me.1) as i64);
                dx * dx + dy * dy
            })
            .map(|(i, _)| i);
        let Some(target) = nearest else { return };
        let moved = a.config.widget_groups[index].widgets.clone();
        a.config.widget_groups[target].widgets.extend(moved);
        a.config.widget_groups.remove(index);
    }
    rebuild(app);
}

/// One capsule per widget, laid out where the widgets are now.
fn split(app: &Shared, index: usize) {
    {
        let mut a = app.borrow_mut();
        save_positions(&mut a);
        let Some(c) = a.widget_windows.get(index) else { return };
        let Some((l, t, _, _)) = win::window_rect(c.hwnd) else { return };
        let scale = c.window.window().scale_factor();
        let card = a.config.widget_size_px();
        let group = c.group.clone();
        let (single_w, single_h) = size(1, false, false, card);
        let (pw, ph) = ((single_w * scale) as i32, (single_h * scale) as i32);
        let head = (HEAD * scale) as i32;
        let step = ((if group.vertical { card.1 } else { card.0 } + INNER_GAP) * scale) as i32;
        let (work, _) = win::monitor_at(l + pw / 2, t + head + ph / 2);

        let pieces: Vec<WidgetGroup> = group
            .widgets
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let (x, y) = if group.vertical { (l, t + head + i as i32 * step) } else { (l + i as i32 * step, t + head) };
                WidgetGroup {
                    widgets: vec![k.clone()],
                    vertical: false,
                    x: if work.width > pw { ((x - work.left) as f64 / (work.width - pw) as f64).clamp(0.0, 1.0) } else { 0.0 },
                    y: if work.height > ph { ((y - work.top) as f64 / (work.height - ph) as f64).clamp(0.0, 1.0) } else { 0.0 },
                }
            })
            .collect();
        a.config.widget_groups.splice(index..=index, pieces);
    }
    rebuild(app);
}

/// Side by side <-> stacked.
fn rotate(app: &Shared, index: usize) {
    {
        let mut a = app.borrow_mut();
        save_positions(&mut a);
        if let Some(g) = a.config.widget_groups.get_mut(index) {
            g.vertical = !g.vertical;
        }
    }
    rebuild(app);
}

/// Apply theme changes to the capsules.
pub fn apply_look(app: &crate::App) {
    for c in &app.widget_windows {
        c.window.set_theme(app.theme.to_ui());
        c.window.set_accent(app.config.accent());
        if c.hwnd != 0 {
            win::set_border(c.hwnd, app.theme.dark, app.theme.border_bgr());
        }
    }
}
