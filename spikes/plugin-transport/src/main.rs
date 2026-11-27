//! HS2-XE0ZVB transport benchmark. One binary is both the host (`bench`) and the
//! plugin (`plugin <json|proto|grpc> [socket]`), so every transport moves the same
//! ticket-shaped payloads. The host is synchronous, like Hot Sheet's provider trait;
//! the gRPC client therefore pays a `block_on` per call, as a real sync host would.

use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::pin::Pin;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use prost::Message;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

pub mod pb {
    tonic::include_proto!("bench");
}

const BODY_LEN: usize = 800;

fn body(i: u64) -> String {
    let base = format!("Ticket {i} body: steps to reproduce, expected and actual behaviour. ");
    let mut s = base.repeat(BODY_LEN / base.len() + 1);
    s.truncate(BODY_LEN);
    s
}

#[derive(Serialize, Deserialize)]
struct JTicket {
    native_id: String,
    title: String,
    body: String,
    labels: Vec<String>,
    status: String,
    updated_at: i64,
}

fn jticket(i: u64) -> JTicket {
    JTicket {
        native_id: i.to_string(),
        title: format!("Ticket number {i} has a moderately descriptive title"),
        body: body(i),
        labels: vec!["bug".into(), "area:sync".into(), "priority:high".into()],
        status: "open".into(),
        updated_at: 1_790_000_000 + i as i64,
    }
}

fn pticket(i: u64) -> pb::Ticket {
    let t = jticket(i);
    pb::Ticket {
        native_id: t.native_id,
        title: t.title,
        body: t.body,
        labels: t.labels,
        status: t.status,
        updated_at: t.updated_at,
    }
}

fn blob(size: u32) -> Vec<u8> {
    (0..size)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect()
}

#[derive(Serialize, Deserialize)]
struct JEvent {
    seq: u64,
    kind: String,
    native_id: String,
}

fn jevent(seq: u64) -> JEvent {
    JEvent {
        seq,
        kind: "changed".into(),
        native_id: (seq % 5000).to_string(),
    }
}

// ---------------------------------------------------------------- JSON-RPC / stdio

#[derive(Serialize)]
struct RespOut<'a, T: Serialize> {
    jsonrpc: &'static str,
    id: &'a Value,
    result: T,
}

fn write_line<T: Serialize>(out: &mut impl Write, v: &T) {
    serde_json::to_writer(&mut *out, v).unwrap();
    out.write_all(b"\n").unwrap();
}

fn json_plugin() {
    let stdin = std::io::stdin().lock();
    let mut out = BufWriter::new(std::io::stdout().lock());
    for line in stdin.lines() {
        let line = line.unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let id = &req["id"];
        let p = &req["params"];
        match req["method"].as_str().unwrap() {
            "get" => {
                let n: u64 = p["native_id"].as_str().unwrap().parse().unwrap();
                write_line(
                    &mut out,
                    &RespOut {
                        jsonrpc: "2.0",
                        id,
                        result: jticket(n),
                    },
                );
            }
            "list" => {
                let c = p["count"].as_u64().unwrap();
                let tickets: Vec<JTicket> = (0..c).map(jticket).collect();
                write_line(
                    &mut out,
                    &RespOut {
                        jsonrpc: "2.0",
                        id,
                        result: json!({ "tickets": tickets }),
                    },
                );
            }
            "blob" => {
                let data = base64::engine::general_purpose::STANDARD
                    .encode(blob(p["size"].as_u64().unwrap() as u32));
                write_line(
                    &mut out,
                    &RespOut {
                        jsonrpc: "2.0",
                        id,
                        result: json!({ "data": data }),
                    },
                );
            }
            "stream" => {
                #[derive(Serialize)]
                struct Notif {
                    jsonrpc: &'static str,
                    method: &'static str,
                    params: JEvent,
                }
                for seq in 0..p["count"].as_u64().unwrap() {
                    write_line(
                        &mut out,
                        &Notif {
                            jsonrpc: "2.0",
                            method: "event",
                            params: jevent(seq),
                        },
                    );
                }
                write_line(
                    &mut out,
                    &RespOut {
                        jsonrpc: "2.0",
                        id,
                        result: json!({ "done": true }),
                    },
                );
            }
            "shutdown" => return,
            m => panic!("unknown method {m}"),
        }
        out.flush().unwrap();
    }
}

struct JsonClient {
    child: Child,
    w: BufWriter<ChildStdin>,
    r: BufReader<ChildStdout>,
    next: u64,
    line: String,
}

#[derive(Deserialize)]
struct RespIn<T> {
    result: T,
}

#[derive(Deserialize)]
struct ListIn {
    tickets: Vec<JTicket>,
}

#[derive(Deserialize)]
struct BlobIn {
    data: String,
}

#[derive(Deserialize)]
struct StreamLine {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    params: Option<JEvent>,
}

impl JsonClient {
    fn spawn() -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["plugin", "json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let w = BufWriter::new(child.stdin.take().unwrap());
        let r = BufReader::with_capacity(1 << 16, child.stdout.take().unwrap());
        JsonClient {
            child,
            w,
            r,
            next: 0,
            line: String::new(),
        }
    }

    fn send(&mut self, method: &str, params: Value) {
        self.next += 1;
        write_line(
            &mut self.w,
            &json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params }),
        );
        self.w.flush().unwrap();
    }

    fn read(&mut self) -> &str {
        self.line.clear();
        self.r.read_line(&mut self.line).unwrap();
        &self.line
    }

    fn call<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> T {
        self.send(method, params);
        let line = self.read().to_owned();
        serde_json::from_str::<RespIn<T>>(&line).unwrap().result
    }
}

impl Client for JsonClient {
    fn get(&mut self, id: u64) -> usize {
        let t: JTicket = self.call("get", json!({ "native_id": id.to_string() }));
        t.body.len()
    }
    fn list(&mut self, n: u32) -> usize {
        let l: ListIn = self.call("list", json!({ "count": n }));
        l.tickets.len()
    }
    fn blob(&mut self, size: u32) -> usize {
        let b: BlobIn = self.call("blob", json!({ "size": size }));
        base64::engine::general_purpose::STANDARD
            .decode(b.data)
            .unwrap()
            .len()
    }
    fn stream(&mut self, n: u32) -> usize {
        self.send("stream", json!({ "count": n }));
        let mut events = 0;
        loop {
            self.line.clear();
            self.r.read_line(&mut self.line).unwrap();
            let l: StreamLine = serde_json::from_str(&self.line).unwrap();
            if l.params.is_some() {
                events += 1;
            } else if l.id.is_some() {
                return events;
            }
        }
    }
    fn close(mut self: Box<Self>) {
        self.send("shutdown", json!({}));
        let _ = self.child.wait();
    }
}

// ------------------------------------------------- length-prefixed protobuf / stdio

const M_GET: u8 = 1;
const M_LIST: u8 = 2;
const M_BLOB: u8 = 3;
const M_STREAM: u8 = 4;
const M_SHUTDOWN: u8 = 5;

fn write_frame(out: &mut impl Write, bytes: &[u8]) {
    out.write_all(&(bytes.len() as u32).to_le_bytes()).unwrap();
    out.write_all(bytes).unwrap();
}

fn read_frame(r: &mut impl Read, buf: &mut Vec<u8>) {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).unwrap();
    buf.resize(u32::from_le_bytes(len) as usize, 0);
    r.read_exact(buf).unwrap();
}

fn proto_plugin() {
    let mut inp = BufReader::new(std::io::stdin().lock());
    let mut out = BufWriter::new(std::io::stdout().lock());
    let mut buf = Vec::new();
    loop {
        let mut m = [0u8; 1];
        if inp.read_exact(&mut m).is_err() {
            return;
        }
        read_frame(&mut inp, &mut buf);
        match m[0] {
            M_GET => {
                let r = pb::GetReq::decode(&buf[..]).unwrap();
                write_frame(
                    &mut out,
                    &pticket(r.native_id.parse().unwrap()).encode_to_vec(),
                );
            }
            M_LIST => {
                let r = pb::ListReq::decode(&buf[..]).unwrap();
                let resp = pb::ListResp {
                    tickets: (0..r.count as u64).map(pticket).collect(),
                };
                write_frame(&mut out, &resp.encode_to_vec());
            }
            M_BLOB => {
                let r = pb::BlobReq::decode(&buf[..]).unwrap();
                write_frame(
                    &mut out,
                    &pb::BlobResp { data: blob(r.size) }.encode_to_vec(),
                );
            }
            M_STREAM => {
                let r = pb::StreamReq::decode(&buf[..]).unwrap();
                for seq in 0..r.count as u64 {
                    let e = jevent(seq);
                    write_frame(
                        &mut out,
                        &pb::Event {
                            seq: e.seq,
                            kind: e.kind,
                            native_id: e.native_id,
                        }
                        .encode_to_vec(),
                    );
                }
                write_frame(&mut out, &[]);
            }
            M_SHUTDOWN => return,
            x => panic!("unknown method {x}"),
        }
        out.flush().unwrap();
    }
}

struct ProtoClient {
    child: Child,
    w: BufWriter<ChildStdin>,
    r: BufReader<ChildStdout>,
    buf: Vec<u8>,
}

impl ProtoClient {
    fn spawn() -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["plugin", "proto"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let w = BufWriter::new(child.stdin.take().unwrap());
        let r = BufReader::with_capacity(1 << 16, child.stdout.take().unwrap());
        ProtoClient {
            child,
            w,
            r,
            buf: Vec::new(),
        }
    }

    fn send(&mut self, m: u8, msg: &impl Message) {
        self.w.write_all(&[m]).unwrap();
        write_frame(&mut self.w, &msg.encode_to_vec());
        self.w.flush().unwrap();
    }

    fn call<T: Message + Default>(&mut self, m: u8, msg: &impl Message) -> T {
        self.send(m, msg);
        read_frame(&mut self.r, &mut self.buf);
        T::decode(&self.buf[..]).unwrap()
    }
}

impl Client for ProtoClient {
    fn get(&mut self, id: u64) -> usize {
        let t: pb::Ticket = self.call(
            M_GET,
            &pb::GetReq {
                native_id: id.to_string(),
            },
        );
        t.body.len()
    }
    fn list(&mut self, n: u32) -> usize {
        let l: pb::ListResp = self.call(M_LIST, &pb::ListReq { count: n });
        l.tickets.len()
    }
    fn blob(&mut self, size: u32) -> usize {
        let b: pb::BlobResp = self.call(M_BLOB, &pb::BlobReq { size });
        b.data.len()
    }
    fn stream(&mut self, n: u32) -> usize {
        self.send(M_STREAM, &pb::StreamReq { count: n });
        let mut events = 0;
        loop {
            read_frame(&mut self.r, &mut self.buf);
            if self.buf.is_empty() {
                return events;
            }
            let _e = pb::Event::decode(&self.buf[..]).unwrap();
            events += 1;
        }
    }
    fn close(mut self: Box<Self>) {
        self.send(M_SHUTDOWN, &pb::GetReq::default());
        let _ = self.child.wait();
    }
}

// --------------------------------------------------------------------- gRPC / UDS

const GRPC_MAX: usize = 256 << 20;

struct Svc;

type EventStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::Event, tonic::Status>> + Send>>;

#[tonic::async_trait]
impl pb::bench_server::Bench for Svc {
    async fn get(
        &self,
        r: tonic::Request<pb::GetReq>,
    ) -> Result<tonic::Response<pb::Ticket>, tonic::Status> {
        Ok(tonic::Response::new(pticket(
            r.into_inner().native_id.parse().unwrap(),
        )))
    }
    async fn list(
        &self,
        r: tonic::Request<pb::ListReq>,
    ) -> Result<tonic::Response<pb::ListResp>, tonic::Status> {
        let c = r.into_inner().count as u64;
        Ok(tonic::Response::new(pb::ListResp {
            tickets: (0..c).map(pticket).collect(),
        }))
    }
    async fn blob(
        &self,
        r: tonic::Request<pb::BlobReq>,
    ) -> Result<tonic::Response<pb::BlobResp>, tonic::Status> {
        Ok(tonic::Response::new(pb::BlobResp {
            data: blob(r.into_inner().size),
        }))
    }
    type StreamStream = EventStream;
    async fn stream(
        &self,
        r: tonic::Request<pb::StreamReq>,
    ) -> Result<tonic::Response<EventStream>, tonic::Status> {
        let c = r.into_inner().count as u64;
        let it = (0..c).map(|s| {
            let e = jevent(s);
            Ok(pb::Event {
                seq: e.seq,
                kind: e.kind,
                native_id: e.native_id,
            })
        });
        Ok(tonic::Response::new(Box::pin(tokio_stream::iter(it))))
    }
}

fn grpc_plugin(sock: &str) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let _ = std::fs::remove_file(sock);
        let l = tokio::net::UnixListener::bind(sock).unwrap();
        println!("ready");
        std::io::stdout().flush().unwrap();
        let svc = pb::bench_server::BenchServer::new(Svc)
            .max_decoding_message_size(GRPC_MAX)
            .max_encoding_message_size(GRPC_MAX);
        tonic::transport::Server::builder()
            .add_service(svc)
            .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(l))
            .await
            .unwrap();
    });
}

struct GrpcClient {
    child: Child,
    rt: tokio::runtime::Runtime,
    c: pb::bench_client::BenchClient<tonic::transport::Channel>,
}

impl GrpcClient {
    fn spawn() -> Self {
        let sock = std::env::temp_dir().join(format!("hs2-bench-{}.sock", std::process::id()));
        let sock_s = sock.to_string_lossy().into_owned();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["plugin", "grpc", &sock_s])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "ready");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let ch = rt
            .block_on(
                tonic::transport::Endpoint::try_from("http://[::]:50051")
                    .unwrap()
                    .connect_with_connector(tower::service_fn(move |_: tonic::transport::Uri| {
                        let p = sock.clone();
                        async move {
                            Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(
                                tokio::net::UnixStream::connect(p).await?,
                            ))
                        }
                    })),
            )
            .unwrap();
        let c = pb::bench_client::BenchClient::new(ch)
            .max_decoding_message_size(GRPC_MAX)
            .max_encoding_message_size(GRPC_MAX);
        GrpcClient { child, rt, c }
    }
}

impl Client for GrpcClient {
    fn get(&mut self, id: u64) -> usize {
        let c = &mut self.c;
        self.rt
            .block_on(c.get(pb::GetReq {
                native_id: id.to_string(),
            }))
            .unwrap()
            .into_inner()
            .body
            .len()
    }
    fn list(&mut self, n: u32) -> usize {
        let c = &mut self.c;
        self.rt
            .block_on(c.list(pb::ListReq { count: n }))
            .unwrap()
            .into_inner()
            .tickets
            .len()
    }
    fn blob(&mut self, size: u32) -> usize {
        let c = &mut self.c;
        self.rt
            .block_on(c.blob(pb::BlobReq { size }))
            .unwrap()
            .into_inner()
            .data
            .len()
    }
    fn stream(&mut self, n: u32) -> usize {
        let c = &mut self.c;
        self.rt.block_on(async {
            let mut s = c
                .stream(pb::StreamReq { count: n })
                .await
                .unwrap()
                .into_inner();
            let mut events = 0;
            while let Some(e) = s.message().await.unwrap() {
                let _ = e;
                events += 1;
            }
            events
        })
    }
    fn close(mut self: Box<Self>) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ------------------------------------------------------------------------ harness

trait Client {
    fn get(&mut self, id: u64) -> usize;
    fn list(&mut self, n: u32) -> usize;
    fn blob(&mut self, size: u32) -> usize;
    fn stream(&mut self, n: u32) -> usize;
    fn close(self: Box<Self>);
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

const GETS: usize = 20_000;
const LIST_N: u32 = 10_000;
const BLOB: u32 = 16 << 20;
const EVENTS: u32 = 100_000;

fn run(name: &str, mk: fn() -> Box<dyn Client>) {
    // Start-up: spawn to first answered call, median of 10 fresh plugins.
    let startup = median(
        (0..10)
            .map(|_| {
                let t = Instant::now();
                let mut c = mk();
                assert_eq!(c.get(1), BODY_LEN);
                let d = t.elapsed();
                c.close();
                d
            })
            .collect(),
    );

    let mut c = mk();
    for i in 0..1000 {
        c.get(i);
    }
    let mut lat: Vec<Duration> = (0..GETS as u64)
        .map(|i| {
            let t = Instant::now();
            c.get(i);
            t.elapsed()
        })
        .collect();
    lat.sort();
    let mean = lat.iter().sum::<Duration>() / GETS as u32;
    let p50 = lat[GETS / 2];
    let p99 = lat[GETS * 99 / 100];

    let list = median(
        (0..5)
            .map(|_| {
                let t = Instant::now();
                assert_eq!(c.list(LIST_N), LIST_N as usize);
                t.elapsed()
            })
            .collect(),
    );
    let blob = median(
        (0..5)
            .map(|_| {
                let t = Instant::now();
                assert_eq!(c.blob(BLOB), BLOB as usize);
                t.elapsed()
            })
            .collect(),
    );
    let stream = median(
        (0..3)
            .map(|_| {
                let t = Instant::now();
                assert_eq!(c.stream(EVENTS), EVENTS as usize);
                t.elapsed()
            })
            .collect(),
    );
    c.close();

    println!(
        "| {name} | {:.0} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.0} | {:.0} |",
        ms(startup) * 1000.0,
        us(mean),
        us(p50),
        us(p99),
        ms(list),
        ms(blob),
        (BLOB as f64 / (1 << 20) as f64) / blob.as_secs_f64(),
        EVENTS as f64 / stream.as_secs_f64() / 1000.0,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("plugin") => match args[2].as_str() {
            "json" => json_plugin(),
            "proto" => proto_plugin(),
            "grpc" => grpc_plugin(&args[3]),
            t => panic!("unknown transport {t}"),
        },
        _ => {
            println!(
                "| transport | startup µs | get mean µs | get p50 µs | get p99 µs | list {LIST_N} ms | blob 16MiB ms | blob MiB/s | stream k events/s |"
            );
            println!("|---|---|---|---|---|---|---|---|---|");
            run("JSON-RPC / stdio", || Box::new(JsonClient::spawn()));
            run("protobuf / stdio", || Box::new(ProtoClient::spawn()));
            run("gRPC / UDS", || Box::new(GrpcClient::spawn()));
        }
    }
}
