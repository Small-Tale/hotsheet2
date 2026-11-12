//! Detected disks for host health (HS2-NHK8V8, `docs/host-health.md` "Monitored disks").
//!
//! Host health always watches the volume that holds the primary ticket store. Other local,
//! writable volumes are **detected**; a newly detected one waits for the user to include or
//! ignore it, and that choice is a machine-wide setting (`host_health.disks`) keyed by a stable
//! volume id, so it survives a removable drive's reconnect and a temporary absence. A volume can
//! also be added by path (HS2-KBVZV1), which covers mounts detection skips, such as a network
//! share. These definitions live in `ticketing` so the CLI can manage the same choices without
//! linking the server.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The machine-wide setting that holds every disk choice.
pub const DISKS_SETTING_KEY: &str = "host_health.disks";

/// The pseudo id of the volume holding the primary store; it is always monitored.
pub const STORE_VOLUME_ID: &str = "store";

/// One saved choice about a volume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskChoice {
    /// Whether the volume counts in host health.
    pub monitor: bool,
    /// The volume's name when the choice was saved, shown while it is absent.
    pub label: String,
    /// Where it was mounted when last seen.
    pub mount: String,
    /// Added by path rather than detected (HS2-KBVZV1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual: bool,
}

/// Every saved choice, by volume id.
pub type DiskChoices = BTreeMap<String, DiskChoice>;

/// Read the stored choices; anything unreadable (a hand-edited file) reads as no choices.
pub fn stored_choices(value: Option<&serde_json::Value>) -> DiskChoices {
    value
        .and_then(|value| serde_json::from_value::<DiskChoices>(value.clone()).ok())
        .map(|mut choices| {
            choices.remove(STORE_VOLUME_ID);
            choices
        })
        .unwrap_or_default()
}

/// One mounted volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Stable across reconnects: `uuid:<volume uuid>` when the OS reports one, else `mount:<path>`.
    pub id: String,
    pub label: String,
    pub mount: PathBuf,
    /// The device number of the mounted file system, to recognise the store's volume.
    pub device: u64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// The id for a volume without an OS volume UUID.
pub fn mount_id(mount: &Path) -> String {
    format!("mount:{}", mount.display())
}

/// A volume's display name: the last path component of its mount point.
pub fn label_for(mount: &Path) -> String {
    mount
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| mount.display().to_string())
}

/// Free and total bytes plus the device number of the file system holding `path`.
#[cfg(unix)]
pub fn space(path: &Path) -> Option<(u64, u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let device = std::fs::metadata(path).ok().map(|meta| {
        use std::os::unix::fs::MetadataExt;
        meta.dev()
    })?;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid NUL-terminated string and `stat` is a writable statvfs buffer.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it initialised the buffer.
    let stat = unsafe { stat.assume_init() };
    #[allow(clippy::useless_conversion)] // HS2-NHK8V8: statvfs field widths differ by platform.
    let block = u64::from(stat.f_frsize);
    #[allow(clippy::useless_conversion)] // HS2-NHK8V8: statvfs field widths differ by platform.
    let (available, blocks) = (u64::from(stat.f_bavail), u64::from(stat.f_blocks));
    Some((
        available.saturating_mul(block),
        blocks.saturating_mul(block),
        device,
    ))
}

#[cfg(not(unix))]
pub fn space(_path: &Path) -> Option<(u64, u64, u64)> {
    None
}

/// Resolve the volume that holds `path` (an added disk): the deepest detected or mounted volume
/// containing it, or the path itself as a mount when nothing else matches.
pub fn volume_for_path(path: &Path, detected: &[Volume]) -> Option<Volume> {
    let path = std::fs::canonicalize(path).ok()?;
    let (free_bytes, total_bytes, device) = space(&path)?;
    if let Some(volume) = detected.iter().find(|volume| volume.device == device) {
        return Some(volume.clone());
    }
    let mount = mount_point(&path, device);
    Some(Volume {
        id: mount_id(&mount),
        label: label_for(&mount),
        mount,
        device,
        free_bytes,
        total_bytes,
    })
}

/// The highest ancestor of `path` on the same device: its mount point.
fn mount_point(path: &Path, device: u64) -> PathBuf {
    let mut mount = path.to_path_buf();
    for ancestor in path.ancestors().skip(1) {
        match space(ancestor) {
            Some((_, _, parent)) if parent == device => mount = ancestor.to_path_buf(),
            _ => break,
        }
    }
    mount
}

/// Volume ids already looked up, by mount point and device, so a sampler that detects volumes
/// every tick asks the OS for a volume UUID only when a mount appears or changes.
#[derive(Debug, Default)]
pub struct VolumeIds {
    known: BTreeMap<(PathBuf, u64), String>,
}

impl VolumeIds {
    fn id(&mut self, mount: &Path, device: u64, lookup: impl FnOnce() -> Option<String>) -> String {
        self.known
            .entry((mount.to_path_buf(), device))
            .or_insert_with(|| {
                lookup()
                    .map(|uuid| format!("uuid:{uuid}"))
                    .unwrap_or_else(|| mount_id(mount))
            })
            .clone()
    }

    fn retain(&mut self, volumes: &[Volume]) {
        self.known.retain(|(mount, device), _| {
            volumes
                .iter()
                .any(|volume| volume.mount == *mount && volume.device == *device)
        });
    }
}

/// Every local, writable, user-visible volume other than system mounts. The caller drops the
/// store's own volume by its device number.
pub fn detected_volumes(ids: &mut VolumeIds) -> Vec<Volume> {
    let volumes = platform_volumes(ids);
    ids.retain(&volumes);
    volumes
}

/// Whether this platform can detect volumes; elsewhere only the store volume and added disks count.
pub const fn detection_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))] // HS2-NHK8V8
fn volume_at(
    mount: PathBuf,
    ids: &mut VolumeIds,
    lookup: impl FnOnce() -> Option<String>,
) -> Option<Volume> {
    let (free_bytes, total_bytes, device) = space(&mount)?;
    (total_bytes > 0).then(|| Volume {
        id: ids.id(&mount, device, lookup),
        label: label_for(&mount),
        mount,
        device,
        free_bytes,
        total_bytes,
    })
}

#[cfg(target_os = "macos")]
fn platform_volumes(ids: &mut VolumeIds) -> Vec<Volume> {
    let mut entries: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: `entries` is a writable out pointer; getmntinfo points it at a buffer it owns.
    let count = unsafe { libc::getmntinfo(&mut entries, libc::MNT_NOWAIT) };
    if count <= 0 || entries.is_null() {
        return Vec::new();
    }
    // SAFETY: getmntinfo returned `count` initialised entries at `entries`.
    let entries = unsafe { std::slice::from_raw_parts(entries, count as usize) };
    let text = |field: &[libc::c_char]| {
        // SAFETY: the kernel NUL-terminates every name within its fixed-size field.
        unsafe { std::ffi::CStr::from_ptr(field.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    entries
        .iter()
        .filter(|entry| macos_listed(entry.f_flags, &text(&entry.f_mntonname)))
        .filter_map(|entry| {
            let mount = PathBuf::from(text(&entry.f_mntonname));
            volume_at(mount.clone(), ids, || macos_volume_uuid(&mount))
        })
        .collect()
}

/// macOS: user volumes mount under `/Volumes`; skip read-only, network, and hidden mounts (local
/// Time Machine snapshots, the system's own volumes).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // HS2-NHK8V8: the pure filter is tested on every host.
pub(crate) fn macos_listed(flags: u32, mount: &str) -> bool {
    const MNT_RDONLY: u32 = 0x0000_0001;
    const MNT_LOCAL: u32 = 0x0000_1000;
    const MNT_DONTBROWSE: u32 = 0x0010_0000;
    mount.starts_with("/Volumes/")
        && flags & MNT_LOCAL != 0
        && flags & (MNT_RDONLY | MNT_DONTBROWSE) == 0
}

/// The volume UUID `diskutil` reports, stable across reconnects.
#[cfg(target_os = "macos")]
fn macos_volume_uuid(mount: &Path) -> Option<String> {
    let output = crate::git::launch("diskutil")
        .arg("info")
        .arg(mount)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_diskutil_uuid(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

/// The `Volume UUID:` line of `diskutil info`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // HS2-NHK8V8: the pure parser is tested on every host.
pub(crate) fn parse_diskutil_uuid(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("Volume UUID:"))
        .map(|uuid| uuid.trim().to_ascii_uppercase())
        .filter(|uuid| uuid.len() >= 32 && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
}

#[cfg(target_os = "linux")]
fn platform_volumes(ids: &mut VolumeIds) -> Vec<Volume> {
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let uuids = linux_uuids(Path::new("/dev/disk/by-uuid"));
    linux_listed(&mounts)
        .into_iter()
        .filter_map(|(device, mount)| {
            volume_at(mount, ids, || {
                std::fs::canonicalize(&device)
                    .ok()
                    .and_then(|device| uuids.get(&device).cloned())
            })
        })
        .collect()
}

/// Linux: real block-device file systems mounted read-write, without boot, snap, or loop mounts.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // HS2-NHK8V8: the pure filter is tested on every host.
pub(crate) fn linux_listed(mounts: &str) -> Vec<(PathBuf, PathBuf)> {
    const TYPES: [&str; 11] = [
        "ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk",
    ];
    let mut seen = std::collections::BTreeSet::new();
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (device, mount, kind, options) = (
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            );
            let mount = mount.replace("\\040", " ");
            let writable = options.split(',').any(|option| option == "rw");
            let hidden = ["/boot", "/snap", "/var/snap", "/efi"]
                .iter()
                .any(|prefix| mount == *prefix || mount.starts_with(&format!("{prefix}/")));
            (device.starts_with("/dev/")
                && !device.starts_with("/dev/loop")
                && TYPES.contains(&kind)
                && writable
                && !hidden
                // A device mounted twice (a bind mount) is one volume.
                && seen.insert(device.to_string()))
            .then(|| (PathBuf::from(device), PathBuf::from(mount)))
        })
        .collect()
}

/// `/dev/disk/by-uuid/<uuid>` symlinks, by resolved device path.
#[cfg(target_os = "linux")]
fn linux_uuids(directory: &Path) -> BTreeMap<PathBuf, String> {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let device = std::fs::canonicalize(entry.path()).ok()?;
            Some((device, entry.file_name().to_string_lossy().into_owned()))
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_volumes(_ids: &mut VolumeIds) -> Vec<Volume> {
    Vec::new()
}

/// Why a disk choice could not be made (HS2-KBVZV1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiskChoiceError {
    #[error("no volume holds {0}")]
    NotFound(String),
    #[error("the ticket store's disk is always monitored")]
    StoreVolume,
    #[error("unknown disk {0}")]
    UnknownDisk(String),
}

/// Read the saved choices from machine-wide settings.
pub fn load_choices(settings: &crate::Settings) -> Result<DiskChoices, crate::SettingsError> {
    Ok(stored_choices(
        settings
            .get(DISKS_SETTING_KEY, crate::Scope::Global)?
            .as_ref(),
    ))
}

/// Save the choices to machine-wide settings; none clears the setting.
pub fn save_choices(
    settings: &crate::Settings,
    choices: &DiskChoices,
) -> Result<(), crate::SettingsError> {
    // A map of plain strings and booleans always serializes.
    let value = serde_json::to_value(choices).unwrap_or_default();
    if choices.is_empty() || value.is_null() {
        settings.unset(DISKS_SETTING_KEY, crate::Scope::Global)?;
    } else {
        settings.set(DISKS_SETTING_KEY, value, crate::Scope::Global)?;
    }
    Ok(())
}

/// Add the volume holding `path` as monitored (HS2-KBVZV1), the one workflow behind
/// `POST /host-health/disks` and `hotsheet-cli host-disks add`. A detected volume keeps its
/// stable id; any other mount (a network share) is keyed by its mount point. The store's volume
/// is refused because it is always monitored. Returns the id and the saved choice.
pub fn add_disk(
    choices: &mut DiskChoices,
    path: &Path,
    store_root: &Path,
    detected: &[Volume],
) -> Result<(String, DiskChoice), DiskChoiceError> {
    let volume = volume_for_path(path, detected)
        .ok_or_else(|| DiskChoiceError::NotFound(path.display().to_string()))?;
    if space(store_root).is_some_and(|(_, _, device)| device == volume.device) {
        return Err(DiskChoiceError::StoreVolume);
    }
    let choice = DiskChoice {
        monitor: true,
        label: volume.label.clone(),
        mount: volume.mount.display().to_string(),
        manual: true,
    };
    choices.insert(volume.id.clone(), choice.clone());
    Ok((volume.id, choice))
}

/// Include or ignore a known volume: one already saved, or one of `present` (`(id, label, mount)`).
pub fn set_monitor(
    choices: &mut DiskChoices,
    id: &str,
    monitor: bool,
    present: &[(String, String, String)],
) -> Result<DiskChoice, DiskChoiceError> {
    if id == STORE_VOLUME_ID {
        return Err(DiskChoiceError::StoreVolume);
    }
    let choice = match (
        choices.get(id),
        present.iter().find(|(known, _, _)| known == id),
    ) {
        (Some(saved), seen) => DiskChoice {
            monitor,
            label: seen.map_or_else(|| saved.label.clone(), |(_, label, _)| label.clone()),
            mount: seen.map_or_else(|| saved.mount.clone(), |(_, _, mount)| mount.clone()),
            manual: saved.manual,
        },
        (None, Some((_, label, mount))) => DiskChoice {
            monitor,
            label: label.clone(),
            mount: mount.clone(),
            manual: false,
        },
        (None, None) => return Err(DiskChoiceError::UnknownDisk(id.to_string())),
    };
    choices.insert(id.to_string(), choice.clone());
    Ok(choice)
}

/// Forget a saved choice; a still-connected detected disk asks again. Returns whether one existed.
pub fn forget_disk(choices: &mut DiskChoices, id: &str) -> bool {
    choices.remove(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nhk8v8_stored_choices_tolerate_hand_edits_and_never_hold_the_store() {
        assert!(stored_choices(None).is_empty());
        assert!(stored_choices(Some(&serde_json::json!("nonsense"))).is_empty());
        let choices = stored_choices(Some(&serde_json::json!({
            "uuid:A": { "monitor": true, "label": "Backup", "mount": "/Volumes/Backup" },
            "store": { "monitor": false, "label": "x", "mount": "/" },
        })));
        assert_eq!(choices.keys().collect::<Vec<_>>(), ["uuid:A"]);
        assert!(!choices["uuid:A"].manual);
        let round = serde_json::to_value(&choices).unwrap();
        assert!(
            round["uuid:A"].get("manual").is_none(),
            "manual is written only when set"
        );
    }

    #[test]
    fn nhk8v8_macos_lists_only_local_writable_user_volumes() {
        const LOCAL: u32 = 0x0000_1000;
        assert!(macos_listed(LOCAL, "/Volumes/Backup"));
        assert!(
            !macos_listed(LOCAL, "/"),
            "the system volume shares the store's container"
        );
        assert!(
            !macos_listed(0, "/Volumes/Share"),
            "network mounts are added by path"
        );
        assert!(
            !macos_listed(LOCAL | 0x1, "/Volumes/Installer"),
            "read-only cannot fill"
        );
        assert!(!macos_listed(
            LOCAL | 0x0010_0000,
            "/Volumes/com.apple.TimeMachine.localsnapshots"
        ));
    }

    #[test]
    fn nhk8v8_diskutil_uuid_is_parsed_and_validated() {
        let text = "   Volume Name:               Backup\n   Volume UUID:               0a1b2c3d-4E5F-6789-ABCD-0123456789AB\n";
        assert_eq!(
            parse_diskutil_uuid(text).as_deref(),
            Some("0A1B2C3D-4E5F-6789-ABCD-0123456789AB")
        );
        assert_eq!(parse_diskutil_uuid("   Volume UUID:   n/a\n"), None);
        assert_eq!(parse_diskutil_uuid("no uuid here"), None);
    }

    #[test]
    fn nhk8v8_linux_lists_real_writable_block_volumes_once() {
        let mounts = "\
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
/dev/sdb1 /media/me/My\\040Backup exfat rw,nosuid 0 0
/dev/sdb1 /mnt/bind exfat rw 0 0
/dev/sdc1 /media/me/Installer iso9660 ro 0 0
/dev/loop3 /snap/core/1 squashfs ro 0 0
/dev/sdd1 /media/me/RO ext4 ro 0 0
tmpfs /run tmpfs rw 0 0
";
        let listed = linux_listed(mounts);
        assert_eq!(
            listed,
            vec![
                (PathBuf::from("/dev/nvme0n1p2"), PathBuf::from("/")),
                (
                    PathBuf::from("/dev/sdb1"),
                    PathBuf::from("/media/me/My Backup")
                ),
            ]
        );
    }

    #[test]
    fn nhk8v8_volume_ids_are_looked_up_once_per_mount_and_fall_back_to_the_mount() {
        let mut ids = VolumeIds::default();
        let mut lookups = 0;
        let mut lookup = |uuid: Option<&str>| {
            lookups += 1;
            uuid.map(str::to_owned)
        };
        let mount = Path::new("/Volumes/Backup");
        assert_eq!(ids.id(mount, 7, || lookup(Some("U1"))), "uuid:U1");
        assert_eq!(ids.id(mount, 7, || lookup(Some("U2"))), "uuid:U1");
        assert_eq!(ids.id(mount, 8, || lookup(None)), "mount:/Volumes/Backup");
        assert_eq!(lookups, 2);
        ids.retain(&[]);
        assert!(ids.known.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn nhk8v8_labels_and_added_paths_resolve_to_their_volume() {
        assert_eq!(label_for(Path::new("/Volumes/Backup")), "Backup");
        assert_eq!(label_for(Path::new("/")), "/");
        let dir = tempfile::tempdir().unwrap();
        let volume = volume_for_path(dir.path(), &[]).expect("a temp dir is on some volume");
        assert!(volume.total_bytes > 0);
        assert!(volume.id.starts_with("mount:"));
        assert!(
            dir.path()
                .canonicalize()
                .unwrap()
                .starts_with(&volume.mount)
        );
        assert!(volume_for_path(&dir.path().join("missing"), &[]).is_none());
    }
}
