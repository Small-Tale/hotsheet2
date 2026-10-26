//! HS2-SFVWN5: disposable, explicitly activated native-port substitute. Never compiled
//! into release builds. Values are synthetic fixture data, scoped by HOTSHEET_HOME.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use super::SecretError;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub(super) fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

pub(super) fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

fn access<T>(
    account: &str,
    action: impl FnOnce(PathBuf) -> Result<T, SecretError>,
) -> Result<T, SecretError> {
    let home = std::env::var_os("HOTSHEET_HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SecretError::Unavailable(
                "synthetic credentials require an explicit HOTSHEET_HOME".into(),
            )
        })?;
    let directory = PathBuf::from(home).join("test-native-credentials");
    fs::create_dir_all(&directory)?;
    if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
        return Err(SecretError::Backend(
            "synthetic credential directory is a symlink".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("fixture.lock"))?;
    let _lock = crate::FileLock::acquire(file)?;
    action(directory.join(format!("{:x}.json", Sha256::digest(account.as_bytes()))))
}

pub(super) fn set(account: &str, value: &str) -> Result<(), SecretError> {
    access(account, |path| {
        let mut temporary =
            tempfile::NamedTempFile::new_in(path.parent().expect("fixture directory"))?;
        temporary.write_all(
            serde_json::to_string(value)
                .expect("string serialization")
                .as_bytes(),
        )?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    })
}

pub(super) fn get(account: &str) -> Result<Option<String>, SecretError> {
    access(account, |path| {
        let mut value = String::new();
        match fs::File::open(&path) {
            Ok(file) => {
                file.take(2 * 1024 * 1024).read_to_string(&mut value)?;
                serde_json::from_str(&value)
                    .map(Some)
                    .map_err(|source| SecretError::Parse {
                        path: path.display().to_string(),
                        source,
                    })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    })
}

pub(super) fn delete(account: &str) -> Result<bool, SecretError> {
    access(account, |path| match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    })
}
