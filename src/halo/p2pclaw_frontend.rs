//! P2PCLAW Frontend Manager
//!
//! Manages the beta-p2pclaw Next.js frontend as a child process and provides
//! a reverse-proxy layer so the entire UI is served through the AgentHALO
//! dashboard at `/p2pclaw-app/*`.
//!
//! The Next.js app runs on a localhost-only port (default 7422) and is never
//! directly exposed. AgentHALO's axum server forwards requests to it, keeping
//! all traffic behind the dashboard's auth layer.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 7422;
const FRONTEND_DIR: &str = "vendor/p2pclaw-frontend";
const FRONTEND_DIR_ENV: &str = "P2PCLAW_FRONTEND_DIR";
const FRONTEND_PORT_ENV: &str = "P2PCLAW_FRONTEND_PORT";

/// Manages the Next.js child process lifecycle.
pub struct P2PClawFrontendManager {
    child: Option<Child>,
    port: u16,
}

impl P2PClawFrontendManager {
    pub fn new() -> Self {
        let port = std::env::var(FRONTEND_PORT_ENV)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_PORT);
        Self { child: None, port }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Check if Node.js and the frontend directory are available.
    pub fn is_available() -> bool {
        let node_ok = Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !node_ok {
            return false;
        }
        let dir = Self::frontend_dir();
        dir.join("package.json").exists() && dir.join("node_modules").exists()
    }

    fn frontend_dir() -> PathBuf {
        if let Ok(raw) = std::env::var(FRONTEND_DIR_ENV) {
            let candidate = PathBuf::from(raw.trim());
            if candidate.join("package.json").exists() {
                return candidate;
            }
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
        if let Some(dir) = exe_dir {
            let candidate = dir.join("..").join(FRONTEND_DIR);
            if candidate.join("package.json").exists() {
                return candidate;
            }
            let candidate = dir.join(FRONTEND_DIR);
            if candidate.join("package.json").exists() {
                return candidate;
            }
        }
        PathBuf::from(FRONTEND_DIR)
    }

    fn upstream_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    /// Start the Next.js dev server. Blocks until ready or timeout (30s).
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        if self.child.is_none()
            && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok()
        {
            // Port already in use — assume it's a manually started instance
            return Ok(());
        }
        if self.child.is_some() {
            self.stop();
        }
        let frontend_dir = Self::frontend_dir();
        if !frontend_dir.join("package.json").exists() {
            return Err(format!(
                "P2PCLAW frontend missing at {} (run: cd {} && npm install && npm run build)",
                frontend_dir.display(),
                FRONTEND_DIR,
            ));
        }

        // Use `npx next start` for production builds, `npx next dev` for dev.
        // Production mode is preferred — faster, more stable, no HMR noise.
        let has_build = frontend_dir.join(".next").exists();
        let port_str = self.port.to_string();
        let (cmd_args, mode): (Vec<&str>, &str) = if has_build {
            (vec!["next", "start", "--port", &port_str], "production")
        } else {
            (vec!["next", "dev", "--port", &port_str], "development")
        };

        let child = Command::new("npx")
            .args(&cmd_args)
            .current_dir(&frontend_dir)
            .env("PORT", self.port.to_string())
            // Set basePath for reverse-proxy routing
            .env("NEXT_PUBLIC_BASE_PATH", "/p2pclaw-app")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("start P2PCLAW frontend ({mode}): {e}"))?;
        self.child = Some(child);

        let started = Instant::now();
        let timeout = Duration::from_secs(30);
        while started.elapsed() < timeout {
            if self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        self.stop();
        Err(format!(
            "P2PCLAW frontend ({mode}) did not become ready within 30s"
        ))
    }

    /// Stop the frontend process.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Check if the frontend is responding.
    pub fn is_running(&self) -> bool {
        std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok()
    }

    /// Forward a GET request to the frontend and return (status, content_type, body_bytes).
    pub fn proxy_get(&self, path: &str) -> Result<(u16, String, Vec<u8>), String> {
        let url = self.upstream_url(path);
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .timeout_global(Some(Duration::from_secs(15)))
                .build(),
        );
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| format!("P2PCLAW frontend GET {path}: {e}"))?;

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();

        let body = resp
            .body_mut()
            .read_to_vec()
            .map_err(|e| format!("read P2PCLAW frontend response: {e}"))?;

        Ok((status, content_type, body))
    }
}

impl Default for P2PClawFrontendManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for P2PClawFrontendManager {
    fn drop(&mut self) {
        self.stop();
    }
}
