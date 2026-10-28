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
