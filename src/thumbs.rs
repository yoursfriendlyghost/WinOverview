//! Live window previews, drawn by Windows itself (DWM thumbnails):
//! - `Thumbs`: previews inside the overview panel ("Live previews" view)
//! - `HoverPopup`: a small floating preview while hovering an app ("Icons" view)

use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::win::hwnd;

/// A rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Px {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Px {
    fn rect(&self) -> RECT {
        RECT { left: self.left, top: self.top, right: self.right, bottom: self.bottom }
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// A window's visible frame on screen (without the invisible resize borders),
/// and that frame relative to the window (what a preview should show).
pub fn frame_of(h: isize) -> Option<(Px, Px)> {
    let w = hwnd(h);
    unsafe {
        let mut wr = RECT::default();
        GetWindowRect(w, &mut wr).ok()?;
        let mut fr = RECT::default();
        let ok = DwmGetWindowAttribute(
            w,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut fr as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        if !ok || fr.right <= fr.left || fr.bottom <= fr.top {
            fr = wr;
        }
        if fr.right <= fr.left || fr.bottom <= fr.top {
            return None;
        }
        let frame = Px { left: fr.left, top: fr.top, right: fr.right, bottom: fr.bottom };
        let source = Px { left: fr.left - wr.left, top: fr.top - wr.top, right: fr.right - wr.left, bottom: fr.bottom - wr.top };
        Some((frame, source))
    }
}

/// Fit a w×h shape inside `area`, centred, keeping its proportions.
pub fn fit(area: Px, w: i32, h: i32) -> Px {
    let (aw, ah) = (area.width().max(1) as f32, area.height().max(1) as f32);
    let aspect = w.max(1) as f32 / h.max(1) as f32;
    let (mut tw, mut th) = (aw, aw / aspect);
    if th > ah {
        th = ah;
        tw = ah * aspect;
    }
    let left = area.left + ((aw - tw) / 2.0).round() as i32;
    let top = area.top + ((ah - th) / 2.0).round() as i32;
    Px { left, top, right: left + tw.round() as i32, bottom: top + th.round() as i32 }
}

fn register(dest: HWND, target: isize, rect: Px, source: Px, opacity: u8) -> Option<isize> {
    unsafe {
        let thumb = DwmRegisterThumbnail(dest, hwnd(target)).ok()?;
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: rect.rect(),
            rcSource: source.rect(),
            opacity,
            fVisible: TRUE,
            fSourceClientAreaOnly: FALSE,
        };
        if DwmUpdateThumbnailProperties(thumb, &props).is_err() {
            let _ = DwmUnregisterThumbnail(thumb);
            return None;
        }
        Some(thumb)
    }
}

// ===================== previews inside the panel =====================

/// One preview: which window, where it goes (pixels inside the panel), how see-through.
#[derive(Clone, Copy, PartialEq)]
pub struct ThumbSpec {
    pub target: isize,
    pub dest: Px,
    pub source: Px,
    pub opacity: u8,
}

#[derive(Default)]
pub struct Thumbs {
    shown: Vec<(ThumbSpec, isize)>,
}

impl Thumbs {
    /// Show exactly these previews on `dest` (keeps the ones that didn't change).
    pub fn set(&mut self, dest: isize, specs: &[ThumbSpec]) {
        if self.shown.len() == specs.len() && self.shown.iter().zip(specs).all(|((a, _), b)| a == b) {
            return;
        }
        self.clear();
        for s in specs {
            if let Some(t) = register(hwnd(dest), s.target, s.dest, s.source, s.opacity) {
                self.shown.push((*s, t));
            }
        }
    }

    pub fn clear(&mut self) {
        for (_, t) in self.shown.drain(..) {
            unsafe {
                let _ = DwmUnregisterThumbnail(t);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }
}

// ===================== hover popup =====================

const TITLE_H: i32 = 30; // at 100%
const PAD: i32 = 8;

/// A small window that floats under the hovered app and shows it live.
/// It never takes focus and lets the mouse pass through.
pub struct HoverPopup {
    hwnd: isize,
    thumb: Option<isize>,
    target: isize,
    /// What WM_PAINT draws (boxed, so the window can keep a pointer to it)
    paint: Box<PaintData>,
}

struct PaintData {
    title: Vec<u16>,
    scale: f32,
    brush: HBRUSH,
    text_color: u32,
}

impl HoverPopup {
    pub fn new(owner: isize, panel_bgr: u32, text_bgr: u32, dark: bool, border_bgr: u32) -> Option<HoverPopup> {
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = w!("WinOverviewHoverPreview");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(popup_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&wc); // fails harmlessly if already registered

            let h = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                hwnd(owner), // owned by the overview, so it follows it across desktops
                None,
                instance,
                None,
            )
            .ok()?;

            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as *const c_void, 4);
            let _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &border_bgr as *const _ as *const c_void, 4);
            let d: i32 = dark.into();
            let _ = DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &d as *const _ as *const c_void, 4);

            let paint = Box::new(PaintData {
                title: Vec::new(),
                scale: 1.0,
                brush: CreateSolidBrush(COLORREF(panel_bgr)),
                text_color: text_bgr,
            });
            SetWindowLongPtrW(h, GWLP_USERDATA, &*paint as *const PaintData as isize);
            Some(HoverPopup { hwnd: h.0 as isize, thumb: None, target: 0, paint })
        }
    }

    /// Show `target` under (or above) the cell at `anchor` (screen pixels), kept inside `screen`.
    pub fn show(&mut self, target: isize, title: &str, anchor: Px, screen: Px, scale: f32) {
        self.hide();
        let Some((frame, source)) = frame_of(target) else { return };
        let s = |v: i32| (v as f32 * scale).round() as i32;

        // Preview up to 340 × 220 (at 100%), keeping the window's shape
        let max = Px { left: 0, top: 0, right: s(340), bottom: s(220) };
        let t = fit(max, frame.width(), frame.height());
        let (w, h) = (t.width() + s(PAD) * 2, t.height() + s(TITLE_H) + s(PAD));

        let mut x = anchor.left + anchor.width() / 2 - w / 2;
        x = x.clamp(screen.left + s(8), (screen.right - w - s(8)).max(screen.left));
        let mut y = anchor.bottom + s(10);
        if y + h > screen.bottom {
            y = anchor.top - h - s(10); // no room below: go above
        }

        self.target = target;
        self.paint.scale = scale;
        self.paint.title = title.encode_utf16().collect();
        let dest = Px { left: s(PAD), top: s(TITLE_H), right: s(PAD) + t.width(), bottom: s(TITLE_H) + t.height() };
        unsafe {
            let me = hwnd(self.hwnd);
            let _ = SetWindowPos(me, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let _ = InvalidateRect(me, None, true);
        }
        self.thumb = register(hwnd(self.hwnd), target, dest, source, 255);
    }

    pub fn hide(&mut self) {
        if let Some(t) = self.thumb.take() {
            unsafe {
                let _ = DwmUnregisterThumbnail(t);
            }
        }
        self.target = 0;
        unsafe {
            let _ = ShowWindow(hwnd(self.hwnd), SW_HIDE);
        }
    }

    pub fn showing(&self) -> isize {
        self.target
    }

}

impl PaintData {
    fn paint(&self, h: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(h, &mut rc);
            FillRect(dc, &rc, self.brush);

            let size = -((12.0 * self.scale).round() as i32);
            let font = CreateFontW(size, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, 0, 0,
                CLEARTYPE_QUALITY.0 as u32, 0, w!("Segoe UI"));
            let old = SelectObject(dc, font);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(self.text_color));
            let pad = (PAD as f32 * self.scale) as i32;
            let mut text_rc = RECT { left: pad + 2, top: 0, right: rc.right - pad, bottom: (TITLE_H as f32 * self.scale) as i32 };
            let mut title = self.title.clone();
            DrawTextW(dc, &mut title, &mut text_rc, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
            SelectObject(dc, old);
            let _ = DeleteObject(font);
            let _ = EndPaint(h, &ps);
        }
    }
}

impl Drop for HoverPopup {
    fn drop(&mut self) {
        self.hide();
        unsafe {
            SetWindowLongPtrW(hwnd(self.hwnd), GWLP_USERDATA, 0);
            let _ = DestroyWindow(hwnd(self.hwnd));
            let _ = DeleteObject(self.paint.brush);
        }
    }
}

unsafe extern "system" fn popup_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let p = GetWindowLongPtrW(h, GWLP_USERDATA) as *const PaintData;
            if let Some(p) = p.as_ref() {
                p.paint(h);
            } else {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(h, &mut ps);
                let _ = EndPaint(h, &ps);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}
