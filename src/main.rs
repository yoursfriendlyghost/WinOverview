//! WinOverview - a minimal virtual desktop overview for Windows 11.
//! Press the shortcut (Alt+` by default) to see every desktop and its apps; drag apps between them.
//!
//! main.rs      start-up, tray icon, shortcut, Settings window
//! overview.rs  the panel: opening/closing, contents, keyboard, live previews
//! snap.rs      the snap picker (drop an app outside the panel)
//! layout.rs    where everything goes
//! themes.rs    colour themes
//! thumbs.rs    live window previews
//! win.rs       everything that talks to Windows

#![windows_subsystem = "windows"]

mod config;
mod free;
mod layout;
mod overview;
mod snap;
mod stack;
mod themes;
mod thumbs;
mod widget_ui;
mod widgets;
mod win;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use slint::{ComponentHandle, Model, ModelRc, Timer, VecModel};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use config::Config;
use layout::Layout;
use themes::Preset;
use win::{IconCache, WinInfo};

pub struct Tray {
    icon: TrayIcon,
    open: MenuItem,
    edit: MenuItem,
    startup: CheckMenuItem,
    settings: MenuItem,
    restart: MenuItem,
    quit: MenuItem,
}

/// A drop outside the panel, waiting for a spot to be picked.
pub struct SnapRequest {
    pub dragged: WinInfo,
    pub partner: Option<WinInfo>,
    pub work_area: win::Monitor,
}

pub struct App {
    pub ui: Overview,
    pub settings: Option<Settings>,
    pub config: Config,
    pub theme: &'static Preset,
    hotkeys: GlobalHotKeyManager,
    hotkey: Option<HotKey>,
    tray: Option<Tray>,

    // what's on screen
    pub windows: Vec<WinInfo>,
    /// Same-app windows grouped per desktop (see overview::Stack)
    pub stacks: Vec<Vec<overview::Stack>>,
    /// The window that shows one stack's windows over the panel (src/stack.rs)
    pub stack_view: Option<StackView>,
    pub stack_hwnd: isize,
    pub stack_thumbs: thumbs::Thumbs,
    pub stack_watch: Timer,
    pub stack_opened: Instant,
    /// The stack that's open: (desktop, app) and its windows
    pub stack_open: Option<(u32, String, Vec<WinInfo>)>,
    pub icons: IconCache,
    pub layout: Layout,
    pub current: u32,
    pub query: String,
    /// Ctrl+A: the whole search text is selected (the next key replaces it)
    pub query_all: bool,
    pub selected: Option<isize>,
    pub monitor: win::Monitor,
    /// The app you were using before opening the overview (the snap picker's partner)
    pub previous: isize,

    // our windows and their state
    pub visible: bool,
    pub shown_at: Instant,
    pub hwnd: isize,
    pub focus_watch: Timer,
    pub thumbs: thumbs::Thumbs,
    pub thumb_timer: Timer,
    pub popup: Option<thumbs::HoverPopup>,
    pub hover_timer: Timer,
    pub hover_id: isize,

    pub picker: Option<SnapPicker>,
    pub picker_hwnd: isize,
    pub picker_watch: Timer,
    pub picker_opened: Instant,
    pub snap: Option<SnapRequest>,

    // edit mode
    pub editing: bool,
    /// Moving a window in edit mode: which one (None = the panel, Some(i) = capsule i),
    /// where the mouse started, where the window started
    pub moving: Option<(Option<usize>, (i32, i32), (i32, i32))>,
    /// Free layout: the widgets' own windows
    pub widget_windows: Vec<free::Capsule>,
    /// The panel's rectangle on screen (x, y, width, height) when last placed
    pub panel_rect: (i32, i32, i32, i32),

    // widgets
    pub music: Option<widgets::Music>,
    pub track: Option<widgets::Track>,
    pub timer: widget_ui::TimerState,
    pub weather_at: Option<Instant>,
    pub weather_busy: bool,
    /// The notes checklist (shared by every window that shows it)
    pub notes: Rc<VecModel<NoteItem>>,
    /// The month the calendar shows (year, month 1-12)
    pub cal_month: (i32, u32),
}

pub type Shared = Rc<RefCell<App>>;

impl App {
    /// The panel's last known rectangle as (left, top, right, bottom).
    pub fn panel_rect_ltrb(&self) -> (i32, i32, i32, i32) {
        let (x, y, w, h) = self.panel_rect;
        (x, y, x + w, y + h)
    }
}

thread_local! {
    static APP: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

/// Run something against the app later, on the UI thread (used by tray/hotkey callbacks).
fn post(f: impl FnOnce(&Shared) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = APP.with(|a| a.borrow().clone()) {
            f(&app);
        }
    });
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}");
        let _ = std::fs::create_dir_all(config::folder());
        let _ = std::fs::write(config::folder().join("error.log"), &msg);
        win::message_box(&format!("WinOverview crashed:\n\n{msg}\n\nDetails were saved to error.log in %AppData%\\WinOverview."));
    }));

    let restarting = std::env::args().any(|a| a == "--restart");
    if !win::single_instance(restarting) {
        return; // already running
    }

    if winvd::get_desktop_count().is_err() {
        win::message_box("Couldn't read your virtual desktops.\nWinOverview needs Windows 11.");
        return;
    }

    let ui = Overview::new().expect("couldn't create the window");
    let config = Config::load();
    let theme = themes::find(&config.theme);
    ui.set_theme(theme.to_ui());
    ui.set_accent(config.accent());

    let app: Shared = Rc::new(RefCell::new(App {
        ui: ui.clone_strong(),
        settings: None,
        config,
        theme,
        hotkeys: GlobalHotKeyManager::new().expect("couldn't set up the keyboard shortcut"),
        hotkey: None,
        tray: None,
        windows: Vec::new(),
        stacks: Vec::new(),
        stack_view: None,
        stack_hwnd: 0,
        stack_thumbs: thumbs::Thumbs::default(),
        stack_watch: Timer::default(),
        stack_opened: Instant::now(),
        stack_open: None,
        icons: IconCache::default(),
        layout: layout::compute(&layout::Options {
            apps: &[0],
            per_row: 5,
            mode: layout::Mode::Icons,
            orient: layout::Orient::Horizontal,
            widgets: &[],
            widgets_first: false,
            max_w: 4000.0,
            max_h: 4000.0,
            top: 0.0,
            widget_w: 248.0,
            widget_h: 156.0,
        }),
        current: 0,
        query: String::new(),
        query_all: false,
        selected: None,
        monitor: win::monitor(false),
        previous: 0,
        visible: false,
        shown_at: Instant::now(),
        hwnd: 0,
        focus_watch: Timer::default(),
        thumbs: thumbs::Thumbs::default(),
        thumb_timer: Timer::default(),
        popup: None,
        hover_timer: Timer::default(),
        hover_id: 0,
        picker: None,
        picker_hwnd: 0,
        picker_watch: Timer::default(),
        picker_opened: Instant::now(),
        snap: None,
        editing: false,
        moving: None,
        widget_windows: Vec::new(),
        panel_rect: (0, 0, 0, 0),
        music: None,
        track: None,
        timer: widget_ui::TimerState::default(),
        weather_at: None,
        weather_busy: false,
        notes: Rc::new(VecModel::default()),
        cal_month: (2026, 1),
    }));
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));

    overview::wire(&app);
    widget_ui::wire(&app);
    let hotkey_text = app.borrow().config.hotkey.clone();
    if let Err(e) = register_hotkey(&app, &hotkey_text) {
        win::message_box(&format!("{e}\nUsing Alt+` instead. You can change it in Settings (tray icon)."));
        let _ = register_hotkey(&app, "Alt+`");
    }
    create_tray(&app);

    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state == HotKeyState::Pressed {
            post(move |app| {
                if app.borrow().hotkey.map(|h| h.id()) == Some(e.id) {
                    overview::toggle(app);
                }
            });
        }
    }));

    let welcomed = config::folder().join(".welcomed");
    if !restarting && !welcomed.exists() {
        let _ = std::fs::write(&welcomed, "");
        win::message_box(&format!(
            "WinOverview is running in the tray.\n\nPress {} to open it.",
            app.borrow().config.hotkey
        ));
    }

    slint::run_event_loop_until_quit().expect("event loop failed");
    std::process::exit(0);
}

fn register_hotkey(app: &Shared, text: &str) -> Result<(), String> {
    let new = config::parse_hotkey(text).ok_or_else(|| format!("\"{text}\" isn't a shortcut I understand."))?;
    let mut a = app.borrow_mut();
    if a.hotkey == Some(new) {
        return Ok(());
    }
    let old = a.hotkey.take();
    if let Some(old) = old {
        let _ = a.hotkeys.unregister(old);
    }
    match a.hotkeys.register(new) {
        Ok(()) => {
            a.hotkey = Some(new);
            Ok(())
        }
        Err(_) => {
            // Put the old one back
            if let Some(old) = old {
                if a.hotkeys.register(old).is_ok() {
                    a.hotkey = Some(old);
                }
            }
            Err(format!("{text} is already used by another app."))
        }
    }
}

// ===================== tray =====================

fn create_tray(app: &Shared) {
    let open = MenuItem::new("Open", true, None);
    let startup = CheckMenuItem::new("Start with Windows", true, win::startup_enabled(), None);
    let settings = MenuItem::new("Settings\u{2026}", true, None);
    let edit = MenuItem::new("Edit layout\u{2026}", true, None);
    let restart_item = MenuItem::new("Restart", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[
        &open,
        &PredefinedMenuItem::separator(),
        &startup,
        &edit,
        &settings,
        &PredefinedMenuItem::separator(),
        &restart_item,
        &quit_item,
    ]);

    let icon = tray_icon::Icon::from_rgba(include_bytes!("../assets/tray32.rgba").to_vec(), 32, 32).ok();
    let mut builder = TrayIconBuilder::new()
        .with_tooltip(format!("WinOverview ({})", app.borrow().config.hotkey))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false);
    if let Some(icon) = icon {
        builder = builder.with_icon(icon);
    }
    let Ok(tray) = builder.build() else { return };
    app.borrow_mut().tray = Some(Tray { icon: tray, open, edit, startup, settings, restart: restart_item, quit: quit_item });

    MenuEvent::set_event_handler(Some(|e: MenuEvent| {
        post(move |app| {
            let which = {
                let a = app.borrow();
                let Some(t) = &a.tray else { return };
                [t.open.id(), t.startup.id(), t.settings.id(), t.restart.id(), t.quit.id(), t.edit.id()]
                    .iter()
                    .position(|id| **id == e.id)
            };
            match which {
                Some(0) => overview::show(app),
                Some(1) => {
                    win::set_startup(!win::startup_enabled());
                    if let Some(t) = &app.borrow().tray {
                        t.startup.set_checked(win::startup_enabled());
                    }
                }
                Some(2) => open_settings(app),
                Some(3) => restart(),
                Some(4) => quit(),
                Some(5) => overview::edit_start(app),
                _ => {}
            }
        });
    }));

    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            post(|app| overview::show(app));
        }
    }));
}

fn quit() {
    APP.with(|a| {
        if let Some(app) = a.borrow().as_ref() {
            let mut a = app.borrow_mut();
            a.thumbs.clear();
            a.popup = None;
            a.tray = None; // removes the tray icon right away
        }
    });
    let _ = slint::quit_event_loop();
}

fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--restart").spawn();
    }
    quit();
}

// ===================== settings window =====================

fn open_settings(app: &Shared) {
    let existing = app.borrow().settings.as_ref().map(|s| s.clone_strong());
    let s = match existing {
        Some(s) => s,
        None => {
            let Ok(s) = Settings::new() else { return };
            let w = Rc::downgrade(app);
            let w1 = w.clone();
            s.on_save(move || {
                if let Some(app) = w1.upgrade() {
                    save_settings(&app);
                }
            });
            s.on_close_window(move || {
                if let Some(app) = w.upgrade() {
                    if let Some(s) = &app.borrow().settings {
                        let _ = s.hide();
                    }
                }
            });
            s.set_themes(ModelRc::new(VecModel::from(themes::choices())));
            let s2 = s.as_weak();
            s.on_privacy_toggle(move |i, on| {
                if let Some(s) = s2.upgrade() {
                    let model = s.get_privacy_apps();
                    if let Some(mut row) = model.row_data(i as usize) {
                        row.on = on;
                        model.set_row_data(i as usize, row);
                    }
                }
            });
            let w1 = Rc::downgrade(app);
            s.on_edit_on_screen(move || {
                if let Some(app) = w1.upgrade() {
                    if let Some(s) = &app.borrow().settings {
                        let _ = s.hide();
                    }
                    overview::edit_start(&app);
                }
            });
            let w1 = Rc::downgrade(app);
            s.on_reset_position(move || {
                if let Some(app) = w1.upgrade() {
                    overview::reset_position(&app);
                    if let Some(s) = &app.borrow().settings {
                        s.set_custom_position(false);
                    }
                }
            });
            app.borrow_mut().settings = Some(s.clone_strong());
            s
        }
    };

    {
        let a = app.borrow();
        let c = &a.config;
        s.set_hotkey(c.hotkey.as_str().into());
        s.set_previews(c.previews());
        s.set_hover_preview(c.hover_preview);
        s.set_per_row(c.desktops_per_row as i32);
        s.set_top_margin(c.top_margin_percent.round() as i32);
        s.set_mouse_monitor(c.open_on_mouse_monitor);
        s.set_startup(win::startup_enabled());
        s.set_accent(c.accent());
        s.set_theme(a.theme.name.into());
        s.set_free_layout(c.free_layout);
        s.set_vertical(c.vertical());
        s.set_widgets_first(c.widgets_first);
        s.set_widget_size(c.widget_size.to_ascii_lowercase().as_str().into());
        s.set_privacy(c.privacy_mode);
        s.set_privacy_apps(ModelRc::new(VecModel::from(privacy_list(&a))));
        s.set_flags(overview::flags(c));
        s.set_city(c.weather_city.as_str().into());
        s.set_fahrenheit(c.weather_fahrenheit);
        s.set_custom_position(c.panel_x >= 0.0 || c.widget_groups.iter().any(|g| g.x >= 0.0));
        s.set_status("".into());
    }
    let _ = s.show();
}

/// Apps to choose from for privacy mode: the ones open now, plus any already chosen.
fn privacy_list(a: &App) -> Vec<PrivacyApp> {
    let mut list: Vec<PrivacyApp> = Vec::new();
    let mut icons = IconCache::default();
    for w in win::list_windows(&[a.hwnd, a.picker_hwnd], win::current_desktop()) {
        let name = win::exe_name(&w.exe).to_lowercase();
        if name.is_empty() || name == "winoverview" || list.iter().any(|p| p.name == name) {
            continue;
        }
        list.push(PrivacyApp { on: a.config.private_apps.contains(&name), icon: icons.get(&w), name: name.into() });
    }
    for name in &a.config.private_apps {
        if !list.iter().any(|p| p.name == name.as_str()) {
            list.push(PrivacyApp { name: name.as_str().into(), icon: Default::default(), on: true });
        }
    }
    list.sort_by(|x, y| y.on.cmp(&x.on).then(x.name.cmp(&y.name)));
    list
}

fn save_settings(app: &Shared) {
    let Some(s) = app.borrow().settings.as_ref().map(|s| s.clone_strong()) else { return };
    let hotkey = s.get_hotkey().trim().to_string();

    if let Err(e) = register_hotkey(app, &hotkey) {
        s.set_status(e.into());
        s.set_status_error(true);
        return;
    }

    let result = {
        let mut a = app.borrow_mut();
        a.config.hotkey = hotkey.clone();
        a.config.mode = if s.get_previews() { "previews" } else { "icons" }.into();
        a.config.hover_preview = s.get_hover_preview();
        a.config.desktops_per_row = s.get_per_row().clamp(1, 10) as u32;
        a.config.top_margin_percent = s.get_top_margin().clamp(0, 40) as f64;
        a.config.open_on_mouse_monitor = s.get_mouse_monitor();
        a.config.free_layout = s.get_free_layout();
        a.config.orientation = if s.get_vertical() { "vertical" } else { "horizontal" }.into();
        a.config.widgets_first = s.get_widgets_first();
        a.config.widget_size = s.get_widget_size().to_string();
        a.config.privacy_mode = s.get_privacy();
        let privacy_apps = s.get_privacy_apps();
        for i in 0..privacy_apps.row_count() {
            if let Some(p) = privacy_apps.row_data(i) {
                a.config.set_private(&p.name, p.on);
            }
        }
        let f = s.get_flags();
        use layout::Widget;
        for (w, on) in [(Widget::Notes, f.notes), (Widget::Music, f.music), (Widget::Timer, f.timer), (Widget::Weather, f.weather), (Widget::Calendar, f.calendar)] {
            a.config.set_widget(w, on);
        }
        let city = s.get_city().trim().to_string();
        let unit = s.get_fahrenheit();
        if city != a.config.weather_city || unit != a.config.weather_fahrenheit {
            a.config.weather_city = city;
            a.config.weather_fahrenheit = unit;
            a.config.weather_latitude = None; // look the city up again
            a.config.weather_longitude = None;
            a.weather_at = None;
        }
        a.config.accent_color = config::to_hex(s.get_accent());
        a.config.theme = s.get_theme().to_string();
        if let Some(t) = &a.tray {
            let _ = t.icon.set_tooltip(Some(format!("WinOverview ({hotkey})")));
        }
        a.config.save()
    };
    apply_look(app);
    widget_ui::settings_changed(app);
    win::set_startup(s.get_startup());
    if let Some(t) = &app.borrow().tray {
        t.startup.set_checked(win::startup_enabled());
    }

    match result {
        Ok(()) => {
            s.set_status(format!("Saved. Press {hotkey} to try it.").into());
            s.set_status_error(false);
        }
        Err(e) => {
            s.set_status(format!("Couldn't save: {e}").into());
            s.set_status_error(true);
        }
    }
}

/// Apply the theme and accent colour to every window.
fn apply_look(app: &Shared) {
    let mut a = app.borrow_mut();
    a.theme = themes::find(&a.config.theme);
    let (ui_theme, accent) = (a.theme.to_ui(), a.config.accent());
    a.ui.set_theme(ui_theme.clone());
    a.ui.set_accent(accent);
    if let Some(p) = &a.picker {
        p.set_theme(ui_theme.clone());
        p.set_accent(accent);
    }
    if let Some(v) = &a.stack_view {
        v.set_theme(ui_theme);
        v.set_accent(accent);
    }
    let (dark, border) = (a.theme.dark, a.theme.border_bgr());
    for h in [a.hwnd, a.picker_hwnd, a.stack_hwnd] {
        if h != 0 {
            win::set_border(h, dark, border);
        }
    }
    free::apply_look(&a);
    a.popup = None; // made again with the new colours when next needed
}
