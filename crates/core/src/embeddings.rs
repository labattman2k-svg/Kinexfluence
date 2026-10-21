//! Semantic embeddings: the S1 signal.
//!
//! A sentence embedding turns a piece of text into a fixed-length vector, so
//! that two pieces of text that mean the same thing land near each other in
//! vector space even when they share no words. S1 is the cosine similarity
//! between a snippet's embedding and its project's.
//!
//! The model is `all-MiniLM-L6-v2`, a 384-dimensional sentence encoder. The
//! exported ONNX graph is the bare transformer: it emits one vector per token,
//! and the sentence embedding is the mean over the tokens that are not
//! padding. The mean pooling and the L2 normalisation the
//! sentence-transformers wrapper applies live in this module, not in the graph.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ort::session::Session;
use ort::value::Value;
use tokenizers::{Tokenizer, TruncationParams};

/// The dimensionality of the embedding model.
///
/// A mismatch here means the wrong model is loaded, and the cosine similarity
/// would still compute -- silently comparing two different spaces.
pub const EMBEDDING_DIM: usize = 384;

/// The longest text, in tokens, the model will encode.
///
/// The model's own configuration caps sequences at 256 tokens. The tokenizer
/// shipped beside it truncates at 128, which is the `bert-base-uncased`
/// default carried over rather than a property of this model, so the limit is
/// set here to the value the model was published with.
pub const MAX_TOKENS: usize = 256;

/// The name of the model file.
pub const MODEL_FILE: &str = "model.onnx";

/// The name of the tokenizer file, which sits beside the model.
pub const TOKENIZER_FILE: &str = "tokenizer.json";

/// The environment variable that names the directory holding the model.
///
/// The model is not bundled into the binary -- it is 90 MB, and the core
/// builds a different ONNX Runtime for every platform it targets -- so it is
/// fetched separately and this is how the server is told where it is.
pub const MODEL_DIR_ENV: &str = "KINEXFLUENCE_MODEL_DIR";

/// The graph's inputs and its one output.
///
/// The exported transformer ends in a `last_hidden_state` node and has no
/// pooled output of its own: the pooling is the sentence-transformers
/// wrapper's job, and it happens here.
const INPUT_IDS: &str = "input_ids";
const ATTENTION_MASK: &str = "attention_mask";
const TOKEN_TYPE_IDS: &str = "token_type_ids";
const OUTPUT_NAME: &str = "last_hidden_state";

/// Everything that can go wrong while embedding text.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// The model file could not be read, or is not a valid ONNX graph.
    #[error("could not load the embedding model: {0}")]
    Model(String),

    /// The tokenizer file could not be read, or is not a valid tokenizer.
    #[error("could not load the tokenizer: {0}")]
    Tokenizer(String),

    /// The model produced something other than the expected output.
    #[error("the embedding model produced an unusable output: {0}")]
    Output(String),

    /// Inference itself failed.
    #[error("embedding inference failed: {0}")]
    Inference(String),
}

impl From<ort::Error> for EmbeddingError {
    fn from(cause: ort::Error) -> Self {
        EmbeddingError::Inference(cause.to_string())
    }
}

/// A text embedder: an ONNX session plus the tokenizer that feeds it.
///
/// The session is not `Sync` in spirit -- ONNX Runtime's own comment says
/// concurrent use segfaults on some execution providers -- so an embedder is
/// held behind a mutex and used one call at a time. That is fine for the
/// current use: one brief and one snippet at a time. A pool of sessions is
/// the obvious thing to reach for if throughput ever matters.
pub struct Embedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl Embedder {
    /// Load an embedder from a model file.
    ///
    /// The tokenizer is the `tokenizer.json` beside the model, which is where
    /// `scripts/download-model.sh` puts it. The tokenizer has to be the one
    /// the model was trained with: a different tokenizer produces different
    /// token ids, and the model then sees nonsense.
    pub fn new(model_path: &Path) -> Result<Self, EmbeddingError> {
        let tokenizer_path = model_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(TOKENIZER_FILE);

        let session = Session::builder()
            .and_then(|builder| builder.commit_from_file(model_path))
            .map_err(|cause| EmbeddingError::Model(cause.to_string()))?;

        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|cause| EmbeddingError::Tokenizer(cause.to_string()))?;

        // The shipped tokenizer truncates at 128 tokens and pads every
        // sequence out to 128. Both are replaced here: the truncation becomes
        // the model's own context length, and the padding goes away entirely,
        // because the graph takes a dynamic sequence length and a short
        // snippet should not cost a full-length forward pass.
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKENS,
                ..TruncationParams::default()
            }))
            .map_err(|cause| EmbeddingError::Tokenizer(cause.to_string()))?;
        tokenizer.with_padding(None);

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    /// Embed a piece of text into a vector.
    ///
    /// The result is mean-pooled over the non-padding tokens and then
    /// L2-normalised, so that a cosine similarity is a plain dot product and
    /// the magnitude of the vector carries no information of its own.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|cause| EmbeddingError::Tokenizer(cause.to_string()))?;

        let ids = encoding.get_ids();
        // The truncation above bounds this, but the count is read from the
        // encoding rather than assumed, so a tokenizer configured differently
        // cannot index past the tensor.
        let count = ids.len();
        if count == 0 {
            return Ok(vec![0.0; EMBEDDING_DIM]);
        }

        let input_ids: Vec<i64> = ids.iter().map(|id| *id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|mask| *mask as i64)
            .collect();
        // A single-segment pair, so every token is of type 0.
        let token_type_ids: Vec<i64> = vec![0; count];

        let shape = vec![1_i64, count as i64];
        let mut session = self.session.lock().map_err(|_| {
            EmbeddingError::Inference("the embedding session lock was poisoned".to_string())
        })?;

        let outputs = session
            .run(ort::inputs![
                INPUT_IDS => Value::from_array((shape.clone(), input_ids))?,
                ATTENTION_MASK => Value::from_array((shape.clone(), attention_mask.clone()))?,
                TOKEN_TYPE_IDS => Value::from_array((shape, token_type_ids))?,
            ])
            .map_err(|cause| EmbeddingError::Inference(cause.to_string()))?;

        let output = outputs.get(OUTPUT_NAME).ok_or_else(|| {
            EmbeddingError::Output(format!(
                "the graph produced no output named {OUTPUT_NAME:?}; it produced {:?}",
                outputs.keys().collect::<Vec<_>>()
            ))
        })?;

        let (dims, data) = output
            .try_extract_tensor::<f32>()
            .map_err(|cause| EmbeddingError::Output(cause.to_string()))?;

        pool(data, dims, &attention_mask, count)
    }
}

/// Mean-pool a token-level output into one sentence vector.
///
/// The token axis is masked with the attention mask, so padding tokens do not
/// drag the mean toward zero, and the result is normalised so the cosine
/// similarity is a dot product.
fn pool(
    data: &[f32],
    dims: &[i64],
    mask: &[i64],
    count: usize,
) -> Result<Vec<f32>, EmbeddingError> {
    // The output is [batch, sequence, hidden].
    let hidden = dimension(dims, 2)?;
    let sequence = dimension(dims, 1)?;
    let batch = dims.first().copied().unwrap_or(1);

    if batch != 1 {
        return Err(EmbeddingError::Output(format!(
            "expected a batch of one, got {batch}"
        )));
    }
    if hidden == 0 {
        return Err(EmbeddingError::Output(
            "the model produced a zero-width vector".to_string(),
        ));
    }

    let row = sequence * hidden;
    if data.len() < row {
        return Err(EmbeddingError::Output(format!(
            "the model produced {} values, fewer than the {row} one row of tokens needs",
            data.len()
        )));
    }

    let mut pooled = vec![0.0_f32; hidden];
    let mut live = 0.0_f32;
    for token in 0..count.min(sequence) {
        if mask.get(token).copied().unwrap_or(0) == 0 {
            continue;
        }
        live += 1.0;
        let start = token * hidden;
        for (index, value) in pooled.iter_mut().enumerate() {
            *value += data[start + index];
        }
    }

    if live > 0.0 {
        for value in &mut pooled {
            *value /= live;
        }
    }

    let norm = pooled.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut pooled {
            *value /= norm;
        }
    }

    Ok(pooled)
}

/// One axis of a tensor's shape.
fn dimension(dims: &[i64], axis: usize) -> Result<usize, EmbeddingError> {
    match dims.get(axis) {
        Some(value) => Ok(*value as usize),
        None => Err(EmbeddingError::Output(format!(
            "expected a 3-dimensional output, got {dims:?}"
        ))),
    }
}

/// The cosine similarity of two vectors.
///
/// Returns `None` when the vectors are of different lengths, or when either is
/// the zero vector -- the cosine of a zero vector is undefined, and returning a
/// number would hide that.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a <= 0.0 || norm_b <= 0.0 {
        return None;
    }

    Some(dot / (norm_a.sqrt() * norm_b.sqrt()))
}

/// The directories the model may live in, in the order they are tried.
///
/// The environment variable comes first, because a user who has put the model
/// somewhere specific has said where it is. The working directory follows,
/// because that is where a checkout has it. The executable's own directory is
/// last, which is where a bundled install would have it.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(dir) = std::env::var_os(MODEL_DIR_ENV) {
        if !dir.is_empty() {
            dirs.push(PathBuf::from(dir));
        }
    }

    dirs.push(PathBuf::from("models"));

    if let Ok(executable) = std::env::current_exe() {
        if let Some(dir) = executable.parent() {
            dirs.push(dir.join("models"));
        }
    }

    dirs
}

/// The directory holding the model and its tokenizer, if there is one.
pub fn model_dir() -> Option<PathBuf> {
    candidate_dirs()
        .into_iter()
        .find(|dir| dir.join(MODEL_FILE).is_file() && dir.join(TOKENIZER_FILE).is_file())
}

/// Load an embedder from the default location, if the model is there.
///
/// A missing model is not an error: the scorer falls back to a neutral S1
/// without it, and a user who has not fetched the model yet should get a
/// working application rather than a failure to start.
pub fn load_default() -> Option<Arc<Embedder>> {
    let dir = model_dir()?;
    match Embedder::new(&dir.join(MODEL_FILE)) {
        Ok(embedder) => Some(Arc::new(embedder)),
        Err(cause) => {
            tracing::warn!(error = %cause, "the embedding model could not be loaded");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooling_ignores_masked_tokens() {
        // Three tokens, the last one masked out. If the padding counted, the
        // mean would be pulled toward it and the result would differ from the
        // same two tokens pooled on their own.
        let data = vec![1.0, 0.0, 0.0, 1.0, 0.9, 0.9];
        let masked = pool(&data, &[1, 3, 2], &[1, 1, 0], 3).expect("the shapes line up");
        let unmasked = pool(&data[..4], &[1, 2, 2], &[1, 1], 2).expect("the shapes line up");

        assert!(
            (masked[0] - unmasked[0]).abs() < 1e-6 && (masked[1] - unmasked[1]).abs() < 1e-6,
            "the masked token must not reach the mean: {masked:?} against {unmasked:?}"
        );
    }

    #[test]
    fn pooling_normalises_the_result() {
        let data = vec![3.0, 4.0, 0.0, 0.0];
        let pooled = pool(&data, &[1, 2, 2], &[1, 1], 2).expect("the shapes line up");
        let norm = pooled.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "got {norm}");
    }
}
