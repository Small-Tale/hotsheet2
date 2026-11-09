//! HS2-2TD7M7: explicit unsupported detached-broker boundary on Windows.
//!
//! The server's normal Windows path uses its in-process `TerminalManager`. A
//! native authenticated broker transport can replace this boundary later.

use std::path::{Path, PathBuf};

use hotsheet_terminals::{BrokerRequest, BrokerResponse, BrokerTermInfo};

#[derive(Debug, Clone)]
pub struct TerminalBroker {
    pub socket: PathBuf,
    pub project: String,
}

fn unsupported() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "detached terminal broker is unavailable on this platform",
    )
}

impl TerminalBroker {
    pub fn ensure(_store_path: &Path) -> anyhow::Result<Self> {
        Err(unsupported().into())
    }

    pub fn discover(_store_path: &Path) -> Option<Self> {
        None
    }

    pub fn at(socket: impl Into<PathBuf>, project: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            project: project.into(),
        }
    }

    pub async fn call(&self, _req: BrokerRequest) -> std::io::Result<BrokerResponse> {
        Err(unsupported())
    }

    pub async fn info(&self, _id: &str) -> std::io::Result<Option<BrokerTermInfo>> {
        Err(unsupported())
    }

    pub async fn kill_all(&self) -> anyhow::Result<usize> {
        Err(unsupported().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_2td7m7_windows_broker_is_explicitly_unsupported() {
        assert!(TerminalBroker::ensure(Path::new("synthetic")).is_err());
        assert!(TerminalBroker::discover(Path::new("synthetic")).is_none());
    }
}
