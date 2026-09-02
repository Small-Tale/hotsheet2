use hotsheet_model::parse_file;

/// Retained fixtures are append-only release artifacts. Never rewrite one to make a
/// reader pass; compatibility means the current reader accepts the original bytes.
#[test]
fn reads_retained_prerelease_ticket_format() {
    let raw = include_str!("fixtures/compatibility/prerelease-schema1-ticket.md");
    let ticket = parse_file(raw).expect("current HS2 reads the retained ticket bytes");
    assert_eq!(ticket.slug, "HS-LEGACY");
    assert_eq!(ticket.details, "Legacy details remain readable.");
    assert_eq!(ticket.notes[0].text, "Legacy note remains readable.");
}
