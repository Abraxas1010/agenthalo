use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maps user-facing string keys to internal vector indices.
/// The underlying NucleusDB state is a Vec<u64>; this registry
/// provides the string-key layer that users and SQL operate on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMap {
    forward: BTreeMap<String, usize>,
    reverse: Vec<Option<String>>,
    next_index: usize,
}

impl KeyMap {
    pub fn new() -> Self {
        Self {
            forward: BTreeMap::new(),
            reverse: Vec::new(),
            next_index: 0,
        }
    }

    /// Get existing index for a key, or create a new mapping.
    pub fn get_or_create(&mut self, key: &str) -> usize {
        if let Some(&idx) = self.forward.get(key) {
            return idx;
        }
        let idx = self.next_index;
        self.forward.insert(key.to_string(), idx);
        while self.reverse.len() <= idx {
            self.reverse.push(None);
        }
        self.reverse[idx] = Some(key.to_string());
        self.next_index = idx + 1;
        idx
    }

    /// Get index for an existing key. Returns None if key doesn't exist.
    pub fn get(&self, key: &str) -> Option<usize> {
        self.forward.get(key).copied()
    }

    /// Get the key name at a given index.
    pub fn key_at(&self, idx: usize) -> Option<&str> {
        self.reverse.get(idx).and_then(|k| k.as_deref())
    }

    /// Iterate all key-index pairs.
    pub fn all_keys(&self) -> impl Iterator<Item = (&str, usize)> {
        self.forward.iter().map(|(k, &v)| (k.as_str(), v))
    }

    /// Number of registered keys.
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// Keys matching a LIKE pattern (% = wildcard).
    /// MVP matcher supports exact and prefix (`foo%`) patterns.
    pub fn keys_matching(&self, pattern: &str) -> Vec<(String, usize)> {
        if let Some(prefix) = pattern.strip_suffix('%') {
            self.forward
                .iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, &v)| (k.clone(), v))
                .collect()
        } else {
            self.forward
                .get(pattern)
                .map(|&v| vec![(pattern.to_string(), v)])
                .unwrap_or_default()
        }
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::new()
    }
}
