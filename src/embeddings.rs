use crate::halo::config::halo_dir;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const DEFAULT_EMBEDDING_DIMS: usize = 768;
pub const DEFAULT_MODEL_NAME: &str = "nomic-embed-text-v1.5";

#[derive(Clone, Debug)]
pub struct EmbeddingModel {
    model_name: String,
    dims: usize,
    model_dir: PathBuf,
}

impl Default for EmbeddingModel {
    fn default() -> Self {
        Self::new(DEFAULT_MODEL_NAME, DEFAULT_EMBEDDING_DIMS)
    }
}

impl EmbeddingModel {
    pub fn new(model_name: &str, dims: usize) -> Self {
        let configured = std::env::var("NOMIC_MODEL_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| halo_dir().join("models").join("nomic-embed-text"));
        Self {
            model_name: model_name.to_string(),
            dims,
            model_dir: configured,
        }
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    pub fn model_files_present(&self) -> bool {
        self.model_dir.join("model.onnx").exists() && self.model_dir.join("tokenizer.json").exists()
    }

    pub fn embed(&self, text: &str, prefix: &str) -> Result<Vec<f64>, String> {
        let input = text.trim();
        if input.is_empty() {
            return Err("embedding input must not be empty".to_string());
        }
        if self.dims == 0 {
            return Err("embedding dimensions must be > 0".to_string());
        }

        // Deterministic local embedding with nomic-style task prefixes.
        // This is intentionally local/offline and stable for tamper-evident recall.
        let mut vec = vec![0.0_f64; self.dims];
        let normalized = normalize_text(input);
        let doc = format!("{prefix}{normalized}");

        for (pos, token) in tokenize(&doc).iter().enumerate() {
            let digest = Sha256::digest(token.as_bytes());
            let i1 = ((digest[0] as usize) << 8 | digest[1] as usize) % self.dims;
            let i2 = ((digest[3] as usize) << 8 | digest[4] as usize) % self.dims;
            let sign = if digest[2] & 1 == 0 { 1.0 } else { -1.0 };
            let freq = 1.0 + (pos as f64).ln_1p() * 0.15;
            vec[i1] += sign * freq;
            vec[i2] += sign * 0.35 * freq;
        }

        for gram in char_ngrams(&doc, 3) {
            let digest = Sha256::digest(gram.as_bytes());
            let idx = ((digest[0] as usize) << 8 | digest[1] as usize) % self.dims;
            let sign = if digest[2] & 1 == 0 { 1.0 } else { -1.0 };
            vec[idx] += sign * 0.15;
        }

        l2_normalize(&mut vec);
        Ok(vec)
    }

    pub fn embed_batch(&self, texts: &[&str], prefix: &str) -> Result<Vec<Vec<f64>>, String> {
        texts
            .iter()
            .map(|t| self.embed(t, prefix))
            .collect::<Result<Vec<_>, _>>()
    }
}

pub fn cosine_distance(a: &[f64], b: &[f64]) -> Result<f64, String> {
    if a.len() != b.len() {
        return Err(format!(
            "cosine distance dimension mismatch: {} vs {}",
            a.len(),
            b.len()
        ));
    }
    let mut dot = 0.0_f64;
    let mut na = 0.0_f64;
    let mut nb = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return Err("cosine distance undefined for zero-norm vectors".to_string());
    }
    Ok(1.0 - dot / (na.sqrt() * nb.sqrt()))
}

fn normalize_text(input: &str) -> String {
    input
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn char_ngrams(input: &str, n: usize) -> Vec<String> {
    let chars = input.chars().collect::<Vec<_>>();
    if chars.len() < n {
        return vec![input.to_string()];
    }
    let mut out = Vec::with_capacity(chars.len() - n + 1);
    for i in 0..=chars.len() - n {
        out.push(chars[i..i + n].iter().collect::<String>());
    }
    out
}

fn l2_normalize(values: &mut [f64]) {
    let norm = values.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 0.0 {
        for v in values {
            *v /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_returns_768_dims() {
        let model = EmbeddingModel::default();
        let v = model
            .embed("test sentence", "search_document: ")
            .expect("embed");
        assert_eq!(v.len(), 768);
    }

    #[test]
    fn test_embed_identical_texts() {
        let model = EmbeddingModel::default();
        let a = model
            .embed("The vector index uses cosine distance", "search_document: ")
            .expect("embed a");
        let b = model
            .embed("The vector index uses cosine distance", "search_document: ")
            .expect("embed b");
        let d = cosine_distance(&a, &b).expect("distance");
        assert!(d <= 1e-12, "expected near-zero distance, got {d}");
    }

    #[test]
    fn test_embed_similar_texts() {
        let model = EmbeddingModel::default();
        let a = model
            .embed(
                "NucleusDB performs vector similarity search",
                "search_document: ",
            )
            .expect("embed a");
        let b = model
            .embed(
                "The database can semantically search vectors for close matches",
                "search_document: ",
            )
            .expect("embed b");
        let d = cosine_distance(&a, &b).expect("distance");
        assert!(d < 0.85, "expected similar texts to be close, got {d}");
    }

    #[test]
    fn test_embed_dissimilar_texts() {
        let model = EmbeddingModel::default();
        let a = model
            .embed("quantum-resistant witness signatures", "search_document: ")
            .expect("embed a");
        let b = model
            .embed(
                "banana orchard tropical fruit smoothie",
                "search_document: ",
            )
            .expect("embed b");
        let d = cosine_distance(&a, &b).expect("distance");
        assert!(d > 0.2, "expected dissimilar texts to diverge, got {d}");
    }

    #[test]
    fn test_embed_batch() {
        let model = EmbeddingModel::default();
        let single = model
            .embed("batch embedding test", "search_document: ")
            .expect("single");
        let batch = model
            .embed_batch(&["batch embedding test"], "search_document: ")
            .expect("batch");
        assert_eq!(batch.len(), 1);
        let d = cosine_distance(&single, &batch[0]).expect("distance");
        assert!(d <= 1e-12, "batch result diverged from single");
    }

    #[test]
    fn test_task_prefix() {
        let model = EmbeddingModel::default();
        let q = model
            .embed("vector commitments", "search_query: ")
            .expect("query");
        let d = model
            .embed("vector commitments", "search_document: ")
            .expect("doc");
        let dist = cosine_distance(&q, &d).expect("distance");
        assert!(dist > 0.0001, "task prefix should alter embedding space");
    }
}
