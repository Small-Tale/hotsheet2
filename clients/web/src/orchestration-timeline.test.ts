import { describe, expect, it } from 'vitest';

import { parseOrchestrationTimeline } from './orchestration-timeline';
import { demoTimeline } from './ux-demo/orchestration-fixtures';

describe('orchestration timeline response contract (HS2-J89JC7)', () => {
  it('accepts every phase the server answers', () => {
    for (const phase of ['no_projects', 'no_plan', 'proposed', 'running', 'paused'] as const) {
      const timeline = demoTimeline(phase);
      expect(parseOrchestrationTimeline(JSON.parse(JSON.stringify(timeline)))).toEqual(timeline);
    }
  });

  it('refuses a drifted response instead of rendering half a view', () => {
    const valid = JSON.parse(JSON.stringify(demoTimeline('running'))) as Record<string, unknown>;
    const drifted = (change: (value: Record<string, unknown>) => void) => {
      const copy = JSON.parse(JSON.stringify(valid)) as Record<string, unknown>;
      change(copy);
      return () => parseOrchestrationTimeline(copy);
    };
    const firstItem = (value: Record<string, unknown>) =>
      ((value.waves as Array<Record<string, unknown>>)[1].items as Array<Record<string, unknown>>)[0];
    expect(drifted((value) => delete value.declared)).toThrow(/orchestration timeline/);
    expect(drifted((value) => (value.phase = 'launching'))).toThrow();
    expect(drifted((value) => (value.running_since_ms = null))).toThrow();
    expect(drifted((value) => ((value.waves as Array<Record<string, unknown>>)[0].kind = 'soon'))).toThrow();
    expect(drifted((value) => (firstItem(value).state = 'sleeping'))).toThrow();
    expect(drifted((value) => (firstItem(value).title = null))).toThrow();
    expect(
      drifted((value) => (firstItem(value).permission = { kind: 'approval', request_id: 'x', action: 'y' })),
    ).toThrow();
    expect(drifted((value) => ((value.kick_off as Record<string, unknown>).can_start = 'yes'))).toThrow();
    expect(() => parseOrchestrationTimeline([])).toThrow();
  });
});
