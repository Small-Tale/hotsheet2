//! Broker transitions after `kill` and while a terminal is exiting (HS2-582G8R).
//!
//! A terminal is in one of three states the broker can observe:
//!
//! - **live**: tracked, child running;
//! - **exited**: tracked, child gone (it exited on its own or was signalled without a broker
//!   `kill`), not yet reaped. Its replay stays readable;
//! - **killed**: `kill` forgot it, so the id no longer names anything.
//!
//! | Op            | killed                     | exited                                    |
//! | ------------- | -------------------------- | ----------------------------------------- |
//! | `kill` again  | `not_found`, nothing else  | first `ok` (forgets it), then `not_found` |
//! | `input`       | `not_found`                | `err` once the PTY closed; never written  |
//! | `clear_replay`| `not_found`                | `ok`; the next attach replays the kept line |
//! | `attach`      | `not_found`, then the end  | replay and size, then the end             |
//! | size claim    | `attach` answers not found | never applied: the stream already ended   |
//!
//! A viewer attached while the terminal is live sees its stream end exactly once, after the
//! last output, when the child exits or `kill` removes it (HS2-QWT94V); a size claim it sends
//! afterwards reaches nothing.
//!
//! Every wait is on broker state or a stream frame with a deadline, never a fixed delay.
use super::*;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(10);

struct Broker {
    _dir: tempfile::TempDir,
    sock: std::path::PathBuf,
    client: BrokerClient,
}

async fn start() -> Broker {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("broker.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    tokio::spawn(serve_broker(
        listener,
        "proj".into(),
        Arc::new(TerminalManager::new()),
    ));
    let client = BrokerClient::connect(&sock).await.unwrap();
    Broker {
        _dir: dir,
        sock,
        client,
    }
}

fn open(id: &str, command: &str, args: &[&str]) -> Request {
    Request::Open {
        id: id.into(),
        kind: TerminalKind::Shell,
        command: command.into(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        cwd: None,
        env: vec![],
        replay_bytes: None,
        push: None,
    }
}

fn resize(viewer: &str, cols: u16, rows: u16) -> StreamIn {
    StreamIn::Resize {
        viewer_id: viewer.into(),
        cols,
        rows,
        focus: true,
        visible: true,
        interacting: true,
        answers_queries: true,
    }
}

impl Broker {
    async fn ask(&mut self, req: Request) -> Response {
        self.client.request(&req).await.unwrap()
    }

    async fn kill(&mut self, id: &str) -> Response {
        self.ask(Request::Kill { id: id.into() }).await
    }

    async fn input(&mut self, id: &str, data: &[u8]) -> Response {
        self.ask(Request::Input {
            id: id.into(),
            data: data.to_vec(),
        })
        .await
    }

    async fn clear(&mut self, id: &str) -> Response {
        self.ask(Request::ClearReplay { id: id.into() }).await
    }

    async fn scrollback(&mut self, id: &str) -> Vec<u8> {
        match self
            .ask(Request::Read {
                id: id.into(),
                bytes: BytesEncoding::Array,
            })
            .await
        {
            Response::Read { scrollback, .. } => scrollback.into_vec(),
            other => panic!("expected the replay of {id}, got {other:?}"),
        }
    }

    async fn alive(&mut self, id: &str) -> bool {
        match self.ask(Request::Info { id: id.into() }).await {
            Response::Terminal { info } => info.alive,
            other => panic!("expected {id}'s info, got {other:?}"),
        }
    }

    async fn ids(&mut self) -> Vec<String> {
        match self.ask(Request::List).await {
            Response::List { terminals } => terminals.into_iter().map(|t| t.id).collect(),
            other => panic!("expected the list, got {other:?}"),
        }
    }

    async fn assert_serving(&mut self) {
        assert!(matches!(self.ask(Request::Ping).await, Response::Pong));
    }

    /// Open `id` as a child that prints `marker` and exits, then wait until the broker still
    /// tracks it, reports it dead, and has drained the marker into its replay.
    async fn open_exited(&mut self, id: &str, marker: &str) {
        let script = format!("printf {marker}");
        let opened = self.ask(open(id, "sh", &["-c", &script])).await;
        assert!(
            matches!(opened, Response::Terminal { ref info } if info.spawned == Some(true)),
            "{opened:?}"
        );
        let deadline = Instant::now() + DEADLINE;
        loop {
            if !self.alive(id).await
                && String::from_utf8_lossy(&self.scrollback(id).await).contains(marker)
            {
                return;
            }
            assert!(Instant::now() < deadline, "{id} never exited");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The next frame, or `None` when the broker ended the stream; panics past the deadline.
async fn next_frame(stream: &mut BrokerStream) -> Option<StreamOut> {
    tokio::time::timeout(DEADLINE, stream.next())
        .await
        .expect("no frame and no stream end before the deadline")
        .unwrap()
}

/// Attach to `id` and consume the replay and the applied size that follow it.
async fn attach(sock: &Path, id: &str) -> (BrokerStream, Vec<u8>, Option<u64>) {
    let mut stream = BrokerStream::open(sock, id).await.unwrap();
    let (replay, stream_id) = match next_frame(&mut stream).await {
        Some(StreamOut::Scrollback { data, stream, .. }) => (data.into_vec(), stream),
        other => panic!("expected the replay of {id}, got {other:?}"),
    };
    assert!(
        matches!(next_frame(&mut stream).await, Some(StreamOut::Size { .. })),
        "the applied size follows the replay"
    );
    (stream, replay, stream_id)
}

#[tokio::test]
async fn hs2_582g8r_kill_twice_is_idempotent_and_the_id_can_be_reopened() {
    let mut broker = start().await;
    // Killing an id the broker never hosted is the same `not_found` as a second kill.
    assert!(matches!(broker.kill("never").await, Response::NotFound));

    for round in 0..2 {
        let opened = broker.ask(open("t", "cat", &[])).await;
        assert!(
            matches!(opened, Response::Terminal { ref info } if info.spawned == Some(true) && info.alive),
            "round {round}: a killed id reopens as a fresh PTY: {opened:?}"
        );
        assert_eq!(broker.ids().await, vec!["t".to_string()]);
        assert!(matches!(broker.kill("t").await, Response::Ok));
        // The second kill is a no-op: no error, nothing listed, the broker keeps serving.
        assert!(matches!(broker.kill("t").await, Response::NotFound));
        assert!(matches!(broker.kill("t").await, Response::NotFound));
        assert!(broker.ids().await.is_empty());
        broker.assert_serving().await;
    }
}

#[tokio::test]
async fn hs2_582g8r_kill_forgets_an_exited_terminal_once() {
    let mut broker = start().await;
    broker.open_exited("e", "exit-marker").await;
    // An exited terminal stays tracked until killed or reaped.
    assert_eq!(broker.ids().await, vec!["e".to_string()]);
    assert!(matches!(broker.kill("e").await, Response::Ok));
    assert!(matches!(broker.kill("e").await, Response::NotFound));
    assert!(matches!(
        broker.ask(Request::Info { id: "e".into() }).await,
        Response::NotFound
    ));
    assert!(broker.ids().await.is_empty());
    broker.assert_serving().await;
}

#[tokio::test]
async fn hs2_582g8r_input_clear_and_attach_after_kill_are_not_found() {
    let mut broker = start().await;
    broker.ask(open("t", "cat", &[])).await;
    assert!(matches!(broker.kill("t").await, Response::Ok));

    // Interleaved and repeated, in both orders: each op names nothing, and none of them
    // brings the terminal back.
    for _ in 0..2 {
        assert!(matches!(
            broker.input("t", b"x\n").await,
            Response::NotFound
        ));
        assert!(matches!(broker.clear("t").await, Response::NotFound));
        assert!(matches!(broker.clear("t").await, Response::NotFound));
        assert!(matches!(
            broker.input("t", b"y\n").await,
            Response::NotFound
        ));
        assert!(matches!(broker.kill("t").await, Response::NotFound));
    }
    assert!(broker.ids().await.is_empty());

    // A size claim needs an attach, and attaching to a killed id is refused before any claim.
    let mut stream = BrokerStream::open(&broker.sock, "t").await.unwrap();
    assert!(matches!(
        next_frame(&mut stream).await,
        Some(StreamOut::NotFound)
    ));
    assert!(
        next_frame(&mut stream).await.is_none(),
        "the refusal ends the stream"
    );
    broker.assert_serving().await;
}

#[tokio::test]
async fn hs2_582g8r_input_while_exiting_is_refused_and_never_written() {
    let mut broker = start().await;
    broker.open_exited("e", "exit-marker").await;
    let before = broker.scrollback("e").await;

    // The child is gone, so the write fails with a typed error once the PTY has closed. It is
    // never `not_found`: the terminal is still tracked.
    let deadline = Instant::now() + DEADLINE;
    loop {
        match broker.input("e", b"late-input\n").await {
            Response::Err { message } => {
                assert!(!message.is_empty());
                break;
            }
            // The parent keeps the slave open until the drain finished (HS2-BCE5XG); a write
            // in that window lands in a buffer nobody reads.
            Response::Ok => {}
            other => panic!("input to an exited terminal answered {other:?}"),
        }
        assert!(
            Instant::now() < deadline,
            "input to an exited PTY kept succeeding"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Refused again on repeat, and nothing was echoed into the replay.
    assert!(matches!(
        broker.input("e", b"again\n").await,
        Response::Err { .. }
    ));
    assert_eq!(broker.scrollback("e").await, before);
    assert!(!broker.alive("e").await);
    assert_eq!(broker.ids().await, vec!["e".to_string()]);
    broker.assert_serving().await;
}

/// The applied size an attach reports right after the replay, then the end of an exited
/// terminal's stream.
async fn attach_exited(sock: &Path, id: &str) -> (Vec<u8>, Option<u64>, (u16, u16)) {
    let mut stream = BrokerStream::open(sock, id).await.unwrap();
    let (replay, stream_id) = match next_frame(&mut stream).await {
        Some(StreamOut::Scrollback { data, stream, .. }) => (data.into_vec(), stream),
        other => panic!("expected the replay of {id}, got {other:?}"),
    };
    let size = match next_frame(&mut stream).await {
        Some(StreamOut::Size { cols, rows, .. }) => (cols, rows),
        other => panic!("expected the applied size after the replay, got {other:?}"),
    };
    // HS2-QWT94V: an exited terminal's attach ends after its preamble, exactly once, and a
    // claim sent on the ended stream is never applied or answered.
    assert!(next_frame(&mut stream).await.is_none(), "the attach ends");
    let _ = stream.send(&resize("late", 100, 30)).await;
    assert!(
        next_frame(&mut stream).await.is_none(),
        "one end, nothing after it"
    );
    (replay, stream_id, size)
}

#[tokio::test]
async fn hs2_582g8r_exited_attach_replays_then_ends_and_a_clear_is_replayed() {
    let mut broker = start().await;
    broker.open_exited("e", "exit-marker").await;
    let sock = broker.sock.clone();
    // An exited terminal stays inspectable: each attach replays it and reports its size, then
    // ends (HS2-QWT94V). The late claim each attach sends never changes the size.
    let (replay, first_stream, size) = attach_exited(&sock, "e").await;
    assert!(String::from_utf8_lossy(&replay).contains("exit-marker"));
    let (_, again_stream, again_size) = attach_exited(&sock, "e").await;
    assert_eq!(
        again_size, size,
        "a claim on an ended stream is never applied"
    );
    assert_eq!(again_stream, first_stream);

    // Clearing is accepted, repeatedly, and each clear starts a new stream id that the next
    // attach replays. The cursor's line (the marker, unterminated) is kept.
    let mut seen = vec![first_stream];
    for _ in 0..2 {
        assert!(matches!(broker.clear("e").await, Response::Ok));
        let (replay, stream, after) = attach_exited(&sock, "e").await;
        assert!(!seen.contains(&stream), "a clear starts a new stream id");
        seen.push(stream);
        assert_eq!(String::from_utf8_lossy(&replay), "exit-marker");
        assert_eq!(after, size);
    }
    assert!(!broker.alive("e").await);

    // Killing it then forgets it once; a later clear names nothing.
    assert!(matches!(broker.kill("e").await, Response::Ok));
    assert!(matches!(broker.clear("e").await, Response::NotFound));
    assert!(matches!(broker.kill("e").await, Response::NotFound));
    broker.assert_serving().await;
}

/// A viewer attached when the terminal is killed sees its stream end exactly once, and a size
/// claim it sends afterwards reaches nothing (HS2-QWT94V: the attach held the terminal, whose
/// output channel stayed open after the kill).
#[tokio::test]
async fn hs2_582g8r_attached_stream_ends_once_on_kill() {
    let mut broker = start().await;
    broker.ask(open("t", "cat", &[])).await;
    let sock = broker.sock.clone();
    let (mut viewer, _, _) = attach(&sock, "t").await;

    assert!(matches!(broker.kill("t").await, Response::Ok));
    assert!(
        next_frame(&mut viewer).await.is_none(),
        "the kill ends the stream"
    );
    // One end, no second exit event, even when the kill repeats.
    assert!(matches!(broker.kill("t").await, Response::NotFound));
    assert!(next_frame(&mut viewer).await.is_none());

    // A late size claim on the ended stream is never applied or reported.
    let _ = viewer.send(&resize("v", 100, 30)).await;
    assert!(next_frame(&mut viewer).await.is_none());
    broker.assert_serving().await;
}

/// Read `stream` to its end, returning the output it carried (size frames are skipped). Panics
/// if the stream does not end before the deadline.
async fn read_to_end(stream: &mut BrokerStream) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(frame) = next_frame(stream).await {
        match frame {
            StreamOut::Output { data } | StreamOut::Scrollback { data, .. } => {
                out.extend_from_slice(&data.into_vec());
            }
            StreamOut::Size { .. } => {}
            other => panic!("unexpected frame before the end: {other:?}"),
        }
    }
    out
}

/// HS2-QWT94V: a child that exits on its own ends every attached stream once, after its last
/// output, while the broker keeps it tracked and inspectable until it is killed or reaped.
#[tokio::test]
async fn hs2_qwt94v_child_exit_ends_every_attached_stream_after_its_last_output() {
    let mut broker = start().await;
    let opened = broker
        .ask(open(
            "x",
            "sh",
            &["-c", "read line; printf 'got-%s-done' \"$line\""],
        ))
        .await;
    assert!(matches!(opened, Response::Terminal { .. }), "{opened:?}");
    let sock = broker.sock.clone();
    let (mut first, _, _) = attach(&sock, "x").await;
    let (mut second, _, _) = attach(&sock, "x").await;

    assert!(matches!(broker.input("x", b"bye\n").await, Response::Ok));
    for viewer in [&mut first, &mut second] {
        let out = read_to_end(viewer).await;
        assert!(
            String::from_utf8_lossy(&out).contains("got-bye-done"),
            "the last output arrives before the end: {:?}",
            String::from_utf8_lossy(&out)
        );
        // Ended once: no second end, and a late claim is never applied or answered.
        assert!(next_frame(viewer).await.is_none());
        let _ = viewer.send(&resize("late", 100, 30)).await;
        assert!(next_frame(viewer).await.is_none());
    }
    assert!(!broker.alive("x").await);
    assert_eq!(broker.ids().await, vec!["x".to_string()]);
    // Still inspectable: a fresh attach replays the output, then ends.
    let (replay, _, _) = attach_exited(&sock, "x").await;
    assert!(String::from_utf8_lossy(&replay).contains("got-bye-done"));
    assert!(matches!(broker.kill("x").await, Response::Ok));
    assert!(matches!(broker.kill("x").await, Response::NotFound));
    broker.assert_serving().await;
}

/// HS2-QWT94V: attach, claim, kill, reattach and reopen, repeated and interleaved with two
/// viewers. A claim before the kill is applied and reported to both; the kill ends both once;
/// a claim after it reaches nothing; reopening the id never revives an ended stream.
#[tokio::test]
async fn hs2_qwt94v_kill_reattach_and_size_claims_interleave_across_reopens() {
    let mut broker = start().await;
    let sock = broker.sock.clone();
    let mut ended: Vec<BrokerStream> = Vec::new();
    for round in 0..3u16 {
        let opened = broker.ask(open("t", "cat", &[])).await;
        assert!(
            matches!(opened, Response::Terminal { ref info } if info.spawned == Some(true) && info.alive),
            "round {round}: {opened:?}"
        );
        let (mut driver, _, _) = attach(&sock, "t").await;
        let (mut watcher, _, _) = attach(&sock, "t").await;
        let cols = 90 + round;
        driver.send(&resize("driver", cols, 30)).await.unwrap();
        for viewer in [&mut driver, &mut watcher] {
            match next_frame(viewer).await {
                Some(StreamOut::Size {
                    cols: got,
                    rows: 30,
                    ..
                }) => assert_eq!(got, cols, "round {round}"),
                other => panic!("round {round}: expected the claimed size, got {other:?}"),
            }
        }
        // Streams ended in earlier rounds stay ended across this reopen.
        for old in &mut ended {
            assert!(next_frame(old).await.is_none(), "round {round}");
        }

        assert!(matches!(broker.kill("t").await, Response::Ok));
        for viewer in [&mut driver, &mut watcher] {
            read_to_end(viewer).await;
            let _ = viewer.send(&resize("driver", 120, 40)).await;
            assert!(next_frame(viewer).await.is_none(), "round {round}");
        }
        assert!(matches!(broker.kill("t").await, Response::NotFound));
        // Reattaching to the killed id is refused, then ends.
        let mut refused = BrokerStream::open(&sock, "t").await.unwrap();
        assert!(matches!(
            next_frame(&mut refused).await,
            Some(StreamOut::NotFound)
        ));
        assert!(next_frame(&mut refused).await.is_none());
        ended.push(driver);
        ended.push(watcher);
    }
    assert!(broker.ids().await.is_empty());
    broker.assert_serving().await;
}

/// HS2-QWT94V: `kill` ends attached streams even while a grandchild that ignores SIGHUP keeps
/// the PTY open. On Linux the drain then never reaches EOF on its own, so only the kill's
/// explicit end ends the stream; macOS revokes the terminal when its session leader dies, so
/// there the drain's EOF ends it too.
#[tokio::test]
async fn hs2_qwt94v_kill_ends_the_stream_while_a_grandchild_holds_the_pty() {
    let mut broker = start().await;
    let script = "(trap '' HUP; exec sleep 60) & printf 'holder=%s;' \"$!\"; wait";
    broker.ask(open("g", "sh", &["-c", script])).await;
    let sock = broker.sock.clone();
    let (mut viewer, replay, _) = attach(&sock, "g").await;
    let mut seen = String::from_utf8_lossy(&replay).into_owned();
    while !seen.contains(';') {
        match next_frame(&mut viewer).await {
            Some(StreamOut::Output { data }) => {
                seen.push_str(&String::from_utf8_lossy(&data.into_vec()));
            }
            Some(StreamOut::Size { .. }) => {}
            other => panic!("expected the holder's pid, got {other:?}"),
        }
    }
    let pid: libc::pid_t = seen
        .split("holder=")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .and_then(|pid| pid.trim().parse().ok())
        .expect("the holder's pid");

    assert!(matches!(broker.kill("g").await, Response::Ok));
    read_to_end(&mut viewer).await;
    let _ = viewer.send(&resize("late", 100, 30)).await;
    assert!(next_frame(&mut viewer).await.is_none());
    // SAFETY: signals the test's own grandchild by the pid it printed.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    broker.assert_serving().await;
}
