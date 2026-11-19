import { describe, expect, it } from 'vitest';

import type { OrchestrationCapabilities } from './api';
import { aiToolsLabel, gpuLabel, systemLabel, toolchainsLabel } from './orchestration-capabilities';
import { parseOrchestrationCapacity, parseOrchestrationSites } from './server-response-parsers';

const windows: OrchestrationCapabilities = {
  os: 'windows',
  os_version: '10.0.26100',
  arch: 'arm64',
  gpu: { present: true, vendors: ['nvidia', 'qualcomm'] },
  ai_tools: [{ tool: 'codex', version: '0.46.0' }, { tool: 'claude' }],
  toolchains: [
    { name: 'cargo', installed: true, version: '1.97.0' },
    { name: 'xcode', installed: false },
    { name: 'git', installed: true },
  ],
};
const bare: OrchestrationCapabilities = { os: 'linux', arch: 'x86_64' };

describe('HS2-ZY4Z4G machine capability labels', () => {
  it('names the system with its release and architecture', () => {
    expect(systemLabel(windows)).toBe('Windows 10.0.26100, arm64');
    expect(systemLabel(bare)).toBe('Linux, x86_64');
    expect(systemLabel({ ...bare, os_distribution: 'ubuntu', os_version: '24.04' })).toBe(
      'Linux (ubuntu 24.04), x86_64',
    );
    expect(systemLabel({ ...bare, os_distribution: 'arch' })).toBe('Linux (arch), x86_64');
    expect(systemLabel({ os: 'macos', os_version: '15.1', arch: 'arm64' })).toBe('macOS 15.1, arm64');
    expect(systemLabel({ os: 'other', arch: 'riscv64' })).toBe('Other OS, riscv64');
  });

  it('tells a missing GPU apart from one the machine could not read', () => {
    expect(gpuLabel(windows)).toBe('NVIDIA, Qualcomm');
    expect(gpuLabel(bare)).toBe('Unknown');
    expect(gpuLabel({ ...bare, gpu: { present: false } })).toBe('None');
    expect(gpuLabel({ ...bare, gpu: { present: true } })).toBe('Present');
    expect(gpuLabel({ ...bare, gpu: { present: true, vendors: ['future'] } })).toBe('future');
  });

  it('lists only installed toolchains and AI tools, with versions when known', () => {
    expect(toolchainsLabel(windows)).toBe('cargo 1.97.0, git');
    expect(toolchainsLabel(bare)).toBe('Unknown');
    expect(toolchainsLabel({ ...bare, toolchains: [{ name: 'xcode', installed: false }] })).toBe('None installed');
    expect(aiToolsLabel(windows, (tool) => (tool === 'codex' ? 'Codex' : 'Claude'))).toBe('Codex 0.46.0, Claude');
    expect(aiToolsLabel(windows)).toBe('codex 0.46.0, claude');
    expect(aiToolsLabel(bare)).toBe('Unknown');
    expect(aiToolsLabel({ ...bare, ai_tools: [] })).toBe('None installed');
  });
});

describe('HS2-ZY4Z4G capacity snapshot parsing', () => {
  const snapshot = {
    sampled_at_ms: 1,
    memory: { free_mb: 1, total_mb: 2 },
    cpu: { load_percent: 1, logical_cpus: 2 },
    disk: { free_mb: 1, total_mb: 2 },
    tools: null,
    sessions: null,
    claims: null,
    claim_namespaces: [],
  };

  it('reads an older host without capabilities and a newer one with them', () => {
    expect(parseOrchestrationCapacity(snapshot).capabilities).toBeUndefined();
    expect(parseOrchestrationCapacity({ ...snapshot, capabilities: windows }).capabilities).toEqual(windows);
    expect(parseOrchestrationCapacity({ ...snapshot, capabilities: bare }).capabilities).toEqual(bare);
  });

  it('refuses a malformed report rather than showing it', () => {
    for (const capabilities of [
      null,
      { arch: 'arm64' },
      { ...bare, gpu: { vendors: [] } },
      { ...bare, toolchains: [{ name: 'cargo' }] },
      { ...bare, ai_tools: [{ version: '1.0' }] },
      { ...bare, os_version: null },
    ])
      expect(() => parseOrchestrationCapacity({ ...snapshot, capabilities })).toThrow();
  });
});

describe('HS2-D4G81Q sites view parsing', () => {
  const site = {
    target_id: 'W1',
    label: 'Widgets',
    project: true,
    destination: null,
    confirmed_at_ms: null,
    last_failure: null,
    stale: true,
  };
  const view = (sites: unknown[]) => ({ revision: 'r1', projects: [{ project_id: 'W1', label: 'Widgets', sites }] });

  it('reads a site without its machine, with it, and with a failed read', () => {
    expect(parseOrchestrationSites(view([site])).projects[0].sites[0].capabilities).toBeUndefined();
    const read = parseOrchestrationSites(
      view([
        { ...site, capabilities: windows },
        { ...site, target_id: 'W2', capabilities_error: 'target_unavailable' },
      ]),
    );
    expect(read.projects[0].sites.map((item) => [item.capabilities, item.capabilities_error])).toEqual([
      [windows, undefined],
      [undefined, 'target_unavailable'],
    ]);
    expect(parseOrchestrationSites({ revision: null, projects: [] }).projects).toEqual([]);
  });

  it('refuses a malformed site rather than showing it', () => {
    for (const bad of [
      { ...site, capabilities: { arch: 'arm64' } },
      { ...site, capabilities_error: null },
      { ...site, project: 'yes' },
    ])
      expect(() => parseOrchestrationSites(view([bad]))).toThrow();
  });
});
