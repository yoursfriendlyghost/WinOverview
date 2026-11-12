//! The overview panel: opening/closing, what's in it, clicks, keyboard, live previews.

use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::Key;
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::layout::{self, Mode, Rect};
use crate::thumbs::{self, Px, ThumbSpec};
use crate::win::{self, WinInfo};
use crate::{snap, AppItem, DeskItem, Overview, Shared};

// ===================== open / close =====================

pub fn toggle(app: &Shared) {
    if app.borrow().visible {
        hide(app);
    } else {
        show(app);
    }
}

pub fn show(app: &Shared) {
    if app.borrow().visible {
        return;
    }
    snap::close(app);
    {
        let mut a = app.borrow_mut();
        let fg = win::foreground();
        if fg != a.hwnd && fg != a.picker_hwnd {
            a.previous = fg;
        }
        a.query.clear();
        a.selected = None;
        a.visible = true;
        a.shown_at = Instant::now();
        a.monitor = win::monitor(a.config.open_on_mouse_monitor);
    }
    refresh(app);
    place(app);

    let ui = app.borrow().ui.clone_strong();
    ui.set_shown(false);
    ui.set_dragging(false);
    ui.set_thumbs_on(false);
    let _ = ui.show();

    let mut h = app.borrow().hwnd;
    if h == 0 {
        h = window_handle(ui.window());
        if h != 0 {
            let (dark, border) = (app.borrow().theme.dark, app.borrow().theme.border_bgr());
            win::style_overlay(h, dark, border);
            win::native_hide(h); // re-show so Windows drops the taskbar button
            win::native_show(h);
            app.borrow_mut().hwnd = h;
        }
    }
    if h != 0 {
        win::bring_to_front(h);
    }
    let ui2 = ui.clone_strong();
    Timer::single_shot(Duration::from_millis(16), move || ui2.set_shown(true));
    sync_thumbs_later(app, 210); // once the panel has slid into place

    // Close when another window takes focus (clicking outside, Alt+Tab, Start menu...)
    let weak = Rc::downgrade(app);
    app.borrow().focus_watch.start(TimerMode::Repeated, Duration::from_millis(60), move || {
        let Some(app) = weak.upgrade() else { return };
        let lost = {
            let a = app.borrow();
            a.visible
                && a.hwnd != 0
                && a.shown_at.elapsed() > Duration::from_millis(350)
                && !a.ui.get_dragging()
                && win::foreground() != a.hwnd
        };
        if lost {
            hide(&app);
        }
    });
}

pub fn hide(app: &Shared) {
    let ui = {
        let mut a = app.borrow_mut();
        a.visible = false;
        a.focus_watch.stop();
        a.thumb_timer.stop();
        a.hover_timer.stop();
        a.hover_id = 0;
        a.thumbs.clear();
        if let Some(p) = a.popup.as_mut() {
            p.hide();
        }
        a.ui.clone_strong()
    };
    ui.set_shown(false);
    ui.set_dragging(false);
    ui.set_thumbs_on(false);
    let _ = ui.hide();
}

/// Hide the overview, then do something (switch desktop, focus an app...).
fn hide_then(app: &Shared, then: impl FnOnce() + 'static) {
    hide(app);
    Timer::single_shot(Duration::from_millis(10), then);
}

pub fn window_handle(window: &slint::Window) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

/// Size the window to the panel and put it near the top of the screen.
fn place(app: &Shared) {
    let a = app.borrow();
    let m = &a.monitor;
    let (w, h) = (a.layout.width, a.layout.height);
    let pw = (w * m.scale) as i32;
    let ph = (h * m.scale) as i32;
    let x = m.left + (m.width - pw) / 2;
    let mut y = m.top + (m.height as f64 * a.config.top_margin_percent / 100.0) as i32;
    if y + ph > m.top + m.height {
        y = (m.top + m.height - ph).max(m.top);
    }
    a.ui.window().set_position(slint::PhysicalPosition::new(x, y));
    a.ui.window().set_size(slint::LogicalSize::new(w, h));
}

// ===================== contents =====================

fn mode(app: &crate::App) -> Mode {
    if app.config.previews() {
        Mode::Previews
    } else {
        Mode::Icons
    }
}

/// Re-read desktops and windows and rebuild what's shown.
pub fn refresh(app: &Shared) {
    let mut a = app.borrow_mut();
    let count = win::desktop_count();
    let current = win::current_desktop();
    let exclude = [a.hwnd, a.picker_hwnd];
    let mut windows = win::list_windows(&exclude, current);
    windows.retain(|w| w.desktop < count);

    let open: Vec<isize> = windows.iter().map(|w| w.hwnd).collect();
    a.icons.prune(&open);
    if a.selected.is_some_and(|s| !open.contains(&s)) {
        a.selected = None;
    }

    let counts: Vec<usize> = (0..count).map(|d| windows.iter().filter(|w| w.desktop == d).count()).collect();
    let max_w = a.monitor.width as f32 / a.monitor.scale - 24.0;
    a.layout = layout::compute(&counts, a.config.desktops_per_row as usize, mode(&a), max_w);
    a.windows = windows;
    a.current = current;
    drop(a);
    update_model(app);
}

/// The apps that get a cell on desktop `d`, top-most first.
fn on_desk(a: &crate::App, d: usize) -> Vec<&WinInfo> {
    let cells = a.layout.desks.get(d).map_or(0, |k| k.cells.len());
    a.windows.iter().filter(|w| w.desktop == d as u32).take(cells).collect()
}

/// Push the current windows/search/selection into the UI (no re-reading from Windows).
pub fn update_model(app: &Shared) {
    {
        let mut a = app.borrow_mut();
        let query = a.query.to_lowercase();

        // When searching, keep the selection on something that matches
        if !query.is_empty() {
            let sel_hits = a.selected.is_some_and(|s| a.windows.iter().any(|w| w.hwnd == s && matches(w, &query)));
            if !sel_hits {
                a.selected = ordered(&a, &query).first().copied();
            }
        }

        let previews = mode(&a) == Mode::Previews;
        let mut desks = Vec::new();
        let plan: Vec<(layout::Desk, Vec<WinInfo>)> = (0..a.layout.desks.len())
            .map(|i| {
                let d = &a.layout.desks[i];
                let desk = layout::Desk { rect: d.rect, cells: d.cells.clone(), more: d.more, more_cell: d.more_cell };
                (desk, on_desk(&a, i).into_iter().cloned().collect())
            })
            .collect();
        for (i, (d, apps_here)) in plan.iter().enumerate() {
            let mut apps = Vec::new();
            for (w, cell) in apps_here.iter().zip(&d.cells) {
                let icon = a.icons.get(w);
                apps.push(AppItem {
                    id: w.hwnd as i32,
                    title: w.title.as_str().into(),
                    icon,
                    hit: query.is_empty() || matches(w, &query),
                    live: previews && !win::is_minimized(w.hwnd),
                    x: cell.x,
                    y: cell.y,
                    w: cell.w,
                    h: cell.h,
                });
            }
            desks.push(DeskItem {
                index: i as i32,
                x: d.rect.x,
                y: d.rect.y,
                w: d.rect.w,
                h: d.rect.h,
                current: i as u32 == a.current,
                apps: ModelRc::new(VecModel::from(apps)),
                more: d.more as i32,
                more_x: d.more_cell.x,
                more_y: d.more_cell.y,
                more_w: d.more_cell.w,
                more_h: d.more_cell.h,
            });
        }
        let ui = &a.ui;
        ui.set_previews(previews);
        ui.set_desks(ModelRc::new(VecModel::from(desks)));
        ui.set_plus_x(a.layout.plus.x);
        ui.set_plus_y(a.layout.plus.y);
        ui.set_plus_w(a.layout.plus.w);
        ui.set_plus_h(a.layout.plus.h);
        ui.set_panel_w(a.layout.width);
        ui.set_panel_h(a.layout.height);
        ui.set_can_close_desk(a.layout.desks.len() > 1);
        ui.set_query(a.query.as_str().into());
        ui.set_selected_id(a.selected.map_or(0, |s| s as i32));
    }
    sync_thumbs(app);
}

fn matches(w: &WinInfo, query: &str) -> bool {
    w.title.to_lowercase().contains(query) || win::exe_name(&w.exe).to_lowercase().contains(query)
}

/// Shown apps in on-screen order (desktop by desktop), only search matches when searching.
fn ordered(a: &crate::App, query: &str) -> Vec<isize> {
    (0..a.layout.desks.len())
        .flat_map(|d| on_desk(a, d))
        .filter(|w| query.is_empty() || matches(w, query))
        .map(|w| w.hwnd)
        .collect()
}

pub fn find(app: &Shared, id: i32) -> Option<WinInfo> {
    app.borrow().windows.iter().find(|w| w.hwnd as i32 == id).cloned()
}

pub fn refresh_soon(app: &Shared, ms: u64) {
    let weak = Rc::downgrade(app);
    Timer::single_shot(Duration::from_millis(ms), move || {
        if let Some(app) = weak.upgrade() {
            if app.borrow().visible {
                refresh(&app);
                place(&app);
                sync_thumbs_later(&app, 120);
            }
        }
    });
}

// ===================== live previews =====================

fn sync_thumbs_later(app: &Shared, ms: u64) {
    let weak = Rc::downgrade(app);
    app.borrow().thumb_timer.start(TimerMode::SingleShot, Duration::from_millis(ms), move || {
        if let Some(app) = weak.upgrade() {
            sync_thumbs(&app);
        }
    });
}

/// Put a live preview over each app cell ("Live previews" view), or remove them.
fn sync_thumbs(app: &Shared) {
    let mut a = app.borrow_mut();
    let ready = a.visible
        && a.hwnd != 0
        && mode(&a) == Mode::Previews
        && a.shown_at.elapsed() > Duration::from_millis(200)
        && !a.ui.get_dragging();
    if !ready {
        a.thumbs.clear();
        a.ui.set_thumbs_on(false);
        return;
    }

    let scale = a.ui.window().scale_factor();
    let px = |v: f32| (v * scale).round() as i32;
    let query = a.query.to_lowercase();
    let mut specs = Vec::new();
    for (i, d) in a.layout.desks.iter().enumerate() {
        for (w, cell) in on_desk(&a, i).into_iter().zip(&d.cells) {
            if win::is_minimized(w.hwnd) {
                continue;
            }
            let Some((frame, source)) = thumbs::frame_of(w.hwnd) else { continue };
            // The part of the cell under its title strip, in pixels inside our window
            let body = Rect {
                x: d.rect.x + cell.x + 5.0,
                y: d.rect.y + cell.y + layout::PREVIEW_HEADER,
                w: cell.w - 10.0,
                h: cell.h - layout::PREVIEW_HEADER - 5.0,
            };
            let area = Px { left: px(body.x), top: px(body.y), right: px(body.x + body.w), bottom: px(body.y + body.h) };
            let dest = thumbs::fit(area, frame.width(), frame.height());
            let hit = query.is_empty() || matches(w, &query);
            specs.push(ThumbSpec { target: w.hwnd, dest, source, opacity: if hit { 255 } else { 55 } });
        }
    }
    let hwnd = a.hwnd;
    a.thumbs.set(hwnd, &specs);
    let on = !a.thumbs.is_empty();
    a.ui.set_thumbs_on(on);
}

// ===================== hover preview (icons view) =====================

fn on_hover(app: &Shared, id: i32, entered: bool, cell: Rect) {
    let mut a = app.borrow_mut();
    let Some(w) = a.windows.iter().find(|w| w.hwnd as i32 == id).cloned() else { return };
    if !entered {
        if a.hover_id == w.hwnd {
            a.hover_id = 0;
            a.hover_timer.stop();
            if let Some(p) = a.popup.as_mut() {
                p.hide();
            }
        }
        return;
    }
    if mode(&a) != Mode::Icons || !a.config.hover_preview || a.ui.get_dragging() || win::is_minimized(w.hwnd) {
        return;
    }
    a.hover_id = w.hwnd;
    let delay = a.config.hover_preview_delay_ms.clamp(0, 3000);
    let weak = Rc::downgrade(app);
    a.hover_timer.start(TimerMode::SingleShot, Duration::from_millis(delay), move || {
        let Some(app) = weak.upgrade() else { return };
        let mut a = app.borrow_mut();
        if !a.visible || a.hover_id != w.hwnd || a.ui.get_dragging() {
            return;
        }
        if a.popup.is_none() {
            let t = a.theme;
            let text = if t.dark { 0x00EAE6E6 } else { 0x00211C1C };
            a.popup = thumbs::HoverPopup::new(a.hwnd, t.panel_bgr(), text, t.dark, t.border_bgr());
        }
        let pos = a.ui.window().position();
        let scale = a.ui.window().scale_factor();
        let anchor = Px {
            left: pos.x + (cell.x * scale) as i32,
            top: pos.y + (cell.y * scale) as i32,
            right: pos.x + ((cell.x + cell.w) * scale) as i32,
            bottom: pos.y + ((cell.y + cell.h) * scale) as i32,
        };
        let (_, screen) = win::monitor_at(anchor.left, anchor.top);
        let screen = Px { left: screen.left, top: screen.top, right: screen.left + screen.width, bottom: screen.top + screen.height };
        if let Some(p) = a.popup.as_mut() {
            p.show(w.hwnd, &w.title, anchor, screen, scale);
        }
    });
}

fn stop_hover(app: &Shared) {
    let mut a = app.borrow_mut();
    a.hover_id = 0;
    a.hover_timer.stop();
    if let Some(p) = a.popup.as_mut() {
        p.hide();
    }
}

// ===================== actions =====================

fn open_window(app: &Shared, w: WinInfo) {
    // Switch first, while the panel still has focus (see win::switch_desktop)
    if w.desktop != win::current_desktop() {
        win::switch_desktop(w.desktop);
    }
    hide_then(app, move || win::focus_window(w.hwnd));
}

fn switch_to(app: &Shared, desktop: u32) {
    if desktop >= win::desktop_count() {
        return;
    }
    // Switch first, while the panel still has focus, then hand focus to that desktop's top app
    win::switch_desktop(desktop);
    let top = win::top_window_on(&app.borrow().windows, desktop);
    hide_then(app, move || {
        if let Some(h) = top {
            win::focus_window(h);
        }
    });
}

/// Moves the desktop's apps to desktop 1 (or 2, if closing desktop 1), then removes it.
fn close_desktop(app: &Shared, index: u32) {
    let count = win::desktop_count();
    if count <= 1 || index >= count {
        return;
    }
    let fallback = if index == 0 { 1 } else { 0 };
    let on_it: Vec<isize> = app.borrow().windows.iter().filter(|w| w.desktop == index).map(|w| w.hwnd).collect();
    for h in on_it {
        if !win::is_pinned(h) {
            win::move_to_desktop(h, fallback);
        }
    }
    win::remove_desktop(index, fallback);
    refresh_soon(app, 150);
}

fn add_desktop(app: &Shared) -> Option<u32> {
    let d = win::create_desktop();
    refresh_soon(app, 120);
    d
}

fn dropped(app: &Shared, id: i32, x: f32, y: f32) {
    let Some(w) = find(app, id) else { return };
    let (target, on_plus, outside) = {
        let a = app.borrow();
        let l = &a.layout;
        (
            l.desks.iter().position(|d| d.rect.contains(x, y)),
            l.plus.contains(x, y),
            x < 0.0 || y < 0.0 || x >= l.width || y >= l.height,
        )
    };
    if let Some(d) = target {
        if d as u32 != w.desktop {
            win::move_to_desktop(w.hwnd, d as u32);
            refresh_soon(app, 60);
            return;
        }
    } else if on_plus {
        if let Some(d) = add_desktop(app) {
            win::move_to_desktop(w.hwnd, d);
            refresh_soon(app, 150);
            return;
        }
    } else if outside {
        snap::open(app, w, x, y);
        return;
    }
    sync_thumbs_later(app, 30); // nothing moved: bring the previews back
}

// ===================== keyboard =====================

fn on_key(app: &Shared, text: &str, ctrl: bool, shift: bool) -> bool {
    let k = |key: Key| SharedString::from(key);
    let t = SharedString::from(text);

    if t == k(Key::Escape) {
        if app.borrow().query.is_empty() {
            hide(app);
        } else {
            app.borrow_mut().query.clear();
            update_model(app);
        }
    } else if t == k(Key::Return) {
        let sel = app.borrow().selected;
        if let Some(w) = sel.and_then(|s| find(app, s as i32)) {
            open_window(app, w);
        }
    } else if t == k(Key::Backspace) {
        app.borrow_mut().query.pop();
        update_model(app);
    } else if t == k(Key::Delete) {
        if let Some(s) = app.borrow().selected {
            win::close_window(s);
        }
        refresh_soon(app, 300);
    } else if t == k(Key::Tab) {
        move_selection(app, if shift { Dir::Left } else { Dir::Right });
    } else if t == k(Key::LeftArrow) {
        move_selection(app, Dir::Left);
    } else if t == k(Key::RightArrow) {
        move_selection(app, Dir::Right);
    } else if t == k(Key::UpArrow) {
        move_selection(app, Dir::Up);
    } else if t == k(Key::DownArrow) {
        move_selection(app, Dir::Down);
    } else {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { return false };
        if ctrl || c.is_control() {
            return false;
        }
        let searching = !app.borrow().query.is_empty();
        if !searching && ('1'..='9').contains(&c) {
            switch_to(app, c as u32 - '1' as u32);
            return true;
        }
        if !searching && c == ' ' {
            return true;
        }
        app.borrow_mut().query.push(c);
        update_model(app);
    }
    true
}

enum Dir {
    Left,
    Right,
    Up,
    Down,
}

fn move_selection(app: &Shared, dir: Dir) {
    {
        let mut a = app.borrow_mut();
        let query = a.query.to_lowercase();
        let list = ordered(&a, &query);
        if list.is_empty() {
            return;
        }
        // Where each listed app sits: (desktop, cell)
        let spot = |h: isize| -> Option<(usize, Rect)> {
            (0..a.layout.desks.len()).find_map(|d| {
                on_desk(&a, d).iter().position(|w| w.hwnd == h).map(|i| (d, a.layout.desks[d].cells[i]))
            })
        };
        let pos = a.selected.and_then(|s| list.iter().position(|&h| h == s));
        let next = match pos {
            None => {
                // Start on the current desktop's first app
                let cur = a.current as usize;
                list.iter().copied().find(|&h| spot(h).is_some_and(|(d, _)| d == cur)).unwrap_or(list[0])
            }
            Some(p) => {
                let n = list.len() as isize;
                match dir {
                    Dir::Left => list[(p as isize - 1).rem_euclid(n) as usize],
                    Dir::Right => list[(p as isize + 1).rem_euclid(n) as usize],
                    Dir::Up | Dir::Down => {
                        // The app straight above/below in the same desktop, if there is one
                        let (desk, here) = spot(list[p]).unwrap_or_default();
                        let down = matches!(dir, Dir::Down);
                        list.iter()
                            .copied()
                            .filter_map(|h| spot(h).map(|(d, r)| (h, d, r)))
                            .filter(|&(_, d, r)| d == desk && (r.x - here.x).abs() < 1.0 && if down { r.y > here.y } else { r.y < here.y })
                            .min_by(|a, b| (a.2.y - here.y).abs().total_cmp(&(b.2.y - here.y).abs()))
                            .map_or(list[p], |(h, _, _)| h)
                    }
                }
            }
        };
        a.selected = Some(next);
    }
    update_model(app);
}

// ===================== wiring =====================

pub fn wire(app: &Shared) {
    let ui: Overview = app.borrow().ui.clone_strong();
    let w = Rc::downgrade(app);

    macro_rules! with {
        (|$app:ident $(, $arg:ident)*| $body:expr) => {{
            let w = w.clone();
            move |$($arg),*| {
                if let Some($app) = w.upgrade() {
                    let $app = &$app;
                    $body
                }
            }
        }};
    }

    ui.on_desk_clicked(with!(|app, i| switch_to(app, i as u32)));
    ui.on_desk_close(with!(|app, i| close_desktop(app, i as u32)));
    ui.on_app_clicked(with!(|app, id| {
        if let Some(win) = find(app, id) {
            open_window(app, win);
        }
    }));
    ui.on_app_close(with!(|app, id| {
        stop_hover(app);
        if let Some(win) = find(app, id) {
            win::close_window(win.hwnd);
        }
        refresh_soon(app, 300); // the app may ask "save changes?" first
    }));
    ui.on_app_hover(with!(|app, id, entered, x, y, cw, ch| on_hover(app, id, entered, Rect { x, y, w: cw, h: ch })));
    ui.on_drag_started(with!(|app, _id| {
        stop_hover(app);
        app.borrow().thumb_timer.stop();
        sync_thumbs(app); // dragging: previews step aside so the dragged icon shows
    }));
    ui.on_app_dropped(with!(|app, id, x, y| dropped(app, id, x, y)));
    ui.on_add_clicked(with!(|app| {
        add_desktop(app);
    }));

    let w1 = w.clone();
    ui.on_key_pressed(move |text, ctrl, shift| match w1.upgrade() {
        Some(app) => on_key(&app, &text, ctrl, shift),
        None => false,
    });
    ui.window().on_close_requested(move || {
        if let Some(app) = w.upgrade() {
            hide(&app);
        }
        slint::CloseRequestResponse::HideWindow
    });
}
