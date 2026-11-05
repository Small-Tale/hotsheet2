import { describe, expect, it } from 'vitest';

import {
  AI_TOOL_PROBES_ENV,
  HOST_HEALTH_SAMPLER_ENV,
  isolateAiToolProbes,
  isolateHostHealthSampler,
} from './test-ai-tool-isolation.mjs';

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
