//! HS2-E0H7S0: one controlled-close request, fenced by caller and generation.

#[derive(Default)]
pub struct CloseLifecycle {
    sequence: u64,
    pending: Option<String>,
    allowed: bool,
}

pub fn caller_allowed(window: &str, webview: &str, origin: &str, expected: &str) -> bool {
    window == "main" && webview == "main" && origin == expected
}

impl CloseLifecycle {
    pub fn allowed(&self) -> bool {
        self.allowed
    }

    pub fn begin(&mut self, window_available: bool) -> Option<String> {
        if !window_available || self.allowed || self.pending.is_some() {
            return None;
        }
        self.sequence += 1;
        let request = self.sequence.to_string();
        self.pending = Some(request.clone());
        Some(request)
    }

    pub fn cancel(&mut self, request: &str) -> bool {
        if self.pending.as_deref() != Some(request) {
            return false;
        }
        self.pending = None;
        true
    }

    pub fn acknowledge(&mut self, request: &str, success: bool) -> Result<bool, &'static str> {
        if !self.cancel(request) {
            return Err("Stale desktop close acknowledgement.");
        }
        self.allowed = success;
        Ok(success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_e0h7s0_ack_caller_requires_exact_main_window_webview_and_current_origin() {
        let origin = "http://127.0.0.1:49152";
        assert!(caller_allowed("main", "main", origin, origin));
        for (window, webview, actual) in [
            ("other", "main", origin),
            ("main", "other", origin),
            ("main", "main", "http://127.0.0.1:49153"),
            ("main", "main", "http://localhost:49152"),
            ("main", "main", "https://127.0.0.1:49152"),
        ] {
            assert!(!caller_allowed(window, webview, actual, origin));
        }
    }

    #[test]
    fn hs2_e0h7s0_repeated_close_quit_failure_retry_and_stale_acknowledgements() {
        let mut lifecycle = CloseLifecycle::default();
        assert!(lifecycle.begin(false).is_none()); // early boot / missing main remains retryable
        let first = lifecycle.begin(true).unwrap();
        assert!(lifecycle.begin(true).is_none()); // close + application quit coalesce
        assert!(!lifecycle.allowed());
        assert!(lifecycle.acknowledge("foreign", true).is_err());
        assert!(!lifecycle.acknowledge(&first, false).unwrap());
        let retry = lifecycle.begin(true).unwrap();
        assert_ne!(retry, first);
        assert!(!lifecycle.cancel(&first));
        assert!(lifecycle.acknowledge(&first, true).is_err());
        assert!(lifecycle.acknowledge(&first, false).is_err());
        assert!(lifecycle.cancel(&retry)); // not-ready or deadline expiry
        assert!(lifecycle.acknowledge(&retry, true).is_err());
        let final_request = lifecycle.begin(true).unwrap();
        assert!(lifecycle.acknowledge(&final_request, true).unwrap());
        assert!(lifecycle.allowed());
        assert!(lifecycle.begin(true).is_none());
        assert!(lifecycle.acknowledge(&final_request, true).is_err());
    }
}
