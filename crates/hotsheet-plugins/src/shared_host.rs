//! Shared session hosts (HS2-MRR6Z0, HS2-4J7WCV): a tool process that serves many sessions at
//! once, such as Codex's `codex app-server --managed-daemon`. MCP servers and hooks it starts
//! inherit the environment of whichever session started the host, so the Hot Sheet terminal
//! identity they see (`HOTSHEET_TERMINAL_ID` and the route-back) belongs to another session.
//! The `hotsheet-mcp` shim and `hotsheet-cli permission-hook` both classify their process
//! ancestry here and ignore that inherited identity under a shared host.

use crate::SharedMcpHost;

/// How many ancestor command lines the classifiers read. A hook or MCP server sits at most a
/// wrapper shell and a launcher below the Codex process that started it.
pub const ANCESTOR_DEPTH: usize = 4;

/// Whether a process runs under a host that serves many sessions at once.
///
/// `ancestors` holds the command lines of the process's parent, grandparent, and so on,
/// nearest first; `hosts` are the shared hosts plugins declare (for Codex, `codex app-server`,
/// the managed daemon). The nearest ancestor running one of the hosts' programs decides: it is
/// shared when a later argument is that host's subcommand, while the same program run
/// interactively hosts only its own session. No such ancestor means no shared host.
///
/// Arguments after a shell script flag (`sh -c …`, `bash -lc …`, `cmd /C …`) are a script, not
/// a program (HS2-4J7WCV): Codex runs hook commands through a shell, so the wrapper of
/// `hotsheet-cli permission-hook --agent codex` names `codex` without running it.
pub fn hosted_by_shared_host(ancestors: &[String], hosts: &[SharedMcpHost]) -> bool {
    for command in ancestors {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        for (index, token) in tokens.iter().enumerate() {
            if index > 0 && is_shell_script_flag(token) {
                break;
            }
            let name = token.rsplit(['/', '\\']).next().unwrap_or(token);
            let name = name.strip_suffix(".exe").unwrap_or(name);
            let matching: Vec<&SharedMcpHost> =
                hosts.iter().filter(|host| host.program == name).collect();
            if !matching.is_empty() {
                return matching
                    .iter()
                    .any(|host| tokens[index + 1..].contains(&host.subcommand.as_str()));
            }
        }
    }
    false
}

/// `-c`, `-lc`, `-ic`, … (a POSIX shell's script flag, possibly combined with other short
/// flags) or `/C` (`cmd.exe`).
fn is_shell_script_flag(token: &str) -> bool {
    token.eq_ignore_ascii_case("/c")
        || token.strip_prefix('-').is_some_and(|flags| {
            !flags.is_empty()
                && !flags.starts_with('-')
                && flags.chars().all(|c| c.is_ascii_alphabetic())
                && flags.contains('c')
        })
}

/// Up to `max` ancestor command lines of this process, nearest first. Read with `ps`, which
/// macOS and Linux both provide (macOS has no `/proc`); an unreadable process table yields what
/// was read so far, and Windows yields none.
pub fn ancestor_commands(max: usize) -> Vec<String> {
    #[cfg(unix)]
    {
        let mut out = Vec::new();
        let mut pid = std::os::unix::process::parent_id();
        while out.len() < max && pid > 1 {
            let Some((parent, command)) = std::process::Command::new("ps")
                .args(["-o", "ppid=,args=", "-p", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| {
                    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    let (ppid, command) = line.split_once(char::is_whitespace)?;
                    Some((ppid.trim().parse::<u32>().ok()?, command.trim().to_owned()))
                })
            else {
                break;
            };
            out.push(command);
            pid = parent;
        }
        out
    }
    #[cfg(not(unix))]
    {
        let _ = max;
        Vec::new()
    }
}

/// Whether a hook run for `agent` must ignore the inherited terminal identity (HS2-4J7WCV).
/// Only the agent's own declared shared hosts are checked (every plugin's when the agent is
/// unknown), and `ancestors` is called only when there are some, so a hook of a tool without a
/// shared host (Claude) never reads the process table.
pub fn hook_identity_untrusted(
    agent: Option<&str>,
    hosts_of: impl Fn(Option<&str>) -> Vec<SharedMcpHost>,
    ancestors: impl FnOnce() -> Vec<String>,
) -> bool {
    let hosts = hosts_of(agent);
    !hosts.is_empty() && hosted_by_shared_host(&ancestors(), &hosts)
}

/// The shared hosts `agent`'s plugin declares, or every plugin's when `agent` is `None` or
/// names no installed plugin.
pub fn shared_hosts_for(agent: Option<&str>) -> Vec<SharedMcpHost> {
    match agent.and_then(crate::find) {
        Some(plugin) => plugin.manifest.mcp.shared_hosts,
        None => crate::shared_mcp_hosts(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(commands: &[&str]) -> Vec<String> {
        commands.iter().map(|c| (*c).to_owned()).collect()
    }

    fn codex() -> Vec<SharedMcpHost> {
        vec![SharedMcpHost {
            program: "codex".into(),
            subcommand: "app-server".into(),
        }]
    }

    /// HS2-MRR6Z0: the nearest ancestor running a declared host program decides.
    #[test]
    fn only_a_declared_shared_host_ancestor_marks_the_host_shared() {
        let hosts = codex();
        let shared = |commands: &[&str]| hosted_by_shared_host(&chain(commands), &hosts);
        // Observed on 0.166.0: the managed daemon parents every daemon-hosted thread's server.
        assert!(shared(&[
            "/u/.codex/packages/app-server-daemon/releases/0.166.0/bin/codex app-server --listen unix:// --managed-daemon",
        ]));
        // An npm install runs the native binary under node; a wrapper shell may sit between.
        assert!(shared(&[
            "/bin/sh -c hotsheet-mcp --project .",
            "node /usr/local/lib/node_modules/@openai/codex/bin/codex app-server",
        ]));
        // A TUI hosting its own session (a --config override or --no-daemon), even when a
        // later ancestor is an app-server.
        assert!(!shared(&[
            "codex --model gpt-6 --config model_reasoning_effort=\"high\"",
            "/x/codex app-server --managed-daemon",
        ]));
        assert!(!shared(&["C:\\tools\\codex.exe --no-daemon"]));
        assert!(shared(&["C:\\tools\\codex.exe app-server"]));
        // Not a declared host (Claude, a shell), an unreadable process table, a lookalike
        // name, or the subcommand before the program.
        assert!(!shared(&["claude --resume", "-zsh"]));
        assert!(!shared(&[]));
        assert!(!shared(&["mycodex app-server"]));
        assert!(!shared(&["app-server-wrapper codex"]));
        // No declared hosts: nothing is shared.
        assert!(!hosted_by_shared_host(&chain(&["codex app-server"]), &[]));
    }

    /// HS2-4J7WCV: Codex runs hooks through a shell whose script names `--agent codex`. That
    /// script is not the nearest Codex ancestor; the process that started the shell is.
    #[test]
    fn a_shell_wrapper_naming_codex_in_its_script_does_not_hide_the_host() {
        let hosts = codex();
        let shared = |commands: &[&str]| hosted_by_shared_host(&chain(commands), &hosts);
        for wrapper in [
            "/bin/sh -c hotsheet-cli permission-hook --agent codex",
            "bash -lc hotsheet-cli permission-hook --agent codex",
            "cmd.exe /D /S /C hotsheet-cli permission-hook --agent codex",
        ] {
            assert!(
                shared(&[wrapper, "/x/codex app-server --managed-daemon"]),
                "{wrapper}"
            );
            assert!(!shared(&[wrapper, "codex --no-daemon"]), "{wrapper}");
            assert!(
                !shared(&[
                    wrapper,
                    "codex --no-daemon --config model_reasoning_effort=\"high\""
                ]),
                "{wrapper}"
            );
        }
        // A program named before the script flag still counts, and Codex's own `-c` (its
        // config override flag) follows the program, so the TUI is still found first.
        assert!(!shared(&["codex -c model_reasoning_effort=\"high\""]));
        assert!(shared(&[
            "/bin/sh /tmp/fake/codex app-server --managed-daemon"
        ]));
        // Long options and option values are not script flags.
        assert!(is_shell_script_flag("-c"));
        assert!(is_shell_script_flag("-lc"));
        assert!(is_shell_script_flag("/C"));
        assert!(!is_shell_script_flag("--config"));
        assert!(!is_shell_script_flag("-"));
        assert!(!is_shell_script_flag("-l"));
        assert!(!is_shell_script_flag("-c1"));
    }

    /// HS2-4J7WCV: the hook guard reads the process table only for an agent with shared hosts.
    #[test]
    fn the_hook_guard_checks_ancestry_only_for_agents_with_shared_hosts() {
        let hosts_of = |agent: Option<&str>| match agent {
            Some("claude") => Vec::new(),
            _ => codex(),
        };
        let daemon = || chain(&["sh -c hook", "/x/codex app-server --managed-daemon"]);
        let tui = || chain(&["sh -c hook", "codex --no-daemon"]);
        assert!(hook_identity_untrusted(Some("codex"), hosts_of, daemon));
        assert!(!hook_identity_untrusted(Some("codex"), hosts_of, tui));
        assert!(hook_identity_untrusted(None, hosts_of, daemon));
        assert!(!hook_identity_untrusted(Some("claude"), hosts_of, || {
            panic!("a tool without shared hosts never reads its ancestry")
        }));
    }

    /// HS2-4J7WCV: the bundled Codex plugin's hosts are what an installed Codex hook checks;
    /// Claude declares none.
    #[test]
    fn bundled_plugins_declare_codex_app_server_as_their_only_shared_host() {
        assert_eq!(shared_hosts_for(Some("codex")), codex());
        assert!(shared_hosts_for(Some("claude")).is_empty());
        assert!(shared_hosts_for(None).contains(&codex()[0]));
        assert!(shared_hosts_for(Some("not-a-plugin")).contains(&codex()[0]));
    }
}
