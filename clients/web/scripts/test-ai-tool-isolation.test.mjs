import { describe, expect, it } from 'vitest';

import { AI_TOOL_PROBES_ENV, isolateAiToolProbes } from './test-ai-tool-isolation.mjs';

describe('HS2-F6R2HQ AI-tool probe isolation', () => {
  it('runs every worker with installed AI-tool probes off', () => {
    expect(process.env[AI_TOOL_PROBES_ENV]).toBe('off');
  });

  it('turns probes off in a given environment without touching other variables', () => {
    const env = { PATH: '/usr/bin', [AI_TOOL_PROBES_ENV]: 'on' };
    expect(isolateAiToolProbes(env)).toEqual({ PATH: '/usr/bin', [AI_TOOL_PROBES_ENV]: 'off' });
  });
});
