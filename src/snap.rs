//! The snap picker: drop an app outside the panel and pick where it goes on screen.
//! The app you were using before opening the overview fills the rest (a split screen).

use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};

use crate::layout::{partner_zone, SNAP_LAYOUTS};
use crate::win::{self, WinInfo};
use crate::{overview, Shared, SnapLayout, SnapPicker, SnapRequest, Zone};

const CARD_W: f32 = 138.0;
const CARD_H: f32 = 88.0;
const GAP: f32 = 8.0;

/// Picker size in logical pixels (must match SnapPicker in ui/app.slint).
fn picker_size() -> (f32, f32) {
    let rows = SNAP_LAYOUTS.len().div_ceil(3) as f32;
    (3.0 * CARD_W + 2.0 * GAP + 28.0, 70.0 + rows * (CARD_H + GAP) - GAP + 14.0)
}

/// "Document - App Name" -> "App Name", shortened.
fn short_title(title: &str) -> String {
    let s = title.rsplit(" - ").next().unwrap_or(title);
    if s.chars().count() > 24 {
        s.chars().take(23).chain(Some('\u{2026}')).collect()
    } else {
        s.to_string()
    }
}

/// `x`, `y`: where it was dropped, in the overview window (logical pixels; outside the panel).
pub fn open(app: &Shared, dragged: WinInfo, x: f32, y: f32) {
    // Where that is on screen
    let (sx, sy) = {
        let a = app.borrow();
        let pos = a.ui.window().position();
        let scale = a.ui.window().scale_factor();
        (pos.x + (x * scale) as i32, pos.y + (y * scale) as i32)
    };
    let (work, _) = win::monitor_at(sx, sy);

    let partner = {
        let a = app.borrow();
        a.windows.iter().find(|w| w.hwnd == a.previous && w.hwnd != dragged.hwnd).cloned()
    };

    overview::hide(app);
    let picker = ensure_picker(app);
    {
        let mut a = app.borrow_mut();
        picker.set_theme(a.theme.to_ui());
        picker.set_accent(a.config.accent());
        picker.set_app_icon(a.icons.get(&dragged));
        picker.set_name(short_title(&dragged.title).into());
        picker.set_has_partner(partner.is_some());
        if let Some(p) = &partner {
            picker.set_partner_icon(a.icons.get(p));
            picker.set_partner_name(short_title(&p.title).into());
        }
        picker.set_shown(false);

        // Centred on the drop point, kept on that screen
        let (w, h) = picker_size();
        let (pw, ph) = ((w * work.scale) as i32, (h * work.scale) as i32);
        let px = (sx - pw / 2).clamp(work.left + 8, (work.left + work.width - pw - 8).max(work.left));
        let py = (sy - (30.0 * work.scale) as i32).clamp(work.top + 8, (work.top + work.height - ph - 8).max(work.top));
        picker.window().set_position(slint::PhysicalPosition::new(px, py));

        a.snap = Some(SnapRequest { dragged, partner, work_area: work });
        a.picker_opened = Instant::now();
    }

    let _ = picker.show();
    let mut h = app.borrow().picker_hwnd;
    if h == 0 {
        h = overview::window_handle(picker.window());
        if h != 0 {
            let (dark, border) = (app.borrow().theme.dark, app.borrow().theme.border_bgr());
            win::style_overlay(h, dark, border);
            win::native_hide(h);
            win::native_show(h);
            app.borrow_mut().picker_hwnd = h;
        }
    }
    if h != 0 {
        win::bring_to_front(h);
    }
    let p2 = picker.clone_strong();
    Timer::single_shot(Duration::from_millis(16), move || p2.set_shown(true));

    // Close when you click somewhere else
    let weak = Rc::downgrade(app);
    app.borrow().picker_watch.start(TimerMode::Repeated, Duration::from_millis(60), move || {
        let Some(app) = weak.upgrade() else { return };
        let lost = {
            let a = app.borrow();
            a.picker_hwnd != 0 && a.picker_opened.elapsed() > Duration::from_millis(400) && win::foreground() != a.picker_hwnd
        };
        if lost {
            close(&app);
        }
    });
}

pub fn close(app: &Shared) {
    let picker = {
        let mut a = app.borrow_mut();
        a.picker_watch.stop();
        a.snap = None;
        a.picker.as_ref().map(|p| p.clone_strong())
    };
    if let Some(p) = picker {
        p.set_shown(false);
        let _ = p.hide();
    }
}

fn ensure_picker(app: &Shared) -> SnapPicker {
    if let Some(p) = &app.borrow().picker {
        return p.clone_strong();
    }
    let p = SnapPicker::new().expect("couldn't create the snap picker");
    let layouts: Vec<SnapLayout> = SNAP_LAYOUTS
        .iter()
        .map(|zones| SnapLayout {
            zones: ModelRc::new(VecModel::from(
                zones.iter().map(|&(x, y, w, h)| Zone { x, y, w, h }).collect::<Vec<_>>(),
            )),
        })
        .collect();
    p.set_layouts(ModelRc::new(VecModel::from(layouts)));

    let w = Rc::downgrade(app);
    let w1 = w.clone();
    p.on_pick(move |layout, fx, fy| {
        if let Some(app) = w1.upgrade() {
            pick(&app, layout as usize, fx, fy);
        }
    });
    let w1 = w.clone();
    p.on_dismiss(move || {
        if let Some(app) = w1.upgrade() {
            close(&app);
        }
    });
    p.window().on_close_requested(move || {
        if let Some(app) = w.upgrade() {
            app.borrow_mut().snap = None;
            app.borrow().picker_watch.stop();
        }
        slint::CloseRequestResponse::HideWindow
    });
    app.borrow_mut().picker = Some(p.clone_strong());
    p
}

/// Put the dragged app in the chosen zone and the previous app in the other one, on this desktop.
fn pick(app: &Shared, layout: usize, fx: f32, fy: f32) {
    let Some(zones) = SNAP_LAYOUTS.get(layout) else { return };
    let chosen = zones
        .iter()
        .position(|&(x, y, w, h)| fx >= x && fx < x + w && fy >= y && fy < y + h)
        .unwrap_or(0);
    let request = app.borrow_mut().snap.take();
    close(app);
    let Some(req) = request else { return };

    let current = win::current_desktop();
    Timer::single_shot(Duration::from_millis(10), move || {
        if req.dragged.desktop != current {
            win::move_to_desktop(req.dragged.hwnd, current);
        }
        win::place_window(req.dragged.hwnd, zones[chosen], &req.work_area);
        if let (Some(p), Some(other)) = (&req.partner, partner_zone(zones.len(), chosen)) {
            if p.desktop != current {
                win::move_to_desktop(p.hwnd, current);
            }
            win::place_window(p.hwnd, zones[other], &req.work_area);
        }
        win::focus_window(req.dragged.hwnd);
    });
}
