//! HTTPS downloads (weather, update checks).

use windows::core::HSTRING;

/// Download something over HTTPS (at most `limit` bytes).
pub fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let r = fetch(url, "GET", limit)?;
    if r.status >= 400 {
        return Err(server_error(r.status, &r.body));
    }
    Ok(r.body)
}

/// Where `url` ends up after redirects (asks for the headers only).
pub fn final_url(url: &str) -> Result<String, String> {
    let r = fetch(url, "HEAD", 0)?;
    if r.status >= 400 {
        return Err(server_error(r.status, &r.body));
    }
    Ok(r.url)
}

/// "The server answered 403 (API rate limit exceeded...)": the reason if the answer has one.
fn server_error(status: u32, body: &[u8]) -> String {
    let reason = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| {
        v["message"].as_str().or_else(|| v["reason"].as_str()).map(|m| m.chars().take(120).collect::<String>())
    });
    match reason {
        Some(r) => format!("the server answered {status} ({r})"),
        None => format!("the server answered {status}"),
    }
}

struct Response {
    status: u32,
    body: Vec<u8>,
    /// The address after redirects
    url: String,
}

/// One HTTPS request with Windows' own HTTP library (WinHTTP: it uses the system proxy and
/// follows redirects).
fn fetch(url: &str, method: &str, limit: usize) -> Result<Response, String> {
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
        let _ = WinHttpSetTimeouts(session.0, 8000, 8000, 15000, 30000);
        let connect = Handle(WinHttpConnect(session.0, &host_w, INTERNET_DEFAULT_HTTPS_PORT as u16, 0));
        if connect.0.is_null() {
            return Err(fail("connect"));
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            &HSTRING::from(method),
            &path_w,
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(fail("request"));
        }
        if host == "api.github.com" {
            let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n".encode_utf16().collect();
            let _ = WinHttpAddRequestHeaders(request.0, &headers, WINHTTP_ADDREQ_FLAG_ADD);
        }
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|_| fail("send"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|_| fail("receive"))?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let _ = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            windows::core::PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut size,
            std::ptr::null_mut(),
        );
        // The address after any redirects
        let mut buf = [0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let final_url = match WinHttpQueryOption(request.0, WINHTTP_OPTION_URL, Some(buf.as_mut_ptr() as *mut _), &mut len) {
            Ok(()) => String::from_utf16_lossy(&buf[..(len as usize / 2).min(buf.len())]).trim_end_matches('\0').to_string(),
            Err(_) => url.to_string(),
        };

        let mut body = Vec::new();
        // a HEAD answer has no body; an error answer is read for its reason (a little of it)
        let limit = if status >= 400 { limit.max(4096) } else { limit };
        loop {
            if method == "HEAD" {
                break;
            }
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
            if body.len() > limit {
                if status >= 400 {
                    break;
                }
                return Err("Download too big".into());
            }
        }
        Ok(Response { status, body, url: final_url })
    }
}
