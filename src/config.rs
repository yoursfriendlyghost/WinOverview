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
        }
    }
}

pub fn folder() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("WinOverview")
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
