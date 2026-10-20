//! Heuristic scoring engine (S2-S7) and project profile entity extraction.
//!
//! Evaluates candidate snippets using six heuristic signals:
//! - S2 (Code): Kind is code vs text.
//! - S3 (Markers): Decision and action markers.
//! - S4 (Entities): Project proper nouns and identifiers.
//! - S5 (Novelty): Stubbed to 0.5 until embedding support lands.
//! - S6 (Position): Position within conversation turns.
//! - S7 (Provenance): Source trust, default 0.5.

use serde::{Deserialize, Serialize};

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

/// The highest raw score reachable while S1 (semantic similarity, weight 0.35)
/// is unimplemented.
///
/// 0.65 today; it becomes 1.0 when S1 lands, and the division below then
/// becomes a no-op rather than needing an edit.
const MAX_RAW: f32 = W_CODE + W_MARKER + W_ENTITY + W_NOVELTY + W_POSITION + W_PROVENANCE;

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
pub fn build_profile(project: &Project) -> ProjectProfile {
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

    ProjectProfile {
        name: project.name.clone(),
        description: project.description.clone(),
        brief: project.brief.clone(),
        entities,
    }
}

/// Score a snippet against a project profile.
pub fn score(profile: &ProjectProfile, snippet: &ScoredSnippetInput) -> ScoreDetails {
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

    ScoreDetails {
        score: compose(code, marker, entity, novelty, position, provenance),
        code,
        marker,
        entity,
        novelty,
        position,
        provenance,
    }
}

/// Compose six signal values into the final score.
///
/// Separate from [`score`] so the mapping can be exercised at points the signal
/// functions cannot produce: S2 is binary, and S5 and S7 are stubbed at 0.5, so
/// no real snippet has every signal at 0.5. Testing only through `score` leaves
/// the middle of the curve unverified.
pub fn compose(
    code: f32,
    marker: f32,
    entity: f32,
    novelty: f32,
    position: f32,
    provenance: f32,
) -> f32 {
    let raw = W_CODE * code
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
pub fn score_snippet(profile: &ProjectProfile, snippet: &ScoredSnippetInput) -> ScoreDetails {
    score(profile, snippet)
}
