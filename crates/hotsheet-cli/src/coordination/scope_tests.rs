//! HS2-YHYNJ6 scope grammar, overlap, keyed sections, handoff review, and reviewed-sharing
//! reservation transitions.

use super::scope::{
    ChangedPath, heading_slug, normalized_scope, normalized_shared, review_handoff, scopes_overlap,
    sections, touched_sections,
};
use super::tests::{NOW, SHA, assignment, fixture, register};
use super::*;

const COVERAGE: &str = "docs/TEST-COVERAGE.md";
const DOC: &str = "\
# Coverage

Intro.

| Feature | Status |
| --- | --- |
| alpha-row | covered |
| beta-row | planned |

### Alpha notes (HS2-AAAAAA)

- alpha detail

### Beta notes (HS2-BBBBBB)

- beta detail
";

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn hs2_yhynj6_section_scopes_are_typed_and_whole_paths_still_normalize() {
    assert_eq!(
        normalized_scope(&strings(&[
            "./docs//TEST-COVERAGE.md#row:alpha-row",
            "docs/TEST-COVERAGE.md#heading:alpha-notes-hs2-aaaaaa",
            "src\\a.rs",
            "src/a.rs",
        ]))
        .unwrap(),
        [
            "docs/TEST-COVERAGE.md#heading:alpha-notes-hs2-aaaaaa",
            "docs/TEST-COVERAGE.md#row:alpha-row",
            "src/a.rs",
        ]
    );
    // A free-form fragment previously escaped the whole-file conflict; it is now refused.
    for bad in [
        "docs/X.md#a",
        "docs/X.md#row:",
        "docs/X.md#row:Upper",
        "docs/X.md#line:4",
        "docs/X.md#row:a b",
        "docs/X.md#row:-lead",
        "/abs.md#row:a",
        "../x.md#row:a",
    ] {
        assert!(normalized_scope(&strings(&[bad])).is_err(), "{bad}");
    }
    assert_eq!(
        normalized_shared(&strings(&["docs//A.md", "docs/A.md"])).unwrap(),
        ["docs/A.md"]
    );
    assert!(normalized_shared(&strings(&["docs/A.md#row:a"])).is_err());
}

#[test]
fn hs2_yhynj6_sections_overlap_only_on_the_same_key_or_a_whole_path() {
    let row_a = "docs/T.md#row:a";
    for (left, right, overlap) in [
        (row_a, "docs/T.md#row:a", true),
        (row_a, "docs/T.md#row:b", false),
        (row_a, "docs/T.md#heading:a", false),
        (row_a, "docs/T.md", true),
        ("docs/T.md", row_a, true),
        (row_a, "docs", true),
        ("docs", row_a, true),
        (row_a, "docs/T.mdx#row:a", false),
        (row_a, "docs/U.md#row:a", false),
        ("docs/T.md", "docs/T.md", true),
        ("src/a.rs", "src/b.rs", false),
    ] {
        assert_eq!(scopes_overlap(left, right), overlap, "{left} vs {right}");
    }
}

#[test]
fn hs2_yhynj6_documents_split_into_keyed_rows_and_headings() {
    assert_eq!(
        heading_slug("Typed write-behind-unavailable refusal (HS2-DEVEYE)"),
        "typed-write-behind-unavailable-refusal-hs2-deveye"
    );
    let parsed = sections(DOC);
    assert!(parsed.ambiguous.is_empty());
    assert_eq!(
        parsed
            .content
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "heading:alpha-notes-hs2-aaaaaa",
            "heading:beta-notes-hs2-bbbbbb",
            "heading:coverage",
            "row:alpha-row",
            "row:beta-row",
        ]
    );
    assert_eq!(parsed.content["row:alpha-row"], "| alpha-row | covered |\n");
    // Headings and rows inside fenced code are content, not new sections.
    let fenced = sections("# Map\n```\n# not-a-heading\n| not-a-row | x |\n```\n");
    assert_eq!(fenced.content.len(), 1);
    assert!(fenced.content["heading:map"].contains("# not-a-heading"));
    let duplicate = sections("## Result\nA\n## Result\nB\n| r | 1 |\n| r | 2 |\n");
    assert_eq!(
        duplicate.ambiguous.into_iter().collect::<Vec<_>>(),
        ["heading:result", "row:r"]
    );
    assert!(
        sections("Lead.\n# Title\n")
            .content
            .contains_key("preamble")
    );
}

#[test]
fn hs2_yhynj6_touched_sections_name_exactly_the_changed_keys() {
    let touched = |after: &str| touched_sections(DOC, after);
    let set = |keys: &[&str]| Some(keys.iter().map(|k| (*k).to_owned()).collect());
    // A new row anywhere in the table, an edited row, and a new heading block.
    assert_eq!(
        touched(&DOC.replace("| beta-row |", "| gamma-row | planned |\n| beta-row |")),
        set(&["row:gamma-row"])
    );
    assert_eq!(
        touched(&DOC.replace("| alpha-row | covered |", "| alpha-row | double |")),
        set(&["row:alpha-row"])
    );
    let added = DOC.replace(
        "### Beta notes",
        "### Gamma notes (HS2-CCCCCC)\n\n- gamma detail\n\n### Beta notes",
    );
    assert_eq!(touched(&added), set(&["heading:gamma-notes-hs2-cccccc"]));
    assert_eq!(touched(DOC), set(&[]));
    // Text outside every keyed section and duplicated keys are whole-file edits.
    assert_eq!(touched(&format!("Lead.\n{DOC}")), None);
    assert_eq!(
        touched(&DOC.replace("| beta-row | planned |", "| alpha-row | planned |")),
        None
    );
    assert_eq!(
        touched_sections(
            "## Result\nA\n## Result\nB\n",
            "## Result\nA\n## Result\nC\n"
        ),
        None
    );
    // Prose under a heading belongs to that heading's section.
    assert_eq!(
        touched(&DOC.replace("Intro.", "Intro, edited.")),
        set(&["heading:coverage"])
    );
}

#[test]
fn hs2_yhynj6_adjacent_inserts_at_one_anchor_stay_independent_sections() {
    // The measured DEVEYE/VBFSZJ shape: two handoffs insert different headings at one line.
    let first = DOC.replace(
        "### Beta notes",
        "### First (HS2-DEVEYE)\n\n- one\n\n### Beta notes",
    );
    let second = DOC.replace(
        "### Beta notes",
        "### Second (HS2-VBFSZJ)\n\n- two\n\n### Beta notes",
    );
    let both = DOC.replace(
        "### Beta notes",
        "### First (HS2-DEVEYE)\n\n- one\n\n### Second (HS2-VBFSZJ)\n\n- two\n\n### Beta notes",
    );
    let first_keys = touched_sections(DOC, &first).unwrap();
    let second_keys = touched_sections(DOC, &second).unwrap();
    assert!(first_keys.is_disjoint(&second_keys));
    // The integrator's merged result touches exactly their union, so it still verifies.
    assert_eq!(
        touched_sections(DOC, &both).unwrap(),
        first_keys.union(&second_keys).cloned().collect()
    );
}

fn changed(path: &str, before: Option<&str>, after: Option<&str>) -> ChangedPath {
    ChangedPath {
        path: path.into(),
        before: before.map(str::to_owned),
        after: after.map(str::to_owned),
    }
}

#[test]
fn hs2_yhynj6_handoff_review_refuses_every_undeclared_change() {
    let row = DOC.replace("| alpha-row | covered |", "| alpha-row | double |");
    let scope = strings(&["src", &format!("{COVERAGE}#row:alpha-row")]);
    review_handoff(
        &scope,
        &[
            changed("src/feature.rs", None, None),
            changed("src/nested/new.rs", None, None),
            changed(COVERAGE, Some(DOC), Some(&row)),
        ],
    )
    .unwrap();
    let refused = |changes: &[ChangedPath], reason: &str| {
        let message = review_handoff(&scope, changes).unwrap_err().to_string();
        assert!(message.contains(reason), "{message}");
        assert!(message.contains("outside the declared scope"), "{message}");
    };
    refused(&[changed("lib/other.rs", None, None)], "path not declared");
    refused(&[changed("srcx/a.rs", None, None)], "path not declared");
    refused(
        &[changed(
            COVERAGE,
            Some(DOC),
            Some(&DOC.replace("| beta-row | planned |", "| beta-row | done |")),
        )],
        "undeclared sections row:beta-row",
    );
    refused(
        &[changed(COVERAGE, Some(DOC), Some(&format!("Lead.\n{DOC}")))],
        "edit outside keyed sections",
    );
    refused(
        &[changed(COVERAGE, None, Some(DOC))],
        "cannot be added or removed",
    );
    refused(
        &[changed(COVERAGE, Some(DOC), None)],
        "cannot be added or removed",
    );
    // A whole-path entry covers any edit to that document.
    review_handoff(
        &strings(&[COVERAGE]),
        &[changed(COVERAGE, Some(DOC), Some("rewritten\n"))],
    )
    .unwrap();
}

const OTHER_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn scoped(id: &str, worker: &str, native: &str, scope: &[&str]) -> Assignment {
    let mut a = assignment(id, worker, native);
    a.generation = format!("{worker}-generation");
    a.scope = strings(scope);
    a
}

fn worker(c: &Coordinator, id: &str) {
    register(c, Role::Worker, id, &format!("{id}-generation"), NOW);
}

fn shared_pool(shared: &[&str]) -> (tempfile::TempDir, Coordinator) {
    let (root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-1", NOW);
    c.publish(
        "root",
        "root-1",
        &|| NOW,
        SHA,
        Vec::new(),
        &[],
        &strings(shared),
        true,
    )
    .unwrap();
    (root, c)
}

fn raw(c: &Coordinator) -> Vec<u8> {
    fs::read(c.directory.join("state.json")).unwrap()
}

fn assign(c: &Coordinator, a: Assignment) -> Result<Assignment> {
    c.assign("dispatcher", "dispatch-1", NOW, a)
}

/// Every refusal leaves the durable state byte-identical.
fn refused(c: &Coordinator, a: Assignment, reason: &str) {
    let before = raw(c);
    let message = assign(c, a).unwrap_err().to_string();
    assert!(message.contains(reason), "{message}");
    assert_eq!(raw(c), before, "refusal changed state: {reason}");
}

fn handoff(c: &Coordinator, id: &str, worker: &str) {
    let generation = format!("{worker}-generation");
    c.worker_transition(worker, &generation, id, Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        worker,
        &generation,
        id,
        Phase::Handoff,
        Some("c".repeat(40)),
    )
    .unwrap();
}

#[test]
fn hs2_yhynj6_reviewed_shared_sections_refill_while_earlier_work_integrates() {
    let (_root, c) = shared_pool(&[COVERAGE]);
    for id in ["one", "two", "three"] {
        worker(&c, id);
    }
    let section = |key: &str| format!("{COVERAGE}#{key}");
    assign(
        &c,
        scoped("op-1", "one", "1", &["crates/a", &section("row:a")]),
    )
    .unwrap();
    handoff(&c, "op-1", "one");
    c.mark("root", "root-1", NOW, "op-1", Phase::Integrating)
        .unwrap();
    // Two more workers truthfully declare the same shared document while op-1 integrates.
    assign(
        &c,
        scoped("op-2", "two", "2", &["crates/b", &section("row:b")]),
    )
    .unwrap();
    c.mark("root", "root-1", NOW, "op-1", Phase::Ci).unwrap();
    assign(
        &c,
        scoped(
            "op-3",
            "three",
            "3",
            &["crates/c", &section("heading:c-notes")],
        ),
    )
    .unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.version, 5);
    let pending = state
        .assignments
        .values()
        .filter(|a| a.phase.pending())
        .count();
    assert_eq!(pending, 3, "the whole-path rule allowed exactly one");
}

#[test]
fn hs2_yhynj6_conflicting_or_unapproved_sharing_is_refused_with_unchanged_state() {
    let (_root, c) = shared_pool(&[COVERAGE]);
    for id in ["one", "two"] {
        worker(&c, id);
    }
    let section = |key: &str| format!("{COVERAGE}#{key}");
    assign(
        &c,
        scoped("op-1", "one", "1", &["crates/a", &section("row:a")]),
    )
    .unwrap();
    // Same section, whole shared file, containing directory, and conflicting code.
    for scope in [
        section("row:a"),
        COVERAGE.to_owned(),
        "docs".to_owned(),
        "crates/a/src/lib.rs".to_owned(),
    ] {
        refused(
            &c,
            scoped("op-2", "two", "2", &["crates/b", &scope]),
            "overlaps",
        );
    }
    // A section of a document the integrator never reviewed as shared.
    refused(
        &c,
        scoped(
            "op-2",
            "two",
            "2",
            &["crates/b", "docs/CODEBASE-MAP.md#heading:map"],
        ),
        "not published as a reviewed shared document",
    );
    // A replay that changes the scope is a different operation.
    refused(
        &c,
        scoped("op-1", "one", "1", &["crates/a", &section("row:z")]),
        "different assignment",
    );
    // A stale dispatcher generation cannot reserve a reviewed section.
    let before = raw(&c);
    assert!(
        c.assign(
            "dispatcher",
            "dispatch-0",
            NOW,
            scoped("op-2", "two", "2", &[&section("row:b")]),
        )
        .is_err()
    );
    assert_eq!(raw(&c), before);
    // Withdrawing the review keeps held sections but refuses new ones; whole-file still overlaps.
    c.publish("root", "root-1", &|| NOW, SHA, Vec::new(), &[], &[], false)
        .unwrap();
    refused(
        &c,
        scoped("op-2", "two", "2", &[&section("row:b")]),
        "not published as a reviewed shared document",
    );
    refused(&c, scoped("op-2", "two", "2", &[COVERAGE]), "overlaps");
}

#[test]
fn hs2_yhynj6_in_flight_sections_need_review_and_fence_their_keys() {
    let (_root, c) = shared_pool(&[COVERAGE]);
    worker(&c, "two");
    let in_flight = strings(&[&format!("{COVERAGE}#row:a")]);
    let error = c
        .publish(
            "root",
            "root-1",
            &|| NOW,
            SHA,
            Vec::new(),
            &in_flight,
            &[],
            false,
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("requires publishing it as a shared document"),
        "{error}"
    );
    c.publish(
        "root",
        "root-1",
        &|| NOW,
        SHA,
        Vec::new(),
        &in_flight,
        &strings(&[COVERAGE]),
        false,
    )
    .unwrap();
    refused(
        &c,
        scoped("op-2", "two", "2", &[&format!("{COVERAGE}#row:a")]),
        "in-flight integration scope",
    );
    assign(
        &c,
        scoped("op-2", "two", "2", &[&format!("{COVERAGE}#row:b")]),
    )
    .unwrap();
}

#[test]
fn hs2_yhynj6_schema_five_needs_the_upgrade_assertion_and_fences_schema_four() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    let shared = strings(&[COVERAGE]);
    let before = raw(&c);
    let error = c
        .publish(
            "root",
            "root-1",
            &|| NOW,
            SHA,
            Vec::new(),
            &[],
            &shared,
            false,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("schema-five artifact"), "{error}");
    assert_eq!(raw(&c), before);
    // Without shared documents, schema four stays exactly what a schema-four artifact reads.
    c.publish(
        "root",
        "root-1",
        &|| NOW,
        OTHER_SHA,
        Vec::new(),
        &[],
        &[],
        false,
    )
    .unwrap();
    let state: serde_json::Value = serde_json::from_slice(&raw(&c)).unwrap();
    assert_eq!(state["version"], 4);
    assert!(state["integration"].get("shared").is_none());
    c.publish(
        "root",
        "root-1",
        &|| NOW,
        SHA,
        Vec::new(),
        &[],
        &shared,
        true,
    )
    .unwrap();
    let state: serde_json::Value = serde_json::from_slice(&raw(&c)).unwrap();
    assert_eq!(state["version"], 5);
    assert_eq!(
        state["integration"]["shared"],
        serde_json::json!([COVERAGE])
    );
    // A schema-four state carrying shared data is refused rather than silently misread.
    let mut forged = state.clone();
    forged["version"] = 4.into();
    let path = c.directory.join("state.json");
    fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
    let error = c.state().unwrap_err().to_string();
    assert!(error.contains("require schema five"), "{error}");
    // Restart: a reopened read keeps schema five and the published review.
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let reopened = c.state().unwrap();
    assert_eq!(reopened.version, 5);
    assert_eq!(reopened.integration.unwrap().shared, [COVERAGE]);
    let mut newer = state;
    newer["version"] = 6.into();
    fs::write(&path, serde_json::to_vec(&newer).unwrap()).unwrap();
    assert!(c.state().unwrap_err().to_string().contains("incompatible"));
}

const OUTLINE: &str = "\
# Guide

## A

A prose

### A1

a1

### A2

a2

## B

B prose

### B1

b1
";

fn block(doc: &str, heading: &str, next: &str) -> String {
    let start = doc.find(heading).unwrap();
    let end = doc[start..].find(next).map_or(doc.len(), |i| start + i);
    doc[start..end].to_owned()
}

#[test]
fn hs2_yhynj6_heading_moves_fail_closed_but_independent_insertions_pass() {
    let touched = |after: &str| touched_sections(OUTLINE, after);
    let set = |keys: &[&str]| Some(keys.iter().map(|k| (*k).to_owned()).collect());
    // Same-parent reorder of unchanged sibling blocks.
    let (a1, a2) = (
        block(OUTLINE, "### A1", "### A2"),
        block(OUTLINE, "### A2", "## B"),
    );
    assert_eq!(
        touched(&OUTLINE.replace(&format!("{a1}{a2}"), &format!("{a2}{a1}"))),
        None
    );
    // Reorder of whole top-level blocks with their children.
    let (a, b) = (
        block(OUTLINE, "## A", "## B"),
        block(OUTLINE, "## B", "\u{0}"),
    );
    assert_eq!(
        touched(&OUTLINE.replace(&format!("{a}{b}"), &format!("{b}{a}"))),
        None
    );
    // Reparenting by moving a block under another parent, by inserting a new parent above
    // existing children, by changing a level, and by removing a parent.
    assert_eq!(touched(&format!("{}{a1}", OUTLINE.replace(&a1, ""))), None);
    assert_eq!(
        touched(&OUTLINE.replace("A prose\n\n", "A prose\n\n## Inserted\n\nnew\n\n")),
        None
    );
    assert_eq!(touched(&OUTLINE.replace("### A1", "#### A1")), None);
    assert_eq!(touched(&OUTLINE.replace("## A\n\nA prose\n\n", "")), None);
    // Independent insertions reparent nothing and touch only their new keys.
    assert_eq!(
        touched(&OUTLINE.replace("## B\n", "### A3\n\na3\n\n## B\n")),
        set(&["heading:a3"])
    );
    assert_eq!(
        touched(&format!("{OUTLINE}\n## C\n\nc\n")),
        set(&["heading:c"])
    );
    assert_eq!(
        touched(&OUTLINE.replace("B prose\n\n", "B prose\n\n### B0\n\nb0\n\n")),
        set(&["heading:b0"])
    );
    // A declared section cannot carry an undeclared move with it.
    let scope = strings(&["docs/G.md#heading:a1"]);
    let moved = OUTLINE.replace(&format!("{a1}{a2}"), &format!("{a2}{a1}"));
    let message = review_handoff(&scope, &[changed("docs/G.md", Some(OUTLINE), Some(&moved))])
        .unwrap_err()
        .to_string();
    assert!(message.contains("edit outside keyed sections"), "{message}");
}

#[test]
fn hs2_yhynj6_publication_samples_its_clock_at_the_serialized_boundary() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    let before = raw(&c);
    // The integrator's 120 s lease lapses while publication waits for the state lock, so a
    // reading taken at the serialized boundary is past expiry; one taken earlier is not.
    let lock = c.directory.join("state.lock");
    let samples = std::cell::RefCell::new(Vec::new());
    let clock = || {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock)
            .unwrap();
        let inside = FileLock::try_acquire(file).unwrap().is_none();
        samples.borrow_mut().push(inside);
        if inside { NOW + 121 } else { NOW }
    };
    let shared = strings(&[COVERAGE]);
    let message = c
        .publish(
            "root",
            "root-1",
            &clock,
            SHA,
            Vec::new(),
            &[],
            &shared,
            true,
        )
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("register or renew the exact integrator session"),
        "{message}"
    );
    assert!(samples.borrow().iter().all(|inside| *inside), "{samples:?}");
    assert_eq!(
        raw(&c),
        before,
        "no schema five or shared policy after expiry"
    );
    // The same generation renews, then publishes at the current time.
    register(&c, Role::Integrator, "root", "root-1", NOW + 121);
    c.publish(
        "root",
        "root-1",
        &|| NOW + 121,
        SHA,
        Vec::new(),
        &[],
        &shared,
        true,
    )
    .unwrap();
    assert_eq!(c.state().unwrap().version, 5);
}

#[test]
fn hs2_yhynj6_only_whole_trailing_blank_lines_separate_sections() {
    let touched = |after: &str| touched_sections(OUTLINE, after);
    let set = |keys: &[&str]| Some(keys.iter().map(|k| (*k).to_owned()).collect());
    // Trailing spaces, tabs or a non-breaking space on a section's last line are content,
    // for the final section and for one followed by a blank separator.
    for suffix in [" ", "\t", "\u{a0}", " \t\u{a0}"] {
        assert_eq!(
            touched(&OUTLINE.replace("b1\n", &format!("b1{suffix}\n"))),
            set(&["heading:b1"]),
            "{suffix:?}"
        );
        assert_eq!(
            touched(&OUTLINE.replace("a2\n", &format!("a2{suffix}\n"))),
            set(&["heading:a2"]),
            "{suffix:?}"
        );
    }
    // A line holding only a non-breaking space is content too, not a blank separator.
    assert_eq!(touched(&format!("{OUTLINE}\u{a0}\n")), set(&["heading:b1"]));
    // Whole blank lines (empty, spaces or tabs only) still only separate sections.
    assert_eq!(
        touched(&OUTLINE.replace("a2\n\n", "a2\n\n\n \t\n")),
        set(&[])
    );
    assert_eq!(
        touched(&format!("{OUTLINE}\n## C\n\nc\n")),
        set(&["heading:c"])
    );
    assert_eq!(
        touched(&OUTLINE.replace("## B\n", "### A3\n\na3\n\n## B\n")),
        set(&["heading:a3"])
    );
    assert_eq!(
        touched(&format!("{OUTLINE}\n### B2\n\nb2\n")),
        set(&["heading:b2"])
    );
    // The undeclared whitespace edit is refused under an unrelated declared section.
    let message = review_handoff(
        &strings(&["docs/G.md#heading:a1"]),
        &[changed(
            "docs/G.md",
            Some(OUTLINE),
            Some(&OUTLINE.replace("a2\n", "a2\u{a0}\n")),
        )],
    )
    .unwrap_err()
    .to_string();
    assert!(
        message.contains("undeclared sections heading:a2"),
        "{message}"
    );
}
