//! HS2-SZF152: the project time zone setting and the deadline helpers (docs/38 §38.10,
//! Time zone unit row).

use super::*;
use serde_json::json;

fn ms(instant: &str) -> i64 {
    instant.parse::<Timestamp>().unwrap().as_millisecond()
}

fn date(text: &str) -> Date {
    parse_due_on(text).unwrap()
}

fn zone(name: &str) -> TimeZone {
    resolve_time_zone(name).unwrap().1
}

fn project() -> (tempfile::TempDir, tempfile::TempDir, Settings) {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let settings = Settings::for_project_with_global_home(root.path(), home.path());
    (root, home, settings)
}

#[test]
fn hs2_szf152_unset_project_reads_utc_never_the_device_zone() {
    let (_root, _home, settings) = project();
    let resolved = project_time_zone(&settings).unwrap();
    assert_eq!(resolved.name, "UTC");
    assert_eq!(resolved.zone, TimeZone::UTC);
    assert!(resolved.invalid_value.is_none());
    assert!(resolved.diagnostic().is_none());
}

#[test]
fn hs2_szf152_shared_writes_are_canonical_and_repeatable() {
    let (_root, _home, settings) = project();
    settings
        .set(TIME_ZONE_SETTING, json!("Europe/Berlin"), Scope::Shared)
        .unwrap();
    assert_eq!(project_time_zone(&settings).unwrap().name, "Europe/Berlin");
    // `utc` is stored as the canonical `UTC`.
    settings
        .set(TIME_ZONE_SETTING, json!("utc"), Scope::Shared)
        .unwrap();
    assert_eq!(
        settings.get(TIME_ZONE_SETTING, Scope::Shared).unwrap(),
        Some(json!("UTC"))
    );
    // Reset to another zone, then repeat the same write: the value stays the same.
    for _ in 0..2 {
        settings
            .set(TIME_ZONE_SETTING, json!("America/New_York"), Scope::Shared)
            .unwrap();
        assert_eq!(
            settings.get(TIME_ZONE_SETTING, Scope::Shared).unwrap(),
            Some(json!("America/New_York"))
        );
    }
    // The conditional writer validates and canonicalizes too.
    assert!(
        settings
            .set_if(TIME_ZONE_SETTING, json!("UTC"), Scope::Shared, |_| true)
            .unwrap()
    );
    assert_eq!(project_time_zone(&settings).unwrap().name, "UTC");
}

#[test]
fn hs2_szf152_local_and_global_writes_are_refused_and_ignored_when_present() {
    let (root, _home, settings) = project();
    settings
        .set(TIME_ZONE_SETTING, json!("Asia/Tokyo"), Scope::Shared)
        .unwrap();
    for scope in [Scope::Local, Scope::Global] {
        let error = settings
            .set(TIME_ZONE_SETTING, json!("Europe/Berlin"), scope)
            .unwrap_err();
        assert_eq!(error.code(), Some(TIME_ZONE_SHARED_ONLY), "{scope:?}");
        let error = settings
            .set_if(TIME_ZONE_SETTING, json!("Europe/Berlin"), scope, |_| true)
            .unwrap_err();
        assert_eq!(error.code(), Some(TIME_ZONE_SHARED_ONLY), "{scope:?}");
        assert_eq!(settings.get(TIME_ZONE_SETTING, scope).unwrap(), None);
    }
    // A value placed in the local file by hand is ignored: only the shared scope counts.
    std::fs::write(
        root.path().join(".hotsheet2/settings.local.json"),
        r#"{"time_zone":"Europe/Berlin"}"#,
    )
    .unwrap();
    assert_eq!(project_time_zone(&settings).unwrap().name, "Asia/Tokyo");
}

#[test]
fn hs2_szf152_unknown_zones_are_refused_on_write_and_read_as_utc() {
    let (root, _home, settings) = project();
    settings
        .set(TIME_ZONE_SETTING, json!("Europe/Berlin"), Scope::Shared)
        .unwrap();
    for invalid in [
        json!("Mars/Olympus_Mons"),
        json!(""),
        json!(" Europe/Berlin"),
        json!("EST5EDT,M3.2.0,M11.1.0"),
        json!("+02:00"),
        json!(5),
        json!(null),
        json!(["UTC"]),
    ] {
        let error = settings
            .set(TIME_ZONE_SETTING, invalid.clone(), Scope::Shared)
            .unwrap_err();
        assert_eq!(error.code(), Some(INVALID_TIME_ZONE), "{invalid}");
        assert!(
            error.to_string().contains("not a known time zone"),
            "{error}"
        );
    }
    // Refused writes leave the stored zone alone.
    assert_eq!(project_time_zone(&settings).unwrap().name, "Europe/Berlin");
    // A hand-edited unknown name reads as UTC and is named by the diagnostic.
    std::fs::write(
        root.path().join(".hotsheet2/settings.json"),
        r#"{"$hotsheetSchema":1,"time_zone":"Mars/Olympus_Mons"}"#,
    )
    .unwrap();
    let resolved = project_time_zone(&settings).unwrap();
    assert_eq!(resolved.name, "UTC");
    assert_eq!(resolved.invalid_value, Some(json!("Mars/Olympus_Mons")));
    assert!(
        resolved
            .diagnostic()
            .unwrap()
            .contains("\"Mars/Olympus_Mons\"")
    );
    // Correcting it clears the diagnostic.
    settings
        .set(TIME_ZONE_SETTING, json!("Europe/Berlin"), Scope::Shared)
        .unwrap();
    assert!(
        project_time_zone(&settings)
            .unwrap()
            .invalid_value
            .is_none()
    );
}

#[test]
fn hs2_szf152_available_zones_start_with_utc_and_resolve() {
    let zones = available_time_zones();
    assert_eq!(zones.first().map(String::as_str), Some("UTC"));
    assert_eq!(zones.iter().filter(|name| *name == "UTC").count(), 1);
    for name in ["Europe/Berlin", "America/New_York", "Asia/Kolkata"] {
        assert!(zones.iter().any(|zone| zone == name), "{name}");
    }
    assert!(zones.iter().all(|name| resolve_time_zone(name).is_some()));
}

#[test]
fn hs2_szf152_due_dates_parse_strictly() {
    assert_eq!(date("2026-12-04").to_string(), "2026-12-04");
    for invalid in [
        "2026-12-4",
        "2026-13-01",
        "2026-02-30",
        "+02026-12-04",
        "2026/12/04",
        "2026-12-04T00:00",
        "",
    ] {
        assert!(parse_due_on(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn hs2_szf152_deadline_is_the_last_millisecond_of_the_local_day() {
    // UTC: an ordinary day.
    assert_eq!(
        deadline_instant_ms(date("2026-12-04"), &TimeZone::UTC),
        ms("2026-12-05T00:00:00Z") - 1
    );
    // Europe/Berlin in winter is UTC+1.
    assert_eq!(
        deadline_instant_ms(date("2026-12-04"), &zone("Europe/Berlin")),
        ms("2026-12-04T23:00:00Z") - 1
    );
    // A zone with a half-hour offset.
    assert_eq!(
        deadline_instant_ms(date("2026-12-04"), &zone("Asia/Kolkata")),
        ms("2026-12-04T18:30:00Z") - 1
    );
    // Leap day, and the turn of the year.
    assert_eq!(
        deadline_instant_ms(date("2028-02-29"), &TimeZone::UTC),
        ms("2028-03-01T00:00:00Z") - 1
    );
    assert_eq!(
        deadline_instant_ms(date("2026-12-31"), &zone("America/New_York")),
        ms("2027-01-01T05:00:00Z") - 1
    );
}

#[test]
fn hs2_szf152_deadline_spans_daylight_saving_transitions() {
    let new_york = zone("America/New_York");
    // Spring forward on Sunday 2026-03-08: that day is 23 hours long.
    let before = deadline_instant_ms(date("2026-03-07"), &new_york);
    let spring = deadline_instant_ms(date("2026-03-08"), &new_york);
    assert_eq!(before, ms("2026-03-08T05:00:00Z") - 1);
    assert_eq!(spring, ms("2026-03-09T04:00:00Z") - 1);
    assert_eq!(spring - before, 23 * 3_600_000);
    // Fall back on Sunday 2026-11-01: that day is 25 hours long.
    let before = deadline_instant_ms(date("2026-10-31"), &new_york);
    let fall = deadline_instant_ms(date("2026-11-01"), &new_york);
    assert_eq!(before, ms("2026-11-01T04:00:00Z") - 1);
    assert_eq!(fall, ms("2026-11-02T05:00:00Z") - 1);
    assert_eq!(fall - before, 25 * 3_600_000);

    let berlin = zone("Europe/Berlin");
    // Europe: 2026-03-29 (23 h) and 2026-10-25 (25 h), changing at 01:00 UTC.
    assert_eq!(
        deadline_instant_ms(date("2026-03-29"), &berlin)
            - deadline_instant_ms(date("2026-03-28"), &berlin),
        23 * 3_600_000
    );
    assert_eq!(
        deadline_instant_ms(date("2026-10-25"), &berlin)
            - deadline_instant_ms(date("2026-10-24"), &berlin),
        25 * 3_600_000
    );

    // A zone whose clocks skip midnight itself (Chile moves 00:00 to 01:00): the next day
    // starts at its first instant, which is still the end of the previous day.
    let santiago = zone("America/Santiago");
    let skipped = deadline_instant_ms(date("2026-09-05"), &santiago);
    assert_eq!(skipped, ms("2026-09-06T04:00:00Z") - 1);
    assert_eq!(
        deadline_instant_ms(date("2026-09-06"), &santiago) - skipped,
        23 * 3_600_000
    );
}

#[test]
fn hs2_szf152_deadline_saturates_at_the_end_of_the_range() {
    let last = Date::MAX;
    assert_eq!(
        deadline_instant_ms(last, &TimeZone::UTC),
        Timestamp::MAX.as_millisecond()
    );
}

#[test]
fn hs2_szf152_overdue_exactly_at_before_and_after_the_instant() {
    let deadline = deadline_instant_ms(date("2026-12-04"), &zone("Europe/Berlin"));
    assert!(!is_overdue(deadline, deadline - 1));
    assert!(!is_overdue(deadline, deadline));
    assert!(is_overdue(deadline, deadline + 1));
}

#[test]
fn hs2_szf152_due_within_compares_instants() {
    let deadline = deadline_instant_ms(date("2026-12-04"), &TimeZone::UTC);
    let day = 86_400_000;
    assert!(is_due_within(deadline, deadline, 0));
    assert!(is_due_within(deadline, deadline - day, 1));
    assert!(!is_due_within(deadline, deadline - day - 1, 1));
    assert!(is_due_within(deadline, deadline - 7 * day, 7));
    assert!(!is_due_within(deadline, deadline - 7 * day - 1, 7));
    // Overdue is never "due within".
    assert!(!is_due_within(deadline, deadline + 1, 7));
    // No overflow at the extremes.
    assert!(!is_due_within(i64::MAX, i64::MIN, u32::MAX));
    assert!(!is_due_within(i64::MAX, 0, 1));
}

#[test]
fn hs2_szf152_relative_days_follow_the_project_zone() {
    // 23:30 UTC on Dec 4 is already Dec 5 in Berlin and still Dec 4 in Los Angeles.
    let now = ms("2026-12-04T23:30:00Z");
    let due = date("2026-12-05");
    assert_eq!(due_day_offset(due, &TimeZone::UTC, now), 1);
    assert_eq!(due_day_offset(due, &zone("Europe/Berlin"), now), 0);
    assert_eq!(due_day_offset(due, &zone("America/Los_Angeles"), now), 1);
    // Changing the zone changes the word, not the stored date.
    assert_eq!(
        due_day_offset(date("2026-12-01"), &zone("Europe/Berlin"), now),
        -4
    );
    assert_eq!(due_day_offset(date("2026-12-01"), &TimeZone::UTC, now), -3);
    // Across a daylight-saving change the count stays in calendar days.
    let after_fall_back = ms("2026-11-02T12:00:00Z");
    assert_eq!(
        due_day_offset(
            date("2026-10-31"),
            &zone("America/New_York"),
            after_fall_back
        ),
        -2
    );
}
