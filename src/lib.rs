pub mod api;
pub mod audit;
pub mod blob_store;
pub mod chebyshev_evictor;
pub mod cli;
pub mod cockpit;
pub mod commitment;
pub mod comms;
pub mod config;
pub mod container;
pub mod crypto_scope;
pub mod dashboard;
pub mod did;
pub mod discord;
pub mod embeddings;
pub mod encrypted_file;
pub mod genesis;
pub mod governor;
pub mod halo;
pub mod hash;
pub mod http_client;
pub mod identity;
pub mod identity_ledger;
pub mod immutable;
pub mod keymap;
pub mod license;
pub mod materialize;
pub mod mcp;
pub mod memory;
pub mod multitenant;
pub mod orchestrator;
pub mod password;
pub mod persistence;
pub mod pod;
pub mod pq;
pub mod protocol;
pub mod security;
pub mod security_utils;
pub mod sheaf;
pub mod sql;
pub mod state;
pub mod transparency;
pub mod trust;
pub mod tui;
pub mod type_map;
pub mod typed_value;
pub mod util;
pub mod vault;
pub mod vc;
pub mod vector_index;
pub mod verifier;
pub mod witness;

pub use multitenant::{
    MultiTenantError, MultiTenantNucleusDb, MultiTenantPolicy, TenantRole, TenantSnapshot,
};
pub use persistence::PersistenceError;
pub use protocol::{CommitError, NucleusDb, QueryProof, VcBackend};
pub use security::{
    default_reduction_contracts, ParameterError, ParameterSet, ReductionContract, RefinementError,
    SecurityPolicyError, VcProfile,
};
pub use state::{Delta, State};

#[doc(hidden)]
pub mod test_support {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;

    /// Global lock for process-wide environment variable mutation in tests.
    /// Rust environment access is process-global and not thread-safe across
    /// concurrent writes, so tests that set/remove env vars must serialize.
    pub fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    pub fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        let mutex = env_lock();
        let guard = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mutex.clear_poison();
        guard
    }

    pub struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        pub fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    pub struct MockOpenAiServer {
        pub base_url: String,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockOpenAiServer {
        pub fn spawn(model: &str, response_text: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
            listener.set_nonblocking(true).expect("set nonblocking");
            let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_flag = shutdown.clone();
            let model = model.to_string();
            let response_text = response_text.to_string();
            let handle = thread::spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut buffer = [0u8; 4096];
                            let bytes = stream.read(&mut buffer).unwrap_or(0);
                            let request = String::from_utf8_lossy(&buffer[..bytes]);
                            let body = if request.starts_with("GET /v1/models") {
                                serde_json::json!({
                                    "object": "list",
                                    "data": [{
                                        "id": model,
                                        "object": "model",
                                        "owned_by": "test"
                                    }]
                                })
                            } else {
                                serde_json::json!({
                                    "id": "chatcmpl-test",
                                    "object": "chat.completion",
                                    "model": model,
                                    "choices": [{
                                        "index": 0,
                                        "message": {
                                            "role": "assistant",
                                            "content": response_text
                                        },
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 11,
                                        "completion_tokens": 7,
                                        "total_tokens": 18
                                    }
                                })
                            };
                            let raw = body.to_string();
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                raw.len(),
                                raw
                            );
                            let _ = stream.write_all(response.as_bytes());
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                base_url,
                shutdown,
                handle: Some(handle),
            }
        }
    }

    impl Drop for MockOpenAiServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            let _ = std::net::TcpStream::connect(self.base_url.trim_start_matches("http://"));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
}
