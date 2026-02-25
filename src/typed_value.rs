//! Typed value layer for NucleusDB.
//!
//! The core state vector remains `Vec<u64>`.  This module provides typed
//! interpretation: each key carries a [`TypeTag`] and its u64 cell is either
//! a direct encoding (Integer, Float, Bool) or a content-hash pointer into
//! the [`BlobStore`](crate::blob_store::BlobStore).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Discriminator stored alongside each key in the [`TypeMap`](crate::type_map::TypeMap).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeTag {
    Null,
    Integer,
    Float,
    Bool,
    Text,
    Json,
    Bytes,
    Vector,
}

impl TypeTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Text => "text",
            Self::Json => "json",
            Self::Bytes => "bytes",
            Self::Vector => "vector",
        }
    }

    pub fn from_str_tag(s: &str) -> Option<Self> {
        match s {
            "null" => Some(Self::Null),
            "integer" => Some(Self::Integer),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            "bytes" => Some(Self::Bytes),
            "vector" => Some(Self::Vector),
            _ => None,
        }
    }

    /// Whether this type stores its data in the blob store (content-addressed).
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Text | Self::Json | Self::Bytes | Self::Vector)
    }
}

impl std::fmt::Display for TypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A fully decoded value.  Produced by reading from the state vector + blob store.
#[derive(Clone, Debug, PartialEq)]
pub enum TypedValue {
    Null,
    Integer(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Json(serde_json::Value),
    Bytes(Vec<u8>),
    Vector(Vec<f64>),
}

/// Sentinel u64 for Null.
pub const NULL_SENTINEL: u64 = u64::MAX;

impl TypedValue {
    pub fn tag(&self) -> TypeTag {
        match self {
            Self::Null => TypeTag::Null,
            Self::Integer(_) => TypeTag::Integer,
            Self::Float(_) => TypeTag::Float,
            Self::Bool(_) => TypeTag::Bool,
            Self::Text(_) => TypeTag::Text,
            Self::Json(_) => TypeTag::Json,
            Self::Bytes(_) => TypeTag::Bytes,
            Self::Vector(_) => TypeTag::Vector,
        }
    }

    /// Encode to u64 cell value.
    ///
    /// For direct types (Integer, Float, Bool, Null) the returned value is the
    /// cell itself and `blob_bytes` is `None`.
    ///
    /// For blob types (Text, Json, Bytes, Vector) the returned u64 is a
    /// content hash of `(key, data)`, and `blob_bytes` contains the serialized
    /// payload that must be stored in the blob store.
    pub fn encode(&self, key: &str) -> (u64, Option<Vec<u8>>) {
        match self {
            Self::Null => (NULL_SENTINEL, None),
            Self::Integer(v) => (i64_to_u64(*v), None),
            Self::Float(v) => (v.to_bits(), None),
            Self::Bool(v) => (if *v { 1 } else { 0 }, None),
            Self::Text(s) => {
                let blob = s.as_bytes().to_vec();
                let cell = content_hash_u64(key, &blob);
                (cell, Some(blob))
            }
            Self::Json(v) => {
                let blob = serde_json::to_vec(v).expect("JSON serialization cannot fail");
                let cell = content_hash_u64(key, &blob);
                (cell, Some(blob))
            }
            Self::Bytes(b) => {
                let cell = content_hash_u64(key, b);
                (cell, Some(b.clone()))
            }
            Self::Vector(dims) => {
                let blob = vector_to_bytes(dims);
                let cell = content_hash_u64(key, &blob);
                (cell, Some(blob))
            }
        }
    }

    /// Decode from u64 cell + optional blob bytes.
    pub fn decode(tag: TypeTag, cell: u64, blob: Option<&[u8]>) -> Result<Self, String> {
        match tag {
            TypeTag::Null => Ok(Self::Null),
            TypeTag::Integer => Ok(Self::Integer(u64_to_i64(cell))),
            TypeTag::Float => Ok(Self::Float(f64::from_bits(cell))),
            TypeTag::Bool => Ok(Self::Bool(cell != 0)),
            TypeTag::Text => {
                let data = blob.ok_or("missing blob for Text value")?;
                String::from_utf8(data.to_vec())
                    .map(Self::Text)
                    .map_err(|e| format!("invalid UTF-8 in Text blob: {e}"))
            }
            TypeTag::Json => {
                let data = blob.ok_or("missing blob for Json value")?;
                serde_json::from_slice(data)
                    .map(Self::Json)
                    .map_err(|e| format!("invalid JSON in blob: {e}"))
            }
            TypeTag::Bytes => {
                let data = blob.ok_or("missing blob for Bytes value")?;
                Ok(Self::Bytes(data.to_vec()))
            }
            TypeTag::Vector => {
                let data = blob.ok_or("missing blob for Vector value")?;
                bytes_to_vector(data).map(Self::Vector)
            }
        }
    }

    /// Render as a user-facing string for SQL results and dashboard display.
    pub fn display_string(&self) -> String {
        match self {
            Self::Null => "NULL".to_string(),
            Self::Integer(v) => v.to_string(),
            Self::Float(v) => format!("{v}"),
            Self::Bool(v) => v.to_string(),
            Self::Text(s) => s.clone(),
            Self::Json(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            Self::Bytes(b) => format!("0x{}", hex::encode(b)),
            Self::Vector(dims) => {
                let inner: Vec<String> = dims.iter().map(|d| format!("{d}")).collect();
                format!("[{}]", inner.join(", "))
            }
        }
    }

    /// Render as serde_json::Value for API responses.
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Integer(v) => serde_json::json!(*v),
            Self::Float(v) => serde_json::json!(*v),
            Self::Bool(v) => serde_json::json!(*v),
            Self::Text(s) => serde_json::json!(s),
            Self::Json(v) => v.clone(),
            Self::Bytes(b) => serde_json::json!(format!("0x{}", hex::encode(b))),
            Self::Vector(dims) => serde_json::json!(dims),
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

/// Encode i64 as u64 via direct bit-cast.
///
/// This preserves backward compatibility: existing u64 values like `42` decode
/// as `i64(42)`.  Negative values wrap via two's complement.
fn i64_to_u64(v: i64) -> u64 {
    v as u64
}

/// Decode u64 back to i64 via direct bit-cast.
fn u64_to_i64(v: u64) -> i64 {
    v as i64
}

/// Compute SHA-256(key | "|" | data) and return first 8 bytes as u64.
pub fn content_hash_u64(key: &str, data: &[u8]) -> u64 {
    let hash = content_hash_full(key, data);
    u64::from_le_bytes(hash[..8].try_into().unwrap())
}

/// Full SHA-256 content hash.
pub fn content_hash_full(key: &str, data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.update(b"|");
    hasher.update(data);
    hasher.finalize().into()
}

/// Serialize Vec<f64> to bytes (little-endian f64 array).
pub fn vector_to_bytes(dims: &[f64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(dims.len() * 8);
    for d in dims {
        buf.extend_from_slice(&d.to_le_bytes());
    }
    buf
}

/// Deserialize bytes back to Vec<f64>.
pub fn bytes_to_vector(data: &[u8]) -> Result<Vec<f64>, String> {
    if data.len() % 8 != 0 {
        return Err(format!(
            "vector blob length {} not divisible by 8",
            data.len()
        ));
    }
    let dims: Vec<f64> = data
        .chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    Ok(dims)
}

/// Simple hex encoding (avoids pulling in a separate crate for this one use).
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ---------------------------------------------------------------------------
// Auto-detection from string literals
// ---------------------------------------------------------------------------

/// Try to infer the type of a string literal for SQL parsing.
/// Returns `(TypedValue, TypeTag)`.
pub fn infer_from_string(s: &str) -> TypedValue {
    let trimmed = s.trim();

    // Try JSON object or array
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return TypedValue::Json(v);
        }
    }

    // Try boolean
    if trimmed.eq_ignore_ascii_case("true") {
        return TypedValue::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return TypedValue::Bool(false);
    }

    // Try integer
    if let Ok(v) = trimmed.parse::<i64>() {
        return TypedValue::Integer(v);
    }

    // Try float
    if let Ok(v) = trimmed.parse::<f64>() {
        return TypedValue::Float(v);
    }

    // Default to text
    TypedValue::Text(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i64_roundtrip() {
        for v in [0i64, 1, -1, i64::MIN, i64::MAX, 42, -42] {
            assert_eq!(u64_to_i64(i64_to_u64(v)), v);
        }
    }

    #[test]
    fn f64_roundtrip() {
        for v in [0.0f64, 1.0, -1.0, 3.14, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(f64::from_bits(v.to_bits()), v);
        }
    }

    #[test]
    fn encode_decode_integer() {
        let tv = TypedValue::Integer(42);
        let (cell, blob) = tv.encode("test");
        assert!(blob.is_none());
        let decoded = TypedValue::decode(TypeTag::Integer, cell, None).unwrap();
        assert_eq!(decoded, TypedValue::Integer(42));
    }

    #[test]
    fn encode_decode_text() {
        let tv = TypedValue::Text("hello world".to_string());
        let (cell, blob) = tv.encode("mykey");
        assert!(blob.is_some());
        let decoded = TypedValue::decode(TypeTag::Text, cell, blob.as_deref()).unwrap();
        assert_eq!(decoded, TypedValue::Text("hello world".to_string()));
    }

    #[test]
    fn encode_decode_json() {
        let obj = serde_json::json!({"name": "Alice", "age": 30});
        let tv = TypedValue::Json(obj.clone());
        let (cell, blob) = tv.encode("user:alice");
        assert!(blob.is_some());
        let decoded = TypedValue::decode(TypeTag::Json, cell, blob.as_deref()).unwrap();
        assert_eq!(decoded, TypedValue::Json(obj));
    }

    #[test]
    fn encode_decode_vector() {
        let dims = vec![0.1, 0.2, 0.3, -1.5, 42.0];
        let tv = TypedValue::Vector(dims.clone());
        let (cell, blob) = tv.encode("doc:embedding");
        assert!(blob.is_some());
        let decoded = TypedValue::decode(TypeTag::Vector, cell, blob.as_deref()).unwrap();
        assert_eq!(decoded, TypedValue::Vector(dims));
    }

    #[test]
    fn content_hash_differs_by_key() {
        let data = b"same data";
        let h1 = content_hash_u64("key1", data);
        let h2 = content_hash_u64("key2", data);
        assert_ne!(h1, h2, "different keys should produce different hashes");
    }

    #[test]
    fn infer_json() {
        let v = infer_from_string(r#"{"name": "Alice"}"#);
        assert!(matches!(v, TypedValue::Json(_)));
    }

    #[test]
    fn infer_integer() {
        let v = infer_from_string("42");
        assert_eq!(v, TypedValue::Integer(42));
    }

    #[test]
    fn infer_text() {
        let v = infer_from_string("hello world");
        assert_eq!(v, TypedValue::Text("hello world".to_string()));
    }

    #[test]
    fn null_sentinel() {
        let tv = TypedValue::Null;
        let (cell, blob) = tv.encode("k");
        assert_eq!(cell, NULL_SENTINEL);
        assert!(blob.is_none());
    }

    #[test]
    fn backward_compat_u64_as_integer() {
        // Existing u64 value 42 should decode as Integer(42) when tag is Integer.
        // Direct bit-cast: 42u64 → 42i64.
        let decoded = TypedValue::decode(TypeTag::Integer, 42u64, None).unwrap();
        assert_eq!(decoded, TypedValue::Integer(42));
    }

    #[test]
    fn negative_integer_roundtrip() {
        let tv = TypedValue::Integer(-1);
        let (cell, blob) = tv.encode("test");
        assert!(blob.is_none());
        let decoded = TypedValue::decode(TypeTag::Integer, cell, None).unwrap();
        assert_eq!(decoded, TypedValue::Integer(-1));
    }
}
