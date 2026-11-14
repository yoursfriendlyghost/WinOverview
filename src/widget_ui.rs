//! Connects the widget cards (ui/widgets.slint) to what they show (src/widgets.rs).

use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};

use crate::layout::Widget;
use crate::widgets::{self, MusicCmd, MusicUpdate};
use crate::{overview, post, win, CalDay, NoteItem, Shared, WidgetState};

// ===================== set-up =====================

pub fn wire(app: &Shared) {
    let ui = app.borrow().ui.clone_strong();
    {
        let a = app.borrow();
        for (text, done) in widgets::load_notes() {
            a.notes.push(NoteItem { text: text.into(), done });
        }
    }
    let g = ui.global::<WidgetState>();
    g.set_notes(ModelRc::from(app.borrow().notes.clone()));
    g.set_notes_has_done(app.borrow().notes.iter().any(|n| n.done));
    wire_state(app, &g);

    let minutes = app.borrow().config.timer_minutes;
    {
        let mut a = app.borrow_mut();
        a.timer.total = Duration::from_secs(minutes as u64 * 60);
        a.timer.left = a.timer.total;
    }
    timer_ui(app);
    settings_changed(app);
}

/// Connect a widget window's buttons (Free layout).
pub fn wire_window(app: &Shared, window: &crate::WidgetWindow) {
    let g = window.global::<WidgetState>();
    g.set_notes(ModelRc::from(app.borrow().notes.clone())); // the same list, shared
    wire_state(app, &g);
}

/// The panel's widget data is the original; widget windows get a copy (Free layout).
pub fn sync_windows(app: &Shared) {
    let a = app.borrow();
    if a.widget_windows.is_empty() {
        return;
    }
    let src = a.ui.global::<WidgetState>();
    for w in &a.widget_windows {
        let g = w.window.global::<WidgetState>();
        g.set_notes_has_done(src.get_notes_has_done());
        g.set_cal_title(src.get_cal_title());
        g.set_cal_days(src.get_cal_days());
        g.set_cal_is_now(src.get_cal_is_now());
        g.set_music_on(src.get_music_on());
        g.set_music_title(src.get_music_title());
        g.set_music_artist(src.get_music_artist());
        g.set_music_playing(src.get_music_playing());
        g.set_music_art(src.get_music_art());
        g.set_music_has_art(src.get_music_has_art());
        g.set_timer_text(src.get_timer_text());
        g.set_timer_progress(src.get_timer_progress());
        g.set_timer_running(src.get_timer_running());
        g.set_timer_started(src.get_timer_started());
        g.set_timer_done(src.get_timer_done());
        g.set_timer_minutes(src.get_timer_minutes());
        g.set_weather_ok(src.get_weather_ok());
        g.set_weather_message(src.get_weather_message());
        g.set_weather_temp(src.get_weather_temp());
        g.set_weather_desc(src.get_weather_desc());
        g.set_weather_place(src.get_weather_place());
        g.set_weather_range(src.get_weather_range());
        g.set_weather_icon(src.get_weather_icon());
    }
}

fn wire_state(app: &Shared, g: &WidgetState<'_>) {
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

    // notes checklist (saved straight away)
    g.on_note_add(with!(|app, text| {
        let text = text.trim().to_string();
        if !text.is_empty() {
            app.borrow().notes.push(NoteItem { text: text.into(), done: false });
            notes_changed(app);
        }
    }));
    g.on_note_toggle(with!(|app, i| {
        let notes = app.borrow().notes.clone();
        if let Some(mut n) = notes.row_data(i as usize) {
            n.done = !n.done;
            notes.set_row_data(i as usize, n);
            notes_changed(app);
        }
    }));
    g.on_note_delete(with!(|app, i| {
        let notes = app.borrow().notes.clone();
        if (i as usize) < notes.row_count() {
            notes.remove(i as usize);
            notes_changed(app);
        }
    }));
    g.on_clear_done(with!(|app| {
        let notes = app.borrow().notes.clone();
        let keep: Vec<NoteItem> = notes.iter().filter(|n| !n.done).collect();
        notes.set_vec(keep);
        notes_changed(app);
    }));

    g.on_cal_prev(with!(|app| move_month(app, -1)));
    g.on_cal_next(with!(|app| move_month(app, 1)));
    g.on_cal_today(with!(|app| {
        let (y, m, _) = widgets::today();
        app.borrow_mut().cal_month = (y, m);
        calendar_ui(app);
    }));

    g.on_music_prev(with!(|app| music(app, MusicCmd::Previous)));
    g.on_music_toggle(with!(|app| music(app, MusicCmd::PlayPause)));
    g.on_music_next(with!(|app| music(app, MusicCmd::Next)));
    g.on_music_open(with!(|app| open_player(app)));

    g.on_timer_preset(with!(|app, m| timer_preset(app, m)));
    g.on_timer_toggle(with!(|app| timer_toggle(app)));
    g.on_timer_reset(with!(|app| timer_reset(app)));

    g.on_weather_refresh(with!(|app| {
        app.borrow_mut().weather_at = None;
        refresh_weather(app);
    }));
}

/// Start/stop background work to match which widgets are switched on.
pub fn settings_changed(app: &Shared) {
    let wants_music = app.borrow().config.has_widget(Widget::Music);
    if wants_music && app.borrow().music.is_none() {
        let m = widgets::Music::start(|update| post(move |app| music_update(app, update)));
        app.borrow_mut().music = Some(m);
    } else if !wants_music {
        app.borrow_mut().music = None; // the thread stops when its channel closes
    }
    if app.borrow().visible {
        panel_opened(app);
    }
}

/// The panel just opened (or the settings changed while it's open).
pub fn panel_opened(app: &Shared) {
    // The calendar opens on this month
    if app.borrow().config.has_widget(Widget::Calendar) {
        let (y, m, _) = widgets::today();
        app.borrow_mut().cal_month = (y, m);
        calendar_ui(app);
    }
    if let Some(m) = &app.borrow().music {
        m.active.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if app.borrow().config.has_widget(Widget::Weather) {
        refresh_weather(app);
    }
    timer_ui(app);
    start_tick(app);
}

pub fn panel_closed(app: &Shared) {
    let a = app.borrow();
    if let Some(m) = &a.music {
        m.active.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    a.timer.tick.stop();
    drop(a);
}

fn notes_changed(app: &Shared) {
    {
        let a = app.borrow();
        let items: Vec<(String, bool)> = a.notes.iter().map(|n| (n.text.to_string(), n.done)).collect();
        widgets::save_notes(&items);
        a.ui.global::<WidgetState>().set_notes_has_done(items.iter().any(|(_, d)| *d));
    }
    sync_windows(app);
}

// ===================== calendar =====================

fn move_month(app: &Shared, step: i32) {
    {
        let mut a = app.borrow_mut();
        let (y, m) = a.cal_month;
        let index = y * 12 + (m as i32 - 1) + step;
        a.cal_month = (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32);
    }
    calendar_ui(app);
}

/// Fill the month grid: 6 weeks starting on a Monday, with the neighbouring months' days greyed.
fn calendar_ui(app: &Shared) {
    {
        let a = app.borrow();
        let (year, month) = a.cal_month;
        let (ty, tm, td) = widgets::today();
        let first = widgets::weekday(year, month, 1) as i32;
        let len = widgets::days_in_month(year, month) as i32;
        let (py, pm) = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
        let prev_len = widgets::days_in_month(py, pm) as i32;

        let days: Vec<CalDay> = (0..42)
            .map(|cell| {
                let n = cell - first + 1;
                if n < 1 {
                    CalDay { day: prev_len + n, this_month: false, today: false }
                } else if n > len {
                    CalDay { day: n - len, this_month: false, today: false }
                } else {
                    let today = (year, month, n as u32) == (ty, tm, td);
                    CalDay { day: n, this_month: true, today }
                }
            })
            .collect();
        let g = a.ui.global::<WidgetState>();
        g.set_cal_title(format!("{} {}", widgets::MONTHS[(month - 1) as usize].to_uppercase(), year).into());
        g.set_cal_days(ModelRc::new(VecModel::from(days)));
        g.set_cal_is_now((year, month) == (ty, tm));
    }
    sync_windows(app);
}

// ===================== music =====================

fn music(app: &Shared, cmd: MusicCmd) {
    if let Some(m) = &app.borrow().music {
        m.send(cmd);
    }
}

fn music_update(app: &Shared, update: MusicUpdate) {
    let ui = app.borrow().ui.clone_strong();
    let g = ui.global::<WidgetState>();
    let mut a = app.borrow_mut();
    match update {
        MusicUpdate::Nothing => {
            g.set_music_on(false);
            g.set_music_playing(false);
            g.set_music_has_art(false);
            a.track = None;
        }
        MusicUpdate::Track(t) => {
            g.set_music_on(true);
            g.set_music_title(t.title.as_str().into());
            g.set_music_artist(t.artist.as_str().into());
            g.set_music_playing(t.playing);
            if a.track.as_ref().map_or(true, |old| old.title != t.title) {
                g.set_music_has_art(false); // until the new art arrives
            }
            a.track = Some(t);
        }
        MusicUpdate::Art(art) => match art {
            Some(rgba) => {
                g.set_music_art(widgets::art_image(&rgba));
                g.set_music_has_art(true);
            }
            None => g.set_music_has_art(false),
        },
    }
    drop(a);
    sync_windows(app);
}

/// Bring up the app that's playing: a window with the song in its title (browser tabs, Spotify),
/// else a window of that app, else start the app.
fn open_player(app: &Shared) {
    let Some(track) = app.borrow().track.clone() else { return };
    let (by_title, by_app) = {
        let a = app.borrow();
        let title = track.title.to_lowercase();
        let by_title = a.windows.iter().find(|w| !title.is_empty() && w.title.to_lowercase().contains(&title)).cloned();

        // "Spotify.exe" / "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" / "MSEdge" -> "spotify" / "msedge"
        let id = track.app.to_lowercase();
        let short = id.rsplit('!').next().unwrap_or(&id).trim_end_matches(".exe").to_string();
        let by_app = a
            .windows
            .iter()
            .find(|w| {
                let exe = win::exe_name(&w.exe).to_lowercase();
                !exe.is_empty() && !short.is_empty() && (exe.contains(&short) || short.contains(&exe))
            })
            .cloned();
        (by_title, by_app)
    };
    match by_title.or(by_app) {
        Some(w) => overview::open_window(app, w),
        None if !track.app.is_empty() => {
            overview::hide(app);
            win::open_app(&track.app);
        }
        None => {}
    }
}

// ===================== timer =====================

pub struct TimerState {
    pub total: Duration,
    /// Time left while paused / not started
    pub left: Duration,
    /// When it rings, while running
    pub end: Option<Instant>,
    pub done: bool,
    pub alarm: Timer,
    pub tick: Timer,
}

impl Default for TimerState {
    fn default() -> Self {
        let total = Duration::from_secs(25 * 60);
        TimerState { total, left: total, end: None, done: false, alarm: Timer::default(), tick: Timer::default() }
    }
}

impl TimerState {
    fn remaining(&self) -> Duration {
        match self.end {
            Some(end) => end.saturating_duration_since(Instant::now()),
            None => self.left,
        }
    }
    fn started(&self) -> bool {
        self.end.is_some() || self.left < self.total
    }
}

fn timer_ui(app: &Shared) {
    show_timer(&app.borrow());
    sync_windows(app);
}

fn show_timer(a: &crate::App) {
    let t = &a.timer;
    let left = t.remaining();
    let secs = left.as_secs() + u64::from(left.subsec_millis() > 0 && t.end.is_some());
    let g = a.ui.global::<WidgetState>();
    g.set_timer_text(format!("{:02}:{:02}", secs / 60, secs % 60).into());
    g.set_timer_progress(1.0 - left.as_secs_f32() / t.total.as_secs_f32().max(1.0));
    g.set_timer_running(t.end.is_some());
    g.set_timer_started(t.started());
    g.set_timer_done(t.done);
    g.set_timer_minutes((t.total.as_secs() / 60) as i32);
}

fn start_tick(app: &Shared) {
    let a = app.borrow();
    if a.timer.end.is_none() || !a.visible {
        a.timer.tick.stop();
        return;
    }
    let weak = Rc::downgrade(app);
    a.timer.tick.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        if let Some(app) = weak.upgrade() {
            timer_ui(&app);
        }
    });
}

fn timer_preset(app: &Shared, minutes: i32) {
    {
        let mut a = app.borrow_mut();
        let t = &mut a.timer;
        t.alarm.stop();
        t.total = Duration::from_secs(minutes.clamp(1, 180) as u64 * 60);
        t.left = t.total;
        t.end = None;
        t.done = false;
        a.config.timer_minutes = minutes.clamp(1, 180) as u32;
        let _ = a.config.save();
    }
    timer_ui(app);
    start_tick(app);
}

fn timer_toggle(app: &Shared) {
    {
        let mut a = app.borrow_mut();
        let t = &mut a.timer;
        if let Some(end) = t.end.take() {
            // pause
            t.left = end.saturating_duration_since(Instant::now());
            t.alarm.stop();
        } else {
            // start / resume
            if t.done || t.left.is_zero() {
                t.left = t.total;
            }
            t.done = false;
            t.end = Some(Instant::now() + t.left);
            let weak = Rc::downgrade(app);
            t.alarm.start(TimerMode::SingleShot, t.left, move || {
                if let Some(app) = weak.upgrade() {
                    timer_finished(&app);
                }
            });
        }
    }
    timer_ui(app);
    start_tick(app);
}

fn timer_reset(app: &Shared) {
    {
        let mut a = app.borrow_mut();
        let t = &mut a.timer;
        t.alarm.stop();
        t.end = None;
        t.left = t.total;
        t.done = false;
    }
    timer_ui(app);
    start_tick(app);
}

fn timer_finished(app: &Shared) {
    {
        let mut a = app.borrow_mut();
        let t = &mut a.timer;
        t.end = None;
        t.left = t.total;
        t.done = true;
    }
    timer_ui(app);
    start_tick(app);
    let minutes = app.borrow().timer.total.as_secs() / 60;
    win::alert(&format!("Your {minutes}-minute timer is done."));
}

// ===================== weather =====================

fn refresh_weather(app: &Shared) {
    let (city, coords, fahrenheit) = {
        let a = app.borrow();
        let fresh = a.weather_at.is_some_and(|t| t.elapsed() < Duration::from_secs(15 * 60));
        if fresh || a.weather_busy {
            return;
        }
        let c = &a.config;
        let coords = c.weather_latitude.zip(c.weather_longitude);
        (c.weather_city.clone(), coords, c.weather_fahrenheit)
    };
    let ui = app.borrow().ui.clone_strong();
    let g = ui.global::<WidgetState>();
    if city.trim().is_empty() {
        g.set_weather_ok(false);
        g.set_weather_message("Set your city in Settings (tray icon \u{2192} Settings)".into());
        return;
    }
    if !g.get_weather_ok() {
        g.set_weather_message("Loading\u{2026}".into());
    }
    app.borrow_mut().weather_busy = true;
    widgets::fetch_weather(city.clone(), coords, fahrenheit, move |result| {
        post(move |app| {
            let ui = app.borrow().ui.clone_strong();
            let g = ui.global::<WidgetState>();
            let mut a = app.borrow_mut();
            a.weather_busy = false;
            match result {
                Ok(w) => {
                    a.weather_at = Some(Instant::now());
                    let (desc, icon) = widgets::describe(w.code, w.day);
                    g.set_weather_ok(true);
                    g.set_weather_temp(format!("{:.0}\u{00B0}", w.temp).into());
                    g.set_weather_desc(desc.into());
                    g.set_weather_icon(icon);
                    g.set_weather_place(w.place.as_str().into());
                    let range = if w.high.is_finite() && w.low.is_finite() {
                        format!("H {:.0}\u{00B0}  L {:.0}\u{00B0}", w.high, w.low)
                    } else {
                        String::new()
                    };
                    g.set_weather_range(range.into());
                    // Remember where the city is, so next time it's one request
                    if a.config.weather_latitude.is_none() && a.config.weather_city == city {
                        a.config.weather_latitude = Some(w.latitude);
                        a.config.weather_longitude = Some(w.longitude);
                        let _ = a.config.save();
                    }
                }
                Err(e) => {
                    g.set_weather_ok(false);
                    g.set_weather_message(e.into());
                }
            }
            drop(a);
            sync_windows(app);
        });
    });
}
