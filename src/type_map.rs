//! Key-to-type mapping for NucleusDB.
//!
//! Each key in the database has an associated [`TypeTag`] that determines how
//! its u64 cell value should be interpreted.  Keys without an entry default to
//! [`TypeTag::Integer`] for backward compatibility with pre-typed data.

use crate::typed_value::TypeTag;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeMap {
    types: BTreeMap<String, TypeTag>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self {
            types: BTreeMap::new(),
        }
    }

    /// Set the type tag for a key.
    pub fn set(&mut self, key: &str, tag: TypeTag) {
        self.types.insert(key.to_string(), tag);
    }

    /// Get the type tag for a key.
    /// Returns `TypeTag::Integer` for untagged keys (backward compatibility).
    pub fn get(&self, key: &str) -> TypeTag {
        self.types
            .get(key)
            .copied()
            .unwrap_or(TypeTag::Integer)
    }

    /// Get the type tag for a key, returning None if untagged.
    pub fn get_opt(&self, key: &str) -> Option<TypeTag> {
        self.types.get(key).copied()
    }

    /// Remove the type tag for a key.
    pub fn remove(&mut self, key: &str) {
        self.types.remove(key);
    }

    /// Number of typed keys.
    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Iterate all (key, tag) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, TypeTag)> {
        self.types.iter().map(|(k, &v)| (k.as_str(), v))
    }
}

impl Default for TypeMap {
    fn default() -> Self {
        Self::new()
    }
}
