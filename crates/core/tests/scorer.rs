//! Unit tests for heuristic scoring (S1-S7) and entity extraction.

use chrono::Utc;
use kfx_core::project::Project;
use kfx_core::scorer::{
    build_profile, compose, extract_entities, score, score_with_floor, ProjectProfile,
    ScoredSnippetInput,
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

/// A profile with the given embedding and the given entities.
fn profile_with(embedding: Option<Vec<f32>>, entities: Vec<String>) -> ProjectProfile {
    ProjectProfile {
        name: "Test".to_string(),
        description: None,
        brief: None,
        embedding,
        entities,
    }
}

/// A snippet that matches every signal it can: it is code, it carries the
/// decision markers, it names the project's entities, and it sits at the end
/// of the conversation.
fn best_input() -> ScoredSnippetInput {
    ScoredSnippetInput {
        kind: "code".to_string(),
        text: "we decided the fix is required for root cause in alpha beta".to_string(),
        role: Some("assistant".to_string()),
        turn_index: Some(9),
        total_turns: Some(10),
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
    let max_profile = profile_with(None, vec!["alpha".to_string(), "beta".to_string()]);
    let max_input = best_input();
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
    let profile = profile_with(
        Some(vec![1.0, 0.0, 0.0]),
        vec!["alpha".to_string(), "beta".to_string()],
    );
    let input = best_input();

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

/// Without the model, the best a snippet can do is just short of the
/// threshold.
///
/// S1 carries 0.35 of the 1.00 total, so a machine with no model leaves that
/// share at the neutral 0.5 and tops out at 0.696 -- just under the 0.70
/// auto-select threshold, where before S1 the same snippet scored 0.832.
///
/// The effect is intended: a semantic signal is what separates a snippet
/// that is about the project from one that merely shares its vocabulary, and
/// a machine that cannot measure that should not claim its best snippets are
/// auto-selectable. The test pins the number so a change to the weights or
/// to the fallback cannot move it unnoticed.
#[test]
fn without_the_model_the_best_snippet_falls_just_short_of_the_threshold() {
    let profile = profile_with(None, vec!["alpha".to_string(), "beta".to_string()]);
    let input = best_input();

    let scored = score(&profile, &input, None);

    assert_eq!(scored.semantic, 0.5, "a missing embedding stays neutral");
    assert!(
        scored.score < 0.70,
        "without the model the best snippet stays under the threshold, got {}",
        scored.score
    );
    assert!(
        scored.score > 0.65,
        "but it stays close to it, so the difference is the model's and not a \
         collapse of the scale, got {}",
        scored.score
    );
}

/// A middling snippet sits near a third. This is the point a user reads as
/// "maybe", so it must not drift into either "yes" or "no".
///
/// Driven through `compose` rather than `score`: `code` is binary and two
/// signals are stubbed, so no real input has all seven at 0.5.
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

// ---------------------------------------------------------------------------
// S1: the semantic signal
//
// S1 is the heaviest weight in the scorer, so a change to how it is computed
// moves every score in the product. These tests pin the three things that
// matter: that it is in the composition at all, that a missing embedding
// leaves it neutral, and that a more similar snippet scores higher.
// ---------------------------------------------------------------------------

#[test]
fn a_missing_embedding_leaves_s1_neutral() {
    let profile = blank_profile();
    let input = ScoredSnippetInput::new("text", "anything at all");

    // Neither side embedded.
    assert_eq!(score(&profile, &input, None).semantic, 0.5);

    // The profile embedded, the snippet did not.
    let embedded_profile = profile_with(Some(vec![1.0, 0.0, 0.0]), Vec::new());
    assert_eq!(score(&embedded_profile, &input, None).semantic, 0.5);

    // The snippet embedded, the profile did not.
    assert_eq!(
        score(&profile, &input, Some(&[1.0, 0.0, 0.0])).semantic,
        0.5
    );
}

#[test]
fn identical_embeddings_give_a_full_s1() {
    let vector = vec![0.6, 0.8, 0.0];
    let profile = profile_with(Some(vector.clone()), Vec::new());
    let input = ScoredSnippetInput::new("text", "anything at all");

    let scored = score(&profile, &input, Some(&vector));

    assert!(
        (scored.semantic - 1.0).abs() < 1e-5,
        "a snippet that embeds to the same vector is maximally similar, got {}",
        scored.semantic
    );
}

#[test]
fn a_more_similar_snippet_scores_higher() {
    // Two unit vectors at 60 degrees to each other: the first is closer to the
    // profile than the second.
    let profile_vector = vec![1.0, 0.0, 0.0];
    let closer = vec![0.9, 0.436, 0.0];
    let further = vec![0.5, 0.866, 0.0];

    let profile = profile_with(Some(profile_vector), Vec::new());
    let input = ScoredSnippetInput::new("text", "anything at all");

    let near = score(&profile, &input, Some(&closer));
    let far = score(&profile, &input, Some(&further));

    assert!(
        near.semantic > far.semantic,
        "the closer snippet should score higher: {} against {}",
        near.semantic,
        far.semantic
    );
    assert!(
        near.score > far.score,
        "and the difference should reach the final score: {} against {}",
        near.score,
        far.score
    );
}

#[test]
fn s1_is_in_the_composition() {
    // The same snippet, once against a profile it matches and once against
    // one it does not. Everything else is held equal, so any difference in
    // the final score is S1's doing.
    let input = ScoredSnippetInput::new("text", "a snippet about databases");
    let profile = profile_with(Some(vec![1.0, 0.0, 0.0]), Vec::new());

    let matching = score(&profile, &input, Some(&[1.0, 0.0, 0.0]));
    let unrelated = score(&profile, &input, Some(&[0.0, 0.0, 1.0]));

    assert!(
        matching.score > unrelated.score,
        "a snippet that means what the project means must score higher: \
         {} against {}",
        matching.score,
        unrelated.score
    );
}

#[test]
fn s1_stays_inside_its_range() {
    // A cosine of -1 means the two texts are opposites. The signal is
    // clamped, so it reaches 0 rather than going negative and dragging the
    // weighted sum below its floor.
    let profile = profile_with(Some(vec![1.0, 0.0]), Vec::new());
    let input = ScoredSnippetInput::new("text", "anything at all");

    let opposed = score(&profile, &input, Some(&[-1.0, 0.0]));

    assert!(
        (0.0..=1.0).contains(&opposed.semantic),
        "S1 must stay in [0, 1], got {}",
        opposed.semantic
    );
    assert!(
        (0.0..=1.0).contains(&opposed.score),
        "and the score must stay in [0, 1], got {}",
        opposed.score
    );
}

// ---------------------------------------------------------------------------
// The probe floor
//
// A probe is the user telling the system that a snippet matters and the
// heuristic was wrong. The floor is what makes that mean something.
//
// The floor is applied to the final score, not to any signal, and the
// signals are left alone. That is the point of the whole mechanism: the
// breakdown has to keep saying what the heuristic said, so a reader can see
// that the ranking was overruled by the user and not by the scoring.
// ---------------------------------------------------------------------------

#[test]
fn a_probe_floor_lifts_a_low_score() {
    let profile = profile_with(None, Vec::new());
    let input = ScoredSnippetInput::new("text", "plain words with nothing in them");

    let unfloored = score(&profile, &input, None);
    let floored = score_with_floor(&profile, &input, None, Some(0.90));

    assert!(
        floored.score >= 0.90,
        "a settled probe should put the snippet at 0.90 or above, got {}",
        floored.score
    );
    assert!(
        floored.score > unfloored.score,
        "the floor should have lifted the score: {} against {}",
        floored.score,
        unfloored.score
    );
    assert_eq!(floored.probe_floor, Some(0.90));
}

#[test]
fn a_probe_floor_never_lowers_a_high_score() {
    // A snippet the heuristic already loved does not need the floor, and
    // must not be dragged down to it.
    let profile = profile_with(
        Some(vec![1.0, 0.0, 0.0]),
        vec!["alpha".to_string(), "beta".to_string()],
    );
    let input = best_input();

    let unfloored = score(&profile, &input, Some(&[1.0, 0.0, 0.0]));
    let floored = score_with_floor(&profile, &input, Some(&[1.0, 0.0, 0.0]), Some(0.90));

    assert!(
        floored.score >= unfloored.score,
        "the floor should not lower a score the heuristic already gave: \
         {} against {}",
        floored.score,
        unfloored.score
    );
}

#[test]
fn a_probe_floor_leaves_the_signals_alone() {
    // The signals are the heuristic's testimony. A floor is not evidence, so
    // it must not be folded into one: the breakdown has to keep saying what
    // the scorer said, and report the floor beside it.
    let profile = profile_with(None, Vec::new());
    let input = ScoredSnippetInput::new("text", "plain words with nothing in them");

    let unfloored = score(&profile, &input, None);
    let floored = score_with_floor(&profile, &input, None, Some(0.90));

    assert_eq!(floored.semantic, unfloored.semantic);
    assert_eq!(floored.code, unfloored.code);
    assert_eq!(floored.marker, unfloored.marker);
    assert_eq!(floored.entity, unfloored.entity);
    assert_eq!(floored.novelty, unfloored.novelty);
    assert_eq!(floored.position, unfloored.position);
    assert_eq!(floored.provenance, unfloored.provenance);
}

#[test]
fn a_snippet_with_no_probe_has_no_floor() {
    let profile = profile_with(None, Vec::new());
    let input = ScoredSnippetInput::new("text", "anything at all");

    let scored = score(&profile, &input, None);

    assert_eq!(
        scored.probe_floor, None,
        "the ordinary case is a snippet nobody has probed"
    );
}

#[test]
fn a_probe_floor_keeps_the_score_in_range() {
    let profile = profile_with(None, Vec::new());
    let input = ScoredSnippetInput::new("text", "anything at all");

    // A floor above 1.0 would be nonsense, and a floor below 0 would be
    // worse. The floor is a bound on the score, not a replacement for it.
    let floored = score_with_floor(&profile, &input, None, Some(1.0));

    assert!(
        (0.0..=1.0).contains(&floored.score),
        "the score must stay in [0, 1], got {}",
        floored.score
    );
}
