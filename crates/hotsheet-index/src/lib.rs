//! The disposable SQLite + FTS5 index over the git-backed store
//! (`docs/03-indexing-and-query.md`). Source of truth is the files; this is a cache
//! that can be dropped and rebuilt at any time. It powers fast structured queries +
//! full-text search so the UI never walks the store to draw a list.
//!
//! v1 indexes the queryable ticket fields + a `tags` table (for tag filters) + an
//! FTS5 table over title/details/notes. The `blocked_by`/`assignees`/`reviews`
//! facet tables in the doc schema are a follow-up.

use std::path::{Path, PathBuf};

use hotsheet_model::{Ticket, Ulid, parse_file};
use hotsheet_ticketing::{FsStore, SortKey, TicketQuery};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use sha2::{Digest, Sha256};

/// Bump to force a full rebuild on open when the on-disk schema is stale.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = r#"
CREATE TABLE tickets (
  rowid           INTEGER PRIMARY KEY,
  store_id        TEXT NOT NULL,
  id              TEXT NOT NULL,
  slug            TEXT NOT NULL,
  title           TEXT NOT NULL,
  details         TEXT NOT NULL DEFAULT '',
  category        TEXT,
  priority        TEXT,
  priority_rank   INTEGER NOT NULL DEFAULT 2,
  status          TEXT,
  status_rank     INTEGER NOT NULL DEFAULT 0,
  close_reason    TEXT,
  duplicate_of    TEXT,
  closed_at       TEXT,
  up_next         INTEGER NOT NULL DEFAULT 0,
  tags_json       TEXT NOT NULL DEFAULT '[]',
  blocked_by_json TEXT NOT NULL DEFAULT '[]',
  created_at      TEXT, updated_at TEXT, completed_at TEXT, verified_at TEXT,
  claimed_by      TEXT, claim_lease_expires_at TEXT, worker_label TEXT, claim_count INTEGER DEFAULT 0,
  legacy_number   TEXT,
  file_path       TEXT NOT NULL,
  content_hash    TEXT NOT NULL,
  UNIQUE(store_id, id)
);
CREATE INDEX idx_tickets_status ON tickets(store_id, status);
CREATE TABLE tags (store_id TEXT, ticket_id TEXT, tag TEXT);
CREATE INDEX idx_tags ON tags(store_id, tag);
CREATE VIRTUAL TABLE tickets_fts USING fts5(title, details, notes);
CREATE TABLE index_meta (key TEXT PRIMARY KEY, value TEXT);
"#;

/// An index error.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("parsing {path}: {source}")]
    Parse {
        path: String,
        source: hotsheet_model::ParseError,
    },
}

/// A query result row. Defined once in `hotsheet_ticketing::wire` (the wire SSOT,
/// `docs/04` §4.2) and re-exported here: the index builds it from its SQL columns,
/// and a serverless scan builds the same struct via `TicketRow::from(&Ticket)`, so a
/// list is identical whichever path produced it.
pub use hotsheet_ticketing::TicketRow;

/// The index over one store.
pub struct Index {
    conn: Connection,
    store_id: String,
}

impl Index {
    /// Open an in-memory index (for tests).
    pub fn open_in_memory(store_id: impl Into<String>) -> Result<Self, IndexError> {
        Self::init(Connection::open_in_memory()?, store_id.into())
    }

    /// Open (or create) a file-backed index. A schema-version mismatch drops + rebuilds.
    pub fn open(db_path: &Path, store_id: impl Into<String>) -> Result<Self, IndexError> {
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn, store_id.into())
    }

    /// Open a file-backed index for a store and reconcile it against the current
    /// files — the "restore from disk on launch" path. Reuses whatever the on-disk
    /// index still has and only re-reads the delta. If the file can't be opened
    /// (corrupt), it's deleted + rebuilt (corruption is a non-event, `docs/03` §3.8).
    /// `store_id` is derived from the store root.
    pub fn open_reconciled(db_path: &Path, store: &FsStore) -> Result<Self, IndexError> {
        let store_id = store.root().display().to_string();
        let index = match Self::open(db_path, store_id.clone()) {
            Ok(index) => index,
            Err(_) => {
                let _ = std::fs::remove_file(db_path);
                let _ = std::fs::remove_file(sidecar(db_path, "-wal"));
                let _ = std::fs::remove_file(sidecar(db_path, "-shm"));
                Self::open(db_path, store_id)?
            }
        };
        index.reconcile(store)?;
        Ok(index)
    }

    fn init(conn: Connection, store_id: String) -> Result<Self, IndexError> {
        let has_meta: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='index_meta'",
                [],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        let version: Option<i64> = if has_meta {
            conn.query_row(
                "SELECT value FROM index_meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|s| s.parse().ok())
        } else {
            None
        };

        if version != Some(SCHEMA_VERSION) {
            conn.execute_batch(
                "DROP TABLE IF EXISTS tickets; DROP TABLE IF EXISTS tags; \
                 DROP TABLE IF EXISTS tickets_fts; DROP TABLE IF EXISTS index_meta;",
            )?;
            conn.execute_batch(SCHEMA)?;
            conn.execute(
                "INSERT INTO index_meta(key, value) VALUES('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )?;
        }
        Ok(Self { conn, store_id })
    }

    /// The stored content hash for a ticket, or `None` if not indexed (change detection).
    pub fn content_hash(&self, id: &Ulid) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT content_hash FROM tickets WHERE store_id=?1 AND id=?2",
                params![self.store_id, id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Insert or update a ticket's index rows + FTS entry.
    pub fn upsert(
        &self,
        t: &Ticket,
        file_path: &str,
        content_hash: &str,
    ) -> Result<(), IndexError> {
        let id = t.id.to_string();
        let tags_json = serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".into());
        let blocked_json = serde_json::to_string(
            &t.blocked_by
                .iter()
                .map(|u| u.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".into());

        self.conn.execute(
            "INSERT INTO tickets(store_id,id,slug,title,details,category,priority,priority_rank,\
             status,status_rank,close_reason,duplicate_of,closed_at,up_next,tags_json,blocked_by_json,\
             created_at,updated_at,completed_at,verified_at,claimed_by,claim_lease_expires_at,\
             worker_label,claim_count,legacy_number,file_path,content_hash) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27) \
             ON CONFLICT(store_id,id) DO UPDATE SET \
             slug=excluded.slug,title=excluded.title,details=excluded.details,category=excluded.category,\
             priority=excluded.priority,priority_rank=excluded.priority_rank,status=excluded.status,\
             status_rank=excluded.status_rank,close_reason=excluded.close_reason,duplicate_of=excluded.duplicate_of,\
             closed_at=excluded.closed_at,up_next=excluded.up_next,tags_json=excluded.tags_json,\
             blocked_by_json=excluded.blocked_by_json,created_at=excluded.created_at,updated_at=excluded.updated_at,\
             completed_at=excluded.completed_at,verified_at=excluded.verified_at,claimed_by=excluded.claimed_by,\
             claim_lease_expires_at=excluded.claim_lease_expires_at,worker_label=excluded.worker_label,\
             claim_count=excluded.claim_count,legacy_number=excluded.legacy_number,file_path=excluded.file_path,\
             content_hash=excluded.content_hash",
            params![
                self.store_id, id, t.slug, t.title, t.details, t.category,
                enum_str(&t.priority), priority_rank(t.priority) as i64,
                enum_str(&t.status), t.status as i64,
                t.close_reason.as_ref().map(enum_str), t.duplicate_of.map(|u| u.to_string()),
                ts(&t.closed_at), t.up_next as i64, tags_json, blocked_json,
                t.created_at.as_str(), t.updated_at.as_str(), ts(&t.completed_at), ts(&t.verified_at),
                t.claimed_by, ts(&t.claim_lease_expires_at), t.worker_label, t.claim_count,
                t.legacy_number, file_path, content_hash,
            ],
        )?;

        let rowid: i64 = self.conn.query_row(
            "SELECT rowid FROM tickets WHERE store_id=?1 AND id=?2",
            params![self.store_id, id],
            |r| r.get(0),
        )?;

        // tags table
        self.conn.execute(
            "DELETE FROM tags WHERE store_id=?1 AND ticket_id=?2",
            params![self.store_id, id],
        )?;
        for tag in &t.tags {
            self.conn.execute(
                "INSERT INTO tags(store_id,ticket_id,tag) VALUES(?1,?2,?3)",
                params![self.store_id, id, tag],
            )?;
        }

        // FTS
        let notes = t
            .notes
            .iter()
            .map(|n| n.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.conn
            .execute("DELETE FROM tickets_fts WHERE rowid=?1", params![rowid])?;
        self.conn.execute(
            "INSERT INTO tickets_fts(rowid,title,details,notes) VALUES(?1,?2,?3,?4)",
            params![rowid, t.title, t.details, notes],
        )?;
        Ok(())
    }

    /// Remove a ticket from the index (a hard-removed / moved-away file).
    pub fn delete(&self, id: &Ulid) -> Result<(), IndexError> {
        let id = id.to_string();
        if let Some(rowid) = self
            .conn
            .query_row(
                "SELECT rowid FROM tickets WHERE store_id=?1 AND id=?2",
                params![self.store_id, id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            self.conn
                .execute("DELETE FROM tickets_fts WHERE rowid=?1", params![rowid])?;
        }
        self.conn.execute(
            "DELETE FROM tickets WHERE store_id=?1 AND id=?2",
            params![self.store_id, id],
        )?;
        self.conn.execute(
            "DELETE FROM tags WHERE store_id=?1 AND ticket_id=?2",
            params![self.store_id, id],
        )?;
        Ok(())
    }

    /// Drop all rows and rebuild from a full store walk. Always safe (disposable).
    pub fn rebuild_from_store(&self, store: &FsStore) -> Result<usize, IndexError> {
        self.conn
            .execute_batch("DELETE FROM tickets; DELETE FROM tags; DELETE FROM tickets_fts;")?;
        let mut count = 0;
        for path in ticket_files(store)? {
            let bytes = std::fs::read(&path)?;
            let ticket = parse_ticket(&path, &bytes)?;
            self.upsert(&ticket, &path.display().to_string(), &hash_bytes(&bytes))?;
            count += 1;
        }
        Ok(count)
    }

    /// Bring the (kept) index up to date with the store: re-parse + upsert only files
    /// whose content hash changed, and delete rows whose file is gone. Returns
    /// `(upserted, deleted)`. Cheaper than a full rebuild on a warm index — this is
    /// what makes "restore from disk on launch" fast (`docs/03` §3.4).
    pub fn reconcile(&self, store: &FsStore) -> Result<(usize, usize), IndexError> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut upserted = 0;
        for path in ticket_files(store)? {
            // The filename stem is the ticket ULID.
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Ulid::from_string(s).ok())
            else {
                continue;
            };
            seen.insert(id.to_string());
            let bytes = std::fs::read(&path)?;
            let hash = hash_bytes(&bytes);
            if self.content_hash(&id)?.as_deref() != Some(hash.as_str()) {
                let ticket = parse_ticket(&path, &bytes)?;
                self.upsert(&ticket, &path.display().to_string(), &hash)?;
                upserted += 1;
            }
        }

        let existing: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM tickets WHERE store_id=?1")?;
            stmt.query_map(params![self.store_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        let mut deleted = 0;
        for id in existing {
            if !seen.contains(&id) {
                if let Ok(u) = Ulid::from_string(&id) {
                    self.delete(&u)?;
                    deleted += 1;
                }
            }
        }
        Ok((upserted, deleted))
    }

    /// Run a structured + full-text query, returning list rows.
    pub fn query(&self, q: &TicketQuery) -> Result<Vec<TicketRow>, IndexError> {
        let mut wheres = vec!["t.store_id = ?".to_string()];
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(self.store_id.clone())];

        if let Some(s) = q.status {
            wheres.push("t.status = ?".into());
            args.push(Box::new(enum_str(&s)));
        }
        if let Some(p) = q.priority {
            wheres.push("t.priority = ?".into());
            args.push(Box::new(enum_str(&p)));
        }
        if let Some(c) = &q.category {
            wheres.push("t.category = ?".into());
            args.push(Box::new(c.clone()));
        }
        if q.up_next_only {
            wheres.push("t.up_next = 1".into());
        }
        if q.open_only {
            wheres.push(
                "t.status NOT IN ('completed','verified','deleted','archive','moved')".into(),
            );
        }
        if let Some(r) = q.close_reason {
            wheres.push("t.close_reason = ?".into());
            args.push(Box::new(enum_str(&r)));
        }
        if let Some(want) = q.closed {
            // `closed` = a close_reason is set; orthogonal to status (HS2-61).
            wheres.push(if want {
                "t.close_reason IS NOT NULL".into()
            } else {
                "t.close_reason IS NULL".into()
            });
        }
        if !q.tags.is_empty() {
            let placeholders = vec!["?"; q.tags.len()].join(",");
            wheres.push(format!(
                "t.id IN (SELECT ticket_id FROM tags WHERE store_id=? AND tag IN ({placeholders}) \
                 GROUP BY ticket_id HAVING COUNT(DISTINCT tag)=?)"
            ));
            args.push(Box::new(self.store_id.clone()));
            for tag in &q.tags {
                args.push(Box::new(tag.clone()));
            }
            args.push(Box::new(q.tags.len() as i64));
        }
        let mut from = "tickets t".to_string();
        if let Some(text) = fts_query(q.text.as_deref()) {
            from = "tickets t JOIN tickets_fts f ON f.rowid = t.rowid".to_string();
            wheres.push("f.tickets_fts MATCH ?".into());
            args.push(Box::new(text));
        }

        let order = match q.sort {
            SortKey::Id => "t.id",
            SortKey::Created => "t.created_at",
            SortKey::Updated => "t.updated_at",
            SortKey::Priority => "t.priority_rank",
            SortKey::Status => "t.status_rank",
            SortKey::Title => "lower(t.title)",
        };

        let limit = match q.limit {
            Some(n) => format!(" LIMIT {n}"),
            None => String::new(),
        };
        let sql = format!(
            "SELECT t.id,t.slug,t.title,t.details,t.category,t.priority,t.status,t.up_next,\
             t.tags_json,t.blocked_by_json,t.created_at,t.updated_at,t.completed_at,t.verified_at,\
             t.closed_at,t.close_reason,t.duplicate_of,t.claimed_by,t.worker_label,t.claim_count,\
             t.legacy_number FROM {from} WHERE {} ORDER BY {order}, t.id{limit}",
            wheres.join(" AND ")
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
                Ok(TicketRow {
                    id: r.get(0)?,
                    slug: r.get(1)?,
                    title: r.get(2)?,
                    details: r.get(3)?,
                    category: r.get(4)?,
                    priority: r.get(5)?,
                    status: r.get(6)?,
                    up_next: r.get::<_, i64>(7)? != 0,
                    tags: json_vec(r.get::<_, String>(8)?),
                    blocked_by: json_vec(r.get::<_, String>(9)?),
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    completed_at: r.get(12)?,
                    verified_at: r.get(13)?,
                    closed_at: r.get(14)?,
                    close_reason: r.get(15)?,
                    duplicate_of: r.get(16)?,
                    claimed_by: r.get(17)?,
                    worker_label: r.get(18)?,
                    claim_count: r.get::<_, i64>(19)? as u32,
                    legacy_number: r.get(20)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

// ---- helpers ---------------------------------------------------------------------

/// Every `<ULID>.md` path under `<store>/tickets/`.
fn ticket_files(store: &FsStore) -> Result<Vec<PathBuf>, IndexError> {
    let dir = store.root().join("tickets");
    let mut out = Vec::new();
    if dir.is_dir() {
        for shard in std::fs::read_dir(&dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(shard.path())? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    out.push(path);
                }
            }
        }
    }
    Ok(out)
}

fn parse_ticket(path: &Path, bytes: &[u8]) -> Result<Ticket, IndexError> {
    parse_file(&String::from_utf8_lossy(bytes)).map_err(|source| IndexError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// A SQLite WAL/SHM sidecar path (`<db>-wal`), for cleanup on a corrupt reopen.
fn sidecar(db_path: &Path, suffix: &str) -> PathBuf {
    let mut name = db_path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// SHA-256 hex of the file bytes — the change-detection content hash (§3.4).
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn enum_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(String::from))
        .unwrap_or_default()
}

use hotsheet_ticketing::ops::priority_rank;

fn ts(o: &Option<hotsheet_model::Timestamp>) -> Option<String> {
    o.as_ref().map(|x| x.as_str().to_string())
}

fn json_vec(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}

/// Turn free text into an FTS5 prefix-AND query (alphanumeric tokens + `*`), or
/// `None` if there's nothing to match. Only alphanumerics survive, so it can't inject.
fn fts_query(text: Option<&str>) -> Option<String> {
    let text = text?;
    let q = text
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("{t}*"))
        .collect::<Vec<_>>()
        .join(" ");
    (!q.is_empty()).then_some(q)
}

#[cfg(test)]
mod tests;
