//! The plugin side of the protocol (docs/38 §38.5–§38.6): answer `$/hello`, then run each
//! request on its own thread so slow calls never block cancels or other calls. Streaming
//! calls send items through [`CallContext::send_item`] before returning. `$/shutdown`
//! is answered after in-flight calls finish; end of input ends the loop the same way.
//!
//! HS2-0M14YS's SDK wraps this loop with typed per-capability traits.

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::handshake::{self, HandshakeError};
use crate::wire::{Message, MessageReader, Payload, Transport, write_message};
use crate::{PROTOCOL_VERSION, PluginError, SHUTDOWN_METHOD, v1};

/// What a plugin tells the host in its handshake answer.
#[derive(Clone, Debug)]
pub struct ServeOptions {
    /// The plugin's own version.
    pub version: String,
    /// The capabilities it implements; must match its manifest.
    pub capabilities: Vec<v1::Capability>,
    /// Transports it speaks. The host's preference order decides among them.
    pub transports: Vec<Transport>,
}

/// A plugin's request handler.
pub trait Handler: Send + Sync + 'static {
    /// Handles one call. Unknown methods should return [`PluginError::unsupported`].
    fn call(
        &self,
        method: &str,
        request: Payload,
        ctx: &CallContext,
    ) -> Result<Payload, PluginError>;
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

#[derive(Default)]
struct CancelState {
    cancelled: Mutex<bool>,
    changed: Condvar,
}

/// The context of one call: its transport, stream output, cancellation and deadline.
pub struct CallContext {
    id: u64,
    transport: Transport,
    out: SharedWriter,
    cancel: Arc<CancelState>,
    deadline: Option<Instant>,
}

impl CallContext {
    /// The request id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The negotiated transport.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// Encodes a reply for this call's transport.
    pub fn reply<T: prost::Message + Serialize>(&self, message: &T) -> Payload {
        Payload::encode(self.transport, message)
    }

    /// Sends one stream item for this call.
    pub fn send_item<T: prost::Message + Serialize>(&self, item: &T) -> io::Result<()> {
        send(
            &self.out,
            self.transport,
            &Message::StreamItem {
                id: self.id,
                payload: self.reply(item),
            },
        )
    }

    /// Sends a log line to the host.
    pub fn log(&self, level: &str, message: impl Into<String>) -> io::Result<()> {
        send(
            &self.out,
            self.transport,
            &Message::Log {
                level: level.to_string(),
                message: message.into(),
            },
        )
    }

    /// Whether the host cancelled this call.
    pub fn is_cancelled(&self) -> bool {
        *lock(&self.cancel.cancelled)
    }

    /// Waits up to `timeout` for a cancel, without polling. Returns whether one came.
    pub fn wait_cancelled(&self, timeout: Duration) -> bool {
        let guard = lock(&self.cancel.cancelled);
        let (guard, _) = self
            .cancel
            .changed
            .wait_timeout_while(guard, timeout, |cancelled| !*cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }

    /// When the host stops waiting, if it set a deadline.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send(out: &SharedWriter, transport: Transport, message: &Message) -> io::Result<()> {
    let mut out = lock(out);
    write_message(&mut *out, transport, message)?;
    out.flush()
}

/// Runs the plugin side until `$/shutdown` or end of input.
pub fn serve(
    input: impl Read,
    output: impl Write + Send + 'static,
    options: &ServeOptions,
    handler: Arc<dyn Handler>,
) -> Result<(), HandshakeError> {
    let mut reader = MessageReader::new(BufReader::new(input), Transport::Json);
    let out: SharedWriter = Arc::new(Mutex::new(Box::new(io::BufWriter::new(output))));
    let hello = handshake::read_hello(&mut reader)?;
    let transport = match handshake::choose(&hello, &options.transports) {
        Ok(transport) => transport,
        Err(error) => {
            handshake::write_hello_ack(&mut *lock(&out), Err(&error))?;
            return Err(HandshakeError::Refused(error));
        }
    };
    let ack = v1::HelloAck {
        protocol_version: PROTOCOL_VERSION,
        transport: transport.name().to_string(),
        plugin: Some(v1::PluginInfo {
            version: options.version.clone(),
        }),
        capabilities: options.capabilities.clone(),
    };
    handshake::write_hello_ack(&mut *lock(&out), Ok(&ack))?;
    reader.set_transport(transport);

    let calls: Arc<Mutex<HashMap<u64, Arc<CancelState>>>> = Arc::default();
    let mut running: Vec<JoinHandle<()>> = Vec::new();
    loop {
        running.retain(|handle| !handle.is_finished());
        let Some(message) = reader.read()? else {
            break;
        };
        match message {
            Message::Request { id, method, .. } if method == SHUTDOWN_METHOD => {
                for handle in running.drain(..) {
                    let _ = handle.join(); // a panicking call already answered `internal`
                }
                send(
                    &out,
                    transport,
                    &Message::Response {
                        id,
                        outcome: Ok(Payload::empty(transport)),
                    },
                )?;
                return Ok(());
            }
            Message::Request {
                id,
                method,
                payload,
                deadline_ms,
            } => {
                let cancel = Arc::new(CancelState::default());
                lock(&calls).insert(id, cancel.clone());
                let ctx = CallContext {
                    id,
                    transport,
                    out: out.clone(),
                    cancel,
                    deadline: (deadline_ms > 0)
                        .then(|| Instant::now() + Duration::from_millis(deadline_ms)),
                };
                let handler = handler.clone();
                let calls = calls.clone();
                running.push(std::thread::spawn(move || {
                    let outcome =
                        catch_unwind(AssertUnwindSafe(|| handler.call(&method, payload, &ctx)))
                            .unwrap_or_else(|_| {
                                Err(PluginError::internal(format!("{method} panicked")))
                            });
                    lock(&calls).remove(&id);
                    // The host may already have gone; nothing else can be done with the error.
                    let _ = send(&ctx.out, transport, &Message::Response { id, outcome });
                }));
            }
            Message::Cancel { id } => {
                if let Some(state) = lock(&calls).get(&id) {
                    *lock(&state.cancelled) = true;
                    state.changed.notify_all();
                }
            }
            // Host-to-plugin answers and logs have no plugin-side consumer in v1.
            Message::Response { .. } | Message::StreamItem { .. } | Message::Log { .. } => {}
        }
    }
    for handle in running {
        let _ = handle.join(); // see above
    }
    Ok(())
}
