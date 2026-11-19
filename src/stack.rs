//! A stack's own window: clicking a stack (several windows of one app on one desktop) opens
//! this over the panel with every one of those windows as a live preview. The panel underneath
//! stays as it was. Click a window to open it; Esc, x or clicking back on the panel closes this.

use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::layout::PREVIEW_HEADER;
use crate::overview::{self, Stack};
use crate::thumbs::{self, Px, ThumbSpec};
use crate::win::{self, WinInfo};
use crate::{AppItem, Shared, StackView};

const SLOT_W: f32 = 236.0;
const SLOT_H: f32 = 160.0;
const HEADER: f32 = 46.0;
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;

/// Columns for `n` windows: a roughly landscape grid.
fn columns(n: usize) -> usize {
    match n {
        0..=3 => n.max(1),
        4 => 2,
        5..=6 | 9 => 3,
        7..=8 | 10..=12 => 4,
        _ => ((n as f32 * 1.5).sqrt().ceil() as usize).min(8),
    }
}

struct Grid {
    w: f32,
    h: f32,
    cells: Vec<(f32, f32, f32, f32)>,
}

/// Cell positions (logical pixels), shrunk to fit inside `max_w` x `max_h`.
fn grid(n: usize, max_w: f32, max_h: f32) -> Grid {
    let cols = columns(n);
    let rows = n.div_ceil(cols).max(1);
    let full_w = 2.0 * PAD + cols as f32 * SLOT_W + (cols - 1) as f32 * GAP;
    let full_h = HEADER + PAD + rows as f32 * SLOT_H + (rows - 1) as f32 * GAP;
    let s = (max_w / full_w).min(max_h / full_h).min(1.0);
    let (sw, sh) = (SLOT_W * s, (SLOT_H * s).max(PREVIEW_HEADER + 30.0));
    let cells = (0..n)
        .map(|i| {
            let (c, r) = ((i % cols) as f32, (i / cols) as f32);
            (PAD + c * (sw + GAP), HEADER + r * (sh + GAP), sw, sh)
        })
        .collect();
    let w = (2.0 * PAD + cols as f32 * sw + (cols - 1) as f32 * GAP).max(260.0);
    let h = HEADER + PAD + rows as f32 * sh + (rows - 1) as f32 * GAP;
    Grid { w, h, cells }
}

/// "explorer" -> "Explorer"
fn app_name(w: &WinInfo) -> String {
    let n = win::exe_name(&w.exe);
    let mut c = n.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

pub fn open(app: &Shared, stack: &Stack) {
    let members: Vec<WinInfo> = {
        let a = app.borrow();
        stack.members.iter().filter_map(|h| a.windows.iter().find(|w| w.hwnd == *h).cloned()).collect()
    };
    if members.is_empty() {
        return;
    }
    let view = ensure_view(app);
    {
        let mut a = app.borrow_mut();
        a.stack_thumbs.clear();
        let scale = a.monitor.scale;
        let max_w = a.monitor.width as f32 / scale * 0.92;
        let max_h = a.monitor.height as f32 / scale * 0.88;
        let g = grid(members.len(), max_w, max_h);

        let items: Vec<AppItem> = members
            .iter()
            .zip(&g.cells)
            .map(|(w, &(x, y, cw, ch))| {
                let private = overview::private(&a.config, w);
                AppItem {
                    id: w.hwnd as i32,
                    title: if private { app_name(w).into() } else { w.title.as_str().into() },
                    icon: a.icons.get(w),
                    hit: true,
                    live: !private && !win::is_minimized(w.hwnd),
                    private,
                    count: 1,
                    x,
                    y,
                    w: cw,
                    h: ch,
                }
            })
            .collect();
        view.set_theme(a.theme.to_ui());
        view.set_accent(a.config.accent());
        view.set_app_icon(a.icons.get(&members[0]));
        view.set_heading(format!("{}  \u{b7}  {} windows", app_name(&members[0]), members.len()).into());
        view.set_items(ModelRc::new(VecModel::from(items)));
        view.set_selected(0);
        view.set_view_w(g.w);
        view.set_view_h(g.h);
        view.set_thumbs_on(false);
        view.set_shown(false);

        // Centred over the panel, kept on its screen
        let (pw, ph) = ((g.w * scale) as i32, (g.h * scale) as i32);
        let pos = a.ui.window().position();
        let size = a.ui.window().size();
        let (cx, cy) = (pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2);
        let m = &a.monitor;
        let x = (cx - pw / 2).clamp(m.left + 8, (m.left + m.width - pw - 8).max(m.left));
        let y = (cy - ph / 2).clamp(m.top + 8, (m.top + m.height - ph - 8).max(m.top));
        view.window().set_position(slint::PhysicalPosition::new(x, y));

        a.stack_open = Some((stack.top.desktop, stack.key.clone(), members));
        a.stack_opened = Instant::now();
    }

    let _ = view.show();
    let mut h = app.borrow().stack_hwnd;
    if h == 0 {
        h = overview::window_handle(view.window());
        if h != 0 {
            let (dark, border) = (app.borrow().theme.dark, app.borrow().theme.border_bgr());
            win::style_overlay(h, dark, border);
            win::native_hide(h);
            win::native_show(h);
            app.borrow_mut().stack_hwnd = h;
        }
    }
    if h != 0 {
        win::set_owner(h, app.borrow().hwnd); // stays above the panel
        win::bring_to_front(h);
    }
    view.invoke_focus_keys();
    let v2 = view.clone_strong();
    Timer::single_shot(Duration::from_millis(16), move || v2.set_shown(true));
    let weak = Rc::downgrade(app);
    Timer::single_shot(Duration::from_millis(160), move || {
        if let Some(app) = weak.upgrade() {
            sync_thumbs(&app);
        }
    });

    // Close when something else takes focus (the panel, or anything outside)
    let weak = Rc::downgrade(app);
    app.borrow().stack_watch.start(TimerMode::Repeated, Duration::from_millis(60), move || {
        let Some(app) = weak.upgrade() else { return };
        let lost = {
            let a = app.borrow();
            a.stack_open.is_some() && a.stack_opened.elapsed() > Duration::from_millis(300) && win::foreground() != a.stack_hwnd
        };
        if lost {
            close(&app);
        }
    });
}

pub fn close(app: &Shared) {
    let view = {
        let mut a = app.borrow_mut();
        a.stack_watch.stop();
        a.stack_thumbs.clear();
        if a.stack_open.take().is_none() {
            return;
        }
        a.stack_view.as_ref().map(|v| v.clone_strong())
    };
    if let Some(v) = view {
        v.set_shown(false);
        v.set_thumbs_on(false);
        let _ = v.hide();
    }
}

/// Close the stack window and give the keyboard back to the panel.
fn back_to_panel(app: &Shared) {
    close(app);
    let (h, ui) = {
        let a = app.borrow();
        (a.hwnd, a.ui.clone_strong())
    };
    if app.borrow().visible && h != 0 {
        win::bring_to_front(h);
        ui.invoke_focus_keys();
    }
}

/// Live previews under each cell's title strip.
fn sync_thumbs(app: &Shared) {
    let mut a = app.borrow_mut();
    let Some((_, _, members)) = a.stack_open.clone() else { return };
    let Some(view) = a.stack_view.as_ref().map(|v| v.clone_strong()) else { return };
    let scale = view.window().scale_factor();
    let px = |v: f32| (v * scale).round() as i32;
    let g = grid_of(&view);
    let mut specs = Vec::new();
    for (w, &(x, y, cw, ch)) in members.iter().zip(&g) {
        if win::is_minimized(w.hwnd) || overview::private(&a.config, w) {
            continue;
        }
        let Some((frame, source)) = thumbs::frame_of(w.hwnd) else { continue };
        let area = Px {
            left: px(x + 5.0),
            top: px(y + PREVIEW_HEADER),
            right: px(x + cw - 5.0),
            bottom: px(y + ch - 5.0),
        };
        specs.push(ThumbSpec { target: w.hwnd, dest: thumbs::fit(area, frame.width(), frame.height()), source, opacity: 255 });
    }
    let h = a.stack_hwnd;
    if h != 0 {
        a.stack_thumbs.set(h, &specs);
    }
    view.set_thumbs_on(!a.stack_thumbs.is_empty());
}

/// The cells as they are in the window right now.
fn grid_of(view: &StackView) -> Vec<(f32, f32, f32, f32)> {
    use slint::Model;
    view.get_items().iter().map(|i| (i.x, i.y, i.w, i.h)).collect()
}

fn pick(app: &Shared, id: i32) {
    let w = app.borrow().stack_open.as_ref().and_then(|(_, _, m)| m.iter().find(|w| w.hwnd as i32 == id).cloned());
    if let Some(w) = w {
        overview::open_window(app, w);
    }
}

/// Close one window; the stack window stays open with the rest (or closes when one is left).
fn close_window(app: &Shared, id: i32) {
    let target = app.borrow().stack_open.as_ref().and_then(|(_, _, m)| m.iter().find(|w| w.hwnd as i32 == id).cloned());
    let Some(target) = target else { return };
    win::close_window(target.hwnd);
    let weak = Rc::downgrade(app);
    Timer::single_shot(Duration::from_millis(300), move || {
        let Some(app) = weak.upgrade() else { return };
        if !app.borrow().visible {
            return;
        }
        let was = app.borrow().stack_open.as_ref().map(|(d, k, _)| (*d, k.clone()));
        overview::refresh(&app);
        let Some((d, key)) = was else { return };
        let stack = {
            let a = app.borrow();
            let single = overview::one_grid(&a);
            a.stacks.iter().flatten().find(|s| s.key == key && (single || s.top.desktop == d)).cloned()
        };
        match stack {
            Some(s) if s.members.len() > 1 => open(&app, &s),
            _ => back_to_panel(&app),
        }
    });
}

/// Arrow keys move the highlight, Enter opens it, Delete closes it.
fn on_key(app: &Shared, text: &str) -> bool {
    use slint::platform::Key;
    let Some(view) = app.borrow().stack_view.as_ref().map(|v| v.clone_strong()) else { return false };
    let n = app.borrow().stack_open.as_ref().map_or(0, |(_, _, m)| m.len()) as i32;
    if n == 0 {
        return false;
    }
    let cols = columns(n as usize) as i32;
    let sel = view.get_selected().clamp(0, n - 1);
    let k = |key: Key| SharedString::from(key);
    let t = SharedString::from(text);
    let next = if t == k(Key::RightArrow) || t == k(Key::Tab) {
        (sel + 1) % n
    } else if t == k(Key::LeftArrow) {
        (sel + n - 1) % n
    } else if t == k(Key::DownArrow) {
        (sel + cols).min(n - 1)
    } else if t == k(Key::UpArrow) {
        (sel - cols).max(0)
    } else if t == k(Key::Return) {
        let id = app.borrow().stack_open.as_ref().map(|(_, _, m)| m[sel as usize].hwnd as i32);
        if let Some(id) = id {
            pick(app, id);
        }
        return true;
    } else if t == k(Key::Delete) {
        let id = app.borrow().stack_open.as_ref().map(|(_, _, m)| m[sel as usize].hwnd as i32);
        if let Some(id) = id {
            close_window(app, id);
        }
        return true;
    } else if let Some(d) = text.chars().next().and_then(|c| c.to_digit(10)).filter(|d| *d >= 1) {
        let i = d as i32 - 1;
        if i < n {
            let id = app.borrow().stack_open.as_ref().map(|(_, _, m)| m[i as usize].hwnd as i32);
            if let Some(id) = id {
                pick(app, id);
            }
        }
        return true;
    } else {
        return false;
    };
    view.set_selected(next);
    true
}

fn ensure_view(app: &Shared) -> StackView {
    if let Some(v) = &app.borrow().stack_view {
        return v.clone_strong();
    }
    let v = StackView::new().expect("couldn't create the stack window");
    let w = Rc::downgrade(app);
    let w1 = w.clone();
    v.on_pick(move |id| {
        if let Some(app) = w1.upgrade() {
            pick(&app, id);
        }
    });
    let w2 = w.clone();
    v.on_close_window(move |id| {
        if let Some(app) = w2.upgrade() {
            close_window(&app, id);
        }
    });
    let w3 = w.clone();
    v.on_dismiss(move || {
        if let Some(app) = w3.upgrade() {
            back_to_panel(&app);
        }
    });
    let w4 = w.clone();
    v.on_key(move |t| w4.upgrade().is_some_and(|app| on_key(&app, &t)));
    app.borrow_mut().stack_view = Some(v.clone_strong());
    v
}
