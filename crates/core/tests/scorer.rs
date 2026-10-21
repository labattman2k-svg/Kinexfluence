//! Unit tests for heuristic scoring (S2-S7) and entity extraction.

use chrono::Utc;
use kfx_core::project::Project;
use kfx_core::scorer::{
    build_profile, compose, extract_entities, score, ProjectProfile, ScoredSnippetInput,
};

fn blank_profile() -> ProjectProfile {
    ProjectProfile {
        name: "Test Project".to_string(),
        description: None,
        brief: None,
        embedding: None,
        entities: Vec::new(),
    }
}

fn dummy_project(name: &str, description: Option<&str>, brief: Option<&str>) -> Project {
    let now = Utc::now();
    Project {
        id: "proj-1".to_string(),
        name: name.to_string(),
        slug: "test-proj".to_string(),
        description: description.map(ToString::to_string),
        brief: brief.map(ToString::to_string),
        status: "recon".to_string(),
        weights_json: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn code_scores_high() {
    let profile = blank_profile();

    let code_input = ScoredSnippetInput::new("code", "fn add(a: i32, b: i32) -> i32 { a + b }");
    let text_input = ScoredSnippetInput::new("text", "fn add(a: i32, b: i32) -> i32 { a + b }");

    let code_score = score(&profile, &code_input, None);
    let text_score = score(&profile, &text_input, None);

    assert_eq!(code_score.code, 1.0);
    assert_eq!(text_score.code, 0.0);
    assert!(
        code_score.score > text_score.score,
        "code snippet ({}) should score higher than identical text snippet ({})",
        code_score.score,
        text_score.score
    );
}

#[test]
fn markers_score_correctly() {
    let profile = blank_profile();

    // 0 markers
    let input_none = ScoredSnippetInput::new("text", "just some general text without keywords");
    assert_eq!(score(&profile, &input_none, None).marker, 0.0);

    // 1 marker (0.25)
    let input_one = ScoredSnippetInput::new("text", "this step is required for testing");
    assert_eq!(score(&profile, &input_one, None).marker, 0.25);

    // 2 markers (0.50), case-insensitive
    let input_two = ScoredSnippetInput::new("text", "WE DECIDED that THE FIX IS ready");
    assert_eq!(score(&profile, &input_two, None).marker, 0.50);

    // 4 markers (1.00)
    let input_four = ScoredSnippetInput::new(
        "text",
        "we decided the fix is found and root cause is required to investigate",
    );
    assert_eq!(score(&profile, &input_four, None).marker, 1.0);

    // 5 markers (capped at 1.00)
    let input_five = ScoredSnippetInput::new(
        "text",
        "we decided must cannot the fix is required for the action item",
    );
    assert_eq!(score(&profile, &input_five, None).marker, 1.0);
}

#[test]
fn entities_match() {
    let project = dummy_project(
        "Atlas Navigator",
        Some("Fast search index with turn_index and snippetId identifiers"),
        Some("Includes PostgreSQL storage"),
    );
    let profile = build_profile(&project, None);

    // Verify entity extraction
    let extracted = extract_entities("Atlas Navigator with turn_index and snippetId");
    assert!(extracted.contains(&"Atlas".to_string()));
    assert!(extracted.contains(&"Navigator".to_string()));
    assert!(extracted.contains(&"turn_index".to_string()));
    assert!(extracted.contains(&"snippetId".to_string()));

    // 0 entities matched
    let input_zero = ScoredSnippetInput::new("text", "completely unrelated text snippet");
    assert_eq!(score(&profile, &input_zero, None).entity, 0.0);

    // 1 entity matched (0.50), case-insensitive
    let input_one = ScoredSnippetInput::new("text", "connecting to postgresql now");
    assert_eq!(score(&profile, &input_one, None).entity, 0.50);

    // 2 entities matched (1.00)
    let input_two = ScoredSnippetInput::new("text", "using atlas with turn_index");
    assert_eq!(score(&profile, &input_two, None).entity, 1.0);

    // 3 entities matched (capped at 1.00)
    let input_three = ScoredSnippetInput::new("text", "atlas navigator contains snippetId");
    assert_eq!(score(&profile, &input_three, None).entity, 1.0);
}

#[test]
fn position_scores_correctly_based_on_turns() {
    let profile = blank_profile();

    // Turn 8 out of 10 (8/10 = 0.8 >= 0.7: last 30% -> 1.0)
    let input_late = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "hello".to_string(),
        role: Some("assistant".to_string()),
        turn_index: Some(8),
        total_turns: Some(10),
    };
    assert_eq!(score(&profile, &input_late, None).position, 1.0);

    // Turn 2 out of 10 (2/10 = 0.2 < 0.7: user role -> 0.5)
    let input_user = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "hello".to_string(),
        role: Some("user".to_string()),
        turn_index: Some(2),
        total_turns: Some(10),
    };
    assert_eq!(score(&profile, &input_user, None).position, 0.5);

    // Turn 2 out of 10 (2/10 = 0.2 < 0.7: assistant role -> 0.3)
    let input_other = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "hello".to_string(),
        role: Some("assistant".to_string()),
        turn_index: Some(2),
        total_turns: Some(10),
    };
    assert_eq!(score(&profile, &input_other, None).position, 0.3);
}

#[test]
fn sigmoid_bounds() {
    let profile = blank_profile();

    // Minimum possible signals
    let min_input = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "plain words".to_string(),
        role: Some("assistant".to_string()),
        turn_index: Some(0),
        total_turns: Some(10),
    };
    let min_score = score(&profile, &min_input, None);
    assert!(
        min_score.score > 0.0 && min_score.score < 1.0,
        "score {} must be strictly in (0, 1)",
        min_score.score
    );

    // Maximum possible signals
    let max_profile = ProjectProfile {
        name: "Test".to_string(),
        description: None,
        brief: None,
        embedding: None,
        entities: vec!["alpha".to_string(), "beta".to_string()],
    };
    let max_input = ScoredSnippetInput {
        kind: "code".to_string(),
        text: "we decided the fix is required for root cause in alpha beta".to_string(),
        role: Some("assistant".to_string()),
        turn_index: Some(9),
        total_turns: Some(10),
    };
    let max_score = score(&max_profile, &max_input, None);
    assert!(
        max_score.score > 0.0 && max_score.score < 1.0,
        "score {} must be strictly in (0, 1)",
        max_score.score
    );

    // Monotonicity: max signals strictly higher than min signals
    assert!(max_score.score > min_score.score);
}

#[test]
fn missing_metadata_defaults_to_half() {
    let profile = blank_profile();

    // Both missing
    let input_both_none = ScoredSnippetInput::new("text", "sample");
    assert_eq!(score(&profile, &input_both_none, None).position, 0.5);

    // Only turn_index missing
    let input_no_idx = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "sample".to_string(),
        role: Some("user".to_string()),
        turn_index: None,
        total_turns: Some(10),
    };
    assert_eq!(score(&profile, &input_no_idx, None).position, 0.5);

    // Only total_turns missing
    let input_no_total = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "sample".to_string(),
        role: Some("user".to_string()),
        turn_index: Some(5),
        total_turns: None,
    };
    assert_eq!(score(&profile, &input_no_total, None).position, 0.5);

    // Zero total_turns
    let input_zero_total = ScoredSnippetInput {
        kind: "text".to_string(),
        text: "sample".to_string(),
        role: Some("user".to_string()),
        turn_index: Some(0),
        total_turns: Some(0),
    };
    assert_eq!(score(&profile, &input_zero_total, None).position, 0.5);
}

// ---------------------------------------------------------------------------
// The composition curve
//
// The bounds test above only asserts that a score lands in (0, 1) and that a
// better snippet outscores a worse one. Both are satisfied by a curve that
// never leaves the bottom third, which is exactly what the first version of
// this mapping did: the weights sum to 0.65 while S1 is unimplemented, so an
// unscaled raw score topped out at 0.65 and `sigmoid(raw - 1.2)` turned that
// into ~0.37. Every snippet was grey and the 0.70 auto-select threshold was
// unreachable by any input.
//
// These tests pin the shape of the curve, not just its range.
// ---------------------------------------------------------------------------

/// The score a perfect snippet would get if every signal, including the two
/// stubbed ones, could reach 1.0.
#[test]
fn perfect_signals_score_above_the_auto_select_threshold() {
    let perfect = compose(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);

    assert!(
        perfect > 0.85,
        "a perfect snippet must be visibly excellent, got {perfect}"
    );
    assert!(
        perfect > 0.70,
        "and must clear the auto-select threshold, got {perfect}"
    );
}

/// The best score a *real* snippet can reach today: S2 is binary, and S5/S7 are
/// stubbed at 0.5, so novelty and provenance cannot both be 1.0 yet.
#[test]
fn best_realisable_snippet_clears_the_auto_select_threshold() {
    // With S1 live, the best realisable snippet needs a matching embedding:
    // S1 carries the heaviest weight of the seven, so a snippet that means
    // what the project means is the one that reaches the top of the scale.
    let profile = ProjectProfile {
        name: "Test".to_string(),
        description: None,
        brief: None,
        embedding: Some(vec![1.0, 0.0, 0.0]),
        entities: vec!["alpha".to_string(), "beta".to_string()],
    };
    let input = ScoredSnippetInput {
        kind: "code".to_string(),
        text: "we decided the fix is required for root cause in alpha beta".to_string(),
        role: Some("assistant".to_string()),
        turn_index: Some(9),
        total_turns: Some(10),
    };

    let scored = score(&profile, &input, Some(&[1.0, 0.0, 0.0]));

    assert!(scored.code == 1.0 && scored.marker == 1.0 && scored.entity == 1.0);
    assert_eq!(scored.position, 1.0);

    assert!(
        scored.score > 0.80,
        "the best realisable snippet should score above 0.80, got {}",
        scored.score
    );
    assert!(
        scored.score > 0.70,
        "and must clear the 0.70 auto-select threshold, got {}",
        scored.score
    );
}

/// A middling snippet sits near a third. This is the point a user reads as
/// "maybe", so it must not drift into either "yes" or "no".
///
/// Driven through `compose` rather than `score`: `code` is binary and two
/// signals are stubbed, so no real input has all six at 0.5.
#[test]
fn neutral_signals_score_around_a_third() {
    let neutral = compose(0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5);

    assert!(
        (neutral - 0.35).abs() < 0.05,
        "a neutral snippet should land near 0.35, got {neutral}"
    );
}

/// The floor stays near zero, so a poor snippet is clearly poor rather than
/// being lifted by the offset.
#[test]
fn worst_signals_score_near_zero() {
    let worst = compose(0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.5);

    assert!(
        worst < 0.10,
        "a snippet hitting nothing should score near zero, got {worst}"
    );
}

/// The curve is monotonic across the whole range, so a better snippet can never
/// score lower than a worse one.
#[test]
fn composition_is_monotonic() {
    let mut previous = -1.0;

    for step in 0..=10 {
        let level = step as f32 / 10.0;
        let scored = compose(level, level, level, level, level, level, level);
        assert!(
            scored > previous,
            "score must increase with every signal at {level}: {scored} vs {previous}"
        );
        previous = scored;
    }
}
