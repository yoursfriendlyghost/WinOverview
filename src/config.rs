//! Settings in %AppData%\WinOverview\config.json (shared with the old C# version:
//! settings this version doesn't know about are kept when saving).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Config {
    /// e.g. "Alt+`", "Ctrl+Alt+Space", "Win+Shift+W"
    pub hotkey: String,
    /// "#RRGGBB"
    pub accent_color: String,
    /// Gap above the panel, as % of the screen height
    pub top_margin_percent: f64,
    pub desktops_per_row: u32,
    pub open_on_mouse_monitor: bool,
    /// "icons" or "previews" (live previews)
    pub mode: String,
    /// Icons view: show a live preview while hovering an app
    pub hover_preview: bool,
    pub hover_preview_delay_ms: u64,
    /// Name of a theme in src/themes.rs
    pub theme: String,

    /// Arrangement: "horizontal" (desktops side by side) or "vertical" (desktops stacked)
    pub orientation: String,
    /// Placement: widgets float in their own capsules anywhere on screen (Free),
    /// instead of sitting inside the panel (Fixed)
    pub free_layout: bool,
    /// Widgets to show, in order: "notes", "music", "timer", "weather"
    pub widgets: Vec<String>,
    /// Widgets above / left of the desktops instead of below / right
    pub widgets_first: bool,
    /// Where the panel sits: fractions (0..1) of the free space across/down the screen.
    /// Below 0 = the default spot (centred, TopMarginPercent down). Set in edit mode.
    pub panel_x: f64,
    pub panel_y: f64,

    /// Weather: a city name, looked up once and remembered as coordinates
    pub weather_city: String,
    pub weather_latitude: Option<f64>,
    pub weather_longitude: Option<f64>,
    pub weather_fahrenheit: bool,

    /// Timer length in minutes
    pub timer_minutes: u32,

    /// Free layout: the capsules (widgets that move together) and where each one sits
    pub widget_groups: Vec<WidgetGroup>,
    /// "small", "medium" or "large" - every widget is this size
    pub widget_size: String,

    /// Privacy mode: these apps show only their icon (no preview, no window title)
    pub privacy_mode: bool,
    /// App names (the .exe name without ".exe", lower case), e.g. "whatsapp", "msedge"
    pub private_apps: Vec<String>,
}

/// A capsule in the Free layout: one or more widgets side by side (or stacked), moved together.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct WidgetGroup {
    /// Widget names, in order
    pub widgets: Vec<String>,
    /// Stacked instead of side by side
    pub vertical: bool,
    /// Where it sits: fractions (0..1) of the free space across/down the screen; below 0 = next to the panel
    pub x: f64,
    pub y: f64,
}

impl Default for WidgetGroup {
    fn default() -> Self {
        WidgetGroup { widgets: Vec::new(), vertical: false, x: -1.0, y: -1.0 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "Alt+`".into(),
            accent_color: "#8AB4F8".into(),
            top_margin_percent: 10.0,
            desktops_per_row: 5,
            open_on_mouse_monitor: true,
            mode: "icons".into(),
            hover_preview: true,
            hover_preview_delay_ms: 400,
            theme: "Midnight".into(),
            orientation: "horizontal".into(),
            widgets: Vec::new(),
            widgets_first: false,
            free_layout: false,
            panel_x: -1.0,
            panel_y: -1.0,
            weather_city: String::new(),
            weather_latitude: None,
            weather_longitude: None,
            weather_fahrenheit: false,
            timer_minutes: 25,
            widget_groups: Vec::new(),
            widget_size: "medium".into(),
            privacy_mode: false,
            private_apps: Vec::new(),
        }
    }
}

pub fn folder() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("WinOverview")
}

/// Add a line to %AppData%\WinOverview\error.log (for problems worth reporting).
pub fn log(line: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(folder());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(folder().join("error.log")) {
        let _ = writeln!(f, "{line}");
    }
}

fn file() -> PathBuf {
    folder().join("config.json")
}

impl Config {
    pub fn load() -> Config {
        let text = std::fs::read_to_string(file()).unwrap_or_default();
        let mut c: Config = serde_json::from_str(&text).unwrap_or_default();
        c.desktops_per_row = c.desktops_per_row.clamp(1, 10);
        c.top_margin_percent = c.top_margin_percent.clamp(0.0, 40.0);
        c.timer_minutes = c.timer_minutes.clamp(1, 180);
        if c.orientation.eq_ignore_ascii_case("free") {
            c.free_layout = true;
            c.orientation = "horizontal".into();
        }
        if c.panel_x > 1.0 || c.panel_y > 1.0 || c.panel_x.is_nan() || c.panel_y.is_nan() {
            c.panel_x = -1.0;
            c.panel_y = -1.0;
        }
        if text.is_empty() {
            let _ = c.save(); // create the file so people can find it
        }
        c
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(folder())?;
        // Merge into what's on disk, so unknown settings survive
        let mut doc: serde_json::Value = std::fs::read_to_string(file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .filter(|v: &serde_json::Value| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        if let (Some(obj), Ok(serde_json::Value::Object(mine))) = (doc.as_object_mut(), serde_json::to_value(self)) {
            for (k, v) in mine {
                obj.insert(k, v);
            }
        }
        std::fs::write(file(), serde_json::to_string_pretty(&doc).unwrap_or_default())
    }

    pub fn previews(&self) -> bool {
        self.mode.eq_ignore_ascii_case("previews")
    }

    pub fn vertical(&self) -> bool {
        self.orientation.eq_ignore_ascii_case("vertical")
    }

    /// Desktops stacked in a column (in both Fixed and Free placement)
    pub fn desktops_vertical(&self) -> bool {
        self.vertical()
    }

    pub fn free(&self) -> bool {
        self.free_layout
    }

    /// Widget card size (logical pixels) for the chosen size.
    pub fn widget_size_px(&self) -> (f32, f32) {
        match self.widget_size.to_ascii_lowercase().as_str() {
            "small" => (216.0, 140.0),
            "large" => (296.0, 188.0),
            _ => (248.0, 156.0),
        }
    }

    /// The capsules, tidied up: every widget that's on is in exactly one, nothing else is.
    pub fn groups(&mut self) -> Vec<WidgetGroup> {
        let on: Vec<String> = self.widget_list().iter().map(|w| w.key().to_string()).collect();
        let mut seen: Vec<String> = Vec::new();
        let mut groups: Vec<WidgetGroup> = Vec::new();
        for g in &self.widget_groups {
            let mut g = g.clone();
            g.widgets.retain(|k| {
                let k = k.to_ascii_lowercase();
                let ok = on.contains(&k) && !seen.contains(&k);
                if ok {
                    seen.push(k);
                }
                ok
            });
            for k in g.widgets.iter_mut() {
                *k = k.to_ascii_lowercase();
            }
            if !g.widgets.is_empty() {
                groups.push(g);
            }
        }
        for k in on {
            if !seen.contains(&k) {
                groups.push(WidgetGroup { widgets: vec![k], ..Default::default() });
            }
        }
        self.widget_groups = groups.clone();
        groups
    }

    /// Is this app's preview hidden? (`exe_name` as from win::exe_name)
    pub fn is_private(&self, exe_name: &str) -> bool {
        self.privacy_mode && !exe_name.is_empty() && self.private_apps.iter().any(|a| a.eq_ignore_ascii_case(exe_name))
    }

    pub fn set_private(&mut self, exe_name: &str, on: bool) {
        let name = exe_name.to_lowercase();
        self.private_apps.retain(|a| *a != name);
        if on && !name.is_empty() {
            self.private_apps.push(name);
            self.private_apps.sort();
        }
    }

    /// The widgets that are switched on, in order (unknown names are skipped).
    pub fn widget_list(&self) -> Vec<crate::layout::Widget> {
        let mut v = Vec::new();
        for w in self.widgets.iter().filter_map(|k| crate::layout::Widget::from_key(k)) {
            if !v.contains(&w) {
                v.push(w);
            }
        }
        v
    }

    pub fn has_widget(&self, w: crate::layout::Widget) -> bool {
        self.widget_list().contains(&w)
    }

    /// Switch a widget on/off (new ones go at the end).
    pub fn set_widget(&mut self, w: crate::layout::Widget, on: bool) {
        let mut list = self.widget_list();
        list.retain(|x| *x != w);
        if on {
            list.push(w);
        }
        self.widgets = list.iter().map(|w| w.key().to_string()).collect();
    }

    pub fn accent(&self) -> slint::Color {
        parse_hex(&self.accent_color).unwrap_or(slint::Color::from_rgb_u8(0x8a, 0xb4, 0xf8))
    }
}

pub fn parse_hex(s: &str) -> Option<slint::Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn to_hex(c: slint::Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue())
}

/// "Alt+`" -> a global hotkey. Returns None if it can't be understood.
pub fn parse_hotkey(text: &str) -> Option<global_hotkey::hotkey::HotKey> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use std::str::FromStr;

    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;
    // "+" itself can't be the key, so splitting on '+' is safe
    for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "alt" => mods |= Modifiers::ALT,
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "win" | "windows" | "super" | "meta" => mods |= Modifiers::META,
            other => {
                if key.is_some() {
                    return None;
                }
                let name = match other {
                    "`" | "~" => "Backquote".to_string(),
                    "space" => "Space".into(),
                    "tab" => "Tab".into(),
                    "enter" | "return" => "Enter".into(),
                    "esc" | "escape" => "Escape".into(),
                    "-" => "Minus".into(),
                    "=" => "Equal".into(),
                    "," => "Comma".into(),
                    "." => "Period".into(),
                    "/" => "Slash".into(),
                    ";" => "Semicolon".into(),
                    "'" => "Quote".into(),
                    "[" => "BracketLeft".into(),
                    "]" => "BracketRight".into(),
                    "\\" => "Backslash".into(),
                    s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic() => format!("Key{}", s.to_ascii_uppercase()),
                    s if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => format!("Digit{s}"),
                    s if s.starts_with('f') && s[1..].parse::<u8>().map_or(false, |n| (1..=24).contains(&n)) => s.to_ascii_uppercase(),
                    _ => return None,
                };
                key = Some(Code::from_str(&name).ok()?);
            }
        }
    }
    let key = key?;
    if mods.is_empty() {
        return None; // a bare key would steal normal typing
    }
    Some(HotKey::new(Some(mods), key))
}
