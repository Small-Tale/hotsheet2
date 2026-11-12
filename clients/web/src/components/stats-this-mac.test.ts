import { describe, expect, it } from 'vitest';

import { hostHealthFixture } from '../ux-demo/host-health-fixtures';
import { StatsThisMac } from './stats-this-mac';
import { GlobalWorkspaceSurface } from './workspace-composition-surfaces';

const cardOf = (html: string, id: string) => new RegExp(`<[^>]*data-card="${id}"[^>]*>`).exec(html)?.[0] ?? '';

describe('HS2-NS0AYR StatsThisMac', () => {
  it('labels the group with the server host, its platform, and the sample time', () => {
    const html = String(StatsThisMac({ state: { snapshot: hostHealthFixture('normal'), loading: false } }));
    expect(html).toContain('aria-label="Server Host studio"');
    expect(html).toContain('>studio<');
    expect(html).toMatch(/Server host · macOS · aarch64 · sampled \d/u);
  });

  it('keeps normal cards untinted and tints only crossed ones by severity', () => {
    const normal = String(StatsThisMac({ state: { snapshot: hostHealthFixture('normal'), loading: false } }));
    for (const id of ['disk', 'memory', 'cpu', 'power']) expect(cardOf(normal, id)).toContain('data-status="normal"');
    expect(normal).not.toContain('data-card="network"');
    const crossed = String(StatsThisMac({ state: { snapshot: hostHealthFixture('crossed'), loading: false } }));
    expect(cardOf(crossed, 'disk')).toMatch(/data-appearance="warning"/u);
    expect(cardOf(crossed, 'memory')).not.toMatch(/data-appearance="(warning|danger)"/u);
    expect(cardOf(crossed, 'network')).toMatch(/data-appearance="danger"/u);
    expect(crossed).toContain('full in ~8h 36m');
  });

  it('HS2-RBFR94 draws the CPU sparkline on a fixed percent scale, warning-toned once crossed', () => {
    const normal = String(StatsThisMac({ state: { snapshot: hostHealthFixture('normal'), loading: false } }));
    const sparklines = normal.match(/<svg[^>]*data-component="glance-sparkline"[^>]*>/gu) ?? [];
    expect(sparklines).toHaveLength(1);
    expect(sparklines[0]).toContain('data-tone="neutral"');
    expect(sparklines[0]).toContain('CPU busy percent, 5-minute averages over the last 45m');
    const crossed = String(StatsThisMac({ state: { snapshot: hostHealthFixture('crossed'), loading: false } }));
    expect(crossed).toMatch(/<svg[^>]*data-component="glance-sparkline"[^>]*data-tone="warning"/u);
  });

  it('renders unsupported signals as unsupported, never as values', () => {
    const html = String(StatsThisMac({ state: { snapshot: hostHealthFixture('unsupported'), loading: false } }));
    expect(html).toContain('Server host · Windows · x86_64');
    expect(html.match(/Not supported/gu)).toHaveLength(3);
    expect(html).toContain('Unavailable');
    expect(html).not.toContain(' free');
  });

  it('distinguishes loading, a failed first read, and a stale snapshot', () => {
    expect(String(StatsThisMac({ state: { loading: true } }))).toContain('Loading Server Host Health');
    expect(String(StatsThisMac({ state: { loading: false, error: 'offline' } }))).toContain(
      'Server host health is unavailable.',
    );
    const stale = String(
      StatsThisMac({ state: { snapshot: hostHealthFixture('normal'), loading: false, error: 'offline' } }),
    );
    expect(stale).toContain('Host health may be out of date.');
    expect(stale).toContain('data-card="disk"');
  });

  it('follows the Projects table and precedes Ticket flow and usage in both Stats scopes', () => {
    for (const projectName of ['demo', undefined]) {
      const html = String(
        GlobalWorkspaceSurface({
          kind: 'stats',
          projectName,
          analytics: [],
          rightNow: { scope: { kind: 'all' }, stores: 0, tiles: {} as never, rows: [] },
          thisMac: { state: { snapshot: hostHealthFixture('normal'), loading: false } },
        }),
      );
      const rightNow = html.indexOf('data-component="stats-right-now"');
      const thisMac = html.indexOf('data-component="stats-this-mac"');
      expect(rightNow).toBeGreaterThan(-1);
      expect(thisMac).toBeGreaterThan(rightNow);
      expect(thisMac).toBeLessThan(html.indexOf('data-component="stats-dashboard"'));
    }
  });
});
