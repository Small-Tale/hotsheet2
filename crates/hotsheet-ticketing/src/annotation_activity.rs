//! Deterministic Markdown activity notes for media-annotation batches.

use hotsheet_model::{AnnotationPoint, AnnotationShape, MediaAnnotation};

fn percent(value: u32) -> String {
    format!("{:.1}%", f64::from(value) / 100.0)
}

fn time(value: u64) -> String {
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let milliseconds = value % 1_000;
    format!("{minutes:02}:{seconds:02}.{milliseconds:03}")
}

fn point(point: &AnnotationPoint) -> String {
    format!("{},{}", percent(point.x), percent(point.y))
}

fn location(annotation: &MediaAnnotation) -> String {
    let rectangle = format!(
        "(x {}, y {}, w {}, h {})",
        percent(annotation.x),
        percent(annotation.y),
        percent(annotation.width),
        percent(annotation.height)
    );
    let geometry = match &annotation.shape {
        None | Some(AnnotationShape::Rect) => rectangle,
        Some(AnnotationShape::Strike) => format!("strike {rectangle}"),
        Some(AnnotationShape::Freehand { points, closed }) => format!(
            "{} freehand through {} points {rectangle}",
            if *closed { "closed" } else { "open" },
            points.len()
        ),
        Some(AnnotationShape::Arrow { points }) => match (points.first(), points.last()) {
            (Some(start), Some(end)) => format!("arrow from {} to {}", point(start), point(end)),
            _ => format!("arrow {rectangle}"),
        },
        Some(AnnotationShape::Insertion { point: location }) => {
            format!("insertion at {}", point(location))
        }
    };
    match (annotation.start_ms, annotation.end_ms) {
        (Some(start), Some(end)) if start == end => {
            format!("{geometry} at {}", time(start))
        }
        (Some(start), Some(end)) => {
            format!("{geometry} from {} to {}", time(start), time(end))
        }
        _ => geometry,
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

fn intents(annotation: &MediaAnnotation) -> String {
    if annotation.intents.is_empty() {
        String::new()
    } else {
        format!(" [intents: {}]", annotation.intents.join(", "))
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
                "- Added `{}`{}{}",
                location(annotation),
                intents(annotation),
                caption(annotation)
            )),
            Some(previous) if previous != annotation => lines.push(format!(
                "- Updated `{}`{}{}",
                location(annotation),
                intents(annotation),
                caption(annotation)
            )),
            Some(_) => {}
        }
    }
    for annotation in before {
        if !after.iter().any(|candidate| candidate.id == annotation.id) {
            lines.push(format!(
                "- Removed `{}`{}{}",
                location(annotation),
                intents(annotation),
                caption(annotation)
            ));
        }
    }
    let mut listed = Vec::new();
    for annotation in after.iter().chain(before) {
        for intent in &annotation.intents {
            if !listed.contains(intent) {
                listed.push(intent.clone());
            }
        }
    }
    let summary = if listed.is_empty() {
        format!("Updated annotations for {filename}")
    } else {
        format!("Updated annotations for {filename} ({})", listed.join(", "))
    };
    Some((summary, lines.join("\n")))
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
            shape: None,
            intents: Vec::new(),
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

    #[test]
    fn shape_activity_names_direction_and_geometry() {
        let mut arrow = annotation("arrow", "Move here");
        arrow.x = 1_200;
        arrow.y = 4_000;
        arrow.width = 4_800;
        arrow.height = 1;
        arrow.shape = Some(AnnotationShape::Arrow {
            points: vec![
                AnnotationPoint { x: 1_200, y: 4_000 },
                AnnotationPoint { x: 6_000, y: 4_000 },
            ],
        });
        let (_, body) = annotation_change_activity("proof.png", &[], &[arrow]).unwrap();
        assert!(body.contains("arrow from 12.0%,40.0% to 60.0%,40.0%"));

        let mut strike = annotation("strike", "Remove");
        strike.shape = Some(AnnotationShape::Strike);
        let mut insertion = annotation("insert", "Add");
        insertion.x = 9_999;
        insertion.y = 9_999;
        insertion.width = 1;
        insertion.height = 1;
        insertion.shape = Some(AnnotationShape::Insertion {
            point: AnnotationPoint {
                x: 10_000,
                y: 10_000,
            },
        });
        let (_, body) = annotation_change_activity("proof.png", &[], &[strike, insertion]).unwrap();
        assert!(body.contains("strike (x 12.1%, y 54.0%"));
        assert!(body.contains("insertion at 100.0%,100.0%"));
    }

    #[test]
    fn activity_summary_and_lines_list_explicit_intents_in_order() {
        let mut first = annotation("first", "Needs work");
        first.intents = vec!["comment".into(), "bug".into(), "future_focus".into()];
        let mut second = annotation("second", "Question");
        second.intents = vec!["question".into(), "bug".into()];
        let (summary, body) =
            annotation_change_activity("proof.png", &[], &[first.clone(), second.clone()]).unwrap();
        assert_eq!(
            summary,
            "Updated annotations for proof.png (comment, bug, future_focus, question)"
        );
        assert!(body.contains("[intents: comment, bug, future_focus] — Needs work"));
        assert!(body.contains("[intents: question, bug] — Question"));
        let (summary, body) =
            annotation_change_activity("proof.png", &[first, second.clone()], &[second]).unwrap();
        assert!(
            summary.contains("future_focus"),
            "removed intents remain in the summary"
        );
        assert!(body.contains("Removed"));
    }
}
