import { describe, expect, it } from 'vitest';

import { MAX_PLAYWRIGHT_RETRIES, playwrightRetries, quarantined, quarantineGrepInvert } from './test-quarantine';

describe('flaky-test quarantine (HS2-3PX1E4)', () => {
  it('skips a quarantined test unless quarantined tests are requested', () => {
    expect(quarantined('HS2-ABC123', {})).toBe(true);
    expect(quarantined('HS2-ABC123', { HOTSHEET_RUN_QUARANTINED: '0' })).toBe(true);
    expect(quarantined('HS2-ABC123', { HOTSHEET_RUN_QUARANTINED: '1' })).toBe(false);
  });

  it('requires the fix ticket slug', () => {
    expect(() => quarantined('flaky', {})).toThrow(/fix ticket's slug/);
    expect(() => quarantined('HS2-abc123', {})).toThrow(/fix ticket's slug/);
  });

  it('drops @quarantine Playwright titles by default', () => {
    const invert = quarantineGrepInvert({});
    expect(invert?.test('drags a card @quarantine HS2-ABC123')).toBe(true);
    expect(invert?.test('drags a card')).toBe(false);
    expect(invert?.test('handles @quarantined-like words')).toBe(false);
    expect(quarantineGrepInvert({ HOTSHEET_RUN_QUARANTINED: '1' })).toBeUndefined();
  });

  it('retries Playwright tests only when the full CI suite asks, and at most twice (HS2-8YHY6V)', () => {
    expect(playwrightRetries({})).toBe(0);
    expect(playwrightRetries({ CI: 'true' })).toBe(0);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: '' })).toBe(0);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: '0' })).toBe(0);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: '1' })).toBe(1);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: ' 2 ' })).toBe(2);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: '9' })).toBe(MAX_PLAYWRIGHT_RETRIES);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: '-1' })).toBe(0);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: '1.5' })).toBe(0);
    expect(playwrightRetries({ HOTSHEET_PLAYWRIGHT_RETRIES: 'yes' })).toBe(0);
  });
});
