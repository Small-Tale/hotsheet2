import { describe, expect, it } from 'vitest';

import { readinessLevelText, readinessReasonText, readinessUnblocks, readinessWhy } from './orchestration-plan';
import type { TimelineItem } from './orchestration-timeline';
import { findTimelineItem } from './orchestration-timeline-board';
import { demoTimeline } from './ux-demo/orchestration-fixtures';

const timeline = demoTimeline('running');
const item = (step: string) => findTimelineItem(timeline, step)!.item;

describe('the readiness plan in words (HS2-15G5T4)', () => {
  it('names every level of docs/26 §26.15 and reads a newer level as its words', () => {
    expect(
      ['confirmed', 'provisional', 'blocked', 'stale', 'cyclic', 'in_progress', 'finished'].map(readinessLevelText),
    ).toEqual(['Confirmed', 'Provisional', 'Blocked', 'Not confirmed', 'In a cycle', 'In progress', 'Finished']);
    expect(readinessLevelText('held_back')).toBe('Held back');
  });

  it('reads reason codes as phrases and an unknown code as its words', () => {
    expect(readinessReasonText('gate_pending')).toBe('integrated; its CI is still running');
    expect(readinessReasonText('plan_cycle')).toBe('its prerequisites lead back to this step');
    expect(readinessReasonText('some_new_code')).toBe('some new code');
  });

  it('says why per prerequisite, then the step’s own reasons, and nothing for a clear step', () => {
    expect(readinessWhy(item('step-7'))).toEqual(['API-12: integrated; its CI is still running']);
    expect(readinessWhy(item('step-8'))).toEqual([]);
    const cyclic: TimelineItem = { ...item('step-8'), level: 'cyclic', reasons: ['plan_cycle', 'plan_cycle'] };
    expect(readinessWhy(cyclic)).toEqual(['Its prerequisites lead back to this step']);
    const mixed: TimelineItem = {
      ...item('step-9'),
      reasons: ['prerequisite_not_finished', 'validation_failed'],
      waits_on: [...(item('step-9').waits_on ?? []), { ...item('step-9').waits_on![0], slug: undefined }],
    };
    expect(readinessWhy(mixed)).toEqual([
      'API-15: not finished yet',
      'A prerequisite: not finished yet',
      'The validation step failed',
    ]);
    // The Prerequisites section already lists what it waits on; only the step's own reasons remain.
    expect(readinessWhy(mixed, { omitPrerequisites: true })).toEqual(['The validation step failed']);
  });

  it('lists the unfinished steps that wait on this one, once each, and none for a leaf', () => {
    expect(readinessUnblocks(item('step-3'), timeline)).toEqual(['WEB-30']);
    expect(readinessUnblocks(item('step-4'), timeline)).toEqual(['DOC-7']);
    expect(readinessUnblocks(item('step-10'), timeline)).toEqual([]);
    // A finished dependent no longer waits; a step without a slug reads by its title.
    const done = demoTimeline('running');
    const dependent = findTimelineItem(done, 'step-7')!.item;
    dependent.state = 'done';
    expect(readinessUnblocks(item('step-3'), done)).toEqual([]);
    const untitled = demoTimeline('running');
    const waiting = findTimelineItem(untitled, 'step-7')!.item;
    waiting.slug = undefined;
    expect(readinessUnblocks(item('step-3'), untitled)).toEqual(['Add the single sign-on login page']);
    waiting.title = undefined;
    expect(readinessUnblocks(item('step-3'), untitled)).toEqual(['Web ticket']);
  });
});
