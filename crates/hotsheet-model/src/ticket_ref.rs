//! A routable ticket identity across connected projects and ticket providers.

use serde::{Deserialize, Serialize};

/// Connection ids are project-scoped and native ids are provider-scoped. Retaining all
/// three parts lets a relationship survive a change of checkout or ticket store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectTicketRef {
    pub project_id: String,
    pub connection_id: String,
    pub native_id: String,
}

impl ProjectTicketRef {
    pub fn qualified(&self) -> String {
        format!(
            "@{}/{}:{}",
            self.project_id, self.connection_id, self.native_id
        )
    }

    pub fn from_qualified(value: &str) -> Option<Self> {
        let (project_id, ticket) = value.strip_prefix('@')?.split_once('/')?;
        let (connection_id, native_id) = ticket.split_once(':')?;
        if project_id.is_empty() || connection_id.is_empty() || native_id.is_empty() {
            return None;
        }
        Some(Self {
            project_id: project_id.to_string(),
            connection_id: connection_id.to_string(),
            native_id: native_id.to_string(),
        })
    }
}
