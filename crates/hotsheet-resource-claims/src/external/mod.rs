//! External claim-backend plugins (HS2-322ZSZ, `docs/36` §36.16): a directory under
//! `<HOTSHEET_HOME>/claim-backends/<id>/` whose command serves each claim operation as
//! one JSON request on stdin and one JSON result line on stdout. The plugin is the
//! backend: it owns atomic acquire, the fencing counter and the backend clock. Hot Sheet
//! owns everything a plugin must never see: renewal tokens (only a digest crosses),
//! metadata keys, and the approval of the plugin's code.

pub mod approval;
mod backend;
pub mod manifest;
pub mod runner;

pub use approval::{
    Approval, ApprovalState, PLUGIN_ROOT_DIR, approve, approve_if_new, plugin_root, require,
    state as approval_state,
};
pub use backend::{
    ExternalBackend, PROTOCOL_VERSION, SharedSecrets, backend_for, backend_with, check_settings,
    load_plugin, resolve_settings, secret_account,
};
pub use manifest::{
    MANIFEST_FILE, ManifestWait, PluginManifest, RESERVED_IDS, SettingSpec, SettingType, load_all,
    valid_plugin_id,
};
pub use runner::{CommandRunner, LiveRunner, ProcessOutput, RunError};
