//! HS2-TAG2ZP: validate Link targets before reusing authenticated headers.

use std::collections::HashSet;

/// A bounded absolute-URL contract, rather than resolving untrusted relative links.
/// Normalize only the origin; explicit ports and path/query bytes remain significant.
pub(crate) fn target(api_base: &str, value: &str) -> Result<String, &'static str> {
    fn parts(value: &str) -> Result<(String, &str), &'static str> {
        if value.chars().any(|c| c.is_whitespace() || c.is_control())
            || value.contains(['\\', '#', '<', '>'])
        {
            return Err("invalid GitHub pagination URL");
        }
        let (_, remainder) = value
            .split_once("://")
            .ok_or("GitHub pagination requires an absolute HTTP(S) URL")?;
        let end =
            value.len() - remainder.len() + remainder.find(['/', '?']).unwrap_or(remainder.len());
        let origin = &value[..end];
        // Encoded authority characters and non-ASCII host interpretations are outside
        // this contract. K0's origin normalizer preserves deliberate explicit ports.
        if !origin.is_ascii() || origin.contains(['%', '<', '>']) {
            return Err("invalid GitHub pagination origin");
        }
        let origin = crate::github_app_config::normalize_web_base(origin)
            .map_err(|_| "invalid GitHub pagination origin")?;
        let suffix = &value[end..];
        let path = suffix.split('?').next().unwrap_or_default();
        // URL transports remove dot segments. Reject their encoded variants too so
        // the Enterprise API prefix cannot be bypassed before transmission.
        let path = path.to_ascii_lowercase().replace("%2e", ".");
        if path.contains("%2f")
            || path.contains("%5c")
            || path.split('/').any(|part| part == "." || part == "..")
        {
            return Err("invalid GitHub pagination path");
        }
        Ok((origin, suffix))
    }
    let (expected, _) = parts(api_base)?;
    let (origin, suffix) = parts(value)?;
    if origin != expected {
        return Err("GitHub pagination target is outside the selected API origin");
    }
    Ok(format!("{origin}{suffix}"))
}

/// Track one walk only: independent provider pages must not share hidden state.
pub(crate) struct Walk {
    visited: HashSet<String>,
    limit: usize,
}

impl Walk {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            visited: HashSet::new(),
            limit,
        }
    }

    pub(crate) fn enter(&mut self, url: &str) -> Result<(), &'static str> {
        if self.visited.contains(url) {
            return Err("GitHub pagination cycle detected");
        }
        if self.visited.len() >= self.limit {
            return Err("GitHub pagination request limit exceeded");
        }
        self.visited.insert(url.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_tag2zp_absolute_origin_contract_preserves_ports_and_path_bytes() {
        for (base, next, expected) in [
            (
                "https://api.github.com",
                "HTTPS://API.GITHUB.COM/issues?page=2",
                "https://api.github.com/issues?page=2",
            ),
            (
                "https://ghe.test:443/api/v3",
                "HTTPS://GHE.TEST:443/api/v3/issues?Case=Keep",
                "https://ghe.test:443/api/v3/issues?Case=Keep",
            ),
            (
                "http://[::1]:8000/api/v3",
                "http://[::1]:8000/api/v3/issues?page=2",
                "http://[::1]:8000/api/v3/issues?page=2",
            ),
        ] {
            assert_eq!(target(base, next).unwrap(), expected);
        }
        for next in [
            "",
            "/issues?page=2",
            "?page=2",
            "//api.github.com/issues?page=2",
            "https://api.github.com.evil.test/issues?page=2",
            "http://api.github.com/issues?page=2",
            "https://api.github.com:443/issues?page=2",
            "https://api.github.com:8443/issues?page=2",
            "https://synthetic@api.github.com/issues?page=2",
            "https://api.github.com/issues?page=2#fragment",
            "https://api.github.com\\@evil.test/issues?page=2",
            " https://api.github.com/issues?page=2",
            "https://api.github.com/issues?x=\n",
            "file://api.github.com/issues?page=2",
            "https://api.github.com%2f.evil.test/issues?page=2",
            "https://api.github.com/api/v3/../issues?page=2",
            "https://api.github.com/api/v3/%2E%2e/issues?page=2",
            "https://api.github.com/api/v3/%2fissues?page=2",
        ] {
            assert!(target("https://api.github.com", next).is_err(), "{next:?}");
        }
        assert!(
            target(
                "https://ghe.test:443/api/v3",
                "https://ghe.test/api/v3/issues?page=2"
            )
            .is_err()
        );
    }

    #[test]
    fn hs2_tag2zp_walk_reports_cycles_and_budget_without_partial_success() {
        let mut walk = Walk::new(2);
        walk.enter("one").unwrap();
        walk.enter("two").unwrap();
        assert_eq!(walk.enter("one"), Err("GitHub pagination cycle detected"));
        assert_eq!(
            walk.enter("three"),
            Err("GitHub pagination request limit exceeded")
        );
    }
}
