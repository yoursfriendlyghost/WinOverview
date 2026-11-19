//! Updates from GitHub Releases: "Check for updates" in Settings asks GitHub for the latest
//! release; "Update now" downloads its WinOverview.exe, swaps it in and restarts.
//!
//! A running exe can't be overwritten, but it can be renamed: the old one becomes
//! WinOverview.old.exe and is deleted the next time the app starts.

use std::path::PathBuf;

use crate::{post, Shared};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "yoursfriendlyghost/WinOverview";
/// The release files to update from, in order of preference
const ASSETS: [&str; 2] = ["WinOverview.exe", "WinOverview-small.exe"];

/// What the last check found.
#[derive(Clone, Default)]
pub struct Release {
    pub version: String,
    pub notes: String,
    /// Where the release files are: <download>/WinOverview.exe
    pub download: String,
    pub page: String,
}

/// "v2.10.1" -> [2, 10, 1]
fn parse(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map_while(|p| p.parse().ok()).collect()
}

pub fn newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

pub fn release_page() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

fn old_exe() -> Option<PathBuf> {
    std::env::current_exe().ok().map(|p| p.with_extension("old.exe"))
}

/// At start-up: remove the exe an update left behind.
pub fn cleanup() {
    if let Some(old) = old_exe() {
        let _ = std::fs::remove_file(old);
    }
}

/// Ask GitHub for the latest release (in the background), then tell Settings.
pub fn check(_app: &Shared) {
    set_status("Checking\u{2026}", false, false);
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(|| {
            let result = latest();
            post(move |app| match result {
                Ok(r) => {
                    let available = newer(&r.version, VERSION) && !r.download.is_empty();
                    if let Some(s) = &app.borrow().settings {
                        s.set_update_notes(if available { r.notes.as_str().into() } else { "".into() });
                    }
                    if available {
                        set_status(&format!("{} is available (you have v{VERSION})", r.version), false, true);
                    } else {
                        set_status(&format!("You have the latest version (v{VERSION})"), false, false);
                    }
                    app.borrow_mut().release = Some(r);
                }
                Err(e) => set_status(&format!("Couldn't check: {e}"), true, false),
            });
        })
        .ok();
}

/// The latest release, from github.com itself: /releases/latest redirects to the newest tag,
/// and the exe is always at /releases/download/<tag>/WinOverview.exe. (GitHub's API has a
/// limit on requests from one place, so it's only asked for the release notes, and a "no" from
/// it doesn't matter.)
fn latest() -> Result<Release, String> {
    let page = crate::net::final_url(&release_page())?;
    let version = page
        .split("/releases/tag/")
        .nth(1)
        .map(|t| t.trim_end_matches('/').to_string())
        .filter(|t| !t.is_empty())
        .ok_or("no release published yet")?;
    let download = format!("https://github.com/{REPO}/releases/download/{version}");
    Ok(Release { notes: notes(&version).unwrap_or_default(), version, download, page })
}

/// The release notes (best effort), trimmed to a readable size.
fn notes(tag: &str) -> Option<String> {
    let body = crate::net::get(&format!("https://api.github.com/repos/{REPO}/releases/tags/{tag}"), 2_000_000).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let mut notes = v["body"].as_str()?.replace("\r\n", "\n").trim().to_string();
    if notes.chars().count() > 700 {
        notes = notes.chars().take(700).collect::<String>() + "\u{2026}";
    }
    Some(notes)
}

/// Download the new exe, swap it in, restart.
pub fn install(app: &Shared) {
    let Some(r) = app.borrow().release.clone() else { return };
    if r.download.is_empty() {
        return;
    }
    set_status(&format!("Downloading {}\u{2026}", r.version), false, false);
    std::thread::Builder::new()
        .name("update-download".into())
        .spawn(move || {
            // the normal exe, or the small one if the release only has that
            let mut result = Err("no exe in the release".to_string());
            for name in ASSETS {
                result = crate::net::get(&format!("{}/{name}", r.download), 80_000_000);
                if result.is_ok() {
                    break;
                }
            }
            let result = result.and_then(|bytes| swap_in(&bytes));
            post(move |_app| match result {
                Ok(exe) => {
                    set_status("Updated. Restarting\u{2026}", false, false);
                    crate::restart_from(exe);
                }
                Err(e) => set_status(&format!("Update failed: {e}"), true, true),
            });
        })
        .ok();
}

/// Returns where the new exe is (start that one: after the rename, Windows may report the
/// running exe under its new .old name).
fn swap_in(bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.len() < 500_000 || !bytes.starts_with(b"MZ") {
        return Err("the download isn't a Windows program".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = old_exe().ok_or("can't find where WinOverview is")?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| no_access(e, &exe))?;
    if let Err(e) = std::fs::write(&exe, bytes) {
        let _ = std::fs::rename(&old, &exe); // put the old one back
        return Err(no_access(e, &exe));
    }
    Ok(exe)
}

fn no_access(e: std::io::Error, exe: &std::path::Path) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "Windows didn't allow changing {} (try moving WinOverview to a folder like Documents, or download it from the release page)",
            exe.display()
        )
    } else {
        e.to_string()
    }
}

/// Settings' update line. `action`: show the button (Update now / Open release page).
fn set_status(text: &str, error: bool, action: bool) {
    let text = text.to_string();
    post(move |app| {
        if let Some(s) = &app.borrow().settings {
            s.set_update_status(text.as_str().into());
            s.set_update_error(error);
            s.set_update_ready(action && !error);
            s.set_update_failed(action && error);
        }
    });
}
