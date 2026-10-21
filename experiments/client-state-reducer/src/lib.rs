//! Disposable API-consumer prototype for HS2-EJ7NWH; no server core dependency.
use serde_json::{Map, Value};

#[derive(Clone)]
struct Change {
    before: Map<String, Value>,
    after: Map<String, Value>,
}

struct History {
    fields: Map<String, Value>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    pending: Option<Change>,
    queued_undo: bool,
}

impl History {
    fn patch(&mut self, patch: &Map<String, Value>) {
        self.fields.extend(patch.clone());
    }

    fn move_history(&mut self, undo: bool) {
        let (source, target) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let Some(change) = source.pop() else { return };
        let (expected, replacement) = if undo {
            (&change.after, &change.before)
        } else {
            (&change.before, &change.after)
        };
        for (key, value) in expected {
            if self.fields.get(key) == Some(value)
                && let Some(new_value) = replacement.get(key)
            {
                self.fields.insert(key.clone(), new_value.clone());
            }
        }
        target.push(change);
    }

    fn step(&mut self, step: &Value) -> Result<(), String> {
        let action = step["action"].as_str().ok_or("missing action")?;
        match action {
            "begin_edit" => {
                if self.pending.is_some() {
                    return Err("edit still pending".into());
                }
                let after = step["patch"].as_object().ok_or("missing patch")?.clone();
                let before = after
                    .keys()
                    .filter_map(|key| {
                        self.fields
                            .get(key)
                            .map(|value| (key.clone(), value.clone()))
                    })
                    .collect();
                self.patch(&after);
                self.pending = Some(Change { before, after });
            }
            "ack" => {
                let change = self.pending.take().ok_or("nothing to acknowledge")?;
                self.undo.push(change);
                if self.undo.len() > 30 {
                    self.undo.remove(0);
                }
                self.redo.clear();
                if self.queued_undo {
                    self.queued_undo = false;
                    self.move_history(true);
                }
            }
            "remote" => self.patch(step["patch"].as_object().ok_or("missing patch")?),
            "undo" => self.move_history(true),
            "redo" => self.move_history(false),
            "queue_undo" => {
                if self.pending.is_none() {
                    return Err("nothing pending".into());
                }
                self.queued_undo = true;
            }
            _ => return Err(format!("unknown action: {action}")),
        }
        Ok(())
    }
}

/// Returns the number of exact step comparisons completed.
pub fn run_fixture(input: &[u8]) -> Result<usize, String> {
    let fixture: Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    if fixture["format"] != "hotsheet-client-state/v1" || fixture["kind"] != "history" {
        return Err("wrong fixture format or kind".into());
    }
    let mut state = History {
        fields: fixture["initial"]
            .as_object()
            .ok_or("missing initial")?
            .clone(),
        undo: Vec::new(),
        redo: Vec::new(),
        pending: None,
        queued_undo: false,
    };
    let steps = fixture["steps"].as_array().ok_or("missing steps")?;
    for (index, step) in steps.iter().enumerate() {
        state.step(step)?;
        let expected = step["expect"].as_object().ok_or("missing expectation")?;
        let actual: Map<String, Value> = expected
            .keys()
            .filter_map(|key| {
                state
                    .fields
                    .get(key)
                    .map(|value| (key.clone(), value.clone()))
            })
            .collect();
        if &actual != expected {
            return Err(format!(
                "step {}: expected {expected:?}, got {actual:?}",
                index + 1
            ));
        }
    }
    Ok(steps.len())
}

// Raw C ABI deliberately exposes the bridge work a browser integration would own.
#[unsafe(no_mangle)]
pub extern "C" fn fixture_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer
}

#[unsafe(no_mangle)]
/// # Safety
/// `pointer` must come from `fixture_alloc(len)` and must be freed exactly once.
pub unsafe extern "C" fn fixture_free(pointer: *mut u8, len: usize) {
    if !pointer.is_null() {
        unsafe {
            drop(Vec::from_raw_parts(pointer, 0, len));
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `pointer` must address `len` initialized, readable bytes for this call.
pub unsafe extern "C" fn fixture_run(pointer: *const u8, len: usize) -> i32 {
    if pointer.is_null() {
        return -1;
    }
    let input = unsafe { std::slice::from_raw_parts(pointer, len) };
    run_fixture(input).map(|count| count as i32).unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::run_fixture;

    const FIXTURE: &[u8] = include_bytes!("../../../docs/contracts/v1/client-state/history.json");

    #[test]
    fn native_fixture_parity() {
        assert_eq!(run_fixture(FIXTURE), Ok(12));
    }

    #[test]
    fn rejects_changed_expectation() {
        let mut fixture: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        fixture["steps"][10]["expect"]["status"] = "started".into();
        assert!(
            run_fixture(fixture.to_string().as_bytes())
                .unwrap_err()
                .contains("step 11")
        );
    }
}
