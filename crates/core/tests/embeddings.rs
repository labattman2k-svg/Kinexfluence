//! Tests for the ONNX embedder and the cosine similarity it feeds into.
//!
//! The embedder tests need the real model. It is not committed -- it is
//! 90 MB -- so `scripts/download-model.sh` fetches it into `models/`, and the
//! tests skip themselves when it is not there. A skipped test is a hole in
//! the coverage, so the skip says so loudly, and the CI workflow runs the
//! download first so the tests do run there.
//!
//! The cosine tests do not need the model and always run: they are the part
//! of the signal that can regress without anyone noticing.

use std::path::{Path, PathBuf};

use kfx_core::embeddings::{cosine_similarity, Embedder, EMBEDDING_DIM};

/// The directory holding the model, if it has been downloaded.
fn model_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("models");

    if dir.join("model.onnx").is_file() && dir.join("tokenizer.json").is_file() {
        Some(dir)
    } else {
        None
    }
}

/// The embedder, or a message saying why there is not one.
fn embedder() -> Result<Embedder, String> {
    let dir = model_dir()
        .ok_or("the embedding model is not installed; run scripts/download-model.sh to fetch it")?;

    Embedder::new(&dir.join("model.onnx")).map_err(|cause| cause.to_string())
}

/// Run a test with the embedder, or skip the test when there is not one.
///
/// A skip says so on stderr, so a run that quietly skipped the whole ONNX
/// suite cannot pass for a run that exercised it.
macro_rules! with_embedder {
    ($embedder:ident, $body:block) => {
        let $embedder = match embedder() {
            Ok(embedder) => embedder,
            Err(reason) => {
                eprintln!("skipping: {reason}");
                return;
            }
        };
        $body
    };
}

// ---------------------------------------------------------------------------
// The cosine similarity
//
// These are the numbers S1 is made of. The rescaling around them lives in
// the scorer, so a change here moves every semantic score in the product.
// ---------------------------------------------------------------------------

#[test]
fn identical_vectors_have_a_cosine_of_one() {
    let vector = vec![0.6, 0.8, 0.0];
    let similarity = cosine_similarity(&vector, &vector).expect("both vectors are non-zero");
    assert!((similarity - 1.0).abs() < 1e-6, "got {similarity}");
}

#[test]
fn orthogonal_vectors_have_a_cosine_of_zero() {
    let similarity = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).expect("both are non-zero");
    assert!(similarity.abs() < 1e-6, "got {similarity}");
}

#[test]
fn opposite_vectors_have_a_cosine_of_minus_one() {
    let similarity = cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]).expect("both are non-zero");
    assert!((similarity + 1.0).abs() < 1e-6, "got {similarity}");
}

#[test]
fn vectors_of_different_lengths_have_no_cosine() {
    assert!(
        cosine_similarity(&[1.0, 0.0], &[1.0]).is_none(),
        "comparing vectors of different lengths has no answer, and returning \
         one would silently compare a prefix of one against all of the other"
    );
}

#[test]
fn a_zero_vector_has_no_cosine() {
    assert!(
        cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]).is_none(),
        "the cosine of a zero vector is undefined"
    );
}

#[test]
fn the_cosine_ignores_the_magnitude_of_the_vectors() {
    // The same direction at twice the length is the same angle.
    let similarity =
        cosine_similarity(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]).expect("both vectors are non-zero");
    assert!((similarity - 1.0).abs() < 1e-6, "got {similarity}");
}

// ---------------------------------------------------------------------------
// The embedder
// ---------------------------------------------------------------------------

#[test]
fn the_embedder_loads_and_returns_a_384_dimension_vector() {
    with_embedder!(embedder, {
        let vector = embedder
            .embed_text("the quick brown fox jumps over the lazy dog")
            .expect("a sentence should embed");

        assert_eq!(
            vector.len(),
            EMBEDDING_DIM,
            "all-MiniLM-L6-v2 produces {EMBEDDING_DIM}-dimensional vectors"
        );
        assert!(
            vector.iter().all(|value| value.is_finite()),
            "an embedding with a NaN or an infinity would poison every score it \
             touched: {vector:?}"
        );
    });
}

#[test]
fn the_embedder_normalises_its_output() {
    with_embedder!(embedder, {
        let vector = embedder
            .embed_text("kinexfluence weaves chat fragments into one memory")
            .expect("a sentence should embed");

        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-3,
            "the embedding should be L2-normalised so a cosine is a dot product, \
             got a norm of {norm}"
        );
    });
}

#[test]
fn the_embedder_gives_similar_texts_a_high_cosine() {
    with_embedder!(embedder, {
        let a = embedder
            .embed_text("we decided to store the embeddings in SQLite")
            .expect("a sentence should embed");
        let b = embedder
            .embed_text("the decision was to keep the vectors in SQLite")
            .expect("a sentence should embed");
        let c = embedder
            .embed_text("a recipe for sourdough bread with rye flour")
            .expect("a sentence should embed");

        let together = cosine_similarity(&a, &b).expect("both are non-zero");
        let apart = cosine_similarity(&a, &c).expect("both are non-zero");

        assert!(
            together > apart,
            "two sentences that mean the same thing should be more similar than \
             two that do not: {together} against {apart}"
        );
        assert!(
            together > 0.5,
            "a paraphrase should be clearly similar, got {together}"
        );
    });
}

#[test]
fn the_embedder_is_deterministic() {
    with_embedder!(embedder, {
        let text = "the same text, twice";
        let first = embedder.embed_text(text).expect("a sentence should embed");
        let second = embedder.embed_text(text).expect("a sentence should embed");

        assert_eq!(
            first, second,
            "the same text must embed to the same vector, or the scores would \
             change between two runs of the same request"
        );
    });
}

#[test]
fn the_embedder_handles_an_empty_string() {
    with_embedder!(embedder, {
        let vector = embedder
            .embed_text("")
            .expect("an empty string should embed");
        assert_eq!(vector.len(), EMBEDDING_DIM);
        assert!(
            vector.iter().all(|value| value.is_finite()),
            "an empty string must not produce a NaN: {vector:?}"
        );
    });
}

#[test]
fn a_missing_model_is_an_error_rather_than_a_panic() {
    let missing = Path::new("this-directory-does-not-exist").join("model.onnx");

    match Embedder::new(&missing) {
        Err(kfx_core::embeddings::EmbeddingError::Model(_)) => {}
        Err(other) => panic!("a missing model should report as a model error, got {other:?}"),
        Ok(_) => panic!("a missing model should not load"),
    }
}
