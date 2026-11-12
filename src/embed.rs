//! Optional neural embeddings for memory search (build with `--features neural`).
//!
//! Adapted from the embedding module of the ADAM project (MIT, same author): a small local
//! sentence-transformer (`all-MiniLM-L6-v2`, quantized) run through ONNX. The model is fetched once, on
//! first use, into the Heron folder and then works offline. Loading and inference sit behind one
//! process-wide mutex, so only one download can ever happen at a time, and a failed load is not cached:
//! the next call simply tries again (a `OnceLock` would remember a network hiccup forever).

use std::path::Path;

/// Tag stored next to every embedding, so vectors from different embedders are never compared.
pub const HASH_TAG: &str = "hash-v1";
pub const NEURAL_TAG: &str = "minilm-l6-v2-q";
pub const REMOTE_TAG: &str = "remote";

pub fn neural_built_in() -> bool {
    cfg!(feature = "neural")
}

#[cfg(feature = "neural")]
pub fn neural_embed(cache_dir: &Path, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
    use std::sync::Mutex;
    static MODEL: Mutex<Option<TextEmbedding>> = Mutex::new(None);
    let mut guard = MODEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        std::fs::create_dir_all(cache_dir)?;
        let options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2Q).with_show_download_progress(false).with_cache_dir(cache_dir.to_path_buf());
        *guard = Some(TextEmbedding::try_new(options).map_err(|e| anyhow::anyhow!("could not load the memory model: {e}"))?);
    }
    let model = guard.as_mut().expect("initialized above");
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    model.embed(refs, None).map_err(|e| anyhow::anyhow!("could not embed text: {e}"))
}

#[cfg(not(feature = "neural"))]
pub fn neural_embed(_cache_dir: &Path, _texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    anyhow::bail!("this build of Heron does not include the neural memory model")
}
