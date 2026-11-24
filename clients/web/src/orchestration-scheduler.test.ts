import { describe, expect, it } from 'vitest';

import {
  decisionText,
  explanationRows,
  lastPassText,
  latestDecisions,
  parseDecisionPage,
  parseExplanation,
  type SchedulerDecision,
  schedulerReasonText,
  siteLabel,
  sitePassDetail,
  sitePassText,
  stepLabel,
} from './orchestration-scheduler';
import { demoClock, demoDecisions, demoTimeline } from './ux-demo/orchestration-fixtures';

const timeline = demoTimeline('running');
const clock = (ms: number) => `t${ms}`;
const decision = (rest: Partial<SchedulerDecision>): SchedulerDecision => ({
  seq: 1,
  at_ms: 0,
  tick_id: 'tick-1',
  mode: 'applied',
  kind: 'start',
  ...rest,
});

describe('the scheduler in words (HS2-77SRHC)', () => {
  it('explains why a ready card is not running, with the last pass and when it looks again', () => {
    expect(explanationRows({ standing: 'not_placed', reason: 'target_cap' }, clock, 5)).toEqual([
      { label: 'Scheduler', value: 'Waits: the project’s session limit is reached (last pass t5)' },
      { label: 'Next', value: 'Looked at again when something it waits on changes' },
    ]);
    expect(explanationRows({ standing: 'not_placed', reason: 'resource_held', retry_at_ms: 9 }, clock)).toEqual([
      { label: 'Scheduler', value: 'Waits: a resource it needs is held' },
      { label: 'Next', value: 'Looked at again by t9' },
    ]);
    expect(explanationRows({ standing: 'start', reason: 'free_session' }, clock)).toEqual([
      { label: 'Scheduler', value: 'It would start on the next pass' },
    ]);
    expect(explanationRows({ standing: 'new_standing', reason: 'x' }, clock)[0].value).toBe('New standing');
    expect(schedulerReasonText('brand_new')).toBe('brand new');
    // HS2-ZY4Z4G
    expect(schedulerReasonText('requirement_unmet')).toBe(
      'its project’s machine does not meet a requirement of the step',
    );
    expect(schedulerReasonText('requirement_unknown')).toContain('has not said whether');
    expect(
      explanationRows({ standing: 'not_placed', reason: 'requirement_unmet', detail: 'os:windows' }, clock),
    ).toEqual([
      { label: 'Scheduler', value: 'Waits: its project’s machine does not meet a requirement of the step' },
      { label: 'Requirement', value: 'os:windows' },
      { label: 'Next', value: 'Looked at again when something it waits on changes' },
    ]);
  });

  it('names steps by slug, title or a short id', () => {
    expect(stepLabel('step-3', timeline)).toBe('API-12');
    expect(stepLabel('01ARZ3NDEKTSV4RRFFQ69G5FAV', timeline)).toBe('step 9G5FAV');
    expect(stepLabel(undefined, timeline)).toBe('a step');
    expect(stepLabel('step-3', undefined)).toBe('step step-3');
  });

  it('words every kind of decision, applied or dry run', () => {
    const say = (rest: Partial<SchedulerDecision>) => decisionText(decision(rest), timeline);
    expect(say({ step_id: 'step-3', tool: 'claude' })).toBe('Started API-12 on Claude');
    expect(say({ step_id: 'step-3', mode: 'dry_run' })).toBe('Would start API-12');
    expect(say({ step_id: 'step-6', outcome: 'permission_required' })).toBe('API-15 waits for permission');
    expect(say({ kind: 'unplaced', step_id: 'step-8', reason: 'resource_held' })).toBe(
      'Deferred DOC-9: a resource it needs is held',
    );
    expect(say({ kind: 'keep', step_id: 'step-4', reason: 'mid_turn' })).toBe('Kept the session of WEB-33: mid turn');
    expect(say({ kind: 'wind_down', reason: 'no_ready_work' })).toBe('Wound down a session: no ready work');
    expect(say({ kind: 'wind_down', mode: 'dry_run' })).toBe('Would wind down a session');
    expect(say({ kind: 'assignment', step_id: 'step-3', outcome: 'claimed' })).toBe('API-12: claimed');
    expect(say({ kind: 'reconcile', step_id: 'step-3', outcome: 'queued' })).toBe('Reconciled API-12: queued');
    expect(say({ kind: 'tick' })).toBe('Ran a scheduling pass');
    expect(say({ kind: 'tick', mode: 'dry_run' })).toBe('Planned a pass (dry run)');
    expect(say({ kind: 'novel_kind', step_id: 'step-3' })).toBe('Novel kind API-12');
  });

  it('reads the last pass and the latest three decisions from the newest page', () => {
    const page = demoDecisions();
    expect(lastPassText(page, demoClock)).toBe('13:58 · started 1, deferred 1');
    expect(latestDecisions(page).map((entry) => entry.seq)).toEqual([41, 40, 39]);
    expect(lastPassText({ entries: [], cursor: 0, more: false }, demoClock)).toBeUndefined();
    const dry = { ...page, entries: [{ ...page.entries[0], mode: 'dry_run' }] };
    expect(lastPassText(dry, demoClock)).toBe('13:58 · started 0, deferred 0 (dry run)');
  });

  it('parses pages and explanations leniently', () => {
    expect(
      parseDecisionPage({
        cursor: 4,
        more: true,
        entries: [{ seq: 5, at_ms: 1, tick_id: 't', kind: 'tick', mode: 'applied' }, { seq: 'x' }, { seq: 4 }],
      }),
    ).toEqual({
      cursor: 4,
      more: true,
      entries: [
        {
          seq: 5,
          at_ms: 1,
          tick_id: 't',
          mode: 'applied',
          kind: 'tick',
          step_id: undefined,
          tool: undefined,
          reason: undefined,
          detail: undefined,
          outcome: undefined,
        },
      ],
    });
    expect(parseDecisionPage({})).toEqual({ cursor: 0, more: false, entries: [] });
    expect(parseExplanation(undefined)).toBeUndefined();
    expect(parseExplanation({ standing: 'x' })).toBeUndefined();
    expect(parseExplanation({ standing: 'not_placed', reason: 'cooldown', retry_at_ms: 3, detail: 'd' })).toEqual({
      standing: 'not_placed',
      reason: 'cooldown',
      retry_at_ms: 3,
      detail: 'd',
      resources: [],
      sites: [],
    });
  });

  it('names the chosen site of a project and why each other site was passed over (HS2-9FKTB3)', () => {
    const explanation = parseExplanation({
      standing: 'start',
      reason: 'ready_work',
      placement: { target_id: 'web' },
      sites: [
        { target_id: 'api', reason: 'requirement_unmet', detail: 'os:windows' },
        { target_id: 'docs', reason: 'not_chosen', detail: 'fewer_sessions' },
        { target_id: 'gone-01ABCDEF', reason: 'not_chosen' },
        { reason: 'malformed' },
      ],
    })!;
    expect(explanation.site).toBe('web');
    expect(explanation.sites).toHaveLength(3);
    expect(explanationRows(explanation, clock, undefined, timeline)).toEqual([
      { label: 'Scheduler', value: 'It would start on the next pass' },
      { label: 'Site', value: 'Web' },
      {
        label: 'Not on API',
        value: 'Its project’s machine does not meet a requirement of the step (os:windows)',
      },
      { label: 'Not on Docs', value: 'Another site’s machine runs fewer sessions' },
      { label: 'Not on site ABCDEF', value: 'Another site was chosen' },
    ]);
    // A one-site project shows no site rows.
    expect(explanationRows({ standing: 'start', reason: 'ready_work', site: 'web', sites: [] }, clock)).toHaveLength(1);
    expect(sitePassText({ target_id: 'x', reason: 'not_chosen', detail: 'site_order' })).toBe(
      'another site was as good and comes first',
    );
    expect(sitePassText({ target_id: 'x', reason: 'target_cap' })).toBe('the project’s session limit is reached');
    expect(siteLabel('api', undefined)).toBe('site api');
  });

  it('words a passed-over site’s policy detail and leaves out a code it cannot word (HS2-MFS7FY)', () => {
    expect(sitePassText({ target_id: 'x', reason: 'policy_denied', detail: 'policy_target_not_permitted' })).toBe(
      'the policy does not allow it (the site is not one the policy lists)',
    );
    expect(sitePassText({ target_id: 'x', reason: 'policy_denied', detail: 'policy_machine_cap' })).toBe(
      'the policy does not allow it (the machine’s session limit is reached)',
    );
    expect(sitePassText({ target_id: 'x', reason: 'target_unavailable', detail: 'capacity_not_read' })).toBe(
      'its project could not be reached (its machine has not been read yet)',
    );
    // A requirement or resource names what the step itself asks for.
    expect(sitePassText({ target_id: 'x', reason: 'resource_held', detail: 'gpu' })).toBe(
      'a resource it needs is held (gpu)',
    );
    // Unknown codes never reach row copy.
    expect(sitePassText({ target_id: 'x', reason: 'policy_denied', detail: 'policy_future_rule' })).toBe(
      'the policy does not allow it',
    );
    expect(sitePassText({ target_id: 'x', reason: 'machine_refused', detail: 'capacity_unknown' })).toBe(
      'its machine has no room',
    );
    expect(sitePassDetail({ target_id: 'x', reason: 'no_tool_available' })).toBeUndefined();
    for (const code of [
      'policy_absent',
      'policy_target_not_permitted',
      'policy_target_disabled',
      'policy_tool_not_permitted',
      'policy_model_not_permitted',
      'policy_load_unknown',
      'policy_target_cap',
      'policy_machine_cap',
      'policy_tool_cap',
      'policy_account_not_permitted',
      'policy_account_cap',
      'policy_speculation_not_allowed',
      'policy_project_software',
    ]) {
      const text = sitePassText({ target_id: 'x', reason: 'policy_denied', detail: code });
      expect(text).not.toContain('_');
      expect(text).toMatch(/^the policy does not allow it \(.+\)$/);
    }
  });
});
