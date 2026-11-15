import { join } from 'node:path';

// HS2-F6R2HQ: browser and Node suites start real Hot Sheet servers and CLIs, whose AI-tool
// discovery would otherwise execute the developer's installed tools (`codex --version`, the
// Codex app-server `model/list`, `opencode models`, `agy models`) with their real
// credentials and network access. Rust tests get the same switch from `.cargo/config.toml`
// (HS2-7W3D7V). Called from each runner config before workers start, so every worker and
// every binary it spawns from `process.env` inherits it.
export const AI_TOOL_PROBES_ENV = 'HOTSHEET_AI_TOOL_PROBES';

export function isolateAiToolProbes(env = process.env) {
  env[AI_TOOL_PROBES_ENV] = 'off';
  return env;
}

// HS2-DTAVQA: real servers started by browser and Node suites must not sample this host, or
// host-dependent `host_health` events would reach the fixtures' change streams.
export const HOST_HEALTH_SAMPLER_ENV = 'HOTSHEET_HOST_HEALTH_SAMPLER';

export function isolateHostHealthSampler(env = process.env) {
  env[HOST_HEALTH_SAMPLER_ENV] = 'off';
  return env;
}

// HS2-XTQHPK: no test may reach the OS keychain. Servers and CLIs that browser and Node
// suites spawn are not test builds, and the keychain is per user rather than per
// HOTSHEET_HOME, so a credential read raises a macOS password prompt. The switch only
// refuses native credentials; it never enables synthetic ones. Rust tests get it from
// `.cargo/config.toml`.
export const NATIVE_SECRETS_ENV = 'HOTSHEET_NATIVE_SECRETS';

export function isolateNativeSecrets(env = process.env) {
  env[NATIVE_SECRETS_ENV] = 'deny';
  return env;
}

// HS2-34QFBQ: a unit test that misses an injected seam can reach code that resolves the
// machine home from `HOTSHEET_HOME` (falling back to `~/.hotsheet2`), the HS1 home, the
// platform cache, or Claude Code's user config, and could start or write the developer's
// real server and stores. Hot Sheet-launched agent shells export `HOTSHEET_HOME` as the real
// home, so an inherited value is not isolation either. Like `.cargo/config.toml`'s `force`
// (HS2-T9VXEP), this overrides any inherited value with paths under a per-run temporary
// root. Tests that need a fresh home still set their own.
export const MACHINE_HOME_ENV = Object.freeze({
  HOTSHEET_HOME: 'hotsheet-home',
  HOTSHEET_LEGACY_HOME: 'hotsheet-legacy-home',
  HOTSHEET_CACHE_DIR: 'hotsheet-cache',
  HOTSHEET_CLAUDE_USER_CONFIG: join('claude-user-config', '.claude.json'),
  HOTSHEET_LAUNCH_AGENTS_DIR: 'launch-agents',
});

export function isolateMachineHome(root, env = process.env) {
  for (const [name, relative] of Object.entries(MACHINE_HOME_ENV)) env[name] = join(root, relative);
  return env;
}
