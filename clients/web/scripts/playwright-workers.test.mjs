import { describe, expect, it } from 'vitest';

import { localPlaywrightWorkers } from './playwright-workers.mjs';

describe('local Playwright worker count (HS2-MHPHZB)', () => {
  it('uses half the cores on an idle machine', () => {
    expect(localPlaywrightWorkers({ cpus: 10, load1: 0.5 })).toBe(5);
    expect(localPlaywrightWorkers({ cpus: 10, load1: 5.9 })).toBe(5);
  });

  it('gives back one worker per core of load beyond the idle half, down to three', () => {
    expect(localPlaywrightWorkers({ cpus: 10, load1: 6 })).toBe(4);
    expect(localPlaywrightWorkers({ cpus: 10, load1: 7.5 })).toBe(3);
    expect(localPlaywrightWorkers({ cpus: 10, load1: 40 })).toBe(3);
    expect(localPlaywrightWorkers({ cpus: 16, load1: 10 })).toBe(6);
  });

  it('never exceeds half the cores on a small machine', () => {
    expect(localPlaywrightWorkers({ cpus: 4, load1: 30 })).toBe(2);
    expect(localPlaywrightWorkers({ cpus: 1, load1: 0 })).toBe(1);
  });

  it('honors an explicit override and leaves CI on the Playwright default', () => {
    expect(localPlaywrightWorkers({ cpus: 10, load1: 40, override: '8' })).toBe(8);
    expect(localPlaywrightWorkers({ cpus: 10, load1: 40, override: 'abc' })).toBe(3);
    expect(localPlaywrightWorkers({ cpus: 10, load1: 40, override: '0' })).toBe(3);
    expect(localPlaywrightWorkers({ cpus: 10, load1: 40, ci: true })).toBeUndefined();
    expect(localPlaywrightWorkers({ cpus: 10, load1: 40, ci: true, override: '2' })).toBe(2);
  });
});
