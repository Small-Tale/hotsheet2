//! HS2-XY0VTN: the plugin-side loop over real pipes — handshake, both transports,
//! concurrent calls, streams, cancellation, errors and shutdown (docs/38 §38.5).

use std::io::{self, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use hotsheet_plugin_protocol::diagnostics::{CAPABILITY, DiagnosticsHandler, method};
use hotsheet_plugin_protocol::handshake::{self, HandshakeError};
use hotsheet_plugin_protocol::serve::{ServeOptions, serve};
use hotsheet_plugin_protocol::wire::{MessageReader, write_message};
use hotsheet_plugin_protocol::{
    Message, PROTOCOL_VERSION, Payload, PluginError, SHUTDOWN_METHOD, Transport, code, v1,
};

/// The write end of an in-process byte pipe; dropping it ends the reader's input.
/// (`std::io::pipe` needs Rust 1.87; the workspace MSRV is 1.85.)
struct PipeWriter(Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .send(buf.to_vec())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PipeReader {
    chunks: Receiver<Vec<u8>>,
    chunk: Vec<u8>,
    pos: usize,
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.chunk.len() {
            match self.chunks.recv() {
                Ok(chunk) => {
                    self.chunk = chunk;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.pos);
        buf[..n].copy_from_slice(&self.chunk[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn pipe() -> (PipeReader, PipeWriter) {
    let (tx, rx) = channel();
    (
        PipeReader {
            chunks: rx,
            chunk: Vec::new(),
            pos: 0,
        },
        PipeWriter(tx),
    )
}

type Started = (
    PipeWriter,
    MessageReader<BufReader<PipeReader>>,
    JoinHandle<Result<(), HandshakeError>>,
    Result<v1::HelloAck, HandshakeError>,
);

struct Host {
    out: PipeWriter,
    reader: MessageReader<BufReader<PipeReader>>,
    transport: Transport,
    next: u64,
    plugin: JoinHandle<Result<(), HandshakeError>>,
}

fn hello(transports: &[&str], versions: &[u32]) -> v1::Hello {
    v1::Hello {
        protocol_versions: versions.to_vec(),
        transports: transports.iter().map(|t| t.to_string()).collect(),
        host: Some(v1::HostInfo {
            name: "test".into(),
            version: "0".into(),
        }),
        plugin_id: "diagnostics".into(),
        data_dir: String::new(),
    }
}

fn options(transports: Vec<Transport>) -> ServeOptions {
    ServeOptions {
        version: "1.2.3".into(),
        capabilities: vec![v1::Capability {
            name: CAPABILITY.into(),
            features: vec![],
        }],
        transports,
    }
}

/// Starts the plugin loop and sends `hello`; returns the host side and the plugin's answer.
fn start(hello: &v1::Hello, supported: Vec<Transport>) -> Started {
    let (plugin_in, mut host_out) = pipe();
    let (host_in, plugin_out) = pipe();
    let options = options(supported);
    let plugin = std::thread::spawn(move || {
        serve(
            plugin_in,
            plugin_out,
            &options,
            Arc::new(DiagnosticsHandler),
        )
    });
    handshake::write_hello(&mut host_out, hello).expect("hello");
    let mut reader = MessageReader::new(BufReader::new(host_in), Transport::Json);
    let mut logs = Vec::new();
    let ack = handshake::read_hello_ack(&mut reader, &mut logs);
    (host_out, reader, plugin, ack)
}

fn connect(preferred: &str) -> Host {
    let hello = hello(&[preferred, "json"], &[PROTOCOL_VERSION]);
    let (out, mut reader, plugin, ack) = start(&hello, vec![Transport::Protobuf, Transport::Json]);
    let ack = ack.expect("ack");
    assert_eq!(ack.plugin.as_ref().expect("plugin info").version, "1.2.3");
    let transport =
        handshake::accept_ack(&hello, &ack, &[CAPABILITY.to_string()]).expect("accepted");
    reader.set_transport(transport);
    Host {
        out,
        reader,
        transport,
        next: 1,
        plugin,
    }
}

impl Host {
    fn send<T: prost::Message + serde::Serialize>(&mut self, method: &str, request: &T) -> u64 {
        let id = self.next;
        self.next += 1;
        let message = Message::Request {
            id,
            method: method.into(),
            payload: Payload::encode(self.transport, request),
            deadline_ms: 0,
        };
        write_message(&mut self.out, self.transport, &message).expect("send");
        self.out.flush().expect("flush");
        id
    }

    fn cancel(&mut self, id: u64) {
        write_message(&mut self.out, self.transport, &Message::Cancel { id }).expect("cancel");
        self.out.flush().expect("flush");
    }

    fn next_message(&mut self) -> Message {
        self.reader
            .read()
            .expect("read")
            .expect("plugin still open")
    }

    /// The final response to `id`, with any stream items that came first.
    fn finish(&mut self, id: u64) -> (Vec<Payload>, Result<Payload, PluginError>) {
        let mut items = Vec::new();
        loop {
            match self.next_message() {
                Message::StreamItem {
                    id: item_id,
                    payload,
                } if item_id == id => items.push(payload),
                Message::Response { id: done, outcome } if done == id => return (items, outcome),
                other => panic!("unexpected {other:?} while waiting for {id}"),
            }
        }
    }

    fn shutdown(mut self) {
        let id = self.send(SHUTDOWN_METHOD, &v1::Empty {});
        let (_, outcome) = self.finish(id);
        outcome.expect("shutdown answered");
        self.plugin
            .join()
            .expect("plugin thread")
            .expect("clean exit");
    }
}

fn each_transport(test: impl Fn(Host)) {
    for preferred in ["protobuf", "json"] {
        let host = connect(preferred);
        assert_eq!(host.transport.name(), preferred);
        test(host);
    }
}

#[test]
fn hs2_xy0vtn_echo_round_trips_bytes_and_text() {
    each_transport(|mut host| {
        let id = host.send(
            method::ECHO,
            &v1::EchoRequest {
                text: "héllo".into(),
                data: (0..=255).collect(),
            },
        );
        let (items, outcome) = host.finish(id);
        assert!(items.is_empty());
        let echoed: v1::EchoResponse = outcome.expect("ok").decode().expect("decodes");
        assert_eq!(echoed.text, "héllo");
        assert_eq!(echoed.data, (0..=255).collect::<Vec<u8>>());
        host.shutdown();
    });
}

#[test]
fn hs2_xy0vtn_streams_arrive_in_order_before_the_response() {
    each_transport(|mut host| {
        let id = host.send(method::COUNT, &v1::CountRequest { count: 500 });
        let (items, outcome) = host.finish(id);
        let seqs: Vec<u64> = items
            .iter()
            .map(|p| p.decode::<v1::CountItem>().expect("item").seq)
            .collect();
        assert_eq!(seqs, (0..500).collect::<Vec<_>>());
        let done: v1::CountResponse = outcome.expect("ok").decode().expect("decodes");
        assert_eq!(done.sent, 500);
        host.shutdown();
    });
}

#[test]
fn hs2_xy0vtn_a_slow_call_does_not_block_others_and_can_be_cancelled() {
    each_transport(|mut host| {
        let slow = host.send(method::SLEEP, &v1::SleepRequest { millis: 60_000 });
        let fast = host.send(
            method::ECHO,
            &v1::EchoRequest {
                text: "first".into(),
                data: vec![],
            },
        );
        // The echo answers while the sleep is still running.
        let (_, outcome) = host.finish(fast);
        assert_eq!(
            outcome
                .expect("ok")
                .decode::<v1::EchoResponse>()
                .expect("echo")
                .text,
            "first"
        );
        host.cancel(slow);
        let (_, outcome) = host.finish(slow);
        assert_eq!(outcome.expect_err("cancelled").code, code::CANCELLED);
        host.shutdown();
    });
}

#[test]
fn hs2_xy0vtn_errors_and_unknown_methods_reach_the_host_intact() {
    each_transport(|mut host| {
        let error = PluginError {
            code: code::CONFLICT.into(),
            message: "stale".into(),
            stale: true,
            ..Default::default()
        };
        let id = host.send(
            method::FAIL,
            &v1::FailRequest {
                error: Some(error.clone()),
            },
        );
        assert_eq!(host.finish(id).1.expect_err("fails"), error);
        let id = host.send("ticket_source.Get", &v1::Empty {});
        let unsupported = host.finish(id).1.expect_err("unsupported");
        assert_eq!(unsupported.code, code::UNSUPPORTED);
        assert_eq!(unsupported.capability, "ticket_source.Get");
        host.shutdown();
    });
}

#[test]
fn hs2_xy0vtn_end_of_input_ends_the_loop_after_in_flight_calls() {
    let mut host = connect("protobuf");
    let id = host.send(method::SLEEP, &v1::SleepRequest { millis: 20 });
    let Host {
        out,
        mut reader,
        plugin,
        ..
    } = host;
    drop(out);
    // The in-flight call still answers before the loop ends.
    assert!(
        matches!(reader.read(), Ok(Some(Message::Response { id: done, outcome: Ok(_) })) if done == id)
    );
    plugin.join().expect("plugin thread").expect("clean exit");
}

#[test]
fn hs2_xy0vtn_handshake_refuses_without_a_shared_transport_or_version() {
    let (_out, _reader, plugin, ack) = start(
        &hello(&["protobuf"], &[PROTOCOL_VERSION]),
        vec![Transport::Json],
    );
    assert!(matches!(ack, Err(HandshakeError::Refused(e)) if e.code == code::UNSUPPORTED));
    assert!(matches!(
        plugin.join().expect("thread"),
        Err(HandshakeError::Refused(_))
    ));

    let (_out, _reader, plugin, ack) = start(&hello(&["json"], &[99]), vec![Transport::Json]);
    assert!(
        matches!(ack, Err(HandshakeError::Refused(e)) if e.message.contains("protocol version"))
    );
    assert!(plugin.join().expect("thread").is_err());
}

#[test]
fn hs2_xy0vtn_host_rejects_an_answer_that_does_not_match_its_offer_or_manifest() {
    let offered = hello(&["json"], &[PROTOCOL_VERSION]);
    let ack = |version: u32, transport: &str, caps: &[&str]| v1::HelloAck {
        protocol_version: version,
        transport: transport.into(),
        plugin: None,
        capabilities: caps
            .iter()
            .map(|c| v1::Capability {
                name: c.to_string(),
                features: vec![],
            })
            .collect(),
    };
    let declared = ["ticket_source".to_string()];
    assert_eq!(
        handshake::accept_ack(&offered, &ack(1, "json", &["ticket_source"]), &declared)
            .expect("ok"),
        Transport::Json
    );
    for bad in [
        ack(2, "json", &["ticket_source"]),
        ack(1, "protobuf", &["ticket_source"]),
        ack(1, "smoke-signals", &["ticket_source"]),
        ack(1, "json", &["ticket_source", "notifier"]),
        ack(1, "json", &[]),
    ] {
        assert!(matches!(
            handshake::accept_ack(&offered, &bad, &declared),
            Err(HandshakeError::Incompatible(_))
        ));
    }
}
