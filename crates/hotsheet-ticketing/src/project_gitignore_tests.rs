//! HS2-61ZMVA: the managed project `.gitignore` block.

use super::*;

fn block() -> String {
    format!("{BLOCK_BEGIN}\n{}\n{BLOCK_END}\n", BLOCK_RULES.join("\n"))
}

fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let status = crate::git::command()
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());
    dir
}

fn read(dir: &Path) -> String {
    std::fs::read_to_string(gitignore_path(dir)).unwrap()
}

fn write(dir: &Path, text: &str) {
    std::fs::write(gitignore_path(dir), text).unwrap();
}

#[test]
fn a_missing_gitignore_gets_only_the_block() {
    let dir = git_repo();
    let report = ensure_project_block(dir.path()).unwrap();
    assert!(report.changed);
    assert_eq!(read(dir.path()), block());
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
}

#[test]
fn the_block_is_appended_after_user_lines_and_is_idempotent() {
    let dir = git_repo();
    write(dir.path(), "target/\n\n\n");
    assert!(ensure_project_block(dir.path()).unwrap().changed);
    let first = read(dir.path());
    assert_eq!(first, format!("target/\n\n{}", block()));
    let again = ensure_project_block(dir.path()).unwrap();
    assert!(!again.changed);
    assert_eq!(read(dir.path()), first);
    assert_eq!(plan_project_block(dir.path()).unwrap(), None);
}

#[test]
fn a_file_without_a_trailing_newline_keeps_its_last_line() {
    let dir = git_repo();
    write(dir.path(), "node_modules/");
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(read(dir.path()), format!("node_modules/\n\n{}", block()));
}

#[test]
fn legacy_single_file_lines_are_folded_into_the_block() {
    let dir = git_repo();
    write(
        dir.path(),
        "target/\n.hotsheet2/settings.local.json\n*.log\n.hotsheet2/store\n",
    );
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(read(dir.path()), format!("target/\n*.log\n\n{}", block()));
}

#[test]
fn equivalent_hand_written_rules_are_left_alone() {
    let dir = git_repo();
    let hand = "# local\n/.hotsheet2/*\n!/.hotsheet2/settings.json\n!/.hotsheet2/groups/\n";
    write(dir.path(), hand);
    assert_eq!(plan_project_block(dir.path()).unwrap(), None);
    let report = ensure_project_block(dir.path()).unwrap();
    assert!(!report.changed);
    assert!(report.conflicts.is_empty());
    assert_eq!(read(dir.path()), hand);
}

#[test]
fn partial_hand_written_rules_still_get_the_block() {
    // Only named files are ignored, so a later machine-local file would be untracked.
    let dir = git_repo();
    write(
        dir.path(),
        ".hotsheet2/settings.local.json\n.hotsheet2/store\n",
    );
    assert!(ensure_project_block(dir.path()).unwrap().changed);
    assert_eq!(read(dir.path()), block());
}

#[test]
fn an_edited_block_is_restored_in_place() {
    let dir = git_repo();
    write(
        dir.path(),
        &format!("a\n{BLOCK_BEGIN}\n/.hotsheet2/\n# note\n{BLOCK_END}\nb\n"),
    );
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(read(dir.path()), format!("a\n{}b\n", block()));
}

#[test]
fn duplicate_blocks_collapse_to_the_first() {
    let dir = git_repo();
    write(dir.path(), &format!("a\n{}b\n{}c\n", block(), block()));
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(read(dir.path()), format!("a\n{}b\nc\n", block()));
}

#[test]
fn a_block_missing_its_end_marker_keeps_the_user_lines_after_it() {
    let dir = git_repo();
    write(
        dir.path(),
        &format!("a\n{BLOCK_BEGIN}\n/.hotsheet2/*\n!/.hotsheet2/settings.json\ndist/\n"),
    );
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(read(dir.path()), format!("a\n{}dist/\n", block()));
}

#[test]
fn a_damaged_block_before_a_complete_one_does_not_swallow_user_lines() {
    let dir = git_repo();
    write(
        dir.path(),
        &format!("{BLOCK_BEGIN}\n/.hotsheet2/*\nuser\n{}tail\n", block()),
    );
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(read(dir.path()), format!("{}user\ntail\n", block()));
}

#[test]
fn crlf_files_stay_crlf() {
    let dir = git_repo();
    write(dir.path(), "target/\r\n.hotsheet2/store\r\n");
    ensure_project_block(dir.path()).unwrap();
    assert_eq!(
        read(dir.path()),
        format!("target/\r\n\r\n{}", block().replace('\n', "\r\n"))
    );
}

#[test]
fn a_bare_directory_rule_is_reported_as_hiding_committed_paths() {
    let dir = git_repo();
    write(dir.path(), ".hotsheet2/\n");
    let report = ensure_project_block(dir.path()).unwrap();
    assert!(report.changed, "the block is still written");
    let hidden: Vec<_> = report
        .conflicts
        .iter()
        .filter(|conflict| conflict.expected == Expectation::Committed)
        .collect();
    assert_eq!(hidden.len(), 2, "{:?}", report.conflicts);
    assert!(
        hidden
            .iter()
            .all(|conflict| conflict.rule.as_deref() == Some(".gitignore:1:.hotsheet2/"))
    );
    assert!(hidden[0].to_string().contains("remove or narrow that rule"));
    assert!(
        hidden[1].to_string().contains("hides .hotsheet2/groups/,"),
        "{}",
        hidden[1]
    );
    assert_eq!(read(dir.path()), format!(".hotsheet2/\n\n{}", block()));
}

#[test]
fn a_later_rule_that_re_includes_a_local_file_is_reported() {
    let dir = git_repo();
    write(dir.path(), &format!("{}!/.hotsheet2/store\n", block()));
    let report = ensure_project_block(dir.path()).unwrap();
    assert!(!report.changed);
    assert_eq!(
        report.conflicts,
        vec![GitignoreConflict {
            path: ".hotsheet2/store".into(),
            expected: Expectation::Ignored,
            rule: Some(".gitignore:6:!/.hotsheet2/store".into()),
        }]
    );
    assert!(
        report.conflicts[0]
            .to_string()
            .contains("re-includes .hotsheet2/store")
    );
}

#[test]
fn a_later_broad_rule_that_hides_settings_is_reported() {
    let dir = git_repo();
    write(dir.path(), &format!("{}*.json\n", block()));
    let report = ensure_project_block(dir.path()).unwrap();
    let paths: Vec<_> = report.conflicts.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            ".hotsheet2/settings.json",
            ".hotsheet2/groups/0000000000.json"
        ]
    );
}

#[test]
fn outside_a_git_work_tree_the_block_is_written_without_conflict_checks() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".hotsheet2/\n");
    let report = ensure_project_block(dir.path()).unwrap();
    assert!(report.changed);
    assert!(report.conflicts.is_empty());
    assert_eq!(read(dir.path()), format!(".hotsheet2/\n\n{}", block()));
}

#[test]
fn a_git_subdirectory_project_is_probed_relative_to_itself() {
    let repo = git_repo();
    let project = repo.path().join("app");
    std::fs::create_dir(&project).unwrap();
    let report = ensure_project_block(&project).unwrap();
    assert!(report.changed);
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
}

#[test]
fn ensure_entry_appends_once_and_preserves_lines() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "target/");
    assert!(ensure_entry(dir.path(), "hotsheet-settings.local.json").unwrap());
    assert!(!ensure_entry(dir.path(), "hotsheet-settings.local.json").unwrap());
    assert_eq!(read(dir.path()), "target/\nhotsheet-settings.local.json\n");
}

#[test]
fn rule_patterns_parse_with_colons_in_the_source() {
    assert_eq!(
        pattern_of(".gitignore:3:/.hotsheet2/*"),
        Some("/.hotsheet2/*")
    );
    assert_eq!(pattern_of(r"C:\Users\x\ignore:12:!a:b"), Some("!a:b"));
    assert_eq!(pattern_of("::"), None);
    assert!(rule_ignores(".gitignore:1:x"));
    assert!(!rule_ignores(".gitignore:1:!x"));
}
