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

    // Composition
    let raw = 0.20 * code
        + 0.15 * marker
        + 0.15 * entity
        + 0.05 * novelty
        + 0.05 * position
        + 0.05 * provenance;

    let final_score = 1.0 / (1.0 + (-(raw - 1.2)).exp());

    ScoreDetails {
        score: final_score,
        code,
        marker,
        entity,
        novelty,
        position,
        provenance,
    }
}

/// Convenience alias for [`score`].
pub fn score_snippet(profile: &ProjectProfile, snippet: &ScoredSnippetInput) -> ScoreDetails {
    score(profile, snippet)
}
