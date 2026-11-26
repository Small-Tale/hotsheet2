import { describe, expect, it } from 'vitest';

import {
  costText,
  type CostTotals,
  leadTimeText,
  moneyText,
  parseProgressSummary,
  progressRows,
  projectRows,
} from './orchestration-progress';
import { demoTimeline } from './ux-demo/orchestration-fixtures';

const cost = (rest: Partial<CostTotals>): CostTotals => ({
  known_usd: 0,
  complete: false,
  items_known: 0,
  items_unknown: 0,
  unpriced_events: 0,
  unknown_reasons: {},
  ...rest,
});

describe('orchestration progress and cost (HS2-XJEH08)', () => {
  it('never reads an unknown cost as zero, and labels a total partial unless complete', () => {
    expect(costText(undefined)).toBe('Unknown (not reported)');
    expect(costText(cost({}))).toBe('Unknown (no work yet)');
    expect(costText(cost({ items_unknown: 2, unknown_reasons: { no_usage_recorded: 2 } }))).toBe(
      'Unknown (no usage recorded)',
    );
    expect(costText(cost({ known_usd: 12.4, complete: true, items_known: 3 }))).toBe('$12.40');
    expect(
      costText(
        cost({
          known_usd: 12.4,
          items_known: 3,
          items_unknown: 2,
          unknown_reasons: { no_usage_recorded: 1, target_remote: 1 },
        }),
      ),
    ).toBe('$12.40 so far · partial: 2 work items have no cost (no usage recorded, target remote)');
    expect(
      costText(cost({ known_usd: 3, items_known: 1, items_unknown: 1, unknown_reasons: { cost_not_read: 1 } })),
    ).toBe('$3.00 so far · partial: 1 work item has no cost (cost not read)');
    expect(costText(cost({ known_usd: 3, items_known: 1, unpriced_events: 1 }))).toBe(
      'At least $3.00 so far · partial: 1 event has no price',
    );
    expect(costText(cost({ known_usd: 3, items_known: 1, unpriced_events: 4 }))).toContain('4 events have no price');
    expect(costText(cost({ items_unknown: 1 }))).toBe('Unknown (not reported)');
  });

  it('keeps money’s cents and sub-cent digits', () => {
    expect(moneyText(0.0042)).toBe('$0.0042');
    expect(moneyText(1234.5)).toBe('$1,234.50');
    expect(moneyText(0)).toBe('$0.00');
  });

  it('averages lead time over the known runs and counts the unknown ones with their reason', () => {
    expect(leadTimeText(undefined)).toBe('Unknown (not reported)');
    expect(leadTimeText({ known: 0, unknown: 0, total_ms: 0, unknown_reasons: {} })).toBe('Unknown (no run yet)');
    expect(leadTimeText({ known: 0, unknown: 2, total_ms: 0, unknown_reasons: { run_unfinished: 2 } })).toBe(
      'Unknown (run unfinished)',
    );
    expect(leadTimeText({ known: 3, unknown: 0, total_ms: 3 * 52 * 60_000, unknown_reasons: {} })).toBe(
      'About 52 min on average over 3 runs',
    );
    expect(leadTimeText({ known: 1, unknown: 1, total_ms: 150 * 60_000, unknown_reasons: { run_unfinished: 1 } })).toBe(
      'About 2 h 30 min on average over 1 run · 1 unknown (run unfinished)',
    );
    expect(leadTimeText({ known: 1, unknown: 0, total_ms: 120 * 60_000, unknown_reasons: {} })).toBe(
      'About 2 h on average over 1 run',
    );
    expect(leadTimeText({ known: 1, unknown: 0, total_ms: 1_000, unknown_reasons: {} })).toBe(
      'Under a minute on average over 1 run',
    );
  });

  it('counts the timeline’s work and reads the summary’s totals and per-project figures', () => {
    const timeline = demoTimeline('running');
    const summary = parseProgressSummary({
      totals: {
        lead_time: { known: 2, unknown: 3, total_ms: 2 * 60 * 60_000, unknown_reasons: { run_unfinished: 3 } },
        cost: {
          known_usd: 8.1,
          complete: false,
          items_known: 2,
          items_unknown: 1,
          unpriced_events: 0,
          unknown_reasons: { no_usage_recorded: 1 },
        },
      },
      targets: [
        { target_id: 'api', cost: { known_usd: 8.1, complete: true, items_known: 2 } },
        { target_id: 'docs', cost: { items_unknown: 1, unknown_reasons: { no_usage_recorded: 1 } } },
        { cost: {} },
      ],
    });
    expect(progressRows(timeline, summary)).toEqual([
      { label: 'Done', value: '2 of 10' },
      { label: 'Working now', value: '3' },
      { label: 'Waiting', value: '5 (2 for you)' },
      { label: 'Lead time', value: 'About 1 h on average over 2 runs · 3 unknown (run unfinished)' },
      { label: 'Cost', value: '$8.10 so far · partial: 1 work item has no cost (no usage recorded)' },
    ]);
    expect(projectRows(timeline, summary)).toEqual([
      { label: 'API', value: '2 done · 1 working · $8.10' },
      // HS2-70MHXJ: a project the summary leaves out has no cost recorded, never "not read".
      { label: 'Web', value: '0 done · 1 working · no cost recorded yet' },
      { label: 'Docs', value: '0 done · 1 working · cost unknown (no usage recorded)' },
    ]);
    // Before the summary is read nothing is claimed.
    expect(progressRows(timeline, undefined).slice(3)).toEqual([
      { label: 'Lead time', value: 'Reading…' },
      { label: 'Cost', value: 'Reading…' },
    ]);
    expect(projectRows(timeline, undefined)[0].value).toBe('2 done · 1 working · reading…');
    // HS2-70MHXJ: a failed read says so in plain words, and a project with nothing yet says no work yet.
    expect(progressRows(timeline, undefined, true).slice(3)).toEqual([
      { label: 'Lead time', value: 'Unknown (the summary could not be read)' },
      { label: 'Cost', value: 'Unknown (the summary could not be read)' },
    ]);
    expect(projectRows(timeline, undefined, true)[0].value).toBe(
      '2 done · 1 working · cost unknown (could not be read)',
    );
    const fresh = { ...timeline, waves: [] };
    expect(projectRows(fresh, parseProgressSummary({ totals: {}, targets: [] })).map((row) => row.value)).toEqual(
      timeline.targets.map(() => '0 done · 0 working · no work yet'),
    );
    for (const rows of [projectRows(fresh, parseProgressSummary({})), progressRows(fresh, summary)])
      for (const row of rows) expect(row.value).not.toMatch(/not read/);
    expect(parseProgressSummary({})).toEqual({ totals: { lead_time: undefined, cost: undefined }, targets: {} });
  });
});
