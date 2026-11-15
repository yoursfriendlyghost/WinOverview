//! The overview panel: opening/closing, what's in it, clicks, keyboard, live previews.

use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::Key;
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::layout::{self, Mode, Orient, Rect, Widget};
use crate::thumbs::{self, Px, ThumbSpec};
use crate::win::{self, WinInfo};
use crate::config::Config;
use crate::{free, snap, widget_ui, AppItem, DeskItem, Overview, Shared, WidgetFlags, WidgetSlot};

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
        a.query_all = false;
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
    ui.invoke_focus_keys();
    free::show(app);
    if h != 0 {
        win::bring_to_front(h); // the panel keeps the keyboard
    }
    let ui2 = ui.clone_strong();
    Timer::single_shot(Duration::from_millis(16), move || ui2.set_shown(true));
    sync_thumbs_later(app, 210); // once the panel has slid into place
    widget_ui::panel_opened(app);

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
                && !a.editing
                && win::foreground() != a.hwnd
                && win::foreground() != a.stack_hwnd
                && !free::hwnds(&a).contains(&win::foreground())
        };
        if lost {
            hide(&app);
        }
    });
}

pub fn hide(app: &Shared) {
    crate::stack::close(app);
    if app.borrow().editing {
        finish_edit(app);
    }
    widget_ui::panel_closed(app);
    free::hide(app);
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

/// Size the window to the panel and put it where it belongs: the spot picked in edit mode,
/// or centred near the top of the screen.
fn place(app: &Shared) {
    let a = app.borrow();
    let m = &a.monitor;
    let (w, h) = (a.layout.width, a.layout.height);
    let pw = (w * m.scale) as i32;
    let ph = (h * m.scale) as i32;
    let c = &a.config;
    let (x, y) = if c.panel_x >= 0.0 && c.panel_y >= 0.0 {
        // Fractions of the free space, so it stays on screen whatever the panel's size
        let x = m.left + ((m.width - pw).max(0) as f64 * c.panel_x).round() as i32;
        let y = m.top + ((m.height - ph).max(0) as f64 * c.panel_y).round() as i32;
        (x, y)
    } else {
        let x = m.left + (m.width - pw) / 2;
        let mut y = m.top + (m.height as f64 * c.top_margin_percent / 100.0) as i32;
        if y + ph > m.top + m.height {
            y = (m.top + m.height - ph).max(m.top);
        }
        (x, y)
    };
    a.ui.window().set_position(slint::PhysicalPosition::new(x, y));
    a.ui.window().set_size(slint::LogicalSize::new(w, h));
    drop(a);
    app.borrow_mut().panel_rect = (x, y, pw, ph);
}

/// Resize to the panel without moving it (edit mode), but keep it on screen.
fn resize_in_place(app: &Shared) {
    let a = app.borrow();
    let win = a.ui.window();
    let scale = win.scale_factor();
    let (w, h) = (a.layout.width, a.layout.height);
    let (pw, ph) = ((w * scale) as i32, (h * scale) as i32);
    let pos = win.position();
    let (work, _) = win::monitor_at(pos.x + pw / 2, pos.y + 10);
    let x = pos.x.clamp(work.left, (work.left + work.width - pw).max(work.left));
    let y = pos.y.clamp(work.top, (work.top + work.height - ph).max(work.top));
    if (x, y) != (pos.x, pos.y) {
        win.set_position(slint::PhysicalPosition::new(x, y));
    }
    win.set_size(slint::LogicalSize::new(w, h));
}

/// Relayout and resize after a change (keeps the spot in edit mode).
fn relayout(app: &Shared) {
    refresh(app);
    if app.borrow().editing {
        resize_in_place(app);
    } else {
        place(app);
    }
    sync_thumbs_later(app, 200);
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

    // Same-app windows on a desktop share one cell (a stack), unless you're searching. Windows come top-most (most recently used) first, so each stack's top
    // card is its most recent window, and stacks are in most-recent order too.
    let searching = !a.query.is_empty();
    let stacks: Vec<Vec<Stack>> = (0..count)
        .map(|d| {
            let mut list: Vec<Stack> = Vec::new();
            for w in windows.iter().filter(|w| w.desktop == d) {
                let key = w.exe.as_deref().map(str::to_lowercase).unwrap_or_else(|| format!("#{}", w.hwnd));
                match list.iter_mut().find(|s| s.key == key && !searching) {
                    Some(s) => s.members.push(w.hwnd),
                    None => list.push(Stack { top: w.clone(), members: vec![w.hwnd], key }),
                }
            }
            list
        })
        .collect();
    let counts: Vec<usize> = stacks.iter().map(|s| s.len()).collect();
    a.stacks = stacks;
    let max_w = a.monitor.width as f32 / a.monitor.scale - 24.0;
    let max_h = a.monitor.height as f32 / a.monitor.scale - 24.0;
    // Free layout: the widgets are their own windows (src/free.rs), not in the panel
    let widgets = if a.config.free() { Vec::new() } else { a.config.widget_list() };
    let opts = layout::Options {
        apps: &counts,
        per_row: a.config.desktops_per_row as usize,
        mode: mode(&a),
        orient: if a.config.desktops_vertical() { Orient::Vertical } else { Orient::Horizontal },
        widgets: &widgets,
        widgets_first: a.config.widgets_first,
        max_w,
        max_h,
        top: if a.editing {
            layout::EDIT_BAR
        } else if !a.query.is_empty() {
            layout::SEARCH_BAR
        } else {
            0.0
        },
        widget_w: a.config.widget_size_px().0,
        widget_h: a.config.widget_size_px().1,
    };
    a.layout = layout::compute(&opts);
    a.windows = windows;
    a.current = current;
    drop(a);
    update_model(app);
}

/// Windows of the same app on one desktop, shown as one cell. `top` is the most recent one.
#[derive(Clone)]
pub struct Stack {
    pub top: WinInfo,
    pub members: Vec<isize>,
    /// The app (its .exe path, lower case)
    pub key: String,
}

/// The cells on desktop `d` (one per stack), top-most first: each stack's top window.
fn on_desk(a: &crate::App, d: usize) -> Vec<&WinInfo> {
    let cells = a.layout.desks.get(d).map_or(0, |k| k.cells.len());
    a.stacks.get(d).map_or(Vec::new(), |s| s.iter().take(cells).map(|s| &s.top).collect())
}

/// How many windows are in the stack whose top window is `hwnd`.
fn stack_size(a: &crate::App, hwnd: isize) -> usize {
    a.stacks.iter().flatten().find(|s| s.top.hwnd == hwnd).map_or(1, |s| s.members.len())
}

/// Push the current windows/search/selection into the UI (no re-reading from Windows).
pub fn update_model(app: &Shared) {
    {
        let mut a = app.borrow_mut();
        let query = a.query.to_lowercase();

        // When searching, keep the selection on something that matches
        if !query.is_empty() {
            let sel_hits = a.selected.is_some_and(|s| a.windows.iter().any(|w| w.hwnd == s && matches(&a.config, w, &query)));
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
                    title: if private(&a.config, w) { win::exe_name(&w.exe).into() } else { w.title.as_str().into() },
                    icon,
                    hit: query.is_empty() || matches(&a.config, w, &query),
                    live: previews && !win::is_minimized(w.hwnd) && !private(&a.config, w),
                    private: private(&a.config, w),
                    count: stack_size(&a, w.hwnd) as i32,
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
        let slots: Vec<WidgetSlot> = a
            .layout
            .widgets
            .iter()
            .map(|(k, r)| WidgetSlot { kind: *k as i32, x: r.x, y: r.y, w: r.w, h: r.h })
            .collect();
        ui.set_widget_slots(ModelRc::new(VecModel::from(slots)));
        ui.set_edit_mode(a.editing);
        ui.set_custom_position(a.config.panel_x >= 0.0 || a.config.widget_groups.iter().any(|g| g.x >= 0.0));
        ui.set_query(a.query.as_str().into());
        ui.set_query_all(a.query_all && !a.query.is_empty());
        ui.set_matches(if query.is_empty() { 0 } else { ordered(&a, &query).len() as i32 });
        ui.set_selected_id(a.selected.map_or(0, |s| s as i32));
    }
    sync_thumbs(app);
}

/// Does a window match the search? Private apps only by their app name, never their title.
fn matches(c: &Config, w: &WinInfo, query: &str) -> bool {
    let name = win::exe_name(&w.exe).to_lowercase();
    name.contains(query) || (!c.is_private(&name) && w.title.to_lowercase().contains(query))
}

pub fn private(c: &Config, w: &WinInfo) -> bool {
    c.is_private(&win::exe_name(&w.exe))
}

/// Shown apps in on-screen order (desktop by desktop), only search matches when searching.
fn ordered(a: &crate::App, query: &str) -> Vec<isize> {
    (0..a.layout.desks.len())
        .flat_map(|d| on_desk(a, d))
        .filter(|w| query.is_empty() || matches(&a.config, w, query))
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
                relayout(&app);
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
            if win::is_minimized(w.hwnd) || private(&a.config, w) {
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
            let hit = query.is_empty() || matches(&a.config, w, &query);
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
    if mode(&a) != Mode::Icons
        || !a.config.hover_preview
        || a.ui.get_dragging()
        || win::is_minimized(w.hwnd)
        || private(&a.config, &w)
    {
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

pub fn open_window(app: &Shared, w: WinInfo) {
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

// ===================== edit mode =====================

pub fn flags(c: &Config) -> WidgetFlags {
    WidgetFlags {
        notes: c.has_widget(Widget::Notes),
        music: c.has_widget(Widget::Music),
        timer: c.has_widget(Widget::Timer),
        weather: c.has_widget(Widget::Weather),
        calendar: c.has_widget(Widget::Calendar),
    }
}

/// Open the panel (if needed) with the edit toolbar: move it, pick row/column and widgets.
pub fn edit_start(app: &Shared) {
    if !app.borrow().visible {
        show(app);
    }
    app.borrow_mut().editing = true;
    free::set_edit(app, true);
    stop_hover(app);
    relayout(app);
}

/// Save where the panel is now, and leave edit mode.
fn finish_edit(app: &Shared) {
    let mut a = app.borrow_mut();
    a.editing = false;
    if a.hwnd != 0 {
        if let Some(r) = win::window_rect(a.hwnd) {
            // The toolbar goes away, so measure from just under it
            let scale = a.ui.window().scale_factor();
            let bar = (layout::EDIT_BAR * scale) as i32;
            let (x, y, w, h) = (r.0, r.1 + bar, r.2 - r.0, r.3 - r.1 - bar);
            let (work, _) = win::monitor_at(x + w / 2, y + h / 2);
            let fx = if work.width > w { (x - work.left) as f64 / (work.width - w) as f64 } else { 0.5 };
            let fy = if work.height > h { (y - work.top) as f64 / (work.height - h) as f64 } else { 0.0 };
            a.config.panel_x = fx.clamp(0.0, 1.0);
            a.config.panel_y = fy.clamp(0.0, 1.0);
            // Within a few pixels of the middle: snap to the middle
            let centre = work.left + (work.width - w) / 2;
            if (x - centre).abs() <= (12.0 * scale) as i32 {
                a.config.panel_x = 0.5;
            }
        }
    }
    free::save_positions(&mut a);
    let _ = a.config.save();
    drop(a);
    free::set_edit(app, false);
}

fn edit_done(app: &Shared) {
    finish_edit(app);
    {
        let mut a = app.borrow_mut();
        a.monitor = {
            let r = win::window_rect(a.hwnd).unwrap_or_default();
            win::monitor_at((r.0 + r.2) / 2, (r.1 + r.3) / 2).0
        };
    }
    relayout(app);
}

/// Back to the default spots: panel centred near the top, widgets next to it.
pub fn reset_position(app: &Shared) {
    {
        let mut a = app.borrow_mut();
        a.config.panel_x = -1.0;
        a.config.panel_y = -1.0;
        free::reset_positions(&mut a);
        let _ = a.config.save();
    }
    if app.borrow().visible {
        relayout(app);
        free::show(app);
        update_model(app);
    }
}

/// Move the panel (`which` = None) or a capsule (`which` = Some(i)) with the mouse, in edit mode.
/// step: 0 = button pressed, 1 = mouse moved, 2 = button released.
pub fn drag_window(app: &Shared, which: Option<usize>, step: i32) {
    let mut a = app.borrow_mut();
    let position = |a: &crate::App| match which {
        None => Some(a.ui.window().position()),
        Some(i) => a.widget_windows.get(i).map(|c| c.window.window().position()),
    };
    match step {
        0 => {
            if let Some(p) = position(&a) {
                a.moving = Some((which, win::cursor_pos(), (p.x, p.y)));
            }
        }
        1 => {
            let Some((w, (cx, cy), (wx, wy))) = a.moving else { return };
            if w != which {
                return;
            }
            let (nx, ny) = win::cursor_pos();
            let to = slint::PhysicalPosition::new(wx + nx - cx, wy + ny - cy);
            match which {
                None => a.ui.window().set_position(to),
                Some(i) => {
                    if let Some(c) = a.widget_windows.get(i) {
                        c.window.window().set_position(to);
                    }
                }
            }
        }
        _ => a.moving = None,
    }
}

/// A desktop was dragged onto another one: move it to that position.
fn desk_dropped(app: &Shared, from: i32, x: f32, y: f32) {
    let target = app.borrow().layout.desks.iter().position(|d| d.rect.contains(x, y));
    if let Some(to) = target {
        if to as i32 != from && from >= 0 {
            win::move_desktop(from as u32, to as u32);
            refresh_soon(app, 80);
        }
    }
}

/// Right-click on an app: hide / show its preview (privacy mode).
fn toggle_private(app: &Shared, id: i32) {
    let Some(w) = find(app, id) else { return };
    let name = win::exe_name(&w.exe);
    if name.is_empty() {
        return;
    }
    {
        let mut a = app.borrow_mut();
        let now_private = !a.config.is_private(&name);
        if now_private {
            a.config.privacy_mode = true;
        }
        a.config.set_private(&name, now_private);
        let _ = a.config.save();
    }
    stop_hover(app);
    update_model(app);
}

// ===================== keyboard =====================

fn on_key(app: &Shared, text: &str, ctrl: bool, shift: bool) -> bool {
    let k = |key: Key| SharedString::from(key);
    let t = SharedString::from(text);

    if app.borrow().editing {
        if t == k(Key::Escape) || t == k(Key::Return) {
            edit_done(app);
        }
        return true;
    }

    // Ctrl+A selects the whole search text; the next key replaces or deletes all of it
    let searching = !app.borrow().query.is_empty();
    let first = text.chars().next();
    if searching && ctrl && matches!(first, Some('a' | 'A' | '\u{1}')) {
        app.borrow_mut().query_all = true;
        update_model(app);
        return true;
    }
    let all = std::mem::take(&mut app.borrow_mut().query_all);
    if searching && (all || ctrl) && (t == k(Key::Backspace) || t == k(Key::Delete)) {
        // Ctrl+Backspace also clears the whole search
        app.borrow_mut().query.clear();
        search_changed(app, true);
        return true;
    }
    if all {
        if let (Some(c), None) = (first, text.chars().nth(1)) {
            if !ctrl && !c.is_control() {
                app.borrow_mut().query = c.to_string();
                search_changed(app, true);
                return true;
            }
        }
        update_model(app); // any other key just drops the selection
    }

    if t == k(Key::Escape) {
        if app.borrow().query.is_empty() {
            hide(app);
        } else {
            app.borrow_mut().query.clear();
            search_changed(app, true);
        }
    } else if t == k(Key::Return) {
        let sel = app.borrow().selected;
        if let Some(w) = sel.and_then(|s| find(app, s as i32)) {
            open_window(app, w);
        }
    } else if t == k(Key::Backspace) {
        let was = !app.borrow().query.is_empty();
        app.borrow_mut().query.pop();
        search_changed(app, was);
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
        search_changed(app, searching);
    }
    true
}

/// The search text changed. When the search capsule appears or goes, the panel grows or shrinks
/// (its top edge stays put).
fn search_changed(app: &Shared, was_searching: bool) {
    let searching = !app.borrow().query.is_empty();
    if searching == was_searching {
        update_model(app);
        return;
    }
    refresh(app);
    resize_in_place(app);
    sync_thumbs_later(app, 200);
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
        // A stack opens in its own window over the panel; a single window opens
        let stack = app.borrow().stacks.iter().flatten().find(|s| s.top.hwnd as i32 == id && s.members.len() > 1).cloned();
        if let Some(s) = stack {
            stop_hover(app);
            crate::stack::open(app, &s);
        } else if let Some(win) = find(app, id) {
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

    ui.on_edit_start(with!(|app| edit_start(app)));
    ui.on_edit_drag(with!(|app, step| drag_window(app, None, step)));
    ui.on_app_private(with!(|app, id| toggle_private(app, id)));
    ui.on_desk_dropped(with!(|app, from, x, y| desk_dropped(app, from, x, y)));
    ui.on_edit_reset(with!(|app| {
        reset_position(app);
        relayout(app);
    }));
    ui.on_edit_done(with!(|app| edit_done(app)));

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
