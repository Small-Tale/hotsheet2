import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  AI_TOOL_PROBES_ENV,
  HOST_HEALTH_SAMPLER_ENV,
  isolateAiToolProbes,
  isolateHostHealthSampler,
  isolateMachineHome,
  MACHINE_HOME_ENV,
} from './test-ai-tool-isolation.mjs';

describe('HS2-34QFBQ machine-home isolation', () => {
  it('runs every worker under one per-run temporary machine home, never the real one', () => {
    const names = Object.keys(MACHINE_HOME_ENV);
    const root = dirname(resolve(process.env.HOTSHEET_HOME ?? ''));
    expect(resolve(process.env.HOTSHEET_HOME ?? '')).not.toBe(resolve(homedir(), '.hotsheet2'));
    expect(resolve(process.env.HOTSHEET_LEGACY_HOME ?? '')).not.toBe(resolve(homedir(), '.hotsheet'));
    expect(root.startsWith(resolve(tmpdir(), 'hotsheet-vitest-home-'))).toBe(true);
    for (const name of names) expect(process.env[name], name).toBe(join(root, MACHINE_HOME_ENV[name]));
  });

  it('overrides an inherited real home and leaves other variables alone', () => {
    const env = { PATH: '/usr/bin', HOTSHEET_HOME: join(homedir(), '.hotsheet2') };
    expect(isolateMachineHome('/run', env)).toEqual({
      PATH: '/usr/bin',
      HOTSHEET_HOME: join('/run', 'hotsheet-home'),
      HOTSHEET_LEGACY_HOME: join('/run', 'hotsheet-legacy-home'),
      HOTSHEET_CACHE_DIR: join('/run', 'hotsheet-cache'),
      HOTSHEET_CLAUDE_USER_CONFIG: join('/run', 'claude-user-config', '.claude.json'),
      HOTSHEET_LAUNCH_AGENTS_DIR: join('/run', 'launch-agents'),
    });
  });
});

describe('HS2-F6R2HQ AI-tool probe isolation', () => {
  it('runs every worker with installed AI-tool probes off', () => {
    expect(process.env[AI_TOOL_PROBES_ENV]).toBe('off');
  });

  it('turns probes off in a given environment without touching other variables', () => {
    const env = { PATH: '/usr/bin', [AI_TOOL_PROBES_ENV]: 'on' };
    expect(isolateAiToolProbes(env)).toEqual({ PATH: '/usr/bin', [AI_TOOL_PROBES_ENV]: 'off' });
  });
});

describe('HS2-DTAVQA host health sampler isolation', () => {
  it('runs every worker with the host health sampler off', () => {
    expect(process.env[HOST_HEALTH_SAMPLER_ENV]).toBe('off');
  });

  it('turns the sampler off in a given environment without touching other variables', () => {
    const env = { PATH: '/usr/bin' };
    expect(isolateHostHealthSampler(env)).toEqual({ PATH: '/usr/bin', [HOST_HEALTH_SAMPLER_ENV]: 'off' });
  });
});
