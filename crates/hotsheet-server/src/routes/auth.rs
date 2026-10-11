//! Secret / primary-store authentication middleware.

use crate::*;

// ---- auth ------------------------------------------------------------------------

/// Routes that read or write the primary store without a checkout or hosted-store id.
/// Keep project/store-scoped routes available when only the primary path was replaced.
pub(crate) fn primary_store_route(path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/tickets",
        "/claim-next",
        "/batch",
        "/setup",
        "/analytics",
        "/confidence-report",
        "/commands",
        "/command-groups",
        "/command-runs",
        "/views",
        "/activity",
        "/ai-settings",
        "/terminal-settings",
        "/provider-connections",
    ];
    path == "/providers"
        || PREFIXES.iter().any(|prefix| {
            path == *prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
}

pub(crate) async fn require_secret(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let presented = req
        .headers()
        .get("x-hotsheet-secret")
        .and_then(|v| v.to_str().ok());
    if presented != Some(state.secret.as_str()) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "missing or invalid secret",
        ));
    }
    if primary_store_route(req.uri().path()) {
        state.require_primary_store_identity()?;
    }
    let is_mutation = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    let is_restart = req.uri().path() == "/lifecycle/restart";
    let _mutation = if is_mutation && !is_restart {
        Some(state.begin_mutation().ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "server is quiescing for a safe restart; retry after reconnecting",
            )
        })?)
    } else {
        None
    };
    Ok(next.run(req).await)
}

#[cfg(test)]
mod primary_store_route_tests {
    use super::primary_store_route;

    #[test]
    fn guards_primary_data_without_capturing_scoped_or_machine_routes() {
        for path in [
            "/tickets",
            "/tickets/HS-123/close",
            "/claim-next",
            "/batch",
            "/providers",
            "/provider-connections/github",
            "/analytics/tickets",
            "/commands/one/run",
            "/activity",
            "/terminal-settings",
        ] {
            assert!(primary_store_route(path), "{path}");
        }
        for path in [
            "/checkouts/project/tickets",
            "/stores/store/tickets",
            "/providers/github/tickets",
            "/projects/open",
            "/compatibility",
            "/permissions",
            "/terminals",
            "/tickets-extra",
        ] {
            assert!(!primary_store_route(path), "{path}");
        }
    }
}
