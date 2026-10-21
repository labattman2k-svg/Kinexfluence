//! Heuristic scoring engine (S1-S7) and project profile entity extraction.
//!
//! Evaluates candidate snippets using seven signals:
//! - S1 (Semantic): Cosine similarity between the snippet's embedding and the
//!   project's, from `all-MiniLM-L6-v2`. Neutral (0.5) when no embedding is
//!   available.
//! - S2 (Code): Kind is code vs text.
//! - S3 (Markers): Decision and action markers.
//! - S4 (Entities): Project proper nouns and identifiers.
//! - S5 (Novelty): Stubbed to 0.5 until a second embedding can measure it.
//! - S6 (Position): Position within conversation turns.
//! - S7 (Provenance): Source trust, default 0.5.

use serde::{Deserialize, Serialize};

use crate::embeddings::{cosine_similarity, Embedder};
use crate::project::Project;

/// Markers indicating key decisions, requirements, or next steps.
pub const MARKERS: &[&str] = &[
    "we decided",
    "must",
    "cannot",
    "the fix is",
    "root cause",
    "required",
    "next step",
    "action item",
];

// ---------------------------------------------------------------------------
// Composition
//
// The weights are named and summed rather than written inline, because the sum
// is load-bearing: it is the denominator that turns a raw score into [0, 1].
// Hardcoding it would let a future signal change quietly rescale every score.
// ---------------------------------------------------------------------------

const W_CODE: f32 = 0.20;
const W_MARKER: f32 = 0.15;
const W_ENTITY: f32 = 0.15;
const W_NOVELTY: f32 = 0.05;
const W_POSITION: f32 = 0.05;
const W_PROVENANCE: f32 = 0.05;

/// The weight of S1, semantic similarity -- the heaviest of the seven.
///
/// S1 is what separates "this snippet is about the project's subject" from
/// "this snippet happens to contain the project's keywords". It carries more
/// weight than any other single signal because it is the one that generalises.
const W_SEMANTIC: f32 = 0.35;

/// The highest raw score reachable.
///
/// Every weight, so the division below normalises by what is actually
/// reachable rather than by an assumed 1.0.
const MAX_RAW: f32 =
    W_SEMANTIC + W_CODE + W_MARKER + W_ENTITY + W_NOVELTY + W_POSITION + W_PROVENANCE;

/// The S1 score used when no embedding is available.
///
/// Halfway, so an unembedded snippet neither gains nor loses the advantage
/// the signal would have given it. Any other default would move every score
/// when the model is absent, which would make the scores incomparable
/// between a machine with the model and one without.
const SEMANTIC_FALLBACK: f32 = 0.5;

/// The cosine similarity that maps to the middle of the S1 range.
///
/// A cosine between two sentences is rarely near its extremes: unrelated
/// sentences sit around 0.0 and near-duplicates around 0.9, so mapping them
/// straight onto [0, 1] would squash every real snippet into a narrow band
/// near the middle. The range is stretched instead, with this midpoint
/// falling on the neutral value.
const SEMANTIC_MIDPOINT: f32 = 0.35;

/// How much of the cosine range one unit of S1 covers.
const SEMANTIC_SPAN: f32 = 0.7;

/// Turn a cosine similarity into an S1 signal in [0, 1].
///
/// The cosine is rescaled around [`SEMANTIC_MIDPOINT`] and clamped: a cosine
/// of 1.0 means the two texts are the same, and a cosine of -1.0 means they
/// are opposites, and neither should push the signal outside its range.
fn semantic_signal(cosine: f32) -> f32 {
    ((cosine - SEMANTIC_MIDPOINT) / SEMANTIC_SPAN + 0.5).clamp(0.0, 1.0)
}

/// Slope and offset of the sigmoid, chosen against two fixed points:
///
/// - `raw_normalized = 1.0` (every signal perfect) -> 0.88, comfortably past
///   the 0.70 auto-select threshold.
/// - `raw_normalized = 0.5` (a middling snippet) -> 0.35, so an unremarkable
///   fragment stays visibly unremarkable.
///
/// Solving `sigmoid(s*1.0 + o) = 0.88` and `sigmoid(s*0.5 + o) = 0.35` gives
/// s ~= 5.2 and o ~= -3.2. A raw score of 0 maps to ~0.04.
const SCORE_SLOPE: f32 = 5.2;
const SCORE_OFFSET: f32 = 3.2;

/// Detailed breakdown of heuristic scores for a snippet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreDetails {
    /// Final composite score bounded by sigmoid in [0, 1].
    pub score: f32,
    /// S1 semantic similarity signal in [0, 1].
    pub semantic: f32,
    /// S2 code signal (1.0 for code, 0.0 otherwise).
    pub code: f32,
    /// S3 decision marker signal in [0, 1].
    pub marker: f32,
    /// S4 entity matching signal in [0, 1].
    pub entity: f32,
    /// S5 novelty signal (stubbed to 0.5).
    pub novelty: f32,
    /// S6 conversation position signal in [0, 1].
    pub position: f32,
    /// S7 source provenance signal (stubbed to 0.5).
    pub provenance: f32,
    /// The relevance floor a probe puts under this snippet, if any.
    ///
    /// `None` when no probe has answered for this snippet, which is the
    /// ordinary case. The floor is reported rather than folded into the
    /// signals, so a reader can see that the ranking was overruled by the
    /// user and not by the heuristic.
    pub probe_floor: Option<f32>,
}

/// Project text context and extracted entities used for scoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectProfile {
    /// Project name.
    pub name: String,
    /// Project description if set.
    pub description: Option<String>,
    /// Project brief if set.
    pub brief: Option<String>,
    /// The project's embedding, when one could be computed.
    ///
    /// `None` when no embedder was available, which makes S1 fall back to
    /// its neutral value rather than scoring a snippet as unrelated.
    pub embedding: Option<Vec<f32>>,
    /// Extracted proper nouns and code-like identifiers.
    pub entities: Vec<String>,
}

/// Input snippet data and conversation context required for scoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredSnippetInput {
    /// Snippet kind (e.g. "code" or "text").
    pub kind: String,
    /// Snippet text content.
    pub text: String,
    /// Author role of the parent message (e.g. "user" or "assistant").
    pub role: Option<String>,
    /// Chronological turn index within the conversation.
    pub turn_index: Option<i32>,
    /// Total number of turns in the parent conversation.
    pub total_turns: Option<i32>,
}

impl ScoredSnippetInput {
    /// Create a new snippet input with default metadata.
    pub fn new(kind: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            text: text.into(),
            role: None,
            turn_index: None,
            total_turns: None,
        }
    }
}

/// Extract candidate entities from text.
///
/// Matches capitalized words with length > 3 characters, camelCase identifiers,
/// and snake_case identifiers.
pub fn extract_entities(text: &str) -> Vec<String> {
    let mut entities = Vec::new();
    for raw_token in text.split_whitespace() {
        let token = raw_token.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if token.is_empty() {
            continue;
        }

        let is_snake = token.contains('_')
            && !token.starts_with('_')
            && !token.ends_with('_')
            && token.chars().all(|c| c.is_alphanumeric() || c == '_')
            && token.chars().any(|c| c.is_alphabetic());

        let is_camel = token.chars().next().is_some_and(|c| c.is_lowercase())
            && token.chars().any(|c| c.is_uppercase())
            && token.chars().all(|c| c.is_alphanumeric());

        let is_capitalized = token.chars().count() > 3
            && token.chars().next().is_some_and(|c| c.is_uppercase())
            && token.chars().all(|c| c.is_alphanumeric());

        if is_snake || is_camel || is_capitalized {
            let entity = token.to_string();
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
    }
    entities
}

/// Build a scoring profile from a project by extracting entities.
///
/// When an embedder is supplied, the project's text is embedded too, and the
/// result becomes the reference point for the S1 signal. The text embedded is
/// the brief if there is one, and otherwise the name -- the two together are
/// what the user typed to describe the project, so they are what the snippets
/// should be compared against.
///
/// Embedding failure is not fatal: the profile still works, and S1 falls back
/// to its neutral value. A model that would not load must not stop the scorer
/// from working at all.
pub fn build_profile(project: &Project, embedder: Option<&Embedder>) -> ProjectProfile {
    let mut entities = Vec::new();
    let mut add_entities_from = |text: &str| {
        for entity in extract_entities(text) {
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
    };

    add_entities_from(&project.name);
    if let Some(desc) = &project.description {
        add_entities_from(desc);
    }
    if let Some(brief) = &project.brief {
        add_entities_from(brief);
    }

    let embedding = embedder.and_then(|embedder| {
        let text = project
            .brief
            .as_deref()
            .filter(|brief| !brief.trim().is_empty())
            .unwrap_or(&project.name);
        match embedder.embed_text(text) {
            Ok(vector) => Some(vector),
            Err(cause) => {
                tracing::warn!("could not embed the project profile: {cause}");
                None
            }
        }
    });

    ProjectProfile {
        name: project.name.clone(),
        description: project.description.clone(),
        brief: project.brief.clone(),
        embedding,
        entities,
    }
}

/// Score a snippet against a project profile.
///
/// The snippet's embedding is optional. Without one -- and without a profile
/// embedding -- S1 falls back to its neutral value, so a machine with no model
/// installed scores the same way as one where the model simply found nothing
/// to say.
///
/// A probe floor is not a parameter here. Almost nothing has one, and the
/// floor is applied to the finished score rather than to any signal, so
/// [`score_with_floor`] is the entry point for a snippet the user has
/// answered a probe about.
pub fn score(
    profile: &ProjectProfile,
    snippet: &ScoredSnippetInput,
    snippet_embedding: Option<&[f32]>,
) -> ScoreDetails {
    score_with_floor(profile, snippet, snippet_embedding, None)
}

/// Score a snippet, and put a probe floor under the result.
///
/// The floor is applied to the final score, not to any signal. A floor is not
/// evidence: the seven signals still say what they said, and the floor is
/// reported beside them. Folding it into a signal would make the breakdown
/// lie about why the snippet ranked where it did.
///
/// The floor lifts a score and never lowers one: a snippet the heuristic
/// already loved keeps the score it had.
pub fn score_with_floor(
    profile: &ProjectProfile,
    snippet: &ScoredSnippetInput,
    snippet_embedding: Option<&[f32]>,
    probe_floor: Option<f32>,
) -> ScoreDetails {
    // S1: Semantic similarity
    let semantic = match (profile.embedding.as_deref(), snippet_embedding) {
        (Some(project), Some(snippet)) => cosine_similarity(project, snippet)
            .map(semantic_signal)
            .unwrap_or(SEMANTIC_FALLBACK),
        _ => SEMANTIC_FALLBACK,
    };

    // S2: Code
    let code = if snippet.kind.eq_ignore_ascii_case("code") {
        1.0
    } else {
        0.0
    };

    // S3: Markers
    let lower_text = snippet.text.to_lowercase();
    let marker_matches: usize = MARKERS
        .iter()
        .map(|marker| lower_text.matches(marker).count())
        .sum();
    let marker = (marker_matches as f32 * 0.25).min(1.0);

    // S4: Entities
    let entity = if profile.entities.is_empty() {
        0.0
    } else {
        let mut entity_matches = 0;
        for ent in &profile.entities {
            if lower_text.contains(&ent.to_lowercase()) {
                entity_matches += 1;
            }
        }
        (entity_matches as f32 * 0.5).min(1.0)
    };

    // S5: Novelty (stubbed to 0.5)
    let novelty = 0.5_f32;

    // S6: Position
    let position = match (snippet.turn_index, snippet.total_turns) {
        (Some(idx), Some(total)) if total > 0 && idx >= 0 => {
            if idx as f32 / total as f32 >= 0.7 {
                1.0
            } else if snippet
                .role
                .as_deref()
                .is_some_and(|r| r.eq_ignore_ascii_case("user"))
            {
                0.5
            } else {
                0.3
            }
        }
        _ => 0.5,
    };

    // S7: Provenance (stubbed to 0.5)
    let provenance = 0.5_f32;

    let composed = compose(
        semantic, code, marker, entity, novelty, position, provenance,
    );

    // The probe floor lifts the score but never lowers it: a snippet the
    // heuristic already loved stays as it was, and one the heuristic
    // dismissed is lifted to what the user said it was worth.
    let score = match probe_floor {
        Some(floor) => composed.max(floor),
        None => composed,
    };

    ScoreDetails {
        score,
        semantic,
        code,
        marker,
        entity,
        novelty,
        position,
        provenance,
        probe_floor,
    }
}

/// Compose seven signal values into the final score.
///
/// Separate from [`score`] so the mapping can be exercised at points the signal
/// functions cannot produce: S2 is binary, and S5 and S7 are stubbed at 0.5, so
/// no real snippet has every signal at 0.5. Testing only through `score` leaves
/// the middle of the curve unverified.
pub fn compose(
    semantic: f32,
    code: f32,
    marker: f32,
    entity: f32,
    novelty: f32,
    position: f32,
    provenance: f32,
) -> f32 {
    let raw = W_SEMANTIC * semantic
        + W_CODE * code
        + W_MARKER * marker
        + W_ENTITY * entity
        + W_NOVELTY * novelty
        + W_POSITION * position
        + W_PROVENANCE * provenance;

    // Normalised by what is actually reachable, not by an assumed 1.0. Leaving
    // it unscaled capped a perfect snippet at MAX_RAW (0.65), and the sigmoid
    // then mapped that to ~0.37 -- under the 0.70 auto-select threshold, so the
    // top third of the scale was dead and no amount of quality could reach it.
    let raw_normalized = (raw / MAX_RAW).clamp(0.0, 1.0);
    1.0 / (1.0 + (-(SCORE_SLOPE * raw_normalized - SCORE_OFFSET)).exp())
}

/// Convenience alias for [`score`].
pub fn score_snippet(
    profile: &ProjectProfile,
    snippet: &ScoredSnippetInput,
    snippet_embedding: Option<&[f32]>,
) -> ScoreDetails {
    score(profile, snippet, snippet_embedding)
}
