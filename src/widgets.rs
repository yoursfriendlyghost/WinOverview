//! What the widgets show. The slow parts (asking Windows what's playing, fetching the weather)
//! run on background threads and send their results to the UI thread.
//!
//! - Music: Windows' media controls (whatever is playing: Spotify, a browser tab, ...)
//! - Weather: open-meteo.com (free, no account), for a city set in Settings
//! - Notes: a text file in %AppData%\WinOverview
//! - Timer: lives in overview.rs (it's just a clock)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use windows::core::{HSTRING, Interface};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::IRandomAccessStream;

use crate::config;

// ===================== music =====================

#[derive(Clone, Default, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// The app playing it (e.g. "Spotify.exe", "MSEdge", "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify")
    pub app: String,
}

pub enum MusicCmd {
    Previous,
    PlayPause,
    Next,
}

pub enum MusicUpdate {
    /// Nothing is playing (or no app shares what it's playing)
    Nothing,
    Track(Track),
    /// Album art, 96×96 RGBA (None = the track has none)
    Art(Option<Vec<u8>>),
}

pub struct Music {
    commands: Sender<MusicCmd>,
    /// Only ask Windows while the panel is open
    pub active: Arc<AtomicBool>,
}

impl Music {
    /// Starts the background thread. `send` is called (on that thread) with every change.
    pub fn start(send: impl Fn(MusicUpdate) + Send + 'static) -> Music {
        let (tx, rx) = channel();
        let active = Arc::new(AtomicBool::new(false));
        let a2 = active.clone();
        std::thread::Builder::new()
            .name("music".into())
            .spawn(move || music_thread(rx, a2, send))
            .ok();
        Music { commands: tx, active }
    }

    pub fn send(&self, cmd: MusicCmd) {
        let _ = self.commands.send(cmd);
    }
}

fn music_thread(rx: Receiver<MusicCmd>, active: Arc<AtomicBool>, send: impl Fn(MusicUpdate)) {
    let mut manager: Option<SessionManager> = None;
    let mut last: Option<Track> = None;
    let mut last_art_for = String::new();

    loop {
        let wait = if active.load(Ordering::Relaxed) { 900 } else { 2500 };
        let cmd = match rx.recv_timeout(Duration::from_millis(wait)) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if cmd.is_none() && !active.load(Ordering::Relaxed) {
            continue;
        }
        if manager.is_none() {
            manager = SessionManager::RequestAsync().and_then(|op| op.get()).ok();
        }
        let Some(m) = &manager else { continue };
        let session = m.GetCurrentSession().ok();

        if let (Some(cmd), Some(s)) = (cmd, &session) {
            let _ = match cmd {
                MusicCmd::Previous => s.TrySkipPreviousAsync().and_then(|op| op.get()),
                MusicCmd::PlayPause => s.TryTogglePlayPauseAsync().and_then(|op| op.get()),
                MusicCmd::Next => s.TrySkipNextAsync().and_then(|op| op.get()),
            };
            std::thread::sleep(Duration::from_millis(250)); // let the app react
        }

        let track = session.as_ref().and_then(read_track);
        if track != last {
            match &track {
                None => send(MusicUpdate::Nothing),
                Some(t) => send(MusicUpdate::Track(t.clone())),
            }
            // New song: fetch its art (apps often add the art a moment after the title)
            if let (Some(t), Some(s)) = (&track, &session) {
                let key = format!("{}\u{1}{}", t.title, t.artist);
                if key != last_art_for {
                    let mut art = read_art(s);
                    for _ in 0..3 {
                        if art.is_some() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(700));
                        art = read_art(s);
                    }
                    send(MusicUpdate::Art(art));
                    last_art_for = key;
                }
            }
            last = track;
        }
    }
}

fn read_track(s: &Session) -> Option<Track> {
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().ok()?.to_string();
    if title.trim().is_empty() {
        return None;
    }
    let playing = s
        .GetPlaybackInfo()
        .and_then(|i| i.PlaybackStatus())
        .map_or(false, |st| st == Status::Playing);
    Some(Track {
        title,
        artist: props.Artist().map(|a| a.to_string()).unwrap_or_default(),
        playing,
        app: s.SourceAppUserModelId().map(|a| a.to_string()).unwrap_or_default(),
    })
}

const ART: u32 = 96;

/// The track's picture, cropped to a square and scaled to 96×96 RGBA.
fn read_art(s: &Session) -> Option<Vec<u8>> {
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let reference = props.Thumbnail().ok()?;
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let stream: IRandomAccessStream = stream.cast().ok()?;
    let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.get().ok()?;
    let (w, h) = (decoder.PixelWidth().ok()?, decoder.PixelHeight().ok()?);
    if w == 0 || h == 0 {
        return None;
    }
    // Scale so the short side is 96, then take the middle square
    let scale = ART as f64 / w.min(h) as f64;
    let (sw, sh) = (((w as f64 * scale).round() as u32).max(ART), ((h as f64 * scale).round() as u32).max(ART));
    let t = BitmapTransform::new().ok()?;
    t.SetScaledWidth(sw).ok()?;
    t.SetScaledHeight(sh).ok()?;
    t.SetInterpolationMode(BitmapInterpolationMode::Fant).ok()?;
    t.SetBounds(BitmapBounds { X: (sw - ART) / 2, Y: (sh - ART) / 2, Width: ART, Height: ART }).ok()?;
    let data = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &t,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .ok()?
        .get()
        .ok()?;
    let pixels = data.DetachPixelData().ok()?.to_vec();
    (pixels.len() == (ART * ART * 4) as usize).then_some(pixels)
}

pub fn art_image(rgba: &[u8]) -> slint::Image {
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba, ART, ART);
    slint::Image::from_rgba8(buf)
}

// ===================== weather =====================

#[derive(Clone, Default)]
pub struct Weather {
    pub place: String,
    pub temp: f64,
    pub high: f64,
    pub low: f64,
    pub code: i32,
    pub day: bool,
    /// Coordinates found for the city (to remember in the settings)
    pub latitude: f64,
    pub longitude: f64,
}

/// What a WMO weather code means: (words, icon). Icons: 0 sun, 1 moon, 2 sun+cloud,
/// 3 cloud, 4 fog, 5 rain, 6 snow, 7 storm.
pub fn describe(code: i32, day: bool) -> (&'static str, i32) {
    match code {
        0 => ("Clear", if day { 0 } else { 1 }),
        1 => ("Mostly clear", if day { 0 } else { 1 }),
        2 => ("Partly cloudy", 2),
        3 => ("Cloudy", 3),
        45 | 48 => ("Fog", 4),
        51..=57 => ("Drizzle", 5),
        61..=67 => ("Rain", 5),
        71..=77 => ("Snow", 6),
        80..=82 => ("Showers", 5),
        85 | 86 => ("Snow showers", 6),
        95..=99 => ("Thunderstorm", 7),
        _ => ("", 3),
    }
}

/// Fetch the weather (on a background thread); `done` gets the result there.
pub fn fetch_weather(
    city: String,
    coords: Option<(f64, f64)>,
    fahrenheit: bool,
    done: impl FnOnce(Result<Weather, String>) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("weather".into())
        .spawn(move || done(get_weather(&city, coords, fahrenheit)))
        .ok();
}

/// Download a small JSON document over HTTPS, using Windows' own HTTP library (WinHTTP).
fn http_get(url: &str) -> Result<serde_json::Value, String> {
    use windows::Win32::Networking::WinHttp::*;

    let rest = url.strip_prefix("https://").ok_or("bad address")?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let host_w = HSTRING::from(host);
    let path_w = HSTRING::from(path.as_str());
    let fail = |what: &str| {
        let code = unsafe { windows::Win32::Foundation::GetLastError() }.0;
        let hint = match code {
            12007 => " - can't find the server; are you online?",
            12029 | 12030 => " - couldn't connect; are you online, or is a firewall blocking it?",
            12002 => " - timed out",
            _ => "",
        };
        format!("No connection ({what} failed, error {code}{hint})")
    };

    struct Handle(*mut core::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    unsafe {
        let session = Handle(WinHttpOpen(
            windows::core::w!("WinOverview"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(fail("WinHttpOpen"));
        }
        let _ = WinHttpSetTimeouts(session.0, 8000, 8000, 10000, 10000);
        let connect = Handle(WinHttpConnect(session.0, &host_w, INTERNET_DEFAULT_HTTPS_PORT as u16, 0));
        if connect.0.is_null() {
            return Err(fail("connect"));
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            windows::core::w!("GET"),
            &path_w,
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(fail("request"));
        }
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|_| fail("send"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|_| fail("receive"))?;

        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).map_err(|_| fail("read"))?;
            if available == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(request.0, body[start..].as_mut_ptr() as *mut _, available, &mut read).map_err(|_| fail("read"))?;
            body.truncate(start + read as usize);
            if body.len() > 1_000_000 {
                break; // not what we asked for
            }
        }
        let v: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| "Unexpected answer from the weather service".to_string())?;
        if v["error"].as_bool() == Some(true) {
            return Err(format!("Weather service: {}", v["reason"].as_str().unwrap_or("error")));
        }
        Ok(v)
    }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn get_weather(city: &str, coords: Option<(f64, f64)>, fahrenheit: bool) -> Result<Weather, String> {
    let city = city.trim();
    if city.is_empty() {
        return Err("Set your city in Settings".into());
    }
    // City -> coordinates (only the first time; they're remembered afterwards)
    let (lat, lon, place) = match coords {
        Some((lat, lon)) => (lat, lon, city.to_string()),
        None => {
            // "Mumbai, India" -> search "Mumbai"
            let name = city.split(',').next().unwrap_or(city).trim();
            let url = format!("https://geocoding-api.open-meteo.com/v1/search?count=1&format=json&name={}", url_encode(name));
            let found = http_get(&url)?;
            let r = found["results"].get(0).ok_or_else(|| format!("Couldn't find \"{city}\""))?;
            let lat = r["latitude"].as_f64().ok_or("Couldn't find that city")?;
            let lon = r["longitude"].as_f64().ok_or("Couldn't find that city")?;
            (lat, lon, r["name"].as_str().unwrap_or(city).to_string())
        }
    };
    let unit = if fahrenheit { "&temperature_unit=fahrenheit" } else { "" };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}\
         &current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min\
         &timezone=auto&forecast_days=1{unit}"
    );
    let w = http_get(&url)?;
    let cur = &w["current"];
    Ok(Weather {
        place,
        temp: cur["temperature_2m"].as_f64().ok_or("No weather data")?,
        code: cur["weather_code"].as_i64().unwrap_or(3) as i32,
        day: cur["is_day"].as_i64().unwrap_or(1) == 1,
        high: w["daily"]["temperature_2m_max"][0].as_f64().unwrap_or(f64::NAN),
        low: w["daily"]["temperature_2m_min"][0].as_f64().unwrap_or(f64::NAN),
        latitude: lat,
        longitude: lon,
    })
}

// ===================== notes =====================

fn notes_file() -> std::path::PathBuf {
    config::folder().join("notes.txt")
}

/// The checklist: one item per line; struck-through items start with "[x] ".
pub fn load_notes() -> Vec<(String, bool)> {
    std::fs::read_to_string(notes_file())
        .unwrap_or_default()
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .map(|l| match l.strip_prefix("[x] ").or_else(|| l.strip_prefix("[X] ")) {
            Some(rest) => (rest.to_string(), true),
            None => (l.strip_prefix("[ ] ").unwrap_or(l).to_string(), false),
        })
        .collect()
}

pub fn save_notes(items: &[(String, bool)]) {
    let text: String = items
        .iter()
        .map(|(t, done)| if *done { format!("[x] {t}\n") } else { format!("{t}\n") })
        .collect();
    let _ = std::fs::create_dir_all(config::folder());
    let _ = std::fs::write(notes_file(), text);
}

// ===================== calendar =====================

/// Today's date (year, month 1-12, day), from the PC's clock.
pub fn today() -> (i32, u32, u32) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wYear as i32, t.wMonth as u32, t.wDay as u32)
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// Day of the week, 0 = Monday ... 6 = Sunday.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    // Sakamoto's method (gives 0 = Sunday)
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let sunday_first = (y + y / 4 - y / 100 + y / 400 + T[(month - 1) as usize] + day as i32).rem_euclid(7);
    ((sunday_first + 6) % 7) as u32
}

pub const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];
