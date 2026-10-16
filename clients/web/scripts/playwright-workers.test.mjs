import { describe, expect, it } from 'vitest';

import { lowerPlaywrightPriority, localPlaywrightWorkers, playwrightNiceness } from './playwright-workers.mjs';

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

describe('local Playwright CPU priority (HS2-FVT34A)', () => {
  it('lowers a normal-priority local run to the default niceness', () => {
    expect(playwrightNiceness({ current: 0 })).toBe(10);
  });

  it('honors an override, and 0 or a negative value opts out', () => {
    expect(playwrightNiceness({ current: 0, override: '15' })).toBe(15);
    expect(playwrightNiceness({ current: 0, override: '40' })).toBe(19);
    expect(playwrightNiceness({ current: 0, override: '0' })).toBeUndefined();
    expect(playwrightNiceness({ current: 0, override: '-5' })).toBeUndefined();
    expect(playwrightNiceness({ current: 0, override: 'abc' })).toBe(10);
  });

  it('never raises an already lowered priority', () => {
    expect(playwrightNiceness({ current: 10 })).toBeUndefined();
    expect(playwrightNiceness({ current: 15 })).toBeUndefined();
    expect(playwrightNiceness({ current: 5 })).toBe(10);
  });

  it('leaves CI alone unless an override asks for a niceness', () => {
    expect(playwrightNiceness({ current: 0, ci: true })).toBeUndefined();
    expect(playwrightNiceness({ current: 0, ci: true, override: '5' })).toBe(5);
  });

  it('applies the niceness and swallows platform errors', () => {
    const calls = [];
    lowerPlaywrightPriority({ getPriority: () => 0, setPriority: (n) => calls.push(n) }, {});
    expect(calls).toEqual([10]);
    lowerPlaywrightPriority({ getPriority: () => 0, setPriority: (n) => calls.push(n) }, { CI: 'true' });
    expect(calls).toEqual([10]);
    expect(() =>
      lowerPlaywrightPriority(
        {
          getPriority: () => 0,
          setPriority: () => {
            throw new Error('EACCES');
          },
        },
        {},
      ),
    ).not.toThrow();
  });
});
