//! HS2-EWF0FT: the Rust CHOICE grammar matches the corpus the web client's parser also reads
//! (`clients/web/src/feedback-choices.test.ts`), so the two implementations cannot drift.

use std::collections::HashSet;

use hotsheet_model::feedback_choices::{
    parse_feedback_choices, selected_feedback_choices_markdown, selected_feedback_group_markdown,
};
use serde_json::{Value, json};

fn corpus() -> Vec<Value> {
    let text = include_str!("fixtures/feedback-choices.json");
    let corpus: Value = serde_json::from_str(text).expect("corpus is JSON");
    corpus["cases"].as_array().expect("cases array").clone()
}

fn string(value: &Value) -> &str {
    value.as_str().expect("string")
}

#[test]
fn hs2_ewf0ft_corpus_case_names_are_unique() {
    let cases = corpus();
    let names: HashSet<&str> = cases.iter().map(|case| string(&case["name"])).collect();
    assert_eq!(names.len(), cases.len());
    assert!(cases.len() >= 40, "the corpus keeps every regression case");
}

#[test]
fn hs2_ewf0ft_parser_matches_the_shared_corpus() {
    for case in corpus() {
        let name = string(&case["name"]);
        let source = string(&case["source"]);
        let parsed = parse_feedback_choices(source);
        if case["groups"].is_null() {
            assert!(
                parsed.is_none(),
                "{name}: expected no choice group, got {parsed:?}"
            );
            continue;
        }
        let document = parsed.unwrap_or_else(|| panic!("{name}: expected choice groups"));
        let actual: Vec<Value> = document
            .groups
            .iter()
            .map(|group| {
                json!({
                    "raw": &source[group.start..group.end],
                    "question": group.question,
                    "inlineQuestion": group.inline_question,
                    "choices": group.choices.iter().map(|choice| json!({
                        "label": choice.label,
                        "details": choice.details,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        assert_eq!(Value::Array(actual), case["groups"], "{name}: groups");
        for (index, group) in document.groups.iter().enumerate() {
            assert_eq!(group.index, index + 1, "{name}: group index");
        }
        let regions: Vec<Value> = document
            .regions
            .iter()
            .map(|region| Value::from(region.source.clone()))
            .collect();
        assert_eq!(Value::Array(regions), case["regions"], "{name}: regions");
        for region in &document.regions {
            assert!(
                source[region.source_start..].starts_with(&region.source),
                "{name}: region offset"
            );
        }
        for selection in case["selections"].as_array().into_iter().flatten() {
            let ids: Vec<&str> = selection["select"]
                .as_array()
                .expect("ids")
                .iter()
                .map(string)
                .collect();
            let markdown = selected_feedback_choices_markdown(source, &ids);
            assert_eq!(
                markdown,
                string(&selection["markdown"]),
                "{name}: selection {ids:?}"
            );
            // A recorded response never reads as a new prompt; the prompt re-parses identically.
            assert!(
                parse_feedback_choices(&markdown).is_none(),
                "{name}: response parses"
            );
            assert_eq!(
                parse_feedback_choices(source).as_ref(),
                Some(&document),
                "{name}: reparse"
            );
        }
    }
}

#[test]
fn hs2_ewf0ft_choice_ids_are_stable_per_group() {
    let document = parse_feedback_choices("CHOICE\n- A\n- B\n\nCHOICE\n\nA) one\nB) two\n")
        .expect("two groups");
    let ids: Vec<Vec<&str>> = document
        .groups
        .iter()
        .map(|group| {
            group
                .choices
                .iter()
                .map(|choice| choice.id.as_str())
                .collect()
        })
        .collect();
    assert_eq!(
        ids,
        [
            vec!["choice-1", "choice-2"],
            vec!["group-2-choice-1", "group-2-choice-2"]
        ]
    );
    assert_eq!(
        selected_feedback_group_markdown(&document.groups[1], &["group-2-choice-2"]),
        "Selected choice:\n- B) two"
    );
    assert_eq!(
        selected_feedback_group_markdown(&document.groups[0], &[]),
        ""
    );
    assert_eq!(
        selected_feedback_choices_markdown("No choices", &["choice-1"]),
        ""
    );
}

#[test]
fn hs2_ewf0ft_offsets_are_byte_offsets_for_non_ascii_text() {
    let source =
        "Welche Option? — wähle\n\n**Option A —** Größer\n\nMehr Text.\n\n**Option B —** Kleiner\n";
    let document = parse_feedback_choices(source).expect("lettered group");
    let group = &document.groups[0];
    assert!(source[group.start..].starts_with("**Option A —**"));
    assert_eq!(group.end, source.len());
    assert_eq!(group.choices[0].label, "Option A — Größer");
    assert_eq!(group.choices[0].details.as_deref(), Some("Mehr Text."));
    assert_eq!(group.question.as_deref(), Some("Welche Option? — wähle"));
}

proptest::proptest! {
    /// Arbitrary Markdown-ish lines with multi-byte characters never panic, and every group and
    /// region offset lands on the text it reports.
    #[test]
    fn hs2_ewf0ft_parser_offsets_hold_for_arbitrary_text(
        lines in proptest::collection::vec(
            "(CHOICE:? ?|\\*\\*|__|Option |[A-D][.):] |- |1\\. |  |\t|> |```|#{1,3} |—|é| |[a-z]{0,3}|\r){0,6}",
            0..12,
        )
    ) {
        let source = lines.join("\n");
        if let Some(document) = parse_feedback_choices(&source) {
            proptest::prop_assert_eq!(document.regions.len(), document.groups.len() + 1);
            for group in &document.groups {
                proptest::prop_assert!(group.start < group.end && group.end <= source.len());
                proptest::prop_assert!(!group.choices.is_empty());
            }
            for region in &document.regions {
                proptest::prop_assert!(source[region.source_start..].starts_with(&region.source));
            }
        }
    }
}
