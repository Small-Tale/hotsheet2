//! Where a link or `window.open` from the desktop webview goes (HS2-0A7FWS).
//!
//! wry creates no new window unless the shell handles the request, and Hot Sheet is one window, so a
//! `target="_blank"` link or `window.open` used to do nothing. External http(s) and mailto targets
//! now open in the user's browser; the app's own navigations stay in the webview.

use tauri::Url;

/// What the shell does with a requested URL.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkRoute {
    /// Stay in the webview (the app's own origin).
    InApp,
    /// Hand the URL to the system browser or mail client, and keep the webview where it is.
    System,
    /// Neither: an empty popup (`about:blank`) or a scheme the shell does not open.
    Ignore,
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.origin() == right.origin()
}

/// A request for a new window (a `target="_blank"` link or `window.open`). The app never opens a
/// second webview, so every openable target goes to the system, including the app's own origin
/// (for example an attachment file), which the local bridge serves to any local browser.
pub fn new_window_route(url: &Url) -> LinkRoute {
    match url.scheme() {
        "http" | "https" | "mailto" => LinkRoute::System,
        _ => LinkRoute::Ignore,
    }
}

/// A navigation of the main webview. Moving within the app's origin is the app itself; leaving it
/// for an external page would replace Hot Sheet, so that page opens in the system browser instead.
pub fn navigation_route(url: &Url, app: &Url) -> LinkRoute {
    if same_origin(url, app) {
        return LinkRoute::InApp;
    }
    match url.scheme() {
        "http" | "https" | "mailto" => LinkRoute::System,
        // Internal schemes the webview loads for itself (about:blank, data:, blob:, tauri:) stay.
        _ => LinkRoute::InApp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    #[test]
    fn hs2_0a7fws_new_windows_open_openable_targets_in_the_system() {
        for target in [
            "https://github.com/login/device",
            "http://example.test/docs",
            "mailto:support@example.invalid",
            // The app's own origin too: an attachment opened in a new tab.
            "http://127.0.0.1:4175/__hotsheet/attachments/a.png",
        ] {
            assert_eq!(
                new_window_route(&url(target)),
                LinkRoute::System,
                "{target}"
            );
        }
        for ignored in [
            "about:blank",
            "javascript:void(0)",
            "file:///etc/hosts",
            "data:text/plain,x",
        ] {
            assert_eq!(
                new_window_route(&url(ignored)),
                LinkRoute::Ignore,
                "{ignored}"
            );
        }
    }

    #[test]
    fn hs2_0a7fws_navigation_keeps_the_app_and_sends_external_pages_out() {
        let app = url("http://127.0.0.1:4175/");
        assert_eq!(
            navigation_route(&url("http://127.0.0.1:4175/settings"), &app),
            LinkRoute::InApp
        );
        assert_eq!(
            navigation_route(&url("http://127.0.0.1:4175/?project=a#x"), &app),
            LinkRoute::InApp
        );
        assert_eq!(
            navigation_route(&url("https://github.com/Small-Tale"), &app),
            LinkRoute::System
        );
        assert_eq!(
            navigation_route(&url("mailto:support@example.invalid"), &app),
            LinkRoute::System
        );
        // Another local port is another origin, not the app.
        assert_eq!(
            navigation_route(&url("http://127.0.0.1:9999/"), &app),
            LinkRoute::System
        );
        assert_eq!(
            navigation_route(&url("about:blank"), &app),
            LinkRoute::InApp
        );
    }
}
