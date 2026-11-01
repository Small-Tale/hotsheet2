//! HS2-HWC0FS: one cooperative publisher, account → metadata lock order.
//! Native SecretStore operations never run inside `Owner::update`.

use super::SecretError;
use super::metadata::StoredMetadata as Metadata;
#[cfg(any(unix, windows))]
use super::metadata::decode;
use std::path::Path;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(any(unix, windows))]
use platform::{CheckedLock, Directories, Image};
#[cfg(any(unix, windows))]
use sha2::{Digest, Sha256};
#[cfg(any(unix, windows))]
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
};
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
use windows as platform;

fn refusal() -> SecretError {
    SecretError::Unavailable("key metadata ownership could not be verified".into())
}

#[cfg(any(unix, windows))]
pub(super) struct Owner {
    requested: PathBuf,
    root: PathBuf,
    accounts: PathBuf,
    proof: Directories,
}
#[cfg(any(unix, windows))]
pub(super) struct Account<'a> {
    owner: &'a Owner,
    lock: CheckedLock,
}
#[cfg(any(unix, windows))]
impl Account<'_> {
    pub(super) fn check(&self) -> Result<(), SecretError> {
        self.owner.check()?;
        self.lock.check()
    }
}
#[cfg(any(unix, windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    BeforeStage,
    Staged,
    Written,
    Synced,
    BeforeRename,
    Renamed,
    Durable,
}

#[cfg(any(unix, windows))]
impl Owner {
    pub(super) fn prepare(requested: &Path) -> Result<Self, SecretError> {
        let (root, accounts, proof) = Directories::prepare(requested)?;
        let owner = Self {
            requested: requested.to_path_buf(),
            root,
            accounts,
            proof,
        };
        owner.check()?;
        Ok(owner)
    }
    fn check(&self) -> Result<(), SecretError> {
        if self.requested.canonicalize()? != self.root {
            return Err(refusal());
        }
        self.proof.check(&self.root, &self.accounts)
    }
    pub(super) fn account(&self, account: &str) -> Result<Account<'_>, SecretError> {
        self.check()?;
        let lock = self.proof.lock(
            self.accounts
                .join(format!("{:x}.lock", Sha256::digest(account.as_bytes()))),
        )?;
        let account = Account { owner: self, lock };
        account.check()?;
        Ok(account)
    }
    fn metadata_lock(&self) -> Result<CheckedLock, SecretError> {
        self.check()?;
        let lock = self.proof.lock(self.root.join("keys.json.lock"))?;
        self.check()?;
        Ok(lock)
    }
    fn load(&self) -> Result<(Metadata, Option<Image>), SecretError> {
        let path = self.root.join("keys.json");
        let (bytes, image) = self.proof.read(&path)?;
        let map = if image.is_none() {
            Metadata::new()
        } else {
            decode(&bytes).map_err(|source| SecretError::Parse {
                path: path.display().to_string(),
                source,
            })?
        };
        Ok((map, image))
    }
    pub(super) fn preflight(&self) -> Result<(), SecretError> {
        let lock = self.metadata_lock()?;
        self.load()?;
        self.check()?;
        lock.check()
    }
    pub(super) fn update<T>(
        &self,
        account: &Account<'_>,
        change: impl FnOnce(&mut Metadata) -> Result<(T, bool), SecretError>,
    ) -> Result<T, SecretError> {
        if !std::ptr::eq(self, account.owner) {
            return Err(refusal());
        }
        account.check()?;
        let lock = self.metadata_lock()?;
        let (mut map, before) = self.load()?;
        let (result, publish) = change(&mut map)?;
        self.check()?;
        lock.check()?;
        account.check()?;
        if publish {
            self.publish_with(account, &lock, before, &map, |_| Ok(()))?;
        }
        Ok(result)
    }
    // One private fault seam, no environment switch or SecretStore callback.
    fn publish_with(
        &self,
        account: &Account<'_>,
        lock: &CheckedLock,
        before: Option<Image>,
        map: &Metadata,
        mut observe: impl FnMut(Step) -> io::Result<()>,
    ) -> Result<(), SecretError> {
        observe(Step::BeforeStage)?;
        let mut staged = self.proof.stage(&self.root)?;
        observe(Step::Staged)?;
        let bytes = serde_json::to_vec_pretty(map).expect("metadata is serializable");
        staged.write_all(&bytes)?;
        staged.write_all(b"\n")?;
        observe(Step::Written)?;
        staged.as_file().sync_all()?;
        observe(Step::Synced)?;
        observe(Step::BeforeRename)?;
        self.check()?;
        account.check()?;
        lock.check()?;
        let target = self.root.join("keys.json");
        if self.proof.image_at(&target)? != before {
            return Err(refusal());
        }
        let published: File = self.proof.rename(staged, &target)?;
        observe(Step::Renamed)?;
        self.check()?;
        account.check()?;
        lock.check()?;
        self.proof.finish(&published, &self.root, &target)?;
        // Platform completion, not a universal power-loss durability claim.
        observe(Step::Durable)?;
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) struct Owner;
#[cfg(not(any(unix, windows)))]
pub(super) struct Account;
#[cfg(not(any(unix, windows)))]
impl Owner {
    pub(super) fn prepare(_: &Path) -> Result<Self, SecretError> {
        Err(refusal())
    }
    pub(super) fn account(&self, _: &str) -> Result<Account, SecretError> {
        Err(refusal())
    }
    pub(super) fn preflight(&self) -> Result<(), SecretError> {
        Err(refusal())
    }
    pub(super) fn update<T>(
        &self,
        _: &Account,
        _: impl FnOnce(&mut Metadata) -> Result<(T, bool), SecretError>,
    ) -> Result<T, SecretError> {
        Err(refusal())
    }
}
#[cfg(not(any(unix, windows)))]
impl Account {
    pub(super) fn check(&self) -> Result<(), SecretError> {
        Err(refusal())
    }
}
