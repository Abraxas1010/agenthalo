//! Customer API key management for the metered proxy.
//!
//! Each customer gets:
//! - A unique API key (prefixed `ah-` for easy identification)
//! - A prepaid USD balance (credit card or crypto top-up)
//! - Per-key usage tracking (tokens, cost, request count)
//! - Rate limiting (RPM and daily token caps)
//!
//! Keys and balances are persisted to `~/.agenthalo/api_keys.json`.
//! The store uses a simple read-modify-write pattern with atomic file replacement.
//!
//! This module is load-bearing for the proxy: `metered_proxy_sync` in `proxy.rs`
//! calls `validate_key`, `get_balance`, `deduct_balance`, and `record_usage`
//! on every request. The billing pipeline, dashboard cost display, and
//! balance alerts all depend on the structures defined here.

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Key types
// ---------------------------------------------------------------------------

/// A customer API key record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomerKey {
    /// Unique key identifier (for internal reference, not the secret).
    pub key_id: String,
    /// The actual API key value (prefixed `ah-`).
    pub api_key: String,
    /// Human-readable label (e.g. customer name or email).
    pub label: String,
    /// Whether this key is active (can make requests).
    pub active: bool,
    /// Prepaid balance in USD.
    pub balance_usd: f64,
    /// Total USD spent lifetime.
    pub total_spent_usd: f64,
    /// Total requests made.
    pub total_requests: u64,
    /// Total input tokens consumed.
    pub total_input_tokens: u64,
    /// Total output tokens consumed.
    pub total_output_tokens: u64,
    /// Unix timestamp of key creation.
    pub created_at: u64,
    /// Unix timestamp of last request.
    pub last_used_at: Option<u64>,
    /// Per-day usage for rate limiting and reporting.
    #[serde(default)]
    pub daily_usage: HashMap<String, DailyUsage>,
}

/// Per-day usage counters (keyed by date string "YYYY-MM-DD").
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DailyUsage {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

// ---------------------------------------------------------------------------
// Key store
// ---------------------------------------------------------------------------

/// Thread-safe customer key store backed by a JSON file.
pub struct CustomerKeyStore {
    path: PathBuf,
    inner: Mutex<StoreData>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreData {
    keys: HashMap<String, CustomerKey>,
    /// Index: api_key_value -> key_id (for O(1) lookup on every request).
    #[serde(default)]
    key_index: HashMap<String, String>,
}

impl CustomerKeyStore {
    /// Open or create the key store at the given path.
    pub fn open(path: PathBuf) -> Self {
        let data = match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => StoreData::default(),
        };
        Self {
            path,
            inner: Mutex::new(data),
        }
    }

    /// Default path: `~/.agenthalo/api_keys.json`.
    pub fn default_path() -> PathBuf {
        crate::halo::config::halo_dir().join("api_keys.json")
    }

    /// Generate a new customer API key.
    pub fn create_key(&self, label: &str, initial_balance_usd: f64) -> CustomerKey {
        let key_id = generate_key_id();
        let api_key = generate_api_key();
        let now = now_unix();

        let customer = CustomerKey {
            key_id: key_id.clone(),
            api_key: api_key.clone(),
            label: label.to_string(),
            active: true,
            balance_usd: initial_balance_usd,
            total_spent_usd: 0.0,
            total_requests: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            created_at: now,
            last_used_at: None,
            daily_usage: HashMap::new(),
        };

        let mut data = self.inner.lock().unwrap();
        data.key_index.insert(api_key, key_id.clone());
        data.keys.insert(key_id, customer.clone());
        drop(data);
        self.persist();

        customer
    }

    /// Validate an API key and return the customer record if valid.
    pub fn validate_key(&self, api_key: &str) -> Option<CustomerKey> {
        let data = self.inner.lock().unwrap();
        let key_id = data.key_index.get(api_key)?;
        data.keys.get(key_id).cloned()
    }

    /// Get current balance for a customer.
    pub fn get_balance(&self, key_id: &str) -> f64 {
        let data = self.inner.lock().unwrap();
        data.keys
            .get(key_id)
            .map(|k| k.balance_usd)
            .unwrap_or(0.0)
    }

    /// Add funds to a customer's balance. Returns new balance.
    pub fn add_balance(&self, key_id: &str, amount_usd: f64) -> f64 {
        let mut data = self.inner.lock().unwrap();
        if let Some(key) = data.keys.get_mut(key_id) {
            key.balance_usd += amount_usd;
            let balance = key.balance_usd;
            drop(data);
            self.persist();
            balance
        } else {
            0.0
        }
    }

    /// Deduct from a customer's balance. Returns remaining balance.
    pub fn deduct_balance(&self, key_id: &str, amount_usd: f64) -> f64 {
        let mut data = self.inner.lock().unwrap();
        if let Some(key) = data.keys.get_mut(key_id) {
            key.balance_usd -= amount_usd;
            key.total_spent_usd += amount_usd;
            let balance = key.balance_usd;
            drop(data);
            self.persist();
            balance
        } else {
            0.0
        }
    }

    /// Record a completed request for usage tracking.
    pub fn record_usage(
        &self,
        key_id: &str,
        _model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    ) {
        let today = today_string();
        let now = now_unix();
        let mut data = self.inner.lock().unwrap();
        if let Some(key) = data.keys.get_mut(key_id) {
            key.total_requests += 1;
            key.total_input_tokens += input_tokens;
            key.total_output_tokens += output_tokens;
            key.last_used_at = Some(now);

            let daily = key.daily_usage.entry(today).or_default();
            daily.requests += 1;
            daily.input_tokens += input_tokens;
            daily.output_tokens += output_tokens;
            daily.cost_usd += cost_usd;
        }
        drop(data);
        self.persist();
    }

    /// Check today's token usage for rate limiting.
    pub fn today_tokens(&self, key_id: &str) -> u64 {
        let today = today_string();
        let data = self.inner.lock().unwrap();
        data.keys
            .get(key_id)
            .and_then(|k| k.daily_usage.get(&today))
            .map(|d| d.input_tokens + d.output_tokens)
            .unwrap_or(0)
    }

    /// Suspend a customer key.
    pub fn suspend_key(&self, key_id: &str) -> bool {
        let mut data = self.inner.lock().unwrap();
        if let Some(key) = data.keys.get_mut(key_id) {
            key.active = false;
            drop(data);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Reactivate a suspended key.
    pub fn activate_key(&self, key_id: &str) -> bool {
        let mut data = self.inner.lock().unwrap();
        if let Some(key) = data.keys.get_mut(key_id) {
            key.active = true;
            drop(data);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Revoke (permanently delete) a customer key.
    pub fn revoke_key(&self, key_id: &str) -> bool {
        let mut data = self.inner.lock().unwrap();
        if let Some(key) = data.keys.remove(key_id) {
            data.key_index.remove(&key.api_key);
            drop(data);
            self.persist();
            true
        } else {
            false
        }
    }

    /// List all customer keys (sensitive: api_key values included for admin).
    pub fn list_keys(&self) -> Vec<CustomerKey> {
        let data = self.inner.lock().unwrap();
        data.keys.values().cloned().collect()
    }

    /// Get a single key by ID.
    pub fn get_key(&self, key_id: &str) -> Option<CustomerKey> {
        let data = self.inner.lock().unwrap();
        data.keys.get(key_id).cloned()
    }

    /// Persist store to disk (atomic write).
    fn persist(&self) {
        let data = self.inner.lock().unwrap();
        let json = match serde_json::to_string_pretty(&*data) {
            Ok(j) => j,
            Err(_) => return,
        };
        drop(data);

        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Atomic write: write to temp file, then rename.
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Key generation
// ---------------------------------------------------------------------------

/// Generate a key ID (internal identifier, not the secret).
fn generate_key_id() -> String {
    let mut buf = [0u8; 8];
    OsRng.fill_bytes(&mut buf);
    format!("cust_{}", hex_encode(&buf))
}

/// Generate an API key (the secret value customers use).
/// Format: `ah-` prefix + 48 hex chars = 24 random bytes.
fn generate_api_key() -> String {
    let mut buf = [0u8; 24];
    OsRng.fill_bytes(&mut buf);
    format!("ah-{}", hex_encode(&buf))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn today_string() -> String {
    let secs = now_unix();
    let days = secs / 86400;
    // Simple date calculation (no chrono dependency for this).
    let epoch = 719163u64; // days from year 0 to 1970-01-01
    let total_days = epoch + days;
    let (y, m, d) = civil_from_days(total_days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert days since epoch to (year, month, day). Algorithm from Howard Hinnant.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (CustomerKeyStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api_keys.json");
        let store = CustomerKeyStore::open(path);
        (store, dir)
    }

    #[test]
    fn create_and_validate_key() {
        let (store, _dir) = temp_store();
        let key = store.create_key("test customer", 10.0);

        assert!(key.api_key.starts_with("ah-"));
        assert!(key.key_id.starts_with("cust_"));
        assert_eq!(key.balance_usd, 10.0);
        assert!(key.active);

        // Validate by API key.
        let found = store.validate_key(&key.api_key).unwrap();
        assert_eq!(found.key_id, key.key_id);

        // Invalid key returns None.
        assert!(store.validate_key("ah-invalid").is_none());
    }

    #[test]
    fn balance_operations() {
        let (store, _dir) = temp_store();
        let key = store.create_key("balance test", 50.0);

        assert_eq!(store.get_balance(&key.key_id), 50.0);

        let after_add = store.add_balance(&key.key_id, 25.0);
        assert_eq!(after_add, 75.0);

        let after_deduct = store.deduct_balance(&key.key_id, 10.0);
        assert_eq!(after_deduct, 65.0);

        // Verify total_spent updated.
        let updated = store.get_key(&key.key_id).unwrap();
        assert_eq!(updated.total_spent_usd, 10.0);
    }

    #[test]
    fn usage_tracking() {
        let (store, _dir) = temp_store();
        let key = store.create_key("usage test", 100.0);

        store.record_usage(&key.key_id, "anthropic/claude-opus-4-6", 500, 200, 0.05);
        store.record_usage(&key.key_id, "openai/gpt-4o", 300, 100, 0.02);

        let updated = store.get_key(&key.key_id).unwrap();
        assert_eq!(updated.total_requests, 2);
        assert_eq!(updated.total_input_tokens, 800);
        assert_eq!(updated.total_output_tokens, 300);
        assert!(updated.last_used_at.is_some());

        assert_eq!(store.today_tokens(&key.key_id), 1100);
    }

    #[test]
    fn suspend_and_activate() {
        let (store, _dir) = temp_store();
        let key = store.create_key("suspend test", 10.0);

        assert!(store.suspend_key(&key.key_id));
        let suspended = store.validate_key(&key.api_key).unwrap();
        assert!(!suspended.active);

        assert!(store.activate_key(&key.key_id));
        let active = store.validate_key(&key.api_key).unwrap();
        assert!(active.active);
    }

    #[test]
    fn revoke_key() {
        let (store, _dir) = temp_store();
        let key = store.create_key("revoke test", 10.0);

        assert!(store.revoke_key(&key.key_id));
        assert!(store.validate_key(&key.api_key).is_none());
        assert!(store.get_key(&key.key_id).is_none());
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api_keys.json");

        let api_key;
        let key_id;
        {
            let store = CustomerKeyStore::open(path.clone());
            let key = store.create_key("persist test", 42.0);
            api_key = key.api_key.clone();
            key_id = key.key_id.clone();
            store.record_usage(&key_id, "model", 100, 50, 0.01);
        }

        // Reopen store from disk.
        let store2 = CustomerKeyStore::open(path);
        let loaded = store2.validate_key(&api_key).unwrap();
        assert_eq!(loaded.key_id, key_id);
        assert_eq!(loaded.balance_usd, 42.0);
        assert_eq!(loaded.total_requests, 1);
    }

    #[test]
    fn list_keys() {
        let (store, _dir) = temp_store();
        store.create_key("alice", 10.0);
        store.create_key("bob", 20.0);
        store.create_key("charlie", 30.0);

        let keys = store.list_keys();
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn today_string_is_valid_date() {
        let date = today_string();
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
    }

    #[test]
    fn api_key_format() {
        let key = generate_api_key();
        assert!(key.starts_with("ah-"));
        assert_eq!(key.len(), 3 + 48); // "ah-" + 48 hex chars
    }
}
