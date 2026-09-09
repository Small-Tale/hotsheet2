//! Deterministic Markdown activity notes for media-annotation batches.

use hotsheet_model::MediaAnnotation;

fn percent(value: u32) -> String {
    format!("{:.1}%", f64::from(value) / 100.0)
}

fn time(value: u64) -> String {
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let milliseconds = value % 1_000;
    format!("{minutes:02}:{seconds:02}.{milliseconds:03}")
}

fn location(annotation: &MediaAnnotation) -> String {
    let rectangle = format!(
        "(x {}, y {}, w {}, h {})",
        percent(annotation.x),
        percent(annotation.y),
        percent(annotation.width),
        percent(annotation.height)
    );
    match (annotation.start_ms, annotation.end_ms) {
        (Some(start), Some(end)) if start == end => {
            format!("{rectangle} at {}", time(start))
        }
        (Some(start), Some(end)) => {
            format!("{rectangle} from {} to {}", time(start), time(end))
        }
        _ => rectangle,
    }
}

fn caption(annotation: &MediaAnnotation) -> String {
    let text = annotation.text.trim().replace('\n', "\n  ");
    if text.is_empty() {
        String::new()
    } else {
        format!(" — {text}")
    }
}

/// Describe one persisted annotation batch as a user-readable Markdown activity.
///
/// Returns `None` when the annotation collections are identical. Added and updated
/// annotations follow the new collection's order; removals follow the old order.
pub fn annotation_change_activity(
    filename: &str,
    before: &[MediaAnnotation],
    after: &[MediaAnnotation],
) -> Option<(String, String)> {
    if before == after {
        return None;
    }
    let mut lines = vec![format!(
        "Annotations changed for [attachment:{filename}](attachment:{filename})"
    )];
    for annotation in after {
        match before
            .iter()
            .find(|candidate| candidate.id == annotation.id)
        {
            None => lines.push(format!(
                "- Added `{}`{}",
                location(annotation),
                caption(annotation)
            )),
            Some(previous) if previous != annotation => lines.push(format!(
                "- Updated `{}`{}",
                location(annotation),
                caption(annotation)
            )),
            Some(_) => {}
        }
    }
    for annotation in before {
        if !after.iter().any(|candidate| candidate.id == annotation.id) {
            lines.push(format!(
                "- Removed `{}`{}",
                location(annotation),
                caption(annotation)
            ));
        }
    }
    Some((
        format!("Updated annotations for {filename}"),
        lines.join("\n"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(id: &str, text: &str) -> MediaAnnotation {
        MediaAnnotation {
            id: id.into(),
            x: 1_210,
            y: 5_400,
            width: 940,
            height: 250,
            start_ms: None,
            end_ms: None,
            text: text.into(),
        }
    }

    #[test]
    fn reports_added_updated_and_removed_annotations_in_stable_order() {
        let removed = annotation("removed", "Old caption");
        let changed = annotation("changed", "Before");
        let mut updated = changed.clone();
        updated.x = 2_000;
        updated.text = "After\nwith detail".into();
        updated.start_ms = Some(1_000);
        updated.end_ms = Some(2_500);
        let added = annotation("added", "New caption");

        let (summary, body) =
            annotation_change_activity("proof.png", &[removed, changed], &[updated, added])
                .unwrap();

        assert_eq!(summary, "Updated annotations for proof.png");
        assert_eq!(
            body,
            concat!(
                "Annotations changed for [attachment:proof.png](attachment:proof.png)\n",
                "- Updated `(x 20.0%, y 54.0%, w 9.4%, h 2.5%) from 00:01.000 to 00:02.500` — After\n",
                "  with detail\n",
                "- Added `(x 12.1%, y 54.0%, w 9.4%, h 2.5%)` — New caption\n",
                "- Removed `(x 12.1%, y 54.0%, w 9.4%, h 2.5%)` — Old caption"
            )
        );
    }

    #[test]
    fn ignores_no_op_batches_and_formats_point_annotations() {
        let mut point = annotation("point", "");
        point.start_ms = Some(61_005);
        point.end_ms = Some(61_005);
        assert!(
            annotation_change_activity("proof.png", &[point.clone()], &[point.clone()]).is_none()
        );

        let (_, body) = annotation_change_activity("clip.mp4", &[], &[point]).unwrap();
        assert!(body.contains("Added `(x 12.1%, y 54.0%, w 9.4%, h 2.5%) at 01:01.005`"));
    }
}
