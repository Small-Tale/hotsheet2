import { describe, expect, it } from 'vitest';

import { HOME_OUTAGE_REASONS, homeOutageBanners, type TimelineHomeOutage } from './orchestration-home-outage';
import { parseOrchestrationTimeline } from './orchestration-timeline';
import { DEMO_NOW, demoClock, demoTimeline } from './ux-demo/orchestration-fixtures';

const outage = (rest: Partial<TimelineHomeOutage> = {}): TimelineHomeOutage => ({
  home_target_id: 'api',
  sites: ['web'],
  since_ms: DEMO_NOW - 5 * 60_000,
  reason: 'unreachable',
  detail: 'unreachable',
  announced: true,
  ...rest,
});

const timeline = (outages?: TimelineHomeOutage[]) => ({ ...demoTimeline('running'), home_outages: outages });

describe('a home site that cannot coordinate its project (HS2-EX594V)', () => {
  it('shows nothing while every home is available', () => {
    expect(homeOutageBanners(timeline(), demoClock)).toEqual([]);
    expect(homeOutageBanners(timeline([]), demoClock)).toEqual([]);
    // A home whose "back" notification is still to go out is not down.
    expect(homeOutageBanners(timeline([outage({ since_ms: undefined, returned_at_ms: DEMO_NOW })]), demoClock)).toEqual(
      [],
    );
  });

  it('names the project, since when, why, what still runs and what to do', () => {
    const [banner] = homeOutageBanners(timeline([outage()]), demoClock);
    expect(banner.key).toBe('api');
    expect(banner.title).toBe('Home Site of API Is Unavailable');
    expect(banner.detail).toContain(`Since ${demoClock(DEMO_NOW - 5 * 60_000)}.`);
    expect(banner.detail).toContain('Its machine cannot be reached.');
    expect(banner.detail).toContain('no failover');
    expect(banner.detail).toContain('Work on Web still runs');
    expect(banner.detail).toContain('claims its ticket for its own session');
    expect(banner.detail.endsWith('(unreachable)')).toBe(true);
  });

  it('words every reason with its own next step', () => {
    const details = HOME_OUTAGE_REASONS.map(
      (reason) => homeOutageBanners(timeline([outage({ reason, detail: undefined })]), demoClock)[0].detail,
    );
    expect(new Set(details).size).toBe(HOME_OUTAGE_REASONS.length);
    expect(details[0]).toContain('Enable it in Settings › Orchestration');
    expect(details[1]).toContain('Bind it again');
    expect(details[3]).toContain('coordination pool');
    // No reason reads as unreachable; a refusal backoff adds no code.
    expect(
      homeOutageBanners(timeline([outage({ reason: undefined, detail: 'refusal_backoff' })]), demoClock)[0].detail,
    ).toMatch(/cannot be reached\. .*paired\.$/);
  });

  it('lists every other site and falls back to ids for unknown targets', () => {
    const [many] = homeOutageBanners(
      timeline([outage({ home_target_id: 'mystery', sites: ['web', 'docs', 'extra'] })]),
      demoClock,
    );
    expect(many.title).toBe('Home Site of mystery Is Unavailable');
    expect(many.detail).toContain('Work on Web, Docs and extra still runs');
    const [none] = homeOutageBanners(timeline([outage({ sites: [] })]), demoClock);
    expect(none.detail).toContain('Work on no other site still runs');
  });

  it('keeps one banner per home, in order', () => {
    const banners = homeOutageBanners(
      timeline([outage(), outage({ home_target_id: 'web', sites: ['api'], reason: 'disabled' })]),
      demoClock,
    );
    expect(banners.map((banner) => banner.key)).toEqual(['api', 'web']);
  });

  it('reads home_outages from the timeline and refuses a drifted one', () => {
    const wire = JSON.parse(JSON.stringify(timeline([outage()])));
    expect(parseOrchestrationTimeline(wire).home_outages?.[0].home_target_id).toBe('api');
    expect(parseOrchestrationTimeline(JSON.parse(JSON.stringify(timeline()))).home_outages).toBeUndefined();
    for (const drift of [
      { home_target_id: 3, sites: [] },
      { home_target_id: 'api', sites: 'web' },
      { home_target_id: 'api', sites: [], reason: 'gone' },
      { home_target_id: 'api', sites: [], since_ms: -1 },
    ])
      expect(() => parseOrchestrationTimeline({ ...wire, home_outages: [drift] })).toThrow();
  });
});
