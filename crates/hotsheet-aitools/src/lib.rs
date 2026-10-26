//! Hot Sheet 2 AI-tool host — the **behavioral** half of the plugin system
//! (`docs/12` §12.2, `docs/05` §5.3). The declarative loader/registry is
//! `hotsheet-plugins`; this crate holds the host-side capabilities that need process
//! I/O (via injected ports), starting with the **drive/transport** interface
//! (`docs/13`).
//!
//! v1 (HS2-106): the [`Drive`] trait + the [`SpawnDrive`] shape (Codex `exec`), over an
//! injected [`ProcessSpawner`] so it is fully testable against a fake. The
//! persistent-channel (Claude) drive, permission bridge, connection registry, and the
//! async `TurnEvent` stream land next.

pub mod acp;
pub mod appserver;
pub mod claude;
pub mod claude_transcript_usage;
pub mod codex;
pub mod codex_thread_watch;
pub mod drive;
pub mod host;
pub mod launch_safety;
pub mod live;
pub mod model_catalog;
pub mod permission;
pub mod permission_pattern;
pub mod permission_suggestions;
mod permission_vault;
/// Capability for subprocess test bootstrap; never selects a backend by itself.
#[doc(hidden)]
pub fn test_permission_vault_available() -> bool {
    cfg!(all(feature = "test-permission-vault", debug_assertions))
}

#[doc(hidden)]
pub fn enable_test_permission_vault() {
    #[cfg(all(feature = "test-permission-vault", debug_assertions))]
    permission_vault::enable_test_permission_vault();
}
pub mod ports;
pub mod probe;
mod procio;
pub mod registry;
pub mod safe_trigger;
pub mod session_push;
pub mod spawn;
pub mod system;
pub mod tool_progress;

pub use acp::{AcpDrive, AcpSession, AcpStdio, usage as acp_usage, validate_opencode_transcript};
pub use appserver::AppServerDrive;
pub use claude::{
    ChannelTiming, ClaudeChannel, ClaudeChannelDrive, ClaudeStreamSpawnOptions,
    ClaudeStreamTransport, claude_result_usage, claude_stream_events,
};
pub use codex::{
    CodexAppServer, CodexDaemonService, PermissionPolicy, ProxyTransport, StdioTransport,
    UdsWsTransport, codex_control_socket_path, ensure_codex_daemon, ensure_codex_daemon_in,
    notification_usage as codex_notification_usage, stop_codex_daemon_in,
    turn_usage as codex_turn_usage,
};
pub use drive::{
    BackingService, ClaudeChannelClient, DoneReason, Drive, DriveCtx, DriveError, DriveInfo,
    PermReq, Target, Transport, TurnControl, TurnEvent, TurnHandle, Usage,
};
pub use host::{TriggerError, TriggerRequest, Triggered, drive_for, trigger};
pub use live::{LiveError, LiveTrigger, TurnDone, run_trigger, run_trigger_controlled};
pub use model_catalog::{
    CommandModelCatalog, ModelCatalogCache, RuntimeModelCatalog, RuntimeModelCatalogSource,
    RuntimeModelSpec, discover_ai_tool_descriptors,
};
pub use permission::{
    DEFAULT_PERMISSION_TIMEOUT, Decision as PermissionDecision, Outcome as PermissionOutcome,
    PermissionAsk, PermissionBridge, Request as PermissionRequest, Resolved as PermissionResolved,
    Rule as PermissionRule, RuleInfo as PermissionRuleInfo, Scope as PermissionScope,
    SharedPermissionBridge, StoredRule as StoredPermissionRule,
    action_key_for as permission_action_key_for, append_rule as append_permission_rule,
    count_legacy_rules as count_legacy_permission_rules,
    discard_legacy_rules as discard_legacy_permission_rules,
    ensure_rules_file as ensure_permission_rules_file,
    load_existing_rules_with_key as load_existing_permission_rules_with_key,
    load_rules as load_permission_rules, load_rules_with_key as load_permission_rules_with_key,
    remove_rule as remove_permission_rule, rules_health as permission_rules_health,
    valid_user_bash_description,
};
pub use permission_pattern::{
    PatternKind as PermissionPatternKind, PatternSpec as PermissionPattern,
};
pub use permission_suggestions::{
    SecretSuggestion as PermissionSecretSuggestion, suggest_secret_spans,
};
pub use ports::{
    AcpClient, AppServerClient, AppServerError, AppServerOutcome, AppServerTurn, ProcessSignals,
    ProcessSpawner, RpcReader, RpcTransport, RpcWriter, SpawnSpec, SpawnedProcess,
};
pub use registry::{Connection, ConnectionRegistry, Role};
pub use safe_trigger::{
    ResumeSession, SafeTrigger, TriggerOptions, TurnRequest, prepare_trigger,
    prepare_trigger_with_home,
};
pub use spawn::{ContentMode, SpawnConfig, SpawnDrive};
pub use system::SystemSpawner;

#[cfg(test)]
mod tests;

/// The environment variable carrying a launched AI session's worker id (HS2-1VAW1C).
pub const WORKER_ID_ENV: &str = "HOTSHEET_WORKER_ID";

/// The acting role every Hot Sheet surface reads when a caller does not pass one
/// (HS2-RD4M29). Launchers set it to `ai` for the AI sessions they start, so the CLI and the
/// MCP shim identify those sessions' mutations as AI without per-call flags.
pub const ACTOR_ROLE_ENV: &str = "HOTSHEET_ACTOR_ROLE";

/// The acting id paired with [`ACTOR_ROLE_ENV`]; launchers set it to the session worker id.
pub const ACTOR_ID_ENV: &str = "HOTSHEET_ACTOR_ID";

/// The actor environment for an AI session a launcher starts (HS2-RD4M29).
#[must_use]
pub fn ai_session_actor_env(worker_id: &str) -> [(&'static str, String); 2] {
    [
        (ACTOR_ROLE_ENV, "ai".to_owned()),
        (ACTOR_ID_ENV, worker_id.to_owned()),
    ]
}

/// The worker id a launcher assigns an AI session: `<tool>-<session>`. The bundled
/// instructions tell the AI to claim with it, and the launcher releases every claim it holds
/// when the session ends.
pub fn session_worker_id(tool: &str, session: &str) -> String {
    format!("{tool}-{session}")
}

#[cfg(test)]
mod worker_id_tests {
    #[test]
    fn a_session_worker_id_names_the_tool_and_session() {
        assert_eq!(super::session_worker_id("claude", "01ABC"), "claude-01ABC");
    }
}
