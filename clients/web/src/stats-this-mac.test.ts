import { describe, expect, it } from 'vitest';

import { parseHostHealth } from './host-health';
import { cpuTrend, formatBytes, hostPlatform, thisMacCards } from './stats-this-mac';
import { hostHealthFixture } from './ux-demo/host-health-fixtures';

const card = (scenario: Parameters<typeof hostHealthFixture>[0], id: string) =>
  thisMacCards(hostHealthFixture(scenario)).find((item) => item.id === id);

describe('HS2-NS0AYR This Mac cards', () => {
  it('fixtures match the server contract', () => {
    for (const scenario of ['normal', 'crossed', 'unsupported'] as const)
      expect(() => parseHostHealth(hostHealthFixture(scenario))).not.toThrow();
  });

  it('shows disk, memory, CPU, and power when normal; network and thermal stay hidden', () => {
    const cards = thisMacCards(hostHealthFixture('normal'));
    expect(cards.map((item) => item.id)).toEqual(['disk', 'memory', 'cpu', 'power']);
    expect(cards.every((item) => item.status === 'normal')).toBe(true);
    expect(card('normal', 'disk')).toMatchObject({ headline: '212 GB free', details: ['steady', 'of 494 GB'] });
    expect(card('normal', 'memory')).toMatchObject({ headline: 'Normal', details: ['swap 1.2 GB · steady'] });
    expect(card('normal', 'cpu')).toMatchObject({ headline: '34%', details: ['top: cargo (88%)'] });
    expect(card('normal', 'power')).toMatchObject({
      headline: 'Kept awake',
      details: ['on AC · lid open · sleep assertion held'],
    });
  });

  it('carries crossed thresholds with rate, projection, and the degraded network', () => {
    const cards = thisMacCards(hostHealthFixture('crossed'));
    expect(cards.map((item) => [item.id, item.status])).toEqual([
      ['disk', 'warning'],
      ['memory', 'normal'],
      ['cpu', 'warning'],
      ['power', 'normal'],
      ['network', 'critical'],
    ]);
    expect(card('crossed', 'disk')).toMatchObject({
      headline: '18 GB free',
      details: ['−2.1 GB/h · full in ~8h 36m', 'of 494 GB'],
    });
    expect(card('crossed', 'cpu')?.details).toEqual(['top: cargo (410%)', 'busy for 16m']);
    expect(card('crossed', 'network')).toMatchObject({ headline: 'No network route', details: [] });
  });

  it('shows thermal only while throttled', () => {
    const snapshot = hostHealthFixture('normal');
    snapshot.signals.thermal = {
      status: 'warning',
      reason: 'throttled',
      since: null,
      throttled: true,
      cpu_speed_limit: 70,
    };
    expect(thisMacCards(snapshot).at(-1)).toMatchObject({
      id: 'thermal',
      headline: 'Throttled',
      details: ['CPU speed limit 70%'],
    });
  });

  it('says unsupported or unavailable and never shows a value', () => {
    const cards = thisMacCards(hostHealthFixture('unsupported'));
    expect(cards.map((item) => [item.id, item.status, item.headline])).toEqual([
      ['disk', 'unsupported', 'Not supported'],
      ['memory', 'unsupported', 'Not supported'],
      ['cpu', 'unsupported', 'Not supported'],
      ['power', 'unavailable', 'Unavailable'],
    ]);
    expect(cards.flatMap((item) => item.details).join(' ')).not.toMatch(/\d/u);
  });

  it.each([
    [{ on_battery: true, battery_percent: 54, lid_closed: false, sleep_prevented: false }, 'On battery · 54%'],
    [{ on_battery: true, battery_percent: 12, lid_closed: true }, 'Lid closed on battery'],
    [{ on_battery: false, sleep_prevented: false, reason: 'idle_sleep_allowed' }, 'May sleep when idle'],
    [{ on_battery: false, sleep_prevented: false, idle_sleep_minutes: 0 }, 'On power'],
    [{ on_battery: null, sleep_prevented: null, lid_closed: null }, 'Unknown'],
  ])('names the power state %j as %s', (power, headline) => {
    const snapshot = hostHealthFixture('normal');
    snapshot.signals.power = { ...snapshot.signals.power, ...power };
    expect(thisMacCards(snapshot)[3]?.headline).toBe(headline);
  });

  it('formats bytes and the host platform', () => {
    expect(formatBytes(512 * 1024 ** 2)).toBe('512 MB');
    expect(formatBytes(1.25 * 1024 ** 2)).toBe('1.3 MB');
    expect(formatBytes(3 * 1024 ** 3)).toBe('3 GB');
    expect(hostPlatform(hostHealthFixture('normal'))).toBe('macOS · aarch64');
    expect(hostPlatform(hostHealthFixture('unsupported'))).toBe('Windows · x86_64');
  });

  it('HS2-RBFR94 draws the CPU hour from the first sample, naming gaps, and nothing without two samples', () => {
    expect(card('normal', 'cpu')?.trend).toEqual({
      values: [12, 18, 9, 22, 41, 37, 28, 31, 34],
      label: 'CPU busy percent, 5-minute averages over the last 45m',
    });
    const history = (average_percent: (number | null)[]) => ({ bucket_seconds: 300, ends_at: null, average_percent });
    expect(cpuTrend(history([null, 10, null, 30.4]))).toEqual({
      values: [10, 0, 30],
      label: 'CPU busy percent, 5-minute averages over the last 15m, 0 where the server took no sample',
    });
    expect(cpuTrend(undefined)).toBeUndefined();
    expect(cpuTrend(history([null, null]))).toBeUndefined();
    expect(cpuTrend(history([null, 40]))).toBeUndefined();
    // An unsupported or unavailable CPU card never shows a trend.
    expect(card('unsupported', 'cpu')?.trend).toBeUndefined();
  });

  it('HS2-CPFBWP replaces the sampled CPU with a live frame and adds the core count and per-core values', () => {
    const live = {
      seq: 4,
      status: 'live' as const,
      sampled_at: '2026-11-06T10:47:02Z',
      busy_percent: 61.4,
      core_percent: [90, 32.8],
      interval_ms: 2000,
    };
    const snapshot = hostHealthFixture('normal');
    const cpu = thisMacCards(snapshot, live).find((item) => item.id === 'cpu');
    expect(cpu).toMatchObject({ headline: '61%', details: ['2 cores · top: cargo (88%)'], cores: [90, 32.8] });
    // Anything but a live frame leaves the sampled reading alone.
    for (const status of ['starting', 'unavailable', 'unsupported'] as const)
      expect(thisMacCards(snapshot, { ...live, status, busy_percent: null, core_percent: [] })[2]).toMatchObject({
        headline: '34%',
        details: ['top: cargo (88%)'],
      });
    expect(thisMacCards(snapshot, undefined)[2]?.cores).toBeUndefined();
    // A live frame shows even while the sampled CPU reader is unavailable.
    snapshot.signals.cpu = { ...snapshot.signals.cpu, status: 'unavailable', busy_percent: null, top_process: null };
    expect(thisMacCards(snapshot, live)[2]).toMatchObject({ headline: '61%', details: ['2 cores'] });
    expect(thisMacCards(snapshot)[2]?.headline).toBe('Unavailable');
  });
});
