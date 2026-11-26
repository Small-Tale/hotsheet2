//! Format-preserving edits of a user's TOML config, as setup makes to Codex's
//! `.codex/config.toml` (HS2-3MH5FH). Only Hot Sheet's own `[<parent>.<name>]` table is
//! replaced or removed; every other comment, key, ordering and blank line stays byte for
//! byte. Each edit re-parses its result and refuses one that does not mean exactly the
//! intended change, so a document shape `toml_edit` cannot express in place is never
//! rewritten lossily.

use toml_edit::{DocumentMut, Item, Table, Value};

/// Set `parent.name = entry`, creating a missing (or replacing a non-table) `parent`.
/// A replaced table keeps its place in the file and the comments above its header.
pub(crate) fn set_table(
    text: &str,
    parent: &str,
    name: &str,
    entry: &toml::Table,
) -> Result<String, String> {
    let mut expected = parse_table(text)?;
    let slot = expected
        .entry(parent.to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !slot.is_table() {
        *slot = toml::Value::Table(toml::Table::new());
    }
    if let Some(servers) = slot.as_table_mut() {
        servers.insert(name.to_string(), toml::Value::Table(entry.clone()));
    }

    let mut doc = parse_document(text)?;
    let root = doc.as_table_mut();
    if !root.get(parent).is_some_and(Item::is_table_like) {
        let mut table = Table::new();
        table.set_implicit(true);
        root.insert(parent, Item::Table(table));
    }
    let mut table = render_entry(entry)?;
    match root.get_mut(parent) {
        Some(Item::Table(servers)) => {
            if let Some(Item::Table(old)) = servers.get(name) {
                if let Some(position) = old.position() {
                    table.set_position(position);
                }
                *table.decor_mut() = old.decor().clone();
            }
            servers.insert(name, Item::Table(table));
        }
        Some(Item::Value(Value::InlineTable(servers))) => {
            servers.insert(name, Value::InlineTable(table.into_inline_table()));
        }
        _ => return Err(format!("`{parent}` is not a table")),
    }
    verified(doc.to_string(), &expected)
}

/// Remove `parent.name`, and `parent` itself when that leaves it empty. `Ok(None)` when
/// there is no such entry.
pub(crate) fn remove_table(text: &str, parent: &str, name: &str) -> Result<Option<String>, String> {
    let mut expected = parse_table(text)?;
    let Some(servers) = expected.get_mut(parent).and_then(toml::Value::as_table_mut) else {
        return Ok(None);
    };
    if servers.remove(name).is_none() {
        return Ok(None);
    }
    if servers.is_empty() {
        expected.remove(parent);
    }

    let mut doc = parse_document(text)?;
    let root = doc.as_table_mut();
    let Some(servers) = root.get_mut(parent).and_then(Item::as_table_like_mut) else {
        return Ok(None);
    };
    servers.remove(name);
    if servers.is_empty() {
        root.remove(parent);
    }
    verified(doc.to_string(), &expected).map(Some)
}

fn parse_table(text: &str) -> Result<toml::Table, String> {
    toml::from_str(text).map_err(|error| error.message().trim_end().to_string())
}

fn parse_document(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>()
        .map_err(|error| error.message().trim_end().to_string())
}

fn verified(out: String, expected: &toml::Table) -> Result<String, String> {
    if parse_table(&out).ok().as_ref() == Some(expected) {
        Ok(out)
    } else {
        Err("the edit could not be made without disturbing the rest of the file".into())
    }
}

/// `entry` in the same layout a fresh file gets (`toml::to_string_pretty`), as a table
/// with no document position, so it renders right after its preceding sibling table.
fn render_entry(entry: &toml::Table) -> Result<Table, String> {
    let pretty = toml::to_string_pretty(entry).map_err(|error| error.to_string())?;
    Ok(detach(parse_document(&pretty)?.as_table()))
}

/// A copy of `table` with every (nested) table's document position cleared.
fn detach(table: &Table) -> Table {
    let mut out = Table::new();
    for (key, item) in table.iter() {
        let item = match item {
            Item::Table(nested) => Item::Table(detach(nested)),
            other => other.clone(),
        };
        out.insert(key, item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> toml::Table {
        toml::from_str(
            "command = \"hotsheet-mcp\"\nargs = [\"--path\", \".\"]\nenv_vars = [\"A\"]\n\n[env]\nHOTSHEET_ACTOR_ROLE = \"ai\"\n",
        )
        .unwrap()
    }

    const USER: &str = "# Codex settings\nmodel = \"synthetic-model\"   # keep inline\n\n# Their server\n[mcp_servers.other]\ncommand = \"other-mcp\"\n\n[profiles.fast]\nmodel = \"small\"\n";

    #[test]
    fn hs2_3mh5fh_inserts_after_sibling_servers_keeping_every_other_byte() {
        let out = set_table(USER, "mcp_servers", "hotsheet", &entry()).unwrap();
        let split = USER.find("[profiles.fast]").unwrap();
        assert!(out.starts_with(&USER[..split]), "{out}");
        assert!(out.ends_with(&USER[split..]), "{out}");
        let inserted = &out[split..out.len() - (USER.len() - split)];
        assert!(inserted.starts_with("[mcp_servers.hotsheet]\n"), "{out}");
        assert!(inserted.contains("[mcp_servers.hotsheet.env]\n"), "{out}");
        let parsed: toml::Table = toml::from_str(&out).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["hotsheet"],
            toml::Value::Table(entry())
        );
        // Setting the same entry again changes nothing.
        assert_eq!(
            set_table(&out, "mcp_servers", "hotsheet", &entry()).unwrap(),
            out
        );
    }

    #[test]
    fn hs2_3mh5fh_replaces_a_stale_entry_in_place() {
        let text = "[mcp_servers.other]\ncommand = \"other-mcp\"\n\n# Hot Sheet (managed)\n[mcp_servers.hotsheet]\ncommand = \"stale-mcp\"\nstale_key = 1\n\n[mcp_servers.hotsheet.env]\nOLD = \"x\"\n\n# tail table\n[tail]\nkey = true\n";
        let out = set_table(text, "mcp_servers", "hotsheet", &entry()).unwrap();
        assert!(!out.contains("stale") && !out.contains("OLD"), "{out}");
        assert!(out.contains("# Hot Sheet (managed)\n[mcp_servers.hotsheet]\n"));
        assert!(
            out.ends_with("\n# tail table\n[tail]\nkey = true\n"),
            "{out}"
        );
        assert!(out.starts_with("[mcp_servers.other]\ncommand = \"other-mcp\"\n\n"));
    }

    #[test]
    fn hs2_3mh5fh_creates_and_replaces_the_parent_table() {
        let empty = set_table("", "mcp_servers", "hotsheet", &entry()).unwrap();
        assert!(empty.starts_with("[mcp_servers.hotsheet]\n"), "{empty}");
        let scalar = set_table("mcp_servers = 3\n", "mcp_servers", "hotsheet", &entry()).unwrap();
        assert!(!scalar.contains("= 3"), "{scalar}");
        let inline = set_table(
            "# c\nmcp_servers = { other = { command = \"o\" } }\n",
            "mcp_servers",
            "hotsheet",
            &entry(),
        )
        .unwrap();
        assert!(inline.starts_with("# c\nmcp_servers = {"), "{inline}");
        let dotted = set_table(
            "mcp_servers.other.command = \"o\" # dotted\n",
            "mcp_servers",
            "hotsheet",
            &entry(),
        )
        .unwrap();
        assert!(dotted.contains("# dotted"), "{dotted}");
        assert!(set_table("a = [", "mcp_servers", "hotsheet", &entry()).is_err());
    }

    #[test]
    fn hs2_3mh5fh_removes_only_the_named_entry() {
        let with = set_table(USER, "mcp_servers", "hotsheet", &entry()).unwrap();
        assert_eq!(
            remove_table(&with, "mcp_servers", "hotsheet").unwrap(),
            Some(USER.to_string())
        );
        assert_eq!(remove_table(USER, "mcp_servers", "hotsheet").unwrap(), None);
        assert_eq!(remove_table("a = 1\n", "mcp_servers", "x").unwrap(), None);
        // The emptied parent goes too; what is left is only the user's own content.
        let only = set_table("# mine\n", "mcp_servers", "hotsheet", &entry()).unwrap();
        let left = remove_table(&only, "mcp_servers", "hotsheet")
            .unwrap()
            .unwrap();
        assert!(!left.contains("mcp_servers"), "{left}");
        assert!(left.contains("# mine"), "{left}");
    }
}
