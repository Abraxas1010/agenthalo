use crate::halo::config;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub api_key: Option<String>,
    pub oauth_token: Option<String>,
    pub oauth_provider: Option<String>,
    pub user_id: Option<String>,
    pub created_at: u64,
}

impl Default for Credentials {
    fn default() -> Self {
        Self {
            api_key: None,
            oauth_token: None,
            oauth_provider: None,
            user_id: None,
            created_at: now_unix(),
        }
    }
}

pub fn save_credentials(path: &Path, creds: &Credentials) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create credentials dir: {e}"))?;
    }
    let raw =
        serde_json::to_string_pretty(creds).map_err(|e| format!("serialize credentials: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("write credentials {}: {e}", path.display()))
}

pub fn load_credentials(path: &Path) -> Result<Credentials, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read credentials {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse credentials {}: {e}", path.display()))
}

pub fn is_authenticated(path: &Path) -> bool {
    if std::env::var("AGENTHALO_API_KEY")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    load_credentials(path)
        .map(|c| {
            c.api_key
                .as_ref()
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
                || c.oauth_token
                    .as_ref()
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

pub fn resolve_api_key(creds_path: &Path) -> Option<String> {
    std::env::var("AGENTHALO_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            load_credentials(creds_path)
                .ok()
                .and_then(|c| c.api_key)
                .filter(|v| !v.trim().is_empty())
        })
        .or_else(|| {
            load_credentials(creds_path)
                .ok()
                .and_then(|c| c.oauth_token)
                .filter(|v| !v.trim().is_empty())
        })
}

pub fn oauth_login(provider: &str) -> Result<Credentials, String> {
    let provider = provider.trim().to_ascii_lowercase();
    if provider != "github" && provider != "google" {
        return Err(format!(
            "unsupported provider '{provider}', expected github|google"
        ));
    }

    config::ensure_halo_dir()?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("bind oauth callback listener: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("set nonblocking: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("read listener address: {e}"))?
        .port();

    let redirect = format!("http://127.0.0.1:{port}/callback");
    let login_url = format!(
        "https://agenthalo.dev/auth/{provider}?redirect={}",
        url_encode(&redirect)
    );

    let _ = webbrowser::open(&login_url);
    println!("Open this URL if your browser did not launch:\n{login_url}");
    println!("Waiting for OAuth callback on {redirect} ...");

    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match listener.accept() {
            Ok((mut stream, _addr)) => {
                let mut buf = [0u8; 8192];
                let n = stream
                    .read(&mut buf)
                    .map_err(|e| format!("read oauth callback request: {e}"))?;
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = extract_http_path(&req)
                    .ok_or_else(|| "invalid oauth callback request".to_string())?;
                let params = parse_query_params(&path);
                let token = params
                    .get("token")
                    .cloned()
                    .or_else(|| params.get("access_token").cloned())
                    .ok_or_else(|| "oauth callback missing token/access_token".to_string())?;
                let user_id = params
                    .get("user_id")
                    .cloned()
                    .or_else(|| params.get("sub").cloned());

                let body = "AgentHALO login successful. You can close this tab.";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());

                return Ok(Credentials {
                    api_key: None,
                    oauth_token: Some(token),
                    oauth_provider: Some(provider),
                    user_id,
                    created_at: now_unix(),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("oauth callback timeout (180s)".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("oauth callback accept failed: {e}")),
        }
    }
}

fn extract_http_path(raw_request: &str) -> Option<String> {
    let line = raw_request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next().map(|p| p.to_string())
}

fn parse_query_params(path: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let (_, query) = match path.split_once('?') {
        Some(v) => v,
        None => return out,
    };
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some(v) => v,
            None => (pair, ""),
        };
        out.insert(url_decode(k), url_decode(v));
    }
    out
}

fn url_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn url_decode(raw: &str) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &raw[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
