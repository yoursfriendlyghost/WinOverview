//! Colour themes for the overview panel and the snap picker.

use crate::Theme;
use slint::Color;

pub struct Preset {
    pub name: &'static str,
    pub dark: bool,
    // 0xAARRGGBB
    panel: u32,
    tile: u32,
    tile_hover: u32,
    tile_current: u32,
    border: u32,
    text: u32,
    faint: u32,
    pill: u32,
    cell: u32,
    cell_hover: u32,
}

const fn p(
    name: &'static str,
    dark: bool,
    panel: u32,
    tile: u32,
    tile_hover: u32,
    tile_current: u32,
    border: u32,
    text: u32,
    faint: u32,
    pill: u32,
    cell: u32,
    cell_hover: u32,
) -> Preset {
    Preset { name, dark, panel, tile, tile_hover, tile_current, border, text, faint, pill, cell, cell_hover }
}

pub const PRESETS: &[Preset] = &[
    p("Midnight", true, 0xFF1B1B1F, 0x08FFFFFF, 0x0FFFFFFF, 0x12FFFFFF, 0x16FFFFFF, 0xFFE6E6EA, 0x50FFFFFF, 0x20FFFFFF, 0x0BFFFFFF, 0x1EFFFFFF),
    p("AMOLED", true, 0xFF000000, 0x0AFFFFFF, 0x12FFFFFF, 0x16FFFFFF, 0x1AFFFFFF, 0xFFF2F2F2, 0x55FFFFFF, 0x22FFFFFF, 0x0DFFFFFF, 0x20FFFFFF),
    p("Nord", true, 0xFF2E3440, 0xFF3B4252, 0xFF434C5E, 0xFF434C5E, 0xFF4C566A, 0xFFECEFF4, 0xFF8892A6, 0xFF4C566A, 0xFF434C5E, 0xFF4C566A),
    p("Mocha", true, 0xFF1E1E2E, 0xFF262637, 0xFF2E2E42, 0xFF313244, 0xFF3A3A50, 0xFFCDD6F4, 0xFF7F849C, 0xFF45475A, 0xFF313244, 0xFF45475A),
    p("Rosé Pine", true, 0xFF191724, 0xFF1F1D2E, 0xFF26233A, 0xFF26233A, 0xFF2F2C44, 0xFFE0DEF4, 0xFF6E6A86, 0xFF393552, 0xFF26233A, 0xFF393552),
    p("Forest", true, 0xFF1A201C, 0xFF222A25, 0xFF29332D, 0xFF2C3730, 0xFF34403A, 0xFFE3EBE5, 0xFF7D8C82, 0xFF35443B, 0xFF2A342E, 0xFF35443B),
    p("Light", false, 0xFFF4F4F6, 0x08000000, 0x0F000000, 0x0E000000, 0x18000000, 0xFF1C1C21, 0x7A000000, 0x16000000, 0x0B000000, 0x18000000),
];

pub fn find(name: &str) -> &'static Preset {
    PRESETS.iter().find(|t| t.name.eq_ignore_ascii_case(name)).unwrap_or(&PRESETS[0])
}

fn c(argb: u32) -> Color {
    Color::from_argb_encoded(argb)
}

impl Preset {
    pub fn to_ui(&self) -> Theme {
        Theme {
            panel: c(self.panel),
            tile: c(self.tile),
            tile_hover: c(self.tile_hover),
            tile_current: c(self.tile_current),
            border: c(self.border),
            text: c(self.text),
            faint: c(self.faint),
            pill: c(self.pill),
            cell: c(self.cell),
            cell_hover: c(self.cell_hover),
            dark: self.dark,
        }
    }

    /// Panel colour as 0x00BBGGRR (for Win32).
    pub fn panel_bgr(&self) -> u32 {
        let (r, g, b) = ((self.panel >> 16) & 0xFF, (self.panel >> 8) & 0xFF, self.panel & 0xFF);
        (b << 16) | (g << 8) | r
    }

    /// The panel's outline, blended onto the panel colour, as 0x00BBGGRR.
    pub fn border_bgr(&self) -> u32 {
        let a = ((self.border >> 24) & 0xFF) as f32 / 255.0;
        let mix = |shift: u32| {
            let top = ((self.border >> shift) & 0xFF) as f32;
            let bottom = ((self.panel >> shift) & 0xFF) as f32;
            ((top * a + bottom * (1.0 - a)).round() as u32).min(255)
        };
        (mix(0) << 16) | (mix(8) << 8) | mix(16)
    }
}

pub fn choices() -> Vec<crate::ThemeChoice> {
    PRESETS
        .iter()
        .map(|t| crate::ThemeChoice { name: t.name.into(), panel: c(t.panel), tile: c(t.tile_current), text: c(t.text) })
        .collect()
}
