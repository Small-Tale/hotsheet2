import { describe, expect, it } from 'vitest';

import {
  fixedInputsToSend,
  formFromTrigger,
  formProblem,
  formToRequest,
  formToSpec,
  historyRow,
  historyRows,
  newFireOperationId,
  newTriggerForm,
  nextRows,
  overlapBanner,
  parseTriggerList,
  parseTriggerReview,
  parseTriggerView,
  permissionRows,
  scopeRows,
  sourceText,
  stateBadge,
  stateBanner,
  statusRows,
  type TriggerForm,
  type TriggerHistoryEntry,
  triggerOverviewText,
  triggerProblemText,
  type TriggerView,
  warningText,
} from './orchestration-triggers';

/** HS2-KXKA7X: synthetic trigger records shaped as the server answers them. */
const NOW = Date.UTC(2026, 10, 26, 15, 0);
const labels = { api: 'API', web: 'Web' };

function rawTrigger(overrides: Record<string, unknown> = {}) {
  return {
    trigger_id: '01M8BG8PA9CXEQY8GRMTB36X92',
    name: 'Repair API main CI',
    source: {
      kind: 'branch_ci_failed',
      target_id: 'api',
      watch: { repository: 'small/api', workflow: 'ci.yml', branch: 'main' },
    },
    action: {
      template: { template_id: 'T1', tool: 'claude', event_inputs: { commit: 'commit', bogus: 'nope' } },
    },
    state: 'dry_run',
    created_at_ms: 1,
    updated_at_ms: 2,
    cursor: { armed_at_ms: 1 },
    history: [],
    reviewed: false,
    running: false,
    ...overrides,
  };
}

const entry = (overrides: Partial<TriggerHistoryEntry>): TriggerHistoryEntry => ({
  delivery_key: 'k',
  kind: 'ci_failed',
  at_ms: NOW - 60_000,
  outcome: 'applied',
  count: 1,
  evidence: {},
  missed: 0,
  ...overrides,
});

describe('HS2-KXKA7X trigger parsing', () => {
  it('parses a trigger view and drops what it does not know', () => {
    const view = parseTriggerView(rawTrigger())!;
    expect(view.source).toEqual({
      kind: 'branch_ci_failed',
      target_id: 'api',
      watch: { repository: 'small/api', workflow: 'ci.yml', branch: 'main' },
    });
    expect(view.action.template?.event_inputs).toEqual({ commit: 'commit' });
    expect(view.overlap).toBe('skip');
    expect(view.next_runs_ms).toEqual([]);
    expect(parseTriggerView({ trigger_id: 'x' })).toBeUndefined();
    expect(parseTriggerView(rawTrigger({ source: { kind: 'unknown' } }))).toBeUndefined();
    expect(parseTriggerList({ triggers: [rawTrigger(), { nope: true }] })).toHaveLength(1);
    expect(parseTriggerList(undefined)).toEqual([]);
  });

  it('parses history with coalesced counts, would-have-run and the running goal', () => {
    const view = parseTriggerView(
      rawTrigger({
        running: true,
        running_goal: 'LCH-12',
        active: { delivery_key: 'a', operation_id: 'trg-x', at_ms: 1 },
        history: [
          {
            delivery_key: 'ci:github-actions:9182:1',
            kind: 'ci_failed',
            at_ms: NOW,
            outcome: 'dry_run',
            evidence: { commit: '3f9c2a1aaaa', ci_run: '9182.1' },
            would: { goal_title: 'Repair API', steps: 1, targets: ['api'], asks: ['api'] },
          },
          { delivery_key: 'x', kind: 'nope', at_ms: NOW, outcome: 'skipped_overlap', count: 2 },
          { delivery_key: 'y', outcome: 'mystery' },
        ],
      }),
    )!;
    expect(view.history).toHaveLength(2);
    expect(view.history[1]).toMatchObject({ kind: 'manual', count: 2 });
    expect(view.active_operation).toBe('trg-x');
    expect(view.running_goal).toBe('LCH-12');
  });

  it('parses a review', () => {
    const review = parseTriggerReview({
      problems: [{ code: 'template_unknown', at: 'action.template.template_id' }],
      scope: {
        watches: {
          kind: 'branch_ci_failed',
          target_id: 'api',
          label: 'API',
          branch: { repository: 'o/n', workflow: 'ci.yml', branch: 'main' },
        },
        projects: [{ target_id: 'api', label: 'API', declared: true, steps: 1 }],
        per_delivery: { goals: 1, steps: 1, goal_title: 'Repair' },
      },
      permissions: [
        { action: 'watch', target_id: 'api', need: 'not_needed' },
        { action: 'tickets_write', need: 'weird' },
      ],
      next: { runs_ms: [], event: 'branch_ci_failed', overlap: 'queue_latest' },
      digest: 'abc',
      template_version: 3,
      turn_on: 'person',
    });
    expect(review.problems).toHaveLength(1);
    expect(review.permissions[1].need).toBe('asks');
    expect(review.next.overlap).toBe('queue_latest');
    expect(review.turn_on).toBe('person');
    expect(review.template_version).toBe(3);
  });
});

describe('HS2-KXKA7X trigger wording', () => {
  it('names every state as a badge', () => {
    expect(stateBadge('on')).toEqual({ label: 'On', tone: 'success' });
    expect(stateBadge('dry_run')).toEqual({ label: 'Dry Run', tone: 'info' });
    expect(stateBadge('paused')).toEqual({ label: 'Paused', tone: 'neutral' });
    expect(stateBadge('needs_review')).toEqual({ label: 'Review', tone: 'warning' });
  });

  it('words every source', () => {
    expect(
      sourceText(
        {
          kind: 'schedule',
          schedule: {
            time_zone: 'UTC',
            local_time: '02:00',
            recurrence: { kind: 'hourly', every: 4, start_date: '2026-11-01' },
          },
        },
        labels,
      ),
    ).toBe('Every 4 hours from 02:00 UTC');
    expect(sourceText(parseTriggerView(rawTrigger())!.source, labels)).toBe('When API’s CI fails on main');
    expect(
      sourceText(
        { kind: 'commit_landed', target_id: 'web', watch: { repository: 'r', workflow: 'w', branch: 'main' } },
        labels,
      ),
    ).toBe('When a commit lands on Web main');
    expect(sourceText({ kind: 'ticket_opened', target_id: 'api', tag: 'bug' }, labels)).toBe(
      'When a ticket tagged bug opens in API',
    );
    expect(sourceText({ kind: 'gate_failed' }, labels)).toBe('When a run’s CI gate fails');
    expect(sourceText({ kind: 'gate_failed', target_id: 'zzz' }, labels)).toBe('When a run’s CI gate fails in zzz');
  });

  it('words each history outcome, newest first', () => {
    expect(historyRow(entry({ outcome: 'applied', goal_slug: 'LCH-12' }), NOW).value).toBe(
      'CI failed on a commit · Ran · goal LCH-12',
    );
    expect(
      historyRow(
        entry({
          kind: 'schedule',
          outcome: 'applied',
          goal_slug: 'LCH-11',
          missed: 1,
          reason: 'permission_required',
        }),
        NOW,
      ).value,
    ).toBe('Ran · goal LCH-11 · waits for permission · missed 1 run while this computer was off; ran once on return');
    expect(
      historyRow(
        entry({
          outcome: 'dry_run',
          evidence: { commit: '3f9c2a1abcdef', ci_run: '9182.1' },
          would: { goal_title: 'Repair API', steps: 1, targets: ['api'], asks: ['api'] },
        }),
        NOW,
      ).value,
    ).toBe('CI failed on 3f9c2a1 (run 9182) · would open “Repair API” with 1 step · would ask in api');
    expect(
      historyRow(
        entry({ outcome: 'dry_run', kind: 'manual', would: { steps: 0, targets: [], asks: ['api'] } }),
        NOW,
        labels,
        false,
      ).value,
    ).toBe('Dry run · Run by hand · would ask in API');
    expect(historyRow(entry({ outcome: 'skipped_overlap', count: 2 }), NOW).value).toBe(
      'Skipped · the last run was still working (2 skips)',
    );
    expect(historyRow(entry({ outcome: 'already_handled' }), NOW).value).toBe(
      'Same event reported again · already handled',
    );
    expect(historyRow(entry({ outcome: 'needs_review', reason: 'template_changed' }), NOW).value).toBe(
      'Waits for a review · its template changed',
    );
    expect(historyRow(entry({ outcome: 'refused', reason: 'trigger_template_missing' }), NOW).value).toBe(
      'CI failed on a commit · Could not run · trigger template missing',
    );
    expect(historyRow(entry({ outcome: 'queued', kind: 'manual' }), NOW).value).toBe(
      'Run by hand · Runs once the current run finishes',
    );
    expect(historyRow(entry({ at_ms: NOW - 60_000 }), NOW).label).toMatch(/^Today /);
    expect(historyRow(entry({ at_ms: NOW - 86_400_000 }), NOW).label).toMatch(/^Yesterday /);
    const view = parseTriggerView(rawTrigger())!;
    view.history = [entry({ delivery_key: 'a' }), entry({ delivery_key: 'b' })];
    expect(historyRows(view, NOW).map((row) => row.key)).toEqual(['b', 'a']);
    // The running delivery shows the goal its apply opened since, not the wait it was recorded with.
    view.history = [entry({ delivery_key: 'c', operation_id: 'trg-1', reason: 'permission_required' })];
    view.active_operation = 'trg-1';
    view.running_goal = 'LCH-9';
    expect(historyRows(view, NOW)[0].value).toBe('CI failed on a commit · Ran · goal LCH-9');
    view.active_operation = 'trg-2';
    expect(historyRows(view, NOW)[0].value).toBe('CI failed on a commit · Ran · waits for permission');
  });

  it('shows the overlap banner only while the skipped run still works', () => {
    const view = parseTriggerView(rawTrigger({ state: 'on', running: true, running_goal: 'LCH-12' }))!;
    view.history = [entry({ outcome: 'applied' }), entry({ outcome: 'skipped_overlap', count: 2 })];
    expect(overlapBanner(view)).toEqual({
      title: 'Skipped 2 Runs: The Last Run Is Still Working',
      detail:
        'A trigger never overlaps its own runs. LCH-12 is still open; the next run starts once it finishes or you close LCH-12.',
      goal: 'LCH-12',
    });
    expect(overlapBanner({ ...view, running: false })).toBeUndefined();
    expect(overlapBanner({ ...view, history: [entry({})] })).toBeUndefined();
  });

  it('offers Turn On for Dry Run and Review, and says when an AI turn on waits', () => {
    const dry = parseTriggerView(
      rawTrigger({ history: [{ delivery_key: 'a', kind: 'ci_failed', at_ms: 1, outcome: 'dry_run' }] }),
    )!;
    expect(stateBanner(dry)).toMatchObject({ title: 'Dry Run', turnOn: true });
    expect(stateBanner(dry)?.detail).toMatch(/^1 event so far/);
    expect(stateBanner({ ...dry, state: 'needs_review' })).toMatchObject({ title: 'Needs Review', turnOn: true });
    expect(stateBanner({ ...dry, state: 'on' })).toBeUndefined();
    expect(stateBanner({ ...dry, state: 'paused' })).toBeUndefined();
    expect(stateBanner({ ...dry, pending_on: { requests: ['r'], at_ms: 1 } })).toMatchObject({
      title: 'Waiting for Permission',
      turnOn: false,
    });
    expect(warningText({ ...dry, warning: { code: 'target_unreachable', since_ms: 1 } })).toContain(
      'target unreachable',
    );
    expect(warningText(dry)).toBeUndefined();
  });

  it('words the status, scope, permissions and next actions', () => {
    const view: TriggerView = {
      ...parseTriggerView(rawTrigger({ state: 'on', template_name: 'Repair failing CI' }))!,
      review: { role: 'human', actor_id: 'me', at_ms: NOW - 1000 },
    };
    const status = statusRows(view, labels, NOW, 'UTC');
    expect(status[0].value).toMatch(/^On · reviewed by you Today/);
    expect(status[1].value).toBe('Applies Repair failing CI');
    expect(status[2]).toEqual({ label: 'Watches', value: 'When API’s CI fails on main' });
    const paused = statusRows(
      {
        ...view,
        state: 'paused',
        source: {
          kind: 'schedule',
          schedule: { time_zone: 'UTC', local_time: '02:00', recurrence: { kind: 'daily', every: 1 } },
        },
        action: { placement: 'start' },
      },
      labels,
      NOW,
    );
    expect(paused[1].value).toBe('Starts orchestration');
    expect(paused[2].value).toBe('Not while paused');
    const review = parseTriggerReview({
      scope: {
        watches: {
          kind: 'branch_ci_failed',
          target_id: 'api',
          label: 'API',
          branch: { repository: 'o/n', workflow: 'ci.yml', branch: 'main' },
        },
        projects: [{ target_id: 'api', label: 'API', declared: true, steps: 1 }],
        per_delivery: { goals: 1, steps: 1, goal_title: 'Repair' },
      },
      permissions: [
        { action: 'watch', target_id: 'api', need: 'not_needed' },
        { action: 'tickets_write', target_id: 'api', need: 'asks' },
        { action: 'session_start', target_id: 'api', need: 'remembered', tool: 'claude' },
        { action: 'placement_start', need: 'not_needed' },
        { action: 'push_deploy', need: 'unchanged' },
      ],
      next: { event: 'branch_ci_failed', overlap: 'skip' },
      turn_on: 'asks',
    });
    expect(scopeRows(review, labels, 'Launch')).toEqual([
      { label: 'Watches', value: 'API · GitHub Actions ci.yml on main' },
      { label: 'Changes', value: 'Launch (goal ticket) and API tickets; no other project' },
      { label: 'Per Event', value: '1 goal, 1 step' },
    ]);
    expect(permissionRows(review, labels).map((row) => `${row.label}: ${row.value}`)).toEqual([
      'Watch API: Never asks; adding API was the consent',
      'Create Tickets in API: Asks through the permission prompt',
      'Start Sessions in API: Allowed by a remembered rule (claude)',
      'Start Orchestration: Allowed by turning the trigger on',
      'Push and Deploy: Unchanged: each project’s integrator and push policy',
    ]);
    expect(nextRows(review, NOW).map((row) => row.label)).toEqual([
      'Each Event',
      'Duplicates',
      'While Running',
      'Turning On',
    ]);
    expect(triggerProblemText({ code: 'action_empty', at: 'action' })).toBe(
      'Choose a template or a placement change to run.',
    );
    expect(triggerProblemText({ code: 'brand_new', at: '' })).toBe('brand new.');
  });
});

describe('HS2-KXKA7X the setup form', () => {
  const branchForm = (): TriggerForm => ({
    ...newTriggerForm({ tool: 'claude', target: 'api' }),
    name: '  Repair API main CI ',
    when: 'event',
    event: 'branch_ci_failed',
    repository: 'small/api',
    workflow: 'ci.yml',
    template: 'T1',
    eventInputs: { commit: 'commit', when: 'occurrence' },
  });

  it('maps the form to the request: trimmed, unavailable event inputs left out, fixed inputs kept', () => {
    expect(formToRequest(branchForm(), undefined, { project: 'api', commit: 'x', empty: '' })).toEqual({
      name: 'Repair API main CI',
      source: {
        kind: 'branch_ci_failed',
        target_id: 'api',
        watch: { repository: 'small/api', workflow: 'ci.yml', branch: 'main' },
      },
      action: {
        template: { template_id: 'T1', tool: 'claude', inputs: { project: 'api' }, event_inputs: { commit: 'commit' } },
      },
      overlap: 'skip',
      start: 'dry_run',
    });
    // A placement-only schedule trigger, paused, with an id when edited.
    const schedule = {
      ...newTriggerForm(),
      name: 'Night shift',
      placement: 'start' as const,
      start: 'paused' as const,
    };
    const request = formToRequest(schedule, 'ID1');
    expect(request).toMatchObject({ trigger_id: 'ID1', action: { placement: 'start' }, start: 'paused' });
    expect(request.source.kind).toBe('schedule');
  });

  it('round-trips a saved trigger back into the form, then edits and resets it', () => {
    const spec = formToSpec(branchForm());
    const view = parseTriggerView({ ...rawTrigger(), ...spec, state: 'paused' })!;
    const form = formFromTrigger(view);
    expect(form).toMatchObject({
      name: 'Repair API main CI',
      when: 'event',
      event: 'branch_ci_failed',
      target: 'api',
      repository: 'small/api',
      template: 'T1',
      tool: 'claude',
      start: 'paused',
      eventInputs: { commit: 'commit' },
    });
    expect(formToSpec(form)).toEqual(spec);
    // Edit again, then reset to a new form: nothing of the edit is left.
    const edited = { ...form, overlap: 'queue_latest' as const };
    expect(formToSpec(edited).overlap).toBe('queue_latest');
    expect(newTriggerForm().name).toBe('');
    const ticket = formFromTrigger(
      parseTriggerView(rawTrigger({ source: { kind: 'ticket_opened', target_id: 'web', tag: 'bug' } }))!,
    );
    expect(formToSpec(ticket).source).toEqual({ kind: 'ticket_opened', target_id: 'web', tag: 'bug' });
    const schedule = formFromTrigger(
      parseTriggerView(
        rawTrigger({
          source: {
            kind: 'schedule',
            schedule: {
              time_zone: 'UTC',
              local_time: '02:00',
              recurrence: { kind: 'hourly', start_date: '2026-11-01', every: 4 },
            },
          },
        }),
      )!,
    );
    expect(formToSpec(schedule).source).toEqual({
      kind: 'schedule',
      schedule: {
        time_zone: 'UTC',
        local_time: '02:00',
        recurrence: { kind: 'hourly', start_date: '2026-11-01', every: 4 },
      },
    });
  });

  it('HS2-SEEMXR round-trips every recurrence the command-schedule dialog offers through Edit…', () => {
    const recurrences = [
      { kind: 'once', date: '2026-12-24' },
      { kind: 'selected_dates', dates: ['2026-12-01', '2026-12-15'] },
      {
        kind: 'weekly',
        start_date: '2026-11-02',
        every: 2,
        weekdays: [1, 3, 5],
        end_date: '2027-03-01',
        count: 10,
        include_dates: ['2026-11-28'],
        exclude_dates: ['2026-12-25', '2027-01-01'],
      },
      {
        kind: 'monthly',
        start_date: '2026-11-01',
        every: 1,
        on: { kind: 'weekday_ordinal', ordinal: -1, weekday: 5 },
      },
      { kind: 'monthly', start_date: '2026-11-01', every: 3, on: { kind: 'last_day' } },
      { kind: 'yearly', start_date: '2026-01-01', every: 1, month: 4, on: { kind: 'day_of_month', day: 15 } },
    ] as const;
    for (const recurrence of recurrences) {
      const schedule = { time_zone: 'Europe/Paris', local_time: '07:30', recurrence };
      const form = formFromTrigger(parseTriggerView(rawTrigger({ source: { kind: 'schedule', schedule } }))!);
      expect(form.when).toBe('schedule');
      expect(formToSpec(form).source, recurrence.kind).toEqual({ kind: 'schedule', schedule });
    }
  });

  it('HS2-40GPZK sends fixed values typed as Use Template… sends them, and Edit… keeps their types', () => {
    const template = {
      template_id: 'T1',
      name: 'Repair',
      description: '',
      state: 'active' as const,
      version: 1,
      goal_title: 'Repair',
      steps: [],
      wait_for_accept: true,
      inputs: [
        { key: 'project', label: 'Project', kind: 'target' as const, required: true, default: 'api', choices: [] },
        { key: 'urgent', label: 'Urgent', kind: 'boolean' as const, required: false, choices: [] },
        { key: 'attempts', label: 'Attempts', kind: 'number' as const, required: false, choices: [] },
        { key: 'size', label: 'Size', kind: 'choice' as const, required: false, choices: ['small', 'large'] },
        { key: 'note', label: 'Note', kind: 'text' as const, required: false, choices: [] },
      ],
    };
    const fixed = { project: 'web', urgent: false, attempts: '3', size: 'large', note: '  look  ', commit: '' };
    expect(fixedInputsToSend(template, fixed)).toEqual({
      project: 'web',
      urgent: false,
      attempts: 3,
      size: 'large',
      note: 'look',
    });
    // Left at the template default: nothing is sent for it.
    expect(fixedInputsToSend(template, { project: '', urgent: undefined })).toEqual({});
    // A number that does not read as one goes as typed, so the server names it.
    expect(fixedInputsToSend(template, { attempts: 'many' })).toEqual({ attempts: 'many' });
    // Without the template (not read yet), values go as entered.
    expect(fixedInputsToSend(undefined, { urgent: true, note: '', size: 'small' })).toEqual({
      urgent: true,
      size: 'small',
    });
    const form = { ...newTriggerForm(), name: 'Repair', template: 'T1', tool: 'claude' };
    const spec = formToSpec(form, fixedInputsToSend(template, fixed));
    expect(spec.action.template?.inputs).toEqual({
      project: 'web',
      urgent: false,
      attempts: 3,
      size: 'large',
      note: 'look',
    });
    const view = parseTriggerView({ ...rawTrigger(), ...spec })!;
    expect(view.action.template?.inputs).toEqual(spec.action.template?.inputs);
    expect(formToSpec(formFromTrigger(view), view.action.template?.inputs ?? {})).toEqual(spec);
  });

  it('HS2-X3FZJE words the overview Triggers row: counts by state and the earliest next run', () => {
    const now = Date.UTC(2026, 10, 26, 12, 0);
    const view = (overrides: Record<string, unknown>) => parseTriggerView(rawTrigger(overrides))!;
    expect(triggerOverviewText(undefined, now, 'UTC')).toBe('Reading the triggers…');
    expect(triggerOverviewText([], now, 'UTC')).toBe('None yet');
    const triggers = [
      view({ trigger_id: 'A', name: 'Nightly', state: 'on', next_runs_ms: [now + 3_600_000 * 14] }),
      view({
        trigger_id: 'B',
        name: 'Hourly bump',
        state: 'dry_run',
        next_runs_ms: [now + 1_800_000, now + 5_400_000],
      }),
      view({ trigger_id: 'C', state: 'paused' }),
      view({ trigger_id: 'D', state: 'needs_review' }),
      view({ trigger_id: 'E', state: 'on' }),
    ];
    expect(triggerOverviewText(triggers, now, 'UTC')).toBe(
      '2 on, 1 dry run, 1 paused, 1 need review · next Today 12:30, Hourly bump',
    );
    // Nothing scheduled: the counts alone; refilled after empty, the next run comes back.
    expect(triggerOverviewText(triggers.slice(2), now, 'UTC')).toBe('1 on, 1 paused, 1 need review');
    expect(triggerOverviewText([], now, 'UTC')).toBe('None yet');
    expect(triggerOverviewText(triggers.slice(0, 1), now, 'UTC')).toBe('1 on · next Nov 27 02:00, Nightly');
  });

  it('names what keeps Save disabled', () => {
    expect(formProblem(newTriggerForm())).toBe('Name the trigger');
    const named = { ...newTriggerForm(), name: 'X' };
    expect(formProblem(named)).toBe('Choose a template or a placement change to run');
    expect(formProblem({ ...named, template: 'T1' })).toBe('Choose the AI tool the planner runs in');
    expect(formProblem({ ...named, placement: 'pause' })).toBeUndefined();
    expect(formProblem({ ...named, when: 'event', event: 'ticket_opened', placement: 'start' })).toBe(
      'Choose a project',
    );
    expect(formProblem({ ...named, when: 'event', event: 'gate_failed', placement: 'start' })).toBeUndefined();
    expect(formProblem({ ...branchForm(), workflow: ' ' })).toBe('Fill in the repository, workflow and branch');
    expect(formProblem(branchForm())).toBeUndefined();
  });

  it('mints bounded manual-run operation ids', () => {
    expect(newFireOperationId(() => 'abc')).toBe('fire-abc');
    expect(newFireOperationId(() => 'x'.repeat(200))).toHaveLength(140);
  });
});
