use crate::halo::config::halo_dir;
use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokenizers::Tokenizer;

pub const DEFAULT_EMBEDDING_DIMS: usize = 768;
pub const DEFAULT_MODEL_NAME: &str = "nomic-embed-text-v1.5";
const HASH_BACKEND_ENV: &str = "AGENTHALO_EMBEDDING_BACKEND";
const HASH_BACKEND_VALUE: &str = "hash-test";

#[derive(Debug)]
struct OnnxEmbeddingRuntime {
    session: std::sync::Mutex<Session>,
    tokenizer: Tokenizer,
}

#[derive(Clone, Debug)]
pub struct EmbeddingModel {
    model_name: String,
    dims: usize,
    model_dir: PathBuf,
    runtime: std::sync::Arc<OnceLock<Result<OnnxEmbeddingRuntime, String>>>,
    hash_backend_override: bool,
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
            runtime: std::sync::Arc::new(OnceLock::new()),
            hash_backend_override: false,
        }
    }

    pub fn new_hash_test_backend(model_name: &str, dims: usize) -> Self {
        let mut model = Self::new(model_name, dims);
        model.hash_backend_override = true;
        model
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
        if self.using_hash_test_backend() {
            return self.embed_hash_fallback(text, prefix);
        }
        self.embed_onnx(text, prefix)
    }

    pub fn embed_batch(&self, texts: &[&str], prefix: &str) -> Result<Vec<Vec<f64>>, String> {
        texts
            .iter()
            .map(|t| self.embed(t, prefix))
            .collect::<Result<Vec<_>, _>>()
    }

    fn using_hash_test_backend(&self) -> bool {
        if self.hash_backend_override {
            return true;
        }
        std::env::var(HASH_BACKEND_ENV)
            .ok()
            .map(|v| v.trim().eq_ignore_ascii_case(HASH_BACKEND_VALUE))
            .unwrap_or(false)
    }

    fn runtime(&self) -> Result<&OnnxEmbeddingRuntime, String> {
        let loaded = self.runtime.get_or_init(|| self.load_runtime());
        loaded
            .as_ref()
            .map_err(|e| format!("embedding runtime unavailable: {e}"))
    }

    fn load_runtime(&self) -> Result<OnnxEmbeddingRuntime, String> {
        let model_path = self.model_dir.join("model.onnx");
        let tokenizer_path = self.model_dir.join("tokenizer.json");
        if !model_path.exists() || !tokenizer_path.exists() {
            return Err(format!(
                "nomic embedding files missing in {} (need model.onnx + tokenizer.json)",
                self.model_dir.display()
            ));
        }

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("load tokenizer {}: {e}", tokenizer_path.display()))?;
        let session = Session::builder()
            .map_err(|e| format!("create ONNX session builder: {e}"))?
            .commit_from_file(&model_path)
            .map_err(|e| format!("load ONNX model {}: {e}", model_path.display()))?;

        Ok(OnnxEmbeddingRuntime {
            session: std::sync::Mutex::new(session),
            tokenizer,
        })
    }

    fn embed_onnx(&self, text: &str, prefix: &str) -> Result<Vec<f64>, String> {
        let input = text.trim();
        if input.is_empty() {
            return Err("embedding input must not be empty".to_string());
        }
        if self.dims == 0 {
            return Err("embedding dimensions must be > 0".to_string());
        }
        let runtime = self.runtime()?;
        let prefixed = format!("{prefix}{input}");
        let encoding = runtime
            .tokenizer
            .encode(prefixed, true)
            .map_err(|e| format!("tokenize embedding input: {e}"))?;
        let token_ids = encoding.get_ids();
        if token_ids.is_empty() {
            return Err("tokenizer returned empty token stream".to_string());
        }

        let seq_len = token_ids.len();
        let ids = Array2::from_shape_vec(
            (1, seq_len),
            token_ids.iter().map(|&v| i64::from(v)).collect::<Vec<_>>(),
        )
        .map_err(|e| format!("build input_ids tensor: {e}"))?;
        let mask = Array2::from_shape_vec(
            (1, seq_len),
            encoding
                .get_attention_mask()
                .iter()
                .map(|&v| i64::from(v))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| format!("build attention_mask tensor: {e}"))?;
        let token_types = Array2::from_shape_vec(
            (1, seq_len),
            encoding
                .get_type_ids()
                .iter()
                .map(|&v| i64::from(v))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| format!("build token_type_ids tensor: {e}"))?;

        let input_ids = Tensor::from_array(ids).map_err(|e| format!("tensor input_ids: {e}"))?;
        let attention_mask =
            Tensor::from_array(mask.clone()).map_err(|e| format!("tensor attention_mask: {e}"))?;
        let token_type_ids =
            Tensor::from_array(token_types).map_err(|e| format!("tensor token_type_ids: {e}"))?;
        let mut session = runtime
            .session
            .lock()
            .map_err(|e| format!("lock ONNX session: {e}"))?;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => attention_mask,
                "token_type_ids" => token_type_ids,
            ])
            .map_err(|e| format!("run ONNX inference: {e}"))?;

        if outputs.len() == 0 {
            return Err("ONNX model returned no outputs".to_string());
        }
        let first = &outputs[0];
        let arr = first
            .try_extract_array::<f32>()
            .map_err(|e| format!("extract embedding tensor: {e}"))?;
        let shape = arr.shape().to_vec();
        if shape.is_empty() {
            return Err("embedding output has empty shape".to_string());
        }

        // Handle either [1, hidden] or [1, seq, hidden] output layouts.
        let mut vec = if shape.len() == 2 {
            if shape[0] != 1 {
                return Err(format!(
                    "unexpected batch size in embeddings output: {}",
                    shape[0]
                ));
            }
            let arr2 = arr
                .into_dimensionality::<ndarray::Ix2>()
                .map_err(|e| format!("reshape embedding output to rank-2 tensor: {e}"))?;
            arr2.row(0)
                .iter()
                .map(|v| f64::from(*v))
                .collect::<Vec<_>>()
        } else if shape.len() == 3 {
            if shape[0] != 1 {
                return Err(format!(
                    "unexpected batch size in embeddings output: {}",
                    shape[0]
                ));
            }
            let arr3 = arr
                .into_dimensionality::<ndarray::Ix3>()
                .map_err(|e| format!("reshape embedding output to rank-3 tensor: {e}"))?;
            let seq = shape[1];
            let hidden = shape[2];
            let mut pooled = vec![0.0_f64; hidden];
            let mut weight_sum = 0.0_f64;
            for i in 0..seq {
                let weight = mask[[0, i]] as f64;
                if weight <= 0.0 {
                    continue;
                }
                for j in 0..hidden {
                    pooled[j] += f64::from(arr3[[0, i, j]]) * weight;
                }
                weight_sum += weight;
            }
            if weight_sum == 0.0 {
                return Err("all attention-mask weights are zero".to_string());
            }
            for v in &mut pooled {
                *v /= weight_sum;
            }
            pooled
        } else {
            return Err(format!(
                "unsupported embedding output rank {} (shape {:?})",
                shape.len(),
                shape
            ));
        };

        if vec.len() != self.dims {
            return Err(format!(
                "embedding dimension mismatch: expected {}, got {}",
                self.dims,
                vec.len()
            ));
        }
        l2_normalize(&mut vec);
        Ok(vec)
    }

    fn embed_hash_fallback(&self, text: &str, prefix: &str) -> Result<Vec<f64>, String> {
        let input = text.trim();
        if input.is_empty() {
            return Err("embedding input must not be empty".to_string());
        }
        if self.dims == 0 {
            return Err("embedding dimensions must be > 0".to_string());
        }

        // Deterministic fallback for isolated tests that explicitly opt in via
        // AGENTHALO_EMBEDDING_BACKEND=hash-test.
        // Deterministic local embedding with nomic-style task prefixes.
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
}

pub fn cosine_distance(a: &[f64], b: &[f64]) -> Result<f64, String> {
    crate::vector_index::cosine_distance_checked(a, b)
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
    use crate::test_support::env_lock;

    fn test_model() -> EmbeddingModel {
        EmbeddingModel::new_hash_test_backend(DEFAULT_MODEL_NAME, DEFAULT_EMBEDDING_DIMS)
    }

    #[test]
    fn test_embed_returns_768_dims() {
        let model = test_model();
        let v = model
            .embed("test sentence", "search_document: ")
            .expect("embed");
        assert_eq!(v.len(), 768);
    }

    #[test]
    fn test_embed_identical_texts() {
        let model = test_model();
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
        let model = test_model();
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
        let model = test_model();
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
        let model = test_model();
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
        let model = test_model();
        let q = model
            .embed("vector commitments", "search_query: ")
            .expect("query");
        let d = model
            .embed("vector commitments", "search_document: ")
            .expect("doc");
        let dist = cosine_distance(&q, &d).expect("distance");
        assert!(dist > 0.0001, "task prefix should alter embedding space");
    }

    #[test]
    fn test_model_files_required_when_hash_backend_disabled() {
        let _guard = env_lock().lock().expect("lock env");
        let prev = std::env::var(HASH_BACKEND_ENV).ok();
        // SAFETY: test-only env mutation is serialized by env_lock().
        unsafe { std::env::remove_var(HASH_BACKEND_ENV) };
        let model = EmbeddingModel::new(DEFAULT_MODEL_NAME, DEFAULT_EMBEDDING_DIMS);
        let err = model
            .embed("requires model files", "search_document: ")
            .expect_err("missing model files must error");
        assert!(err.contains("embedding runtime unavailable"));
        match prev {
            Some(v) => {
                // SAFETY: test-only env mutation is serialized by env_lock().
                unsafe { std::env::set_var(HASH_BACKEND_ENV, v) };
            }
            None => {
                // SAFETY: test-only env mutation is serialized by env_lock().
                unsafe { std::env::remove_var(HASH_BACKEND_ENV) };
            }
        }
    }
}
