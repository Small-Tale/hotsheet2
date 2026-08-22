//! The **people roster** (`docs/10` §10.2, HS2-20) — a **committed** `people.json` at the
//! store root that maps a person's git identity (email) to a friendly name (and optionally a
//! GitHub login). Because it's committed (Tier A shared, `docs/02` §2.11) it syncs to the
//! whole team, so `dana@example.com` renders as "Dana" everywhere without a separate accounts
//! system. Assignment itself uses the raw git email; this is only the display/seed layer.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One roster entry, keyed by git `email`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Person {
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
}

/// The committed `people.json` roster.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Roster {
    #[serde(default)]
    pub people: Vec<Person>,
}

impl Roster {
    /// Load the store's `people.json`, or an empty roster if absent (a fresh store has none).
    pub fn load(store_root: &Path) -> io::Result<Roster> {
        match std::fs::read_to_string(Self::path(store_root)) {
            Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Roster::default()),
            Err(e) => Err(e),
        }
    }

    /// Write `people.json` (committed with the next store commit / sync).
    pub fn save(&self, store_root: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(Self::path(store_root), json + "\n")
    }

    /// Add or update a person (matched by email); returns whether an existing entry changed.
    pub fn upsert(&mut self, person: Person) -> bool {
        if let Some(existing) = self.people.iter_mut().find(|p| p.email == person.email) {
            let changed = *existing != person;
            *existing = person;
            changed
        } else {
            self.people.push(person);
            true
        }
    }

    /// The friendly display name for an email — the roster `name` if present, else the email.
    pub fn display_name<'a>(&'a self, email: &'a str) -> &'a str {
        self.people
            .iter()
            .find(|p| p.email == email)
            .and_then(|p| p.name.as_deref())
            .unwrap_or(email)
    }

    fn path(store_root: &Path) -> PathBuf {
        store_root.join("people.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_is_empty_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Roster::load(dir.path()).unwrap().people.is_empty());

        let mut r = Roster::default();
        assert!(r.upsert(Person {
            email: "dana@x.co".into(),
            name: Some("Dana".into()),
            github: None
        }));
        r.save(dir.path()).unwrap();

        let back = Roster::load(dir.path()).unwrap();
        assert_eq!(back.people.len(), 1);
        assert_eq!(back.display_name("dana@x.co"), "Dana");
        assert_eq!(
            back.display_name("nobody@x.co"),
            "nobody@x.co",
            "unknown → the email"
        );
    }

    #[test]
    fn upsert_updates_in_place() {
        let mut r = Roster::default();
        r.upsert(Person {
            email: "a@x.co".into(),
            name: None,
            github: None,
        });
        let changed = r.upsert(Person {
            email: "a@x.co".into(),
            name: Some("Alex".into()),
            github: None,
        });
        assert!(changed);
        assert_eq!(r.people.len(), 1, "same email updates, not duplicates");
        assert_eq!(r.display_name("a@x.co"), "Alex");
    }
}
