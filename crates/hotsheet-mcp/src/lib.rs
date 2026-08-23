//! MCP shim: a stdio JSON-RPC 2.0 server exposing the `hotsheet_*` tools
//! (`docs/05-ai-tool-plugins.md` §5.8). Per-project — an AI tool spawns it from its
//! own config so the tool config owns which project it reaches.
//!
//! Two backends implement the same [`Backend`] trait, so the tool surface is
//! identical either way (this is what makes a headless, server-less agent work):
//! - [`CoreBackend`] — **direct to disk** over `hotsheet_ticketing::ops`, no server
//!   required (symmetric with the CLI's direct-to-disk path). The default.
//! - [`HttpBackend`] — proxies a running `hotsheet-server` over HTTP, for
//!   index-backed reads + instant broadcast when a server is up.

use serde_json::{Value, json};

/// The MCP protocol revision we speak.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// The HTTP backend a tool call proxies to (real in the binary; faked in tests).
pub trait Backend {
    fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError>;
    fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError>;
}

/// A failed backend call: an HTTP status (when the server answered) + a message.
#[derive(Debug)]
pub struct BackendError {
    pub status: Option<u16>,
    pub message: String,
}

/// Handle one JSON-RPC message. Returns the response value, or `None` for a
/// notification (no `id`). Transport-level infallible.
pub fn handle_message(msg: &Value, backend: &dyn Backend) -> Option<Value> {
    let id = msg.get("id").cloned()?; // notifications (no id) get no reply
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => Ok(initialize_result()),
        "tools/list" => Ok(tools_list()),
        "tools/call" => call_tool(msg.get("params"), backend),
        "ping" => Ok(json!({})),
        other => Err(RpcError::method_not_found(other)),
    };
    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(e) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message } })
        }
    })
}

struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn method_not_found(m: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {m}"),
        }
    }
    fn invalid_params(m: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: m.into(),
        }
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "hotsheet-mcp", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn tools_list() -> Value {
    let str_prop = |desc: &str| json!({ "type": "string", "description": desc });
    json!({ "tools": [
        {
            "name": "hotsheet_query",
            "description": "List/filter tickets (status, priority, category, tags, text, up_next, open, close_reason, closed, sort). Returns compact rows (no Markdown body) by default — pass compact=false for bodies, or use hotsheet_get for one ticket. Use limit to cap results.",
            "inputSchema": { "type": "object", "properties": {
                "status": str_prop("filter by status"),
                "priority": str_prop("filter by priority"),
                "category": str_prop("filter by category"),
                "tags": str_prop("comma-separated tags the ticket must all carry"),
                "text": str_prop("substring across title/details/notes"),
                "up_next": { "type": "boolean" },
                "open": { "type": "boolean" },
                "close_reason": str_prop("filter by close reason (completed|not_planned|duplicate|obsolete)"),
                "closed": { "type": "boolean", "description": "true = only closed tickets (a close_reason is set); false = only tickets with none" },
                "assignee": str_prop("filter to tickets assigned to this person (git email, or 'me' for your git identity)"),
                "review_requested": str_prop("filter to tickets with a review request for this person (git email, or 'me')"),
                "claimed": { "type": "boolean", "description": "true = only claimed tickets; false = only unclaimed" },
                "blocked": { "type": "boolean", "description": "true = only blocked tickets (a blocker isn't done); false = only unblocked" },
                "created_after": str_prop("only tickets created at/after this ISO-8601 time"),
                "created_before": str_prop("only tickets created at/before this ISO-8601 time"),
                "updated_after": str_prop("only tickets updated at/after this ISO-8601 time"),
                "updated_before": str_prop("only tickets updated at/before this ISO-8601 time"),
                "sort": str_prop("id|created|updated|priority|status|title"),
                "limit": { "type": "integer", "description": "cap the number of rows returned (after sort)" },
                "page_after": str_prop("keyset cursor: a ULID; return only rows strictly after it in sort order (page a large store without OFFSET)"),
                "fields": str_prop("comma-separated field allow-list for a leaner row (e.g. 'slug,status,up_next,title'); slug is always kept"),
                "compact": { "type": "boolean", "description": "omit the Markdown body from each row (default true)" }
            } }
        },
        {
            "name": "hotsheet_get",
            "description": "Fetch one ticket by slug or ULID.",
            "inputSchema": { "type": "object", "properties": { "id": str_prop("slug or ULID") }, "required": ["id"] }
        },
        {
            "name": "hotsheet_create",
            "description": "Create a ticket.",
            "inputSchema": { "type": "object", "properties": {
                "title": str_prop("required"),
                "category": str_prop(""), "priority": str_prop(""),
                "details": str_prop(""), "tags": { "type": "array", "items": { "type": "string" } },
                "up_next": { "type": "boolean" },
                "blocked_by": { "type": "array", "items": { "type": "string" }, "description": "blocker tickets (slug or ULID)" }
            }, "required": ["title"] }
        },
        {
            "name": "hotsheet_update",
            "description": "Update a ticket's fields (title/details/category/priority/status/tags/up_next/blocked_by) and/or append a note.",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("slug or ULID"),
                "title": str_prop(""), "details": str_prop(""), "category": str_prop(""),
                "priority": str_prop(""), "status": str_prop(""),
                "tags": { "type": "array", "items": { "type": "string" } }, "up_next": { "type": "boolean" },
                "blocked_by": { "type": "array", "items": { "type": "string" }, "description": "replace the blocker set (slug or ULID); [] clears it" },
                "note": str_prop("append a progress note")
            }, "required": ["id"] }
        },
        {
            "name": "hotsheet_close",
            "description": "Record a close outcome (completed|not_planned|duplicate|obsolete).",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("slug or ULID"),
                "reason": str_prop("completed|not_planned|duplicate|obsolete"),
                "duplicate_of": str_prop("required when reason=duplicate")
            }, "required": ["id", "reason"] }
        },
        {
            "name": "hotsheet_batch",
            "description": "Apply the same field update (status/priority/tags/up_next/category) to many tickets at once. Returns {updated:[slugs], errors:[{id,message}]} — one bad id never aborts the rest.",
            "inputSchema": { "type": "object", "properties": {
                "ids": { "type": "array", "items": { "type": "string" }, "description": "tickets to update (slug or ULID)" },
                "status": str_prop(""), "priority": str_prop(""), "category": str_prop(""),
                "up_next": { "type": "boolean" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "replace the tag set on each" }
            }, "required": ["ids"] }
        },
        {
            "name": "hotsheet_announce",
            "description": "Broadcast an ephemeral message to everyone watching this project live (over the WebSocket bus). Not persisted — it isn't a ticket note and isn't replayed; a client not connected when it fires misses it. Requires a running server (the serverless direct-to-disk mode has no bus).",
            "inputSchema": { "type": "object", "properties": {
                "message": str_prop("the announcement text"),
                "store": str_prop("target store URL id (default: this project's store)")
            }, "required": ["message"] }
        },
        {
            "name": "hotsheet_claim_next",
            "description": "Atomically claim the next available ticket (open, unblocked, unclaimed/expired; prefers Up Next). Returns the claimed ticket, or null if nothing is claimable.",
            "inputSchema": { "type": "object", "properties": {
                "worker": str_prop("worker id recorded on the claim (default: worker)"),
                "label": str_prop("human-readable worker label"),
                "lease_minutes": { "type": "integer", "description": "lease length in minutes (default 30)" }
            } }
        },
        {
            "name": "hotsheet_release",
            "description": "Release a claim (only the holding worker unless force).",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("slug or ULID"),
                "worker": str_prop("the releasing worker (default: worker)"),
                "force": { "type": "boolean", "description": "release even if held by another worker" }
            }, "required": ["id"] }
        },
        {
            "name": "hotsheet_renew",
            "description": "Renew a claim's lease (must be the holding worker).",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("slug or ULID"),
                "worker": str_prop("the holding worker (default: worker)"),
                "lease_minutes": { "type": "integer", "description": "new lease length in minutes (default 30)" }
            }, "required": ["id"] }
        },
        {
            "name": "hotsheet_copy",
            "description": "Copy a ticket into another store as a NEW ticket (new ULID, destination prefix, copied_from provenance). The source is untouched; workflow/claim/close state is reset. Returns the new ticket + its destination store.",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("source ticket (slug or ULID)"),
                "to": str_prop("destination store: its URL id (server) or path (serverless)")
            }, "required": ["id", "to"] }
        },
        {
            "name": "hotsheet_move",
            "description": "Move a ticket to another store, KEEPING the same ULID so references survive. The source keeps a `moved` tombstone redirecting to the destination. Requires confirm=true: git history in the source never forgets, so the ticket (and attachments) remain recoverable there.",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("ticket to move (slug or ULID)"),
                "to": str_prop("destination store: its URL id (server) or path (serverless)"),
                "confirm": { "type": "boolean", "description": "must be true — acknowledges the source git-retention caveat" }
            }, "required": ["id", "to", "confirm"] }
        }
    ] })
}

fn call_tool(params: Option<&Value>, backend: &dyn Backend) -> Result<Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::invalid_params("missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing tool name"))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Tool-level failures are reported as an `isError` result, not an RPC error.
    Ok(match dispatch(name, &args, backend) {
        Ok(value) => tool_text(&render_result(&value), false),
        Err(message) => tool_text(&message, true),
    })
}

/// Above this many bytes the pretty-printer's indentation is pure token overhead (a
/// whole-store list dump), so we drop to compact JSON. Small results stay pretty for
/// readability (HS2-GY3GWT item 2).
const PRETTY_MAX_BYTES: usize = 8 * 1024;

/// Serialize a tool result: pretty when small, compact once it crosses [`PRETTY_MAX_BYTES`]
/// (the whitespace on a big list would otherwise inflate the caller's token count).
fn render_result(value: &Value) -> String {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
    if pretty.len() <= PRETTY_MAX_BYTES {
        pretty
    } else {
        serde_json::to_string(value).unwrap_or(pretty)
    }
}

fn dispatch(name: &str, args: &Value, backend: &dyn Backend) -> Result<Value, String> {
    match name {
        "hotsheet_query" => backend.get("/tickets", &query_pairs(args)).map_err(be_msg),
        "hotsheet_get" => backend
            .get(&format!("/tickets/{}", arg_str(args, "id")?), &[])
            .map_err(be_msg),
        "hotsheet_create" => backend.send("POST", "/tickets", args).map_err(be_msg),
        "hotsheet_update" => {
            let id = arg_str(args, "id")?;
            backend
                .send("PATCH", &format!("/tickets/{id}"), &without(args, "id"))
                .map_err(be_msg)
        }
        "hotsheet_close" => {
            let id = arg_str(args, "id")?;
            let body = json!({
                "reason": arg_str(args, "reason")?,
                "duplicate_of": args.get("duplicate_of").cloned().unwrap_or(Value::Null),
            });
            backend
                .send("POST", &format!("/tickets/{id}/close"), &body)
                .map_err(be_msg)
        }
        "hotsheet_batch" => backend.send("POST", "/batch", args).map_err(be_msg),
        "hotsheet_announce" => backend.send("POST", "/announce", args).map_err(be_msg),
        "hotsheet_claim_next" => backend.send("POST", "/claim-next", args).map_err(be_msg),
        "hotsheet_release" => {
            let id = arg_str(args, "id")?;
            backend
                .send(
                    "POST",
                    &format!("/tickets/{id}/release"),
                    &without(args, "id"),
                )
                .map_err(be_msg)
        }
        "hotsheet_renew" => {
            let id = arg_str(args, "id")?;
            backend
                .send(
                    "POST",
                    &format!("/tickets/{id}/renew"),
                    &without(args, "id"),
                )
                .map_err(be_msg)
        }
        // Cross-store copy / move (HS2-60 / HS2-S4H2AM): `to` + `confirm` ride the body
        // (a serverless `to` is a store path with slashes, so it can't go in the URL).
        "hotsheet_copy" => {
            let id = arg_str(args, "id")?;
            backend
                .send("POST", &format!("/tickets/{id}/copy"), &without(args, "id"))
                .map_err(be_msg)
        }
        "hotsheet_move" => {
            let id = arg_str(args, "id")?;
            backend
                .send("POST", &format!("/tickets/{id}/move"), &without(args, "id"))
                .map_err(be_msg)
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn query_pairs(args: &Value) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for key in [
        "status",
        "priority",
        "category",
        "tags",
        "text",
        "up_next",
        "open",
        "close_reason",
        "closed",
        "assignee",
        "review_requested",
        "claimed",
        "blocked",
        "created_after",
        "created_before",
        "updated_after",
        "updated_before",
        "sort",
        "limit",
        "page_after",
        "fields",
        "compact",
    ] {
        if let Some(v) = args.get(key) {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Null => continue,
                other => other.to_string(),
            };
            if !s.is_empty() {
                pairs.push((key.to_string(), s));
            }
        }
    }
    pairs
}

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("missing required argument '{key}'"))
}

fn without(args: &Value, key: &str) -> Value {
    let mut v = args.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove(key);
    }
    v
}

fn be_msg(e: BackendError) -> String {
    // Backend-neutral: a CoreBackend has no server, and an HttpBackend's transport
    // error already reads as a connection problem.
    match e.status {
        Some(code) => format!("error {code}: {}", e.message),
        None => e.message,
    }
}

fn tool_text(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

// ---- direct-core backend (serverless) --------------------------------------------

mod core_backend {
    use super::{Backend, BackendError};
    use hotsheet_model::{NoteKind, Ticket, Timestamp, Ulid};
    use hotsheet_ticketing::{
        ApiTicket, FsStore, NewTicket, OpError, SortKey, StoreError, StoreRegistry, TicketPatch,
        TicketQuery, TicketRow, ops,
    };
    use serde_json::Value;
    use std::path::Path;
    use time::OffsetDateTime;

    /// Serves the `hotsheet_*` tools straight from the store on disk — no server, no
    /// index (reads are an `ops::query` file scan, per `docs/04` §4.4). A running
    /// server's watcher still picks up whatever this writes. Time + id minting go
    /// through injected closures so tests can be deterministic.
    pub struct CoreBackend {
        store: FsStore,
        now: Box<dyn Fn() -> Timestamp>,
        mint: Box<dyn Fn() -> Ulid>,
    }

    impl CoreBackend {
        /// Open the store at `path` with real wall-clock time + random ULIDs.
        pub fn open(path: &Path) -> Result<Self, StoreError> {
            Ok(Self::new(FsStore::open(path)?))
        }

        /// Wrap an already-open store (used by tests via a `TempStore`).
        pub fn new(store: FsStore) -> Self {
            Self {
                store,
                now: Box::new(default_now),
                mint: Box::new(Ulid::new),
            }
        }

        /// Override the clock + minter (deterministic tests).
        pub fn with_mint(
            mut self,
            now: impl Fn() -> Timestamp + 'static,
            mint: impl Fn() -> Ulid + 'static,
        ) -> Self {
            self.now = Box::new(now);
            self.mint = Box::new(mint);
            self
        }

        fn resolve(&self, needle: &str) -> Result<Ticket, BackendError> {
            ops::resolve(&self.store, needle)
                .map_err(store_err)?
                .ok_or_else(|| not_found(needle))
        }

        /// Open the destination store for a copy/move — serverless, `to` is its path.
        fn open_dest(&self, body: &Value) -> Result<FsStore, BackendError> {
            let to = str_field(body, "to").ok_or_else(|| bad_request("'to' is required"))?;
            FsStore::open(Path::new(&to)).map_err(store_err)
        }
    }

    impl Backend for CoreBackend {
        fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
            if path == "/tickets" {
                let q = build_query(query, self.store.root())?;
                let compact = wants_compact(query);
                let rows: Vec<TicketRow> = ops::query(&self.store, &q)
                    .map_err(store_err)?
                    .iter()
                    .map(|t| {
                        if compact {
                            TicketRow::compact(t)
                        } else {
                            TicketRow::from(t)
                        }
                    })
                    .collect();
                // Optional leaner projection (fields=slug,status,…) — HS2-GY3GWT.
                let fields = query_fields(query);
                let mut vals: Vec<Value> = rows.iter().map(to_value).collect();
                hotsheet_ticketing::wire::project_fields(&mut vals, &fields);
                return Ok(Value::Array(vals));
            }
            if let Some(id) = ticket_id(path) {
                let t = self.resolve(id)?;
                return Ok(to_value(&ApiTicket::from(&t)));
            }
            Err(bad_request(format!("unsupported GET {path}")))
        }

        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            match method {
                "POST" if path == "/tickets" => {
                    let prefix = self.store.metadata().map_err(store_err)?.ticket_prefix;
                    let blocked_by =
                        ops::resolve_blockers(&self.store, None, &str_vec(body, "blocked_by"))
                            .map_err(op_err)?;
                    let new = NewTicket {
                        title: str_field(body, "title")
                            .ok_or_else(|| bad_request("title is required"))?,
                        category: str_field(body, "category").unwrap_or_else(|| "issue".into()),
                        priority: opt_enum(body, "priority")?.unwrap_or_default(),
                        details: str_field(body, "details").unwrap_or_default(),
                        tags: str_vec(body, "tags"),
                        up_next: body
                            .get("up_next")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        blocked_by,
                    };
                    let t = ops::create(&self.store, (self.mint)(), &prefix, (self.now)(), new)
                        .map_err(store_err)?;
                    Ok(to_value(&ApiTicket::from(&t)))
                }
                "POST" if close_id(path).is_some() => {
                    let t = self.resolve(close_id(path).unwrap())?;
                    let reason = opt_enum(body, "reason")?
                        .ok_or_else(|| bad_request("reason is required"))?;
                    let dup = match body
                        .get("duplicate_of")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        Some(d) => Some(self.resolve(d)?.id),
                        None => None,
                    };
                    let closed = ops::close(&self.store, &t.id, (self.now)(), reason, dup)
                        .map_err(op_err)?;
                    Ok(to_value(&ApiTicket::from(&closed)))
                }
                "PATCH" if ticket_id(path).is_some() => {
                    let t = self.resolve(ticket_id(path).unwrap())?;
                    // A present `blocked_by` (even []) replaces the set; absent leaves it.
                    let blocked_by = match body.get("blocked_by").filter(|v| !v.is_null()) {
                        Some(_) => Some(
                            ops::resolve_blockers(
                                &self.store,
                                Some(&t.id),
                                &str_vec(body, "blocked_by"),
                            )
                            .map_err(op_err)?,
                        ),
                        None => None,
                    };
                    let patch = TicketPatch {
                        title: str_field(body, "title"),
                        details: str_field(body, "details"),
                        category: str_field(body, "category"),
                        priority: opt_enum(body, "priority")?,
                        status: opt_enum(body, "status")?,
                        tags: body
                            .get("tags")
                            .filter(|v| !v.is_null())
                            .map(|_| str_vec(body, "tags")),
                        up_next: body.get("up_next").and_then(Value::as_bool),
                        blocked_by,
                    };
                    let updated =
                        ops::update(&self.store, &t.id, (self.now)(), patch).map_err(store_err)?;
                    let latest = match str_field(body, "note").filter(|s| !s.is_empty()) {
                        Some(text) => ops::add_note(
                            &self.store,
                            &t.id,
                            (self.mint)(),
                            (self.now)(),
                            NoteKind::Regular,
                            text,
                        )
                        .map_err(store_err)?,
                        None => updated,
                    };
                    Ok(to_value(&ApiTicket::from(&latest)))
                }
                // Batch (HS2-86): apply the same update to many tickets, reusing PATCH so the
                // per-ticket behavior is identical. One bad id doesn't abort the rest.
                "POST" if path == "/batch" => {
                    let ids: Vec<String> = body
                        .get("ids")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut update = body.clone();
                    if let Some(o) = update.as_object_mut() {
                        o.remove("ids");
                    }
                    let mut updated = Vec::new();
                    let mut errors = Vec::new();
                    for id in ids {
                        match self.send("PATCH", &format!("/tickets/{id}"), &update) {
                            Ok(t) => {
                                if let Some(slug) = t.get("slug").and_then(Value::as_str) {
                                    updated.push(Value::from(slug));
                                }
                            }
                            Err(e) => {
                                errors.push(serde_json::json!({ "id": id, "message": e.message }))
                            }
                        }
                    }
                    Ok(serde_json::json!({ "updated": updated, "errors": errors }))
                }
                // Coordination (HS2-86): claim the next available ticket / release / renew.
                "POST" if path == "/claim-next" => {
                    let now = (self.now)();
                    let lease = now.plus_minutes(lease_minutes(body));
                    let worker = str_field(body, "worker").unwrap_or_else(|| "worker".into());
                    let label = str_field(body, "label");
                    let claimed = ops::claim_next(&self.store, &now, lease, &worker, label)
                        .map_err(store_err)?;
                    Ok(match claimed {
                        Some(t) => to_value(&ApiTicket::from(&t)),
                        None => Value::Null,
                    })
                }
                "POST" if release_id(path).is_some() => {
                    let t = self.resolve(release_id(path).unwrap())?;
                    let worker = str_field(body, "worker").unwrap_or_else(|| "worker".into());
                    let force = body.get("force").and_then(Value::as_bool).unwrap_or(false);
                    let released = ops::release(&self.store, &t.id, (self.now)(), &worker, force)
                        .map_err(op_err)?;
                    Ok(to_value(&ApiTicket::from(&released)))
                }
                "POST" if renew_id(path).is_some() => {
                    let t = self.resolve(renew_id(path).unwrap())?;
                    let now = (self.now)();
                    let lease = now.plus_minutes(lease_minutes(body));
                    let worker = str_field(body, "worker").unwrap_or_else(|| "worker".into());
                    let renewed =
                        ops::renew(&self.store, &t.id, now, lease, &worker).map_err(op_err)?;
                    Ok(to_value(&ApiTicket::from(&renewed)))
                }
                // Cross-store copy (HS2-60 / HS2-S4H2AM): serverless, `to` is the
                // destination store's path. New ULID + `copied_from`; source untouched.
                "POST" if copy_id(path).is_some() => {
                    let t = self.resolve(copy_id(path).unwrap())?;
                    let dest = self.open_dest(body)?;
                    let new =
                        ops::copy_ticket(&self.store, &dest, &t.id, (self.mint)(), (self.now)())
                            .map_err(op_err)?;
                    Ok(with_store(
                        to_value(&ApiTicket::from(&new)),
                        &[("store", StoreRegistry::store_id(&dest))],
                    ))
                }
                // Cross-store move (HS2-60 / HS2-S4H2AM): same ULID + source tombstone.
                // Requires confirm=true (the git-retention caveat, docs/02 §2.13).
                "POST" if move_id(path).is_some() => {
                    if !body
                        .get("confirm")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        return Err(bad_request(
                            "move requires confirm=true: the source store's git history keeps \
                             the ticket (and any attachments) even after the move — docs/02 §2.13",
                        ));
                    }
                    let t = self.resolve(move_id(path).unwrap())?;
                    let dest = self.open_dest(body)?;
                    let dest_id = StoreRegistry::store_id(&dest);
                    let outcome =
                        ops::move_ticket(&self.store, &dest, &t.id, &dest_id, (self.now)())
                            .map_err(op_err)?;
                    Ok(with_store(
                        to_value(&ApiTicket::from(&outcome.moved)),
                        &[
                            ("store", dest_id),
                            ("source_store", StoreRegistry::store_id(&self.store)),
                            ("tombstone", outcome.tombstone.slug.clone()),
                        ],
                    ))
                }
                // Announce (HS2-HHDNTH) is a live WS broadcast — there's no bus in the
                // serverless direct-to-disk mode, so guide the caller to a server.
                "POST" if path == "/announce" => Err(bad_request(
                    "announce needs a running server (a live WebSocket broadcast); \
                     run the MCP shim with --server, or start `hotsheet-cli serve`",
                )),
                _ => Err(bad_request(format!("unsupported {method} {path}"))),
            }
        }
    }

    /// The lease length from a request body (minutes), defaulting to 30.
    fn lease_minutes(body: &Value) -> i64 {
        body.get("lease_minutes")
            .and_then(Value::as_i64)
            .unwrap_or(30)
    }

    fn default_now() -> Timestamp {
        Timestamp::from_datetime(OffsetDateTime::now_utc())
    }

    /// `/tickets/{id}` (and nothing more) → the id.
    fn ticket_id(path: &str) -> Option<&str> {
        let rest = path.strip_prefix("/tickets/")?;
        (!rest.is_empty() && !rest.contains('/')).then_some(rest)
    }

    /// `/tickets/{id}/close` → the id.
    fn close_id(path: &str) -> Option<&str> {
        path.strip_prefix("/tickets/")?.strip_suffix("/close")
    }

    /// `/tickets/{id}/release` → the id.
    fn release_id(path: &str) -> Option<&str> {
        path.strip_prefix("/tickets/")?.strip_suffix("/release")
    }

    /// `/tickets/{id}/renew` → the id.
    fn renew_id(path: &str) -> Option<&str> {
        path.strip_prefix("/tickets/")?.strip_suffix("/renew")
    }

    /// `/tickets/{id}/copy` → the id.
    fn copy_id(path: &str) -> Option<&str> {
        path.strip_prefix("/tickets/")?.strip_suffix("/copy")
    }

    /// `/tickets/{id}/move` → the id.
    fn move_id(path: &str) -> Option<&str> {
        path.strip_prefix("/tickets/")?.strip_suffix("/move")
    }

    /// Flatten extra string fields onto an `ApiTicket` value object (the `store` /
    /// `source_store` / `tombstone` copy/move envelope; matches the server's flattened DTO).
    fn with_store(mut ticket: Value, extra: &[(&str, String)]) -> Value {
        if let Some(o) = ticket.as_object_mut() {
            for (k, v) in extra {
                o.insert((*k).to_string(), Value::from(v.clone()));
            }
        }
        ticket
    }

    fn build_query(
        pairs: &[(String, String)],
        store_root: &std::path::Path,
    ) -> Result<TicketQuery, BackendError> {
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, v)| v.as_str())
        };
        let sort = match get("sort") {
            Some(s) => s.parse::<SortKey>().map_err(bad_request)?,
            None => SortKey::default(),
        };
        // `me` → the store's git user.email; an unresolvable `me` is an error, not a silent
        // match-everyone (HS2-TCDTCH, docs/10 §10.3).
        let resolve_person = |v: Option<&str>| -> Result<Option<String>, BackendError> {
            match v {
                None => Ok(None),
                Some(raw) if raw.eq_ignore_ascii_case(hotsheet_ticketing::ME) => {
                    hotsheet_ticketing::current_user_email(store_root)
                        .map(Some)
                        .ok_or_else(|| {
                            bad_request("cannot resolve 'me': no git user.email configured")
                        })
                }
                Some(raw) => Ok(Some(raw.to_string())),
            }
        };
        let page_after = match get("page_after") {
            Some(s) => Some(
                hotsheet_model::Ulid::from_string(s)
                    .map_err(|_| bad_request(format!("invalid page_after cursor '{s}'")))?,
            ),
            None => None,
        };
        Ok(TicketQuery {
            status: opt_enum_str(get("status"))?,
            priority: opt_enum_str(get("priority"))?,
            category: get("category").map(str::to_string),
            tags: get("tags")
                .map(|t| {
                    t.split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            text: get("text").map(str::to_string),
            up_next_only: get("up_next") == Some("true"),
            open_only: get("open") == Some("true"),
            close_reason: opt_enum_str(get("close_reason"))?,
            closed: get("closed").map(|v| v == "true"),
            assignee: resolve_person(get("assignee"))?,
            claimed: get("claimed").map(|v| v == "true"),
            review_requested: resolve_person(get("review_requested"))?,
            blocked: get("blocked").map(|v| v == "true"),
            sort,
            limit: match get("limit") {
                Some(s) => Some(
                    s.parse::<usize>()
                        .map_err(|_| bad_request(format!("invalid limit '{s}'")))?,
                ),
                None => None,
            },
            page_after,
            ..Default::default()
        })
    }

    /// The `fields=` allow-list for a leaner list projection (comma-separated; HS2-GY3GWT).
    fn query_fields(pairs: &[(String, String)]) -> Vec<String> {
        pairs
            .iter()
            .find(|(k, _)| k == "fields")
            .map(|(_, v)| {
                v.split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A list is compact (no Markdown body) unless `compact=false` is passed.
    fn wants_compact(pairs: &[(String, String)]) -> bool {
        pairs
            .iter()
            .find(|(k, _)| k == "compact")
            .map(|(_, v)| v != "false")
            .unwrap_or(true)
    }

    // ---- small conversions + errors ----------------------------------------------

    fn to_value<T: serde::Serialize>(v: &T) -> Value {
        serde_json::to_value(v).unwrap_or(Value::Null)
    }

    fn str_field(body: &Value, key: &str) -> Option<String> {
        body.get(key).and_then(Value::as_str).map(str::to_string)
    }

    /// Read `key` as a string array, tolerating a single comma-joined string.
    fn str_vec(body: &Value, key: &str) -> Vec<String> {
        match body.get(key) {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            Some(Value::String(s)) => s
                .split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Parse an enum from a body string field via serde (matches serialization).
    fn opt_enum<T: serde::de::DeserializeOwned>(
        body: &Value,
        key: &str,
    ) -> Result<Option<T>, BackendError> {
        match body.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => opt_enum_str(Some(s)),
            Some(_) => Err(bad_request(format!("{key} must be a string"))),
        }
    }

    fn opt_enum_str<T: serde::de::DeserializeOwned>(
        s: Option<&str>,
    ) -> Result<Option<T>, BackendError> {
        match s {
            None => Ok(None),
            Some(s) => serde_json::from_value(Value::String(s.to_string()))
                .map(Some)
                .map_err(|_| bad_request(format!("invalid value '{s}'"))),
        }
    }

    fn not_found(id: &str) -> BackendError {
        BackendError {
            status: Some(404),
            message: format!("no ticket matching '{id}'"),
        }
    }

    fn bad_request(message: impl Into<String>) -> BackendError {
        BackendError {
            status: Some(400),
            message: message.into(),
        }
    }

    fn store_err(e: StoreError) -> BackendError {
        let status = match &e {
            StoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => 404,
            _ => 500,
        };
        BackendError {
            status: Some(status),
            message: e.to_string(),
        }
    }

    fn op_err(e: OpError) -> BackendError {
        match e {
            OpError::Store(s) => store_err(s),
            e @ (OpError::DuplicateNeedsTarget | OpError::SelfBlock(_)) => {
                bad_request(e.to_string())
            }
            e @ OpError::UnknownTicket(_) => not_found(&e.to_string()),
            other => BackendError {
                status: Some(409),
                message: other.to_string(),
            },
        }
    }
}

pub use core_backend::CoreBackend;

// ---- HTTP backend ----------------------------------------------------------------

/// Proxies to a running server over HTTP with the shared secret.
pub struct HttpBackend {
    base: String,
    secret: String,
    agent: ureq::Agent,
}

impl HttpBackend {
    pub fn new(base: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            secret: secret.into(),
            agent: ureq::agent(),
        }
    }
}

impl Backend for HttpBackend {
    fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
        let mut req = self
            .agent
            .get(&format!("{}{path}", self.base))
            .set("X-Hotsheet-Secret", &self.secret);
        for (k, v) in query {
            req = req.query(k, v);
        }
        exec(req.call())
    }

    fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
        let req = self
            .agent
            .request(method, &format!("{}{path}", self.base))
            .set("X-Hotsheet-Secret", &self.secret)
            .set("Content-Type", "application/json");
        let payload = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
        exec(req.send_string(&payload))
    }
}

fn exec(res: Result<ureq::Response, ureq::Error>) -> Result<Value, BackendError> {
    match res {
        Ok(resp) => {
            let text = resp.into_string().unwrap_or_default();
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(String::from))
                .unwrap_or(text);
            Err(BackendError {
                status: Some(code),
                message,
            })
        }
        Err(e) => Err(BackendError {
            status: None,
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeBackend {
        calls: RefCell<Vec<String>>,
    }
    impl Backend for FakeBackend {
        fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls
                .borrow_mut()
                .push(format!("GET {path} {query:?}"));
            Ok(json!([{ "slug": "HS-AAA111", "title": "t" }]))
        }
        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            self.calls
                .borrow_mut()
                .push(format!("{method} {path} {body}"));
            Ok(json!({ "slug": "HS-NEW999", "title": body.get("title") }))
        }
    }

    fn req(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    #[test]
    fn initialize_advertises_tools() {
        let r = handle_message(&req("initialize", json!({})), &FakeBackend::default()).unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(r["result"]["serverInfo"]["name"], "hotsheet-mcp");
        assert!(r["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_has_the_hotsheet_tools() {
        let r = handle_message(&req("tools/list", json!({})), &FakeBackend::default()).unwrap();
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for want in [
            "hotsheet_query",
            "hotsheet_get",
            "hotsheet_create",
            "hotsheet_update",
            "hotsheet_close",
        ] {
            assert!(names.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn a_notification_gets_no_reply() {
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_message(&note, &FakeBackend::default()).is_none());
    }

    #[test]
    fn unknown_method_is_an_rpc_error() {
        let r = handle_message(&req("nope", json!({})), &FakeBackend::default()).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn create_tool_proxies_a_post() {
        let backend = FakeBackend::default();
        let r = handle_message(
            &req(
                "tools/call",
                json!({ "name": "hotsheet_create", "arguments": { "title": "Fix it" } }),
            ),
            &backend,
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], false);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("HS-NEW999")
        );
        assert!(backend.calls.borrow()[0].starts_with("POST /tickets"));
        assert!(backend.calls.borrow()[0].contains("Fix it"));
    }

    #[test]
    fn query_tool_forwards_filters() {
        let backend = FakeBackend::default();
        handle_message(
            &req(
                "tools/call",
                json!({ "name": "hotsheet_query", "arguments": { "status": "started", "text": "x" } }),
            ),
            &backend,
        );
        let call = backend.calls.borrow()[0].clone();
        assert!(call.starts_with("GET /tickets"));
        assert!(call.contains("status"));
        assert!(call.contains("started"));
    }

    #[test]
    fn missing_required_argument_is_a_tool_error() {
        let r = handle_message(
            &req(
                "tools/call",
                json!({ "name": "hotsheet_get", "arguments": {} }),
            ),
            &FakeBackend::default(),
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("missing required argument 'id'")
        );
    }

    // ---- CoreBackend: the whole loop, serverless, straight to disk ----------------

    use hotsheet_ticketing::{FsStore, StoreMetadata, ops};

    /// A CoreBackend over a fresh temp store (real ops, no server).
    fn core() -> (tempfile::TempDir, CoreBackend) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, CoreBackend::new(store))
    }

    /// Call a tool and return the parsed JSON the agent would see (or the error text).
    fn call(backend: &dyn Backend, name: &str, args: Value) -> Value {
        let r = handle_message(
            &req("tools/call", json!({ "name": name, "arguments": args })),
            backend,
        )
        .unwrap();
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        if r["result"]["isError"] == Value::Bool(true) {
            return json!({ "error": text });
        }
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
    }

    #[test]
    fn render_result_switches_to_compact_above_the_threshold() {
        // A small result stays pretty (indented, multi-line) for readability.
        let small = render_result(&json!({ "a": 1, "b": 2 }));
        assert!(
            small.contains('\n'),
            "small result should be pretty-printed"
        );

        // A large result drops to compact (no indentation newlines) to save tokens.
        let big: Vec<Value> = (0..3000)
            .map(|i| json!({ "slug": format!("HS-{i:05}"), "title": "some title text" }))
            .collect();
        let out = render_result(&Value::Array(big));
        assert!(out.len() > PRETTY_MAX_BYTES);
        assert!(
            !out.contains('\n'),
            "a large result should be compact JSON, not pretty-printed"
        );
    }

    #[test]
    fn announce_is_a_tool_but_errors_serverless() {
        // It's advertised in the tool list.
        let tools = tools_list();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"hotsheet_announce"));

        // Serverless (CoreBackend) has no WS bus → a clear "needs a server" error.
        let (_d, backend) = core();
        let r = call(&backend, "hotsheet_announce", json!({ "message": "hi" }));
        assert!(
            r["error"]
                .as_str()
                .unwrap_or("")
                .contains("needs a running server"),
            "expected a needs-a-server error, got {r}"
        );
    }

    #[test]
    fn query_supports_fields_projection_over_the_shim() {
        let (_d, backend) = core();
        call(
            &backend,
            "hotsheet_create",
            json!({ "title": "a", "category": "bug" }),
        );
        call(
            &backend,
            "hotsheet_create",
            json!({ "title": "b", "category": "task" }),
        );

        // fields=status keeps only status (+ slug, always) on every row.
        let rows = call(&backend, "hotsheet_query", json!({ "fields": "status" }));
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        for row in arr {
            let keys: std::collections::HashSet<&str> = row
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["slug", "status"].into_iter().collect());
        }
    }

    #[test]
    fn corebackend_runs_the_full_loop_with_no_server() {
        let (_d, backend) = core();

        // create
        let created = call(
            &backend,
            "hotsheet_create",
            json!({ "title": "Fix flicker", "category": "bug", "priority": "high", "up_next": true }),
        );
        let id = created["id"].as_str().unwrap().to_string();
        let slug = created["slug"].as_str().unwrap().to_string();
        assert!(slug.starts_with("HS-"));
        assert_eq!(created["priority"], "high");
        assert_eq!(created["status"], "not_started");

        // get by slug (resolve is case-insensitive)
        let got = call(
            &backend,
            "hotsheet_get",
            json!({ "id": slug.to_lowercase() }),
        );
        assert_eq!(got["id"], id);
        assert_eq!(got["title"], "Fix flicker");

        // query: up_next + text filter finds it (row shape: enum-as-string)
        let rows = call(
            &backend,
            "hotsheet_query",
            json!({ "up_next": true, "text": "flick" }),
        );
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["slug"], slug);
        assert_eq!(arr[0]["priority"], "high");

        // update → started, with a progress note appended in the same call
        let updated = call(
            &backend,
            "hotsheet_update",
            json!({ "id": id, "status": "started", "tags": ["ui", "urgent"], "note": "picked this up" }),
        );
        assert_eq!(updated["status"], "started");
        assert_eq!(updated["tags"], json!(["ui", "urgent"]));
        assert_eq!(updated["notes"][0]["text"], "picked this up");

        // close → records the outcome AND settles status: a close_reason may never sit on
        // an active status, so closing the `started` ticket moves it to `completed`
        // (HS2-3XHT9P). It drops from an open query and its completed_at is stamped.
        let closed = call(
            &backend,
            "hotsheet_close",
            json!({ "id": id, "reason": "completed" }),
        );
        assert_eq!(closed["close_reason"], "completed");
        assert!(closed["closed_at"].is_string());
        assert_eq!(
            closed["status"], "completed",
            "close settles active → completed"
        );
        assert!(closed["completed_at"].is_string());
        let open = call(&backend, "hotsheet_query", json!({ "open": true }));
        assert!(open.as_array().unwrap().is_empty());

        // Reopening (status → started) clears the close annotation — the inverse half of
        // the invariant, so the two can never disagree.
        let reopened = call(
            &backend,
            "hotsheet_update",
            json!({ "id": id, "status": "started" }),
        );
        assert_eq!(reopened["status"], "started");
        assert!(
            reopened["close_reason"].is_null(),
            "reopen clears close_reason"
        );
        let open = call(&backend, "hotsheet_query", json!({ "open": true }));
        assert_eq!(open.as_array().unwrap().len(), 1);
    }

    #[test]
    fn corebackend_cross_store_copy_and_move_over_mcp() {
        let (_srcd, backend) = core(); // source store, prefix HS
        let destd = tempfile::tempdir().unwrap();
        let dest = FsStore::init(destd.path(), &StoreMetadata::new("DS")).unwrap();
        let to = destd.path().display().to_string();

        let created = call(
            &backend,
            "hotsheet_create",
            json!({ "title": "Portable idea", "up_next": true }),
        );
        let id = created["id"].as_str().unwrap().to_string();
        let src_slug = created["slug"].as_str().unwrap().to_string();

        // copy → NEW ULID, destination prefix, copied_from provenance; source untouched.
        let copied = call(&backend, "hotsheet_copy", json!({ "id": id, "to": to }));
        assert_ne!(copied["id"], created["id"], "copy mints a new ULID");
        assert!(copied["slug"].as_str().unwrap().starts_with("DS-"));
        assert_eq!(copied["copied_from"], created["id"]);
        assert_eq!(copied["status"], "not_started");
        assert!(
            copied["up_next"].as_bool() == Some(false),
            "copy resets up_next"
        );
        // The source still resolves to the original (a copy leaves it be).
        assert_eq!(
            call(&backend, "hotsheet_get", json!({ "id": id }))["id"],
            created["id"]
        );

        // move without confirm → a tool error naming the caveat; source unchanged.
        let denied = call(&backend, "hotsheet_move", json!({ "id": id, "to": to }));
        assert!(denied["error"].as_str().unwrap().contains("confirm"));
        assert_eq!(
            call(&backend, "hotsheet_get", json!({ "id": id }))["status"],
            "not_started",
            "a denied move changes nothing"
        );

        // move with confirm → same ULID lives in dest; source becomes a `moved` tombstone.
        let moved = call(
            &backend,
            "hotsheet_move",
            json!({ "id": id, "to": to, "confirm": true }),
        );
        assert_eq!(moved["id"], created["id"], "move keeps the ULID");
        assert!(moved["slug"].as_str().unwrap().starts_with("DS-"));
        assert_eq!(
            moved["tombstone"], src_slug,
            "tombstone keeps the source slug"
        );
        assert!(!moved["source_store"].as_str().unwrap().is_empty());
        // Source now holds a moved tombstone redirecting to the destination store.
        let tomb = call(&backend, "hotsheet_get", json!({ "id": id }));
        assert_eq!(tomb["status"], "moved");
        assert!(tomb["moved_to_store"].is_string());
        // The live instance is really in the destination store.
        let live = ops::resolve(&dest, &id).unwrap().expect("live in dest");
        assert_eq!(live.status, hotsheet_model::Status::NotStarted);
    }

    #[test]
    fn corebackend_claim_release_renew_over_mcp() {
        let (_d, backend) = core();
        let created = call(
            &backend,
            "hotsheet_create",
            json!({ "title": "drain me", "up_next": true }),
        );
        let slug = created["slug"].as_str().unwrap().to_string();

        // Claim the next available ticket for a worker.
        let claimed = call(
            &backend,
            "hotsheet_claim_next",
            json!({ "worker": "agent-1" }),
        );
        assert_eq!(claimed["slug"], slug);
        assert_eq!(claimed["claimed_by"], "agent-1");

        // Renew extends the lease (holder only).
        let renewed = call(
            &backend,
            "hotsheet_renew",
            json!({ "id": slug, "worker": "agent-1", "lease_minutes": 60 }),
        );
        assert_eq!(renewed["claimed_by"], "agent-1");

        // A wrong worker can't release without force → tool error.
        let denied = call(
            &backend,
            "hotsheet_release",
            json!({ "id": slug, "worker": "someone-else" }),
        );
        assert!(denied["error"].as_str().unwrap().contains("claimed by"));

        // The holder releases it.
        let released = call(
            &backend,
            "hotsheet_release",
            json!({ "id": slug, "worker": "agent-1" }),
        );
        assert!(released["claimed_by"].is_null());

        // Nothing left to claim → null.
        let none = call(
            &backend,
            "hotsheet_claim_next",
            json!({ "worker": "agent-1" }),
        );
        // ... after releasing, the ticket is claimable again, so claim it, then complete it.
        assert_eq!(none["slug"], slug, "released ticket is claimable again");
        call(
            &backend,
            "hotsheet_update",
            json!({ "id": slug, "status": "completed" }),
        );
        let empty = call(
            &backend,
            "hotsheet_claim_next",
            json!({ "worker": "agent-1" }),
        );
        assert!(empty.is_null(), "no claimable tickets → null");
    }

    #[test]
    fn corebackend_batch_updates_many_and_reports_bad_ids() {
        let (_d, backend) = core();
        let a = call(&backend, "hotsheet_create", json!({ "title": "one" }));
        let b = call(&backend, "hotsheet_create", json!({ "title": "two" }));
        let a_slug = a["slug"].as_str().unwrap().to_string();
        let b_slug = b["slug"].as_str().unwrap().to_string();

        // Batch: mark both started + up_next, plus one bad id that must not abort the rest.
        let res = call(
            &backend,
            "hotsheet_batch",
            json!({ "ids": [a_slug, b_slug, "HS-NOPE00"], "status": "started", "up_next": true }),
        );
        let updated = res["updated"].as_array().unwrap();
        assert_eq!(updated.len(), 2, "both good ids updated");
        assert_eq!(
            res["errors"].as_array().unwrap().len(),
            1,
            "the bad id is reported"
        );
        assert_eq!(res["errors"][0]["id"], "HS-NOPE00");

        // The updates actually applied.
        let started = call(
            &backend,
            "hotsheet_query",
            json!({ "status": "started", "up_next": true }),
        );
        assert_eq!(started.as_array().unwrap().len(), 2);
    }

    #[test]
    fn corebackend_reports_not_found_and_bad_input_as_tool_errors() {
        let (_d, backend) = core();

        let missing = call(&backend, "hotsheet_get", json!({ "id": "HS-NOPE00" }));
        assert!(
            missing["error"]
                .as_str()
                .unwrap()
                .contains("no ticket matching 'HS-NOPE00'")
        );

        // close as duplicate without a target is rejected by ops, surfaced as a tool error
        let created = call(&backend, "hotsheet_create", json!({ "title": "t" }));
        let id = created["id"].as_str().unwrap();
        let dup = call(
            &backend,
            "hotsheet_close",
            json!({ "id": id, "reason": "duplicate" }),
        );
        assert!(
            dup["error"]
                .as_str()
                .unwrap()
                .contains("duplicate target is required")
        );
    }

    #[test]
    fn query_is_compact_by_default_and_full_on_request() {
        let (_d, backend) = core();
        call(
            &backend,
            "hotsheet_create",
            json!({ "title": "with a body", "details": "a long markdown body here" }),
        );

        // Default list omits the Markdown body entirely (the big field): the key is
        // absent, not just empty.
        let compact = call(&backend, "hotsheet_query", json!({}));
        let row = &compact.as_array().unwrap()[0];
        assert_eq!(row["title"], "with a body");
        assert!(row.get("details").is_none(), "compact row omits details");

        // Opt in to bodies with compact=false.
        let full = call(&backend, "hotsheet_query", json!({ "compact": false }));
        assert_eq!(
            full.as_array().unwrap()[0]["details"],
            "a long markdown body here"
        );
    }

    #[test]
    fn blocked_by_set_clear_and_reject_through_the_full_stack() {
        let (_d, backend) = core();
        let a = call(&backend, "hotsheet_create", json!({ "title": "blocker" }));
        let a_slug = a["slug"].as_str().unwrap().to_string();
        let a_id = a["id"].as_str().unwrap().to_string();

        // create with a blocker by slug
        let b = call(
            &backend,
            "hotsheet_create",
            json!({ "title": "blocked", "blocked_by": [a_slug] }),
        );
        let b_id = b["id"].as_str().unwrap().to_string();
        assert_eq!(b["blocked_by"], json!([a_id]));

        // update: clearing with [] empties the set
        let cleared = call(
            &backend,
            "hotsheet_update",
            json!({ "id": b_id, "blocked_by": [] }),
        );
        assert_eq!(cleared["blocked_by"], json!([]));

        // update: omitting blocked_by leaves it unchanged (re-set first, then touch title)
        call(
            &backend,
            "hotsheet_update",
            json!({ "id": b_id, "blocked_by": [a_id] }),
        );
        let touched = call(
            &backend,
            "hotsheet_update",
            json!({ "id": b_id, "title": "blocked v2" }),
        );
        assert_eq!(touched["blocked_by"], json!([a_id]), "absent leaves it");

        // unknown blocker and self-reference are tool errors
        let unknown = call(
            &backend,
            "hotsheet_update",
            json!({ "id": b_id, "blocked_by": ["HS-NOPE00"] }),
        );
        assert!(
            unknown["error"]
                .as_str()
                .unwrap()
                .contains("no ticket matching")
        );
        let selfblock = call(
            &backend,
            "hotsheet_update",
            json!({ "id": b_id, "blocked_by": [b_id] }),
        );
        assert!(
            selfblock["error"]
                .as_str()
                .unwrap()
                .contains("cannot block itself")
        );
    }

    #[test]
    fn query_limit_caps_rows_and_rejects_garbage() {
        let (_d, backend) = core();
        for i in 0..5 {
            call(
                &backend,
                "hotsheet_create",
                json!({ "title": format!("t{i}") }),
            );
        }
        let all = call(&backend, "hotsheet_query", json!({}));
        assert_eq!(all.as_array().unwrap().len(), 5);

        let two = call(&backend, "hotsheet_query", json!({ "limit": 2 }));
        assert_eq!(two.as_array().unwrap().len(), 2);

        // A non-numeric limit is a tool error, not a silent full scan.
        let bad = call(&backend, "hotsheet_query", json!({ "limit": "lots" }));
        assert!(bad["error"].as_str().unwrap().contains("invalid limit"));
    }
}
