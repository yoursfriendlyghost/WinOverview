//! Everything that talks to Windows: listing app windows, icons, focusing/closing,
//! virtual desktops (via the winvd crate), monitors, window styling, autostart.

use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn hwnd(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

// ===================== app windows =====================

#[derive(Clone, Debug)]
pub struct WinInfo {
    pub hwnd: isize,
    pub title: String,
    pub exe: Option<String>,
    pub desktop: u32,
}

/// App windows on all virtual desktops (like Alt+Tab), top-most first.
pub fn list_windows(exclude: &[isize], current: u32) -> Vec<WinInfo> {
    let mut handles: Vec<isize> = Vec::new();
    unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
        let v = &mut *(lp.0 as *mut Vec<isize>);
        v.push(h.0 as isize);
        TRUE
    }
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }

    let me = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    for h in handles {
        if exclude.contains(&h) {
            continue;
        }
        // Never list our own windows (panel, widgets, settings, snap picker)
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd(h), Some(&mut pid));
        }
        if pid == me {
            continue;
        }
        let w = hwnd(h);
        unsafe {
            if !IsWindowVisible(w).as_bool() {
                continue;
            }
            if GetWindow(w, GW_OWNER).map_or(false, |o| !o.is_invalid()) {
                continue; // dialogs / popups
            }
            let ex = GetWindowLongPtrW(w, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 {
                continue;
            }
        }

        let desktop = winvd::get_desktop_by_window(w).and_then(|d| d.get_index()).ok();
        let pinned = winvd::is_pinned_window(w).unwrap_or(false);

        // Cloaked by the shell = on another virtual desktop (keep). Any other cloak = hidden app (skip).
        let mut cloaked: u32 = 0;
        unsafe {
            let _ = DwmGetWindowAttribute(w, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4);
        }
        if cloaked != 0 && (cloaked != DWM_CLOAKED_SHELL || desktop.is_none()) {
            continue;
        }

        let title = window_text(w);
        if title.is_empty() || class_name(w) == "Progman" {
            continue;
        }
        let desktop = if pinned { current } else { desktop.unwrap_or(current) };
        out.push(WinInfo { hwnd: h, title, exe: exe_path(w), desktop });
    }
    out
}

fn window_text(w: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(w);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(w, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn class_name(w: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(w, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn exe_path(w: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }
}

pub fn exe_name(path: &Option<String>) -> String {
    path.as_deref()
        .and_then(|p| p.rsplit('\\').next())
        .map(|n| n.trim_end_matches(".exe").trim_end_matches(".EXE").to_string())
        .unwrap_or_default()
}

// ===================== icons =====================

/// App icons as images, cached per .exe (or per window for UWP apps).
#[derive(Default)]
pub struct IconCache {
    by_exe: HashMap<String, Option<slint::Image>>,
    by_window: HashMap<isize, Option<slint::Image>>,
}

impl IconCache {
    pub fn get(&mut self, info: &WinInfo) -> slint::Image {
        let frame_host = info
            .exe
            .as_deref()
            .map_or(false, |e| e.to_ascii_lowercase().ends_with("applicationframehost.exe"));

        if let (Some(exe), false) = (&info.exe, frame_host) {
            let img = self.by_exe.entry(exe.clone()).or_insert_with(|| icon_from_file(exe)).clone();
            if let Some(img) = img {
                return img;
            }
        }
        self.by_window
            .entry(info.hwnd)
            .or_insert_with(|| icon_from_window(hwnd(info.hwnd)))
            .clone()
            .unwrap_or_default()
    }

    /// Forget windows that have closed.
    pub fn prune(&mut self, open: &[isize]) {
        self.by_window.retain(|h, _| open.contains(h));
    }
}

fn icon_from_file(path: &str) -> Option<slint::Image> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut large = HICON::default();
    let mut small = HICON::default();
    unsafe {
        let size = 64u32 | (16u32 << 16);
        SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut large), Some(&mut small), size).ok().ok()?;
        let img = hicon_to_image(large);
        if !large.is_invalid() {
            let _ = DestroyIcon(large);
        }
        if !small.is_invalid() {
            let _ = DestroyIcon(small);
        }
        img
    }
}

fn icon_from_window(w: HWND) -> Option<slint::Image> {
    unsafe {
        let mut result: usize = 0;
        SendMessageTimeoutW(w, WM_GETICON, WPARAM(ICON_BIG as usize), LPARAM(0), SMTO_ABORTIFHUNG, 100, Some(&mut result));
        if result == 0 {
            result = GetClassLongPtrW(w, GCLP_HICON);
        }
        if result == 0 {
            return None;
        }
        hicon_to_image(HICON(result as *mut c_void)) // not ours to destroy
    }
}

unsafe fn hicon_to_image(icon: HICON) -> Option<slint::Image> {
    if icon.is_invalid() {
        return None;
    }
    let mut info = ICONINFO::default();
    GetIconInfo(icon, &mut info).ok()?;
    let color = info.hbmColor;
    let mask = info.hbmMask;

    let result = (|| {
        if color.is_invalid() {
            return None;
        }
        let mut bm = BITMAP::default();
        if GetObjectW(color, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if w <= 0 || h <= 0 {
            return None;
        }
        let pixels = read_bitmap(color, w, h)?;
        let mut rgba = Vec::with_capacity(pixels.len());
        let mut any_alpha = false;
        for px in pixels.chunks_exact(4) {
            any_alpha |= px[3] != 0;
            rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        // Old-style icon without alpha: use the mask instead
        if !any_alpha {
            let m = read_bitmap(mask, w, h);
            for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
                let transparent = m.as_ref().map_or(false, |m| m[i * 4] != 0);
                px[3] = if transparent { 0 } else { 255 };
            }
        }
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, w as u32, h as u32);
        Some(slint::Image::from_rgba8(buf))
    })();

    if !color.is_invalid() {
        let _ = DeleteObject(color);
    }
    if !mask.is_invalid() {
        let _ = DeleteObject(mask);
    }
    result
}

unsafe fn read_bitmap(bmp: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h; // top-down
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB.0;
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (lines > 0).then_some(buf)
}

// ===================== actions =====================

pub fn focus_window(h: isize) {
    let w = hwnd(h);
    unsafe {
        if IsIconic(w).as_bool() {
            let _ = ShowWindow(w, SW_RESTORE);
        }
        // Windows only lets the foreground app hand over focus; a tap of Alt counts as "user input"
        let alt = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, dwFlags: flags, ..Default::default() } },
        };
        SendInput(&[alt(KEYBD_EVENT_FLAGS(0)), alt(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(w);
    }
}

pub fn close_window(h: isize) {
    unsafe {
        let _ = PostMessageW(hwnd(h), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn bring_to_front(h: isize) {
    unsafe {
        let _ = SetForegroundWindow(hwnd(h));
    }
}

// ===================== virtual desktops =====================

pub fn desktop_count() -> u32 {
    winvd::get_desktop_count().unwrap_or(1).max(1)
}

pub fn current_desktop() -> u32 {
    winvd::get_current_desktop().and_then(|d| d.get_index()).unwrap_or(0)
}

/// Switch to desktop `i`. Call it while one of our windows still has focus: if an app on the
/// old desktop has focus, Windows can pull you straight back to it.
pub fn switch_desktop(i: u32) -> bool {
    if current_desktop() == i {
        return true;
    }
    let first = winvd::switch_desktop(i);
    if current_desktop() == i {
        return true;
    }
    // Second try with the taskbar focused, so no app window can hold us on the old desktop
    unsafe {
        if let Ok(taskbar) = FindWindowW(w!("Shell_TrayWnd"), None) {
            let _ = SetForegroundWindow(taskbar);
        }
    }
    let second = winvd::switch_desktop(i);
    let ok = current_desktop() == i;
    if !ok {
        crate::config::log(&format!("Couldn't switch to desktop {}: {first:?} / {second:?}", i + 1));
    }
    ok
}

/// The top-most app window on a desktop (from a list in top-most-first order).
pub fn top_window_on(windows: &[WinInfo], desktop: u32) -> Option<isize> {
    windows.iter().find(|w| w.desktop == desktop && !is_minimized(w.hwnd)).map(|w| w.hwnd)
}

pub fn move_to_desktop(h: isize, i: u32) {
    let _ = winvd::move_window_to_desktop(i, &hwnd(h));
}

pub fn is_pinned(h: isize) -> bool {
    winvd::is_pinned_window(hwnd(h)).unwrap_or(false)
}

pub fn pin(h: isize) {
    let _ = winvd::pin_window(hwnd(h));
}

/// Move desktop `from` to position `to` (both 0-based).
pub fn move_desktop(from: u32, to: u32) {
    if let Err(e) = winvd::move_desktop(from, to) {
        crate::config::log(&format!("Couldn't move desktop {} to {}: {e:?}", from + 1, to + 1));
    }
}

/// Returns the new desktop's number.
pub fn create_desktop() -> Option<u32> {
    winvd::create_desktop().ok().and_then(|d| d.get_index().ok())
}

pub fn remove_desktop(i: u32, fallback: u32) {
    let _ = winvd::remove_desktop(i, fallback);
}

// ===================== monitors =====================

/// Work area (screen minus taskbar) in physical pixels, and its scale (1.0 = 100%).
pub struct Monitor {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f32,
}

pub fn monitor(under_mouse: bool) -> Monitor {
    unsafe {
        let mut pt = POINT::default();
        if under_mouse {
            let _ = GetCursorPos(&mut pt);
        }
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let r = mi.rcWork;
        Monitor { left: r.left, top: r.top, width: r.right - r.left, height: r.bottom - r.top, scale: dx as f32 / 96.0 }
    }
}

/// The monitor containing a point (physical pixels): its work area, full bounds and scale.
pub fn monitor_at(x: i32, y: i32) -> (Monitor, Monitor) {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx as f32 / 96.0;
        let m = |r: RECT| Monitor { left: r.left, top: r.top, width: r.right - r.left, height: r.bottom - r.top, scale };
        (m(mi.rcWork), m(mi.rcMonitor))
    }
}

// ===================== our own window =====================

/// Rounded corners, a border to match the theme, no taskbar button, and visible on every desktop.
pub fn style_overlay(h: isize, dark: bool, border_bgr: u32) {
    let w = hwnd(h);
    unsafe {
        let ex = GetWindowLongPtrW(w, GWL_EXSTYLE);
        let ex = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(w, GWL_EXSTYLE, ex);
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(w, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as *const c_void, 4);
    }
    set_border(h, dark, border_bgr);
    pin(h);
}

/// Outline colour (0x00BBGGRR) and light/dark window frame.
pub fn set_border(h: isize, dark: bool, border_bgr: u32) {
    let w = hwnd(h);
    unsafe {
        let _ = DwmSetWindowAttribute(w, DWMWA_BORDER_COLOR, &border_bgr as *const _ as *const c_void, 4);
        let d: i32 = dark.into();
        let _ = DwmSetWindowAttribute(w, DWMWA_USE_IMMERSIVE_DARK_MODE, &d as *const _ as *const c_void, 4);
    }
}

pub fn is_minimized(h: isize) -> bool {
    unsafe { IsIconic(hwnd(h)).as_bool() }
}

/// Move/resize a window to a zone (fractions of `area`, physical pixels).
pub fn place_window(h: isize, zone: (f32, f32, f32, f32), area: &Monitor) {
    let w = hwnd(h);
    let (zx, zy, zw, zh) = zone;
    unsafe {
        if zw >= 1.0 && zh >= 1.0 {
            let _ = ShowWindow(w, SW_MAXIMIZE);
            return;
        }
        if IsIconic(w).as_bool() || IsZoomed(w).as_bool() {
            let _ = ShowWindow(w, SW_RESTORE);
        }
        let x = area.left + (zx * area.width as f32).round() as i32;
        let y = area.top + (zy * area.height as f32).round() as i32;
        let cw = (zw * area.width as f32).round() as i32;
        let ch = (zh * area.height as f32).round() as i32;

        // Windows have invisible resize borders; grow the rect so the visible edges line up
        let (mut l, mut t, mut r, mut b) = (0, 0, 0, 0);
        let mut wr = RECT::default();
        let mut fr = RECT::default();
        if GetWindowRect(w, &mut wr).is_ok()
            && DwmGetWindowAttribute(w, DWMWA_EXTENDED_FRAME_BOUNDS, &mut fr as *mut RECT as *mut c_void,
                std::mem::size_of::<RECT>() as u32).is_ok()
        {
            l = fr.left - wr.left;
            t = fr.top - wr.top;
            r = wr.right - fr.right;
            b = wr.bottom - fr.bottom;
        }
        let _ = SetWindowPos(w, None, x - l, y - t, cw + l + r, ch + t + b, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn native_hide(h: isize) {
    unsafe {
        let _ = ShowWindow(hwnd(h), SW_HIDE);
    }
}

pub fn native_show(h: isize) {
    unsafe {
        let _ = ShowWindow(hwnd(h), SW_SHOW);
    }
}

// ===================== start with Windows =====================

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("WinOverview");

pub fn startup_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn set_startup(on: bool) {
    unsafe {
        if on {
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
            let value: Vec<u16> = format!("\"{exe}\"").encode_utf16().chain(Some(0)).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                RUN_NAME,
                REG_SZ.0,
                Some(value.as_ptr() as *const c_void),
                (value.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME);
        }
    }
}

// ===================== app lifetime =====================

/// True if this is the only copy running. Waits a bit first when restarting.
pub fn single_instance(wait: bool) -> bool {
    for _ in 0..if wait { 30 } else { 1 } {
        unsafe {
            if let Ok(handle) = CreateMutexW(None, true, w!("WinOverview.SingleInstance")) {
                if GetLastError() != ERROR_ALREADY_EXISTS {
                    // Not closed: the mutex is held until the process exits
                    return true;
                }
                let _ = CloseHandle(handle);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// A window's rectangle on screen: (left, top, right, bottom).
pub fn window_rect(h: isize) -> Option<(i32, i32, i32, i32)> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd(h), &mut r).ok()? };
    Some((r.left, r.top, r.right, r.bottom))
}

/// The mouse position on screen (physical pixels).
pub fn cursor_pos() -> (i32, i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

/// Make `owner` own window `h`: no taskbar button of its own, always in front of its owner,
/// and it goes wherever the owner goes.
pub fn set_owner(h: isize, owner: isize) {
    if h == 0 || owner == 0 {
        return;
    }
    unsafe {
        SetWindowLongPtrW(hwnd(h), GWLP_HWNDPARENT, owner);
    }
}

/// Start an app from its Windows app id (e.g. "Spotify.exe" or "SpotifyAB.SpotifyMusic_...!Spotify").
pub fn open_app(app_id: &str) {
    let target = format!("shell:AppsFolder\\{app_id}");
    let wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

/// A sound and a small message that doesn't hold anything up (timer finished).
pub fn alert(text: &str) {
    unsafe {
        let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONINFORMATION);
    }
    let text = text.to_string();
    std::thread::spawn(move || {
        let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(None, PCWSTR(t.as_ptr()), w!("WinOverview"), MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND);
        }
    });
}

pub fn message_box(text: &str) {
    let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(None, PCWSTR(t.as_ptr()), w!("WinOverview"), MB_OK | MB_ICONWARNING);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vdesktop_create_and_remove() {
        let initial_count = desktop_count();
        println!("Initial desktop count: {}", initial_count);
        let new_idx = create_desktop();
        println!("Created desktop index: {:?}", new_idx);
        assert!(new_idx.is_some(), "create_desktop failed");
        let count_after_create = desktop_count();
        println!("Count after create: {}", count_after_create);
        assert_eq!(count_after_create, initial_count + 1);

        let created = new_idx.unwrap();
        remove_desktop(created, 0);
        let count_after_remove = desktop_count();
        println!("Count after remove: {}", count_after_remove);
        assert_eq!(count_after_remove, initial_count);
    }
}
