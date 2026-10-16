//! The live terminal attach WebSocket and its control-frame protocol.

use crate::AppState;
use crate::dto::WsParams;
use crate::routes::terminals::{broker_err, term_key};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// `GET /terminals/{id}/attach?secret=…` — the **live** terminal attach (HS2-XTTTMV): a
/// WebSocket that first replays the scrollback (one binary frame), then streams each new PTY
/// output chunk as a binary frame and forwards any binary/text the viewer sends as PTY input.
/// The socket closes when the terminal's child exits or the viewer disconnects.
pub(crate) async fn attach_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Response {
    if params.secret.as_deref() != Some(state.secret.as_str()) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    // Broker mode: the PTY lives in the detached broker. Confirm the terminal exists (clean
    // 404) before upgrading, then bridge the WebSocket to a streaming broker connection
    // (HS2-ERT00F item 4). The size self-heal on disconnect happens broker-side.
    if let Some(tb) = &state.terminal_broker {
        match tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
            .await
        {
            Ok(hotsheet_terminals::BrokerResponse::Read { .. }) => {}
            Ok(hotsheet_terminals::BrokerResponse::NotFound) => {
                return (StatusCode::NOT_FOUND, "no such terminal").into_response();
            }
            Ok(other) => return broker_err(other).into_response(),
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}")).into_response();
            }
        }
        let socket_path = tb.socket.clone();
        return ws.on_upgrade(move |socket| broker_attach_loop(socket, socket_path, id));
    }
    let Some(term) = state.terminals.get(&term_key(&state, &id)) else {
        return (StatusCode::NOT_FOUND, "no such terminal").into_response();
    };
    ws.on_upgrade(move |socket| terminal_attach_loop(socket, term))
}

fn default_true() -> bool {
    true
}

/// An inbound control message on the terminal WS (a Text frame). Today: a **size claim**
/// (HS2-BD7Q74). Ordinary Text is raw PTY input so a simple client can type over the same
/// socket; malformed JSON carrying the reserved `resize` member is dropped (Binary is input).
#[derive(Deserialize)]
struct TermControl {
    #[serde(default)]
    resize: Option<ResizeClaim>,
    /// A viewer's liveness probe (HS2-51HQ3S), answered with `{"terminal_pong":n}`.
    #[serde(default)]
    terminal_ping: Option<u64>,
}

/// A viewport's leased size claim (HS2-BD7Q74).
#[derive(Deserialize)]
struct ResizeClaim {
    viewer_id: String,
    cols: u16,
    rows: u16,
    #[serde(default)]
    focus: bool,
    #[serde(default = "default_true")]
    visible: bool,
    /// This claim was driven by a genuine user interaction (tap/click/focus/keystroke) rather
    /// than a heartbeat; only interacting claims advance the size-arbiter recency (HS2-3ZBQDG).
    #[serde(default)]
    interacting: bool,
}

enum TerminalText<'a> {
    Resize(ResizeClaim),
    Ping(u64),
    InvalidControl,
    Input(&'a str),
}

fn classify_terminal_text(value: &str) -> TerminalText<'_> {
    match serde_json::from_str(value) {
        Ok(TermControl {
            resize: Some(resize),
            ..
        }) => return TerminalText::Resize(resize),
        Ok(TermControl {
            terminal_ping: Some(seq),
            ..
        }) => return TerminalText::Ping(seq),
        _ => {}
    }
    if serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .filter(|json| json.get("resize").is_some() || json.get("terminal_ping").is_some())
        .is_some()
    {
        return TerminalText::InvalidControl;
    }
    TerminalText::Input(value)
}

/// The size the server chose, pushed to every viewer on attach and whenever it changes.
#[derive(Serialize)]
struct SizeMsg<'a> {
    pty_size: PtySizeMsg,
    driven_by: Option<&'a str>,
}
#[derive(Serialize)]
struct PtySizeMsg {
    cols: u16,
    rows: u16,
}

/// The `{pty_size, driven_by}` text frame a viewer receives on attach and on every change.
fn terminal_size_message(cols: u16, rows: u16, driven_by: Option<&str>) -> Message {
    let msg = SizeMsg {
        pty_size: PtySizeMsg { cols, rows },
        driven_by,
    };
    // Serializing two integers and an optional string cannot fail.
    Message::Text(serde_json::to_string(&msg).unwrap_or_default().into())
}

/// A replay after initial attach replaces the viewer's emulator state rather than appending.
fn terminal_replay_control() -> Message {
    Message::Text(r#"{"terminal_replay":"replace"}"#.into())
}

/// Sent first on every attach: this server answers `terminal_ping` probes (HS2-51HQ3S). A
/// viewer only sends probes after this frame, so an older server never sees one as PTY input,
/// and a viewer only treats a silent socket as dead once it knows pongs would arrive.
fn terminal_liveness_hello() -> Message {
    Message::Text(r#"{"terminal_liveness":1}"#.into())
}

/// The reply to a viewer's `{"terminal_ping":n}` probe. A half-open socket (a slept laptop, a
/// changed network, a restarted proxy) can stay OPEN in the browser while nothing flows; the
/// missing pong is how the viewer notices and reconnects (HS2-51HQ3S).
fn terminal_pong(seq: u64) -> Message {
    Message::Text(format!(r#"{{"terminal_pong":{seq}}}"#).into())
}

/// Monotonic-ish wall clock in ms for the size arbiter (real millis; the arbiter is
/// deterministic given it).
fn term_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Drive one attached viewer: replay scrollback, then interleave live output → socket, the
/// arbiter's size decisions → socket, and socket input/claims → PTY, on one task via
/// `select!`. On disconnect the viewport's size claim is dropped so the size self-heals.
async fn terminal_attach_loop(
    mut socket: WebSocket,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
) {
    use tokio::sync::broadcast::error::RecvError;

    // Snapshot and subscribe under one output boundary so concurrent PTY output lands exactly
    // once in either the replay or the live stream (HS2-5W0V9M).
    let (snapshot, mut rx) = term.subscribe_with_scrollback();
    let mut size_rx = term.subscribe_size();
    let mut my_viewer: Option<String> = None;

    // An empty binary replay is still the attach boundary. Reconnecting browsers use it to
    // atomically clear stale emulator state without blanking while waiting for a payload.
    if socket.send(Message::Binary(snapshot.into())).await.is_err() {
        return;
    }
    // The size channel only reports changes, so a viewer of a stable-size terminal would never
    // learn the PTY grid. Report the applied size right after the replay (HS2-7Y1BQ2); the size
    // receiver is already subscribed, so a concurrent change still arrives after this frame.
    if let Some(current) = term.current_size() {
        if socket
            .send(terminal_size_message(
                current.cols,
                current.rows,
                current.driven_by.as_deref(),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    // The liveness hello follows the attach preamble (replay, then size), so viewers that read
    // those two frames first are unaffected (HS2-51HQ3S).
    if socket.send(terminal_liveness_hello()).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            output = rx.recv() => match output {
                Ok(chunk) => {
                    if socket.send(Message::Binary(chunk.into())).await.is_err() {
                        break; // viewer went away
                    }
                }
                // Fell behind the fan-out buffer — re-sync from a fresh snapshot.
                Err(RecvError::Lagged(_)) => {
                    let (snap, replacement) = term.subscribe_with_scrollback();
                    rx = replacement;
                    if socket.send(terminal_replay_control()).await.is_err() {
                        break;
                    }
                    if socket.send(Message::Binary(snap.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break, // terminal ended
            },
            size = size_rx.recv() => match size {
                Ok(d) => {
                    if socket.send(terminal_size_message(d.cols, d.rows, d.driven_by.as_deref())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {} // a missed size update self-corrects on the next claim
                Err(RecvError::Closed) => break,
            },
            inbound = socket.recv() => match inbound {
                // A write to an exited PTY is dropped; the exit is reported separately.
                Some(Ok(Message::Binary(b))) => { let _ = term.write(&b); }
                Some(Ok(Message::Text(t))) => {
                    match classify_terminal_text(&t) {
                        // A size claim: feed the arbiter (it resizes + broadcasts if the size changed).
                        TerminalText::Resize(r) => {
                            my_viewer = Some(r.viewer_id.clone());
                            let now = term_now_ms();
                            term.claim_size(
                                hotsheet_terminals::ViewportClaim {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    interacting: r.interacting,
                                    activity_at_ms: now,
                                },
                                now,
                            );
                        }
                        TerminalText::Ping(seq) => {
                            if socket.send(terminal_pong(seq)).await.is_err() {
                                break;
                            }
                        }
                        // A malformed control frame is never terminal input: dropping it avoids
                        // echoing protocol JSON into the user's shell during transient layout.
                        TerminalText::InvalidControl => {}
                        TerminalText::Input(input) => { let _ = term.write(input.as_bytes()); }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(_)) => break,
            },
        }
    }

    // Self-heal: drop this viewport's claim so the PTY size recomputes for the rest.
    if let Some(v) = my_viewer {
        term.drop_viewer(&v, term_now_ms());
    }
}

/// Bridge one attached viewer to a terminal that lives in the **detached broker** (HS2-ERT00F
/// item 4): open a streaming broker connection, then forward broker output/size frames → the
/// WebSocket and the viewer's input/size claims → the broker, on one task via `select!`. The
/// broker replays the scrollback as its first frame(s) and drops the size claim when this
/// connection closes, so the size self-heal holds even across a server restart.
async fn broker_attach_loop(mut socket: WebSocket, broker_socket: std::path::PathBuf, id: String) {
    let mut stream = match hotsheet_terminals::BrokerStream::open(&broker_socket, &id).await {
        Ok(s) => s,
        Err(_) => return, // dropping the socket closes it
    };
    let mut received_initial_replay = false;
    let mut liveness_announced = false;

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Ok(Some(f)) => {
                    use hotsheet_terminals::StreamOut as S;
                    match f {
                        S::Scrollback { data } => {
                            if received_initial_replay
                                && socket.send(terminal_replay_control()).await.is_err()
                            {
                                break;
                            }
                            received_initial_replay = true;
                            if socket.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        S::Output { data } => {
                            if socket.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        S::Size { cols, rows, driven_by } => {
                            if socket.send(terminal_size_message(cols, rows, driven_by.as_deref())).await.is_err() {
                                break;
                            }
                            // Announce liveness after the broker's replay-then-size preamble (HS2-51HQ3S).
                            if !liveness_announced {
                                liveness_announced = true;
                                if socket.send(terminal_liveness_hello()).await.is_err() {
                                    break;
                                }
                            }
                        }
                        // The terminal's gone (or the attach failed) — end the viewer session.
                        S::NotFound | S::Err { .. } => break,
                    }
                }
                Ok(None) | Err(_) => break, // broker closed (terminal ended / broker gone)
            },
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Binary(b))) => {
                    if stream
                        .send(&hotsheet_terminals::StreamIn::Input { data: b.to_vec() })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Text(t))) => {
                    let sent = match classify_terminal_text(&t) {
                        // A size claim forwards as a Resize frame the broker feeds to its arbiter.
                        TerminalText::Resize(r) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Resize {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    interacting: r.interacting,
                                })
                                .await
                        }
                        // The server answers liveness probes itself; the broker never sees them.
                        TerminalText::Ping(seq) => {
                            if socket.send(terminal_pong(seq)).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        TerminalText::InvalidControl => continue,
                        TerminalText::Input(input) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Input {
                                    data: input.as_bytes().to_vec(),
                                })
                                .await
                        }
                    };
                    if sent.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(_)) => break,
            },
        }
    }

    // Dropping `stream` closes the broker connection, so the broker self-heals the size claim;
    // dropping `socket` closes the WebSocket to the viewer.
}

#[cfg(test)]
mod terminal_text_tests {
    use super::{
        TerminalText, classify_terminal_text, terminal_liveness_hello, terminal_pong,
        terminal_replay_control,
    };
    use axum::extract::ws::Message;

    #[test]
    fn malformed_resize_controls_are_dropped_instead_of_becoming_pty_input() {
        assert!(matches!(
            classify_terminal_text(r#"{"resize":{"viewer_id":"viewer","cols":null,"rows":null}}"#),
            TerminalText::InvalidControl
        ));
        assert!(matches!(
            classify_terminal_text(r#"{"resize":{"viewer_id":"viewer","cols":80,"rows":24}}"#),
            TerminalText::Resize(_)
        ));
        assert!(matches!(
            classify_terminal_text("echo hello\n"),
            TerminalText::Input("echo hello\n")
        ));
    }

    #[test]
    fn liveness_probes_are_answered_and_never_become_pty_input() {
        assert!(matches!(
            classify_terminal_text(r#"{"terminal_ping":7}"#),
            TerminalText::Ping(7)
        ));
        // A malformed probe is dropped, not typed into the shell.
        assert!(matches!(
            classify_terminal_text(r#"{"terminal_ping":"x"}"#),
            TerminalText::InvalidControl
        ));
        assert!(matches!(
            terminal_pong(7),
            Message::Text(value) if value.as_str() == r#"{"terminal_pong":7}"#
        ));
        assert!(matches!(
            terminal_liveness_hello(),
            Message::Text(value) if value.as_str() == r#"{"terminal_liveness":1}"#
        ));
    }

    #[test]
    fn lag_resync_has_an_explicit_replacement_control() {
        assert!(matches!(
            terminal_replay_control(),
            Message::Text(value) if value.as_str() == r#"{"terminal_replay":"replace"}"#
        ));
    }
}
