import { describe, expect, it } from 'vitest';

import {
  createInteractionTracker,
  createLatencyBudget,
  describeViolation,
  devLatencyEnabled,
  interactionLabel,
  isLongRunningRequest,
  type LabelTarget,
  type LatencySample,
  requestRouteLabel,
  serverHandlerMs,
  serverTimingMs,
  summarizeViolation,
} from './dev-latency';

const interaction = (duration_ms: number): LatencySample => ({ kind: 'interaction', label: 'click', duration_ms });

describe('latency budget', () => {
  it('flags any sample over the 150 ms peak immediately', () => {
    const budget = createLatencyBudget();
    expect(budget.record(interaction(150))).toBeUndefined();
    const violation = budget.record(interaction(151));
    expect(violation?.reasons).toEqual(['peak']);
    expect(violation?.peak_budget_ms).toBe(150);
  });

  it('flags the rolling average once it crosses 100 ms and re-arms after recovering', () => {
    const budget = createLatencyBudget({ window: 5, minAverageSamples: 5 });
    // Four samples: below the minimum count, so no average judgement yet.
    for (const value of [140, 140, 140, 140]) expect(budget.record(interaction(value))).toBeUndefined();
    const crossed = budget.record(interaction(140));
    expect(crossed?.reasons).toEqual(['average']);
    expect(crossed?.average_ms).toBe(140);
    // Still above budget: not reported again for every sample.
    expect(budget.record(interaction(140))).toBeUndefined();
    // Recover below budget, then cross again: reported once more.
    for (const value of [10, 10, 10, 10, 10]) expect(budget.record(interaction(value))).toBeUndefined();
    // The window now mixes old fast samples with new slow ones; it crosses on the fourth.
    for (const value of [149, 149, 149]) expect(budget.record(interaction(value))).toBeUndefined();
    expect(budget.record(interaction(149))?.reasons).toEqual(['average']);
  });

  it('reports peak and average together and keeps interaction and request windows apart', () => {
    const budget = createLatencyBudget({ window: 3, minAverageSamples: 3 });
    budget.record(interaction(120));
    budget.record(interaction(120));
    expect(budget.record({ kind: 'request', label: 'GET /x', duration_ms: 20 })).toBeUndefined();
    expect(budget.record(interaction(200))?.reasons).toEqual(['peak', 'average']);
    expect(budget.average('request')).toBe(20);
    expect(budget.average('interaction')).toBeCloseTo(146.67, 1);
  });

  it('starts empty', () => {
    expect(createLatencyBudget().average('request')).toBe(0);
  });
});

describe('request classification', () => {
  it.each([
    ['POST', '/drive/connections/abc/turns'],
    ['POST', '/terminals'],
    ['POST', '/checkouts/x/tickets/HS-ABCD/attachments'],
    ['POST', '/setup/claude'],
    ['POST', '/commands/build/run'],
    ['POST', '/__hotsheet/project-api/p/lifecycle/restart'],
  ])('treats %s %s as expected long-running', (method, path) => {
    expect(isLongRunningRequest(method, path)).toBe(true);
  });

  it.each([
    ['GET', '/checkouts/x/tickets/HS-ABCD'],
    ['PATCH', '/tickets/HS-ABCD'],
    ['GET', '/checkouts/x/tickets/HS-ABCD/attachments'],
    ['GET', '/tickets?up_next=true'],
  ])('budgets %s %s', (method, path) => {
    expect(isLongRunningRequest(method, path)).toBe(false);
  });

  it('templates ids out of route labels', () => {
    expect(requestRouteLabel('patch', '/checkouts/hotsheet2-a1b2c3d4/tickets/HS2-9Z0Q8N?x=1')).toBe(
      'PATCH /checkouts/{id}/tickets/{id}',
    );
    expect(requestRouteLabel('GET', '/__hotsheet/project-api/proj-1/tickets/01M4YDRRN3FGFWDEH5RYFEP0YW')).toBe(
      'GET /tickets/{id}',
    );
    expect(requestRouteLabel('GET', '/checkouts/%2Fpath/corrupt-tickets/12')).toBe(
      'GET /checkouts/{id}/corrupt-tickets/{id}',
    );
    expect(requestRouteLabel('GET', '/commits/0123456789abcdef')).toBe('GET /commits/{id}');
    expect(requestRouteLabel('GET', '')).toBe('GET /');
  });

  it('reads the handler duration from Server-Timing', () => {
    expect(serverHandlerMs('admission;dur=1, handler;dur=12.5')).toBe(12.5);
    expect(serverHandlerMs('handler;desc="x";dur=3')).toBe(3);
    expect(serverHandlerMs('handler;dur=abc')).toBeUndefined();
    expect(serverHandlerMs('total;dur=4')).toBeUndefined();
    expect(serverHandlerMs(null)).toBeUndefined();
    expect(serverTimingMs('handler;dur=1, bridge;dur=27.25, upstream;dur=2', 'bridge')).toBe(27.25);
    expect(serverTimingMs('handler;dur=1', 'bridge')).toBeUndefined();
  });
});

function trackerHarness(options: { settleMs?: number; maxWaitMs?: number } = {}) {
  let clock = 0;
  const timers: Array<{ at: number; run: () => void }> = [],
    paints: Array<() => void> = [],
    completed: LatencySample[] = [];
  const tracker = createInteractionTracker({
    now: () => clock,
    afterPaint: (callback) => paints.push(callback),
    setTimer: (callback, ms) => timers.push({ at: clock + ms, run: callback }),
    onComplete: (sample) => completed.push(sample),
    ...options,
  });
  const advance = (ms: number) => {
    clock += ms;
    for (const timer of timers.filter((item) => item.at <= clock)) {
      timers.splice(timers.indexOf(timer), 1);
      timer.run();
    }
  };
  const paint = (ms = 0) => {
    clock += ms;
    for (const callback of paints.splice(0)) callback();
  };
  /** The main thread is busy: time passes but no timer gets to run. */
  const stall = (ms: number) => {
    clock += ms;
  };
  return { tracker, advance, paint, stall, completed };
}

describe('interaction tracker', () => {
  it('completes a request-free interaction at the paint after its settle window', () => {
    const { tracker, advance, paint, completed } = trackerHarness();
    tracker.begin('click button "Save"', 0);
    advance(50);
    expect(completed).toEqual([]);
    paint(16);
    expect(completed).toEqual([{ kind: 'interaction', label: 'click button "Save"', duration_ms: 66, requests: [] }]);
    expect(tracker.pendingCount()).toBe(0);
  });

  it('waits for every request the interaction started, then the paint', () => {
    const { tracker, advance, paint, completed } = trackerHarness();
    tracker.begin('click row', 0);
    advance(5);
    const first = tracker.requestStarted('GET /tickets/{id}', false);
    advance(10);
    const second = tracker.requestStarted('GET /tickets/{id}/backlinks', false);
    advance(50);
    paint();
    expect(completed).toEqual([]);
    first({ label: 'GET /tickets/{id}', duration_ms: 200, server_ms: 190 });
    paint();
    expect(completed).toEqual([]);
    advance(300);
    second({ label: '', duration_ms: 350 });
    paint(10);
    expect(completed).toHaveLength(1);
    expect(completed[0].duration_ms).toBe(375);
    expect(completed[0].requests).toEqual([
      { label: 'GET /tickets/{id}', duration_ms: 200, server_ms: 190 },
      { label: 'GET /tickets/{id}/backlinks', duration_ms: 350 },
    ]);
  });

  it('does not wait for long-running or late requests, and ignores repeated completion', () => {
    const { tracker, advance, paint, completed } = trackerHarness();
    tracker.begin('click Send', 0);
    const turn = tracker.requestStarted('POST /drive/{id}/turns', true);
    advance(60);
    const late = tracker.requestStarted('GET /late', false);
    paint(4);
    expect(completed.map((sample) => sample.duration_ms)).toEqual([64]);
    turn({ label: 'POST /drive/{id}/turns', duration_ms: 9_000 });
    late({ label: 'GET /late', duration_ms: 10 });
    late({ label: 'GET /late', duration_ms: 10 });
    paint();
    expect(completed).toHaveLength(1);
    expect(completed[0].requests).toEqual([]);
  });

  it('attributes a request started past the window while a busy main thread delayed the settle timer', () => {
    const { tracker, advance, paint, stall, completed } = trackerHarness();
    tracker.begin('click row', 0);
    // The click's own render runs for 120 ms, then issues the detail read before any timer runs.
    stall(120);
    const detail = tracker.requestStarted('GET /tickets/{id}', false);
    advance(0); // the overdue settle timer runs now
    paint();
    expect(completed).toEqual([]);
    advance(400);
    detail({ label: 'GET /tickets/{id}', duration_ms: 400 });
    paint(10);
    expect(completed).toHaveLength(1);
    expect(completed[0].duration_ms).toBe(530);
    expect(completed[0].requests).toEqual([{ label: 'GET /tickets/{id}', duration_ms: 400 }]);
    // Once the settle timer has run, a later request is not the click's.
    tracker.begin('click again', 530);
    advance(100);
    tracker.requestStarted('GET /later', false);
    paint();
    expect(completed.map((sample) => sample.label)).toEqual(['click row', 'click again']);
  });

  it('attributes a request to the newest open interaction only', () => {
    const { tracker, advance, paint, completed } = trackerHarness();
    tracker.begin('first', 0);
    advance(20);
    tracker.begin('second', 20);
    const request = tracker.requestStarted('GET /x', false);
    advance(40);
    paint();
    expect(completed.map((sample) => sample.label)).toEqual(['first']);
    advance(30);
    request({ label: 'GET /x', duration_ms: 70 });
    paint();
    expect(completed.map((sample) => sample.label)).toEqual(['first', 'second']);
    expect(completed[1].requests).toHaveLength(1);
  });

  it('reports an interaction that never finishes at the maximum wait', () => {
    const { tracker, advance, completed } = trackerHarness({ maxWaitMs: 1_000 });
    tracker.begin('stuck', 0);
    tracker.requestStarted('GET /hang', false);
    advance(1_000);
    expect(completed.map((sample) => sample.duration_ms)).toEqual([1_000]);
    advance(1_000);
    expect(completed).toHaveLength(1);
  });
});

describe('labels and switches', () => {
  it('names the targeted control', () => {
    const element = (tagName: string, attributes: Record<string, string>, text: string, control?: LabelTarget) => {
      const node: LabelTarget = {
        tagName,
        textContent: text,
        getAttribute: (name) => attributes[name] ?? null,
        closest: () => control ?? null,
      };
      return node;
    };
    const button = element('BUTTON', { 'aria-label': 'Close ticket' }, 'x');
    expect(interactionLabel(element('SPAN', {}, 'x', button), 'click')).toBe('click button "Close ticket"');
    const row = element('DIV', { role: 'row' }, '  Fix   the\n login bug ');
    expect(interactionLabel(element('SPAN', {}, '', row), 'click')).toBe('click div "Fix the login bug"');
    expect(interactionLabel(element('A', {}, 'a'.repeat(60)), 'click')).toBe(`click a "${'a'.repeat(47)}…"`);
    expect(interactionLabel(element('A', { title: 'Open' }, 'ignored'), 'key Enter on')).toBe('key Enter on a "Open"');
    expect(interactionLabel(element('P', { 'data-dev-latency-label': 'Board' }, ''), 'click')).toBe('click p "Board"');
    expect(interactionLabel(element('P', {}, ''), 'click')).toBe('click p');
    expect(interactionLabel(null, 'click')).toBe('click');
  });

  it('describes violations with their cause', () => {
    const base = { average_ms: 120, average_budget_ms: 100, peak_budget_ms: 150 } as const;
    expect(
      describeViolation({
        ...base,
        reasons: ['peak'],
        sample: {
          kind: 'interaction',
          label: 'click row',
          duration_ms: 812.4,
          requests: [
            { label: 'GET /a', duration_ms: 10 },
            { label: 'GET /tickets/{id}', duration_ms: 790, server_ms: 12 },
          ],
        },
      }),
    ).toBe(
      'Slow interaction: click row took 812 ms (peak budget 150 ms); slowest request GET /tickets/{id} 790 ms (server 12 ms)',
    );
    expect(describeViolation({ ...base, reasons: ['average'], sample: interaction(110) })).toBe(
      'Slow interaction: click took 110 ms (average 120 ms over 100 ms budget); no server request (client render)',
    );
    expect(
      describeViolation({
        ...base,
        reasons: ['peak'],
        sample: { kind: 'request', label: 'GET /x', duration_ms: 400, server_ms: 380 },
      }),
    ).toBe('Slow request: GET /x took 400 ms (peak budget 150 ms) (server 380 ms)');
  });

  it('summarizes violations briefly for the dev indicator', () => {
    const base = { average_ms: 120, average_budget_ms: 100, peak_budget_ms: 150 } as const;
    expect(
      summarizeViolation({ ...base, reasons: ['peak'], sample: { ...interaction(812.4), label: 'click div "Slow"' } }),
    ).toBe('Dev: slow interaction, 812 ms (budget 150 ms peak): click div "Slow"');
    expect(
      summarizeViolation(
        {
          ...base,
          reasons: ['average'],
          sample: { kind: 'request', label: `GET /${'x'.repeat(70)}`, duration_ms: 120 },
        },
        3,
      ),
    ).toBe(`Dev: slow request, 120 ms (budget 100 ms average): GET /${'x'.repeat(50)}… · 3 slow so far`);
  });

  it('runs in a dev browser, stays off under automation unless enabled, and honors off', () => {
    expect(devLatencyEnabled(null, false)).toBe(true);
    expect(devLatencyEnabled(null, true)).toBe(false);
    expect(devLatencyEnabled('on', true)).toBe(true);
    expect(devLatencyEnabled('off', false)).toBe(false);
  });
});
