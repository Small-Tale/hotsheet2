import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import type { TimelineItem } from './orchestration-timeline';
import {
  cardTime,
  doneRepair,
  dryRunPlan,
  findTimelineItem,
  heldTemplatePlans,
  heldTemplateStatus,
  heldTemplateTitle,
  machineLabel,
  orchestrationLine,
  runAttention,
  runNotice,
  runStateText,
  schedulerHold,
  sessionText,
  startBlockedInSettings,
  startBlockedReason,
  startColumnId,
  stateBadge,
  stateWords,
  stepOfCard,
  targetLineKey,
  targetLineMap,
  targetLineOrchestration,
  timelineCard,
  timelineColumns,
  timelineStatus,
  toolLabel,
  toolText,
  waveTitle,
} from './orchestration-timeline-board';
import { DEMO_NOW, demoClock, demoTimeline } from './ux-demo/orchestration-fixtures';
import { provisionalLine } from './worker-provisional';

const item = (step: string, phase: Parameters<typeof demoTimeline>[0] = 'running') =>
  findTimelineItem(demoTimeline(phase), step)!.item;

// The fixtures' clock prints UTC (`demoClock`), while "About" or "Tomorrow" compares local calendar
// days, so the test pins the local zone to UTC; otherwise it depends on the host's zone (HS2-SVA8HP).
beforeAll(() => {
  vi.stubEnv('TZ', 'UTC');
});
afterAll(() => {
  vi.unstubAllEnvs();
});

describe('orchestration timeline on the ticket board (HS2-J89JC7)', () => {
  it('puts the newest wave leftmost and Done rightmost, with estimated starts in the titles', () => {
    const running = demoTimeline('running');
    expect(timelineColumns(running, { clock: demoClock }).map((column) => column.title)).toEqual([
      'Later · About 18:00',
      'Then · About 16:30',
      'Next · About 15:00',
      'Working Now',
      'Done',
    ]);
    const proposed = demoTimeline('proposed');
    expect(timelineColumns(proposed, { clock: demoClock }).map((column) => column.title)).toEqual([
      'Third · About +3 h',
      'Second · About +2 h',
      'First · On Start',
    ]);
    // Without an estimate a wave says only its name; past the tenth it is numbered.
    expect(waveTitle({ kind: 'next', ordinal: 0, items: [] }, running)).toBe('Next');
    expect(
      waveTitle({ kind: 'planned', ordinal: 11, estimated_start_ms: DEMO_NOW + 3_600_000, items: [] }, proposed),
    ).toBe('Wave 12 · About +1 h');
    expect(
      waveTitle({ kind: 'planned', ordinal: 1, estimated_start_ms: DEMO_NOW + 600_000, items: [] }, proposed),
    ).toBe('Second · About +10 min');
  });

  it('groups each column by declared project and keeps cards apart from this project’s tickets', () => {
    const columns = timelineColumns(demoTimeline('running'), { selectedStep: 'step-4', collapsedGroups: ['web'] });
    const working = columns.find((column) => column.id === 'orchestration-working_now')!;
    expect(working.tickets.map((ticket) => [ticket.group, ticket.groupName])).toEqual([
      ['api', 'API'],
      ['web', 'Web'],
      ['docs', 'Docs'],
    ]);
    expect(working.collapsedGroups).toEqual(['web']);
    expect(working.tickets.map((ticket) => ticket.selected)).toEqual([false, true, false]);
    for (const ticket of working.tickets) {
      expect(ticket.qualifiedId).toMatch(/^orchestration:step-/);
      expect(stepOfCard(ticket.qualifiedId)).toBe(ticket.qualifiedId!.slice('orchestration:'.length));
      expect(ticket.draggable).toBe(false);
      expect(ticket.upNextEligible).toBe(false);
    }
    expect(stepOfCard('HS2-1:01ARZ3')).toBeUndefined();
    expect(stepOfCard(undefined)).toBeUndefined();
  });

  it('gathers a card under the site its run was placed on, not the site its step names (HS2-6GE7WM)', () => {
    const timeline = demoTimeline('running');
    // The step names the local Web project; the scheduler placed its run on the remote API site.
    const placed: TimelineItem = {
      ...item('step-4'),
      site_id: 'api',
      project_label: 'API',
      location: 'remote',
      machine_label: 'Build host',
    };
    const card = timelineCard(placed, timeline, undefined, demoClock);
    expect([card.group, card.groupName, card.remoteClaim]).toEqual(['api', 'API', true]);
    expect(machineLabel(placed)).toBe('Build host');
    // A server older than the field groups by the step's own target.
    const older = timelineCard({ ...item('step-4'), site_id: undefined }, timeline, undefined, demoClock);
    expect([older.group, older.groupName]).toEqual(['web', 'Web']);
  });

  it('states every item state in words with its tone, and lets the status badge speak for running and done', () => {
    expect(stateWords(item('step-3'), false)).toBeUndefined();
    expect(stateWords(item('step-1'), false)).toBeUndefined();
    expect(stateWords(item('step-5'), false)).toEqual({ label: 'Provisional on WEB-33', tone: 'info' });
    expect(stateWords(item('step-8'), false)).toEqual({ label: 'Ready', tone: 'success' });
    expect(stateWords(item('step-6'), false)).toEqual({ label: 'Waits for permission', tone: 'warning' });
    expect(stateWords(item('step-7'), false)).toEqual({ label: 'Waits for your decision on LCH-1', tone: 'warning' });
    expect(stateWords(item('step-9'), false)).toEqual({ label: 'Waits on API-15', tone: 'neutral' });
    expect(stateWords(item('step-9'), true)).toEqual({ label: 'After API-15', tone: 'neutral' });
    const bare: TimelineItem = { ...item('step-9'), waits_on: undefined, goal_slug: undefined };
    expect(stateWords(bare, false)?.label).toBe('Waits on a prerequisite');
    expect(stateWords({ ...bare, state: 'waits_for_decision' }, false)?.label).toBe('Waits for your decision');
    expect(stateWords({ ...bare, state: 'provisional' }, false)?.label).toBe('Provisional');
    // HS2-36XDDX: a step its template holds for a person's accept, proposed or running alike.
    const held: TimelineItem = {
      ...bare,
      state: 'waits_for_accept',
      accept: { operation_id: 'op', template_name: 'T' },
    };
    for (const proposed of [false, true])
      expect(stateWords(held, proposed)).toEqual({
        label: 'Waits for the plan to be accepted',
        tone: 'warning',
      });
    expect(stateBadge(held)).toEqual({ label: 'Waits for accept', tone: 'warning' });
  });

  it('maps an item onto the shared ticket summary with its orchestration line', () => {
    const timeline = demoTimeline('running');
    const remote = timelineCard(item('step-3'), timeline, 'step-3', demoClock);
    expect(remote).toMatchObject({
      slug: 'API-12',
      title: 'Add the single sign-on token endpoint',
      status: 'started',
      startedPhase: 'working',
      priority: 'high',
      busy: true,
      remoteClaim: true,
      agentName: 'Claude',
      updatedLabel: 'Since 13:55',
      selected: true,
      orchestration: {
        state: undefined,
        placement: 'Build host · Claude',
        goal: 'LCH-1',
        goalTitle: 'Ship single sign-on',
      },
    });
    const local = timelineCard(item('step-4'), timeline, undefined, demoClock);
    expect(local.remoteClaim).toBe(false);
    expect(local.orchestration?.placement).toBe('This machine · Codex');
    const waiting = timelineCard(item('step-9'), timeline, undefined, demoClock);
    expect(waiting).toMatchObject({ status: 'not_started', blocked: true, busy: false, updatedLabel: 'About 16:30' });
    const done = timelineCard(item('step-1'), timeline, undefined, demoClock);
    expect(done).toMatchObject({ status: 'completed', updatedLabel: 'Done 11:20' });
    // A remote ticket without a label still reads as itself.
    const bare = timelineCard(
      { ...item('step-8'), title: undefined, slug: undefined, tool: undefined },
      timeline,
      undefined,
    );
    expect(bare.title).toBe(`${bare.slug} in Docs`);
    expect(bare.orchestration?.placement).toBe('This machine');
    // A proposed plan reads relative starts and never shows Blocked.
    const proposed = demoTimeline('proposed');
    expect(timelineCard(findTimelineItem(proposed, 'step-9')!.item, proposed, undefined, demoClock)).toMatchObject({
      blocked: false,
      updatedLabel: 'About +2 h',
      orchestration: { state: 'After API-15' },
    });
    expect(cardTime(findTimelineItem(proposed, 'step-3')!.item, proposed, demoClock)).toBe('On start');
    expect(cardTime({ ...item('step-9'), estimated_start_ms: undefined }, timeline)).toBe('Not estimated');
    expect(cardTime({ ...item('step-1'), finished_at_ms: undefined }, timeline)).toBe('Done');
  });

  it('names machines and tools, the start column, the status and why Start is refused', () => {
    expect(machineLabel({ location: 'remote', machine_label: 'Build host' })).toBe('Build host');
    expect(machineLabel({ location: 'remote' })).toBe('Remote server');
    expect(machineLabel({ location: 'unbound' })).toBe('Not bound on this machine');
    expect([toolLabel('claude'), toolLabel('codex'), toolLabel('cursor'), toolLabel(undefined)]).toEqual([
      'Claude',
      'Codex',
      'Cursor',
      undefined,
    ]);
    expect(startColumnId(demoTimeline('running'))).toBe('orchestration-working_now');
    expect(startColumnId(demoTimeline('proposed'))).toBe('orchestration-planned-0');
    expect(startColumnId(demoTimeline('no_plan'))).toBeUndefined();
    expect(timelineStatus(demoTimeline('running'), demoClock)).toBe('Running Since 12:02 · 3 Sessions');
    expect(timelineStatus(demoTimeline('paused'))).toBe('Paused · 2 Sessions');
    // HS2-38R8J2: until placement was ever on, a plan with work started by hand is Not Started, then Paused.
    const neverStarted = { ...demoTimeline('paused'), never_started: true };
    const working = neverStarted.waves.find((wave) => wave.kind === 'working_now')?.items.length ?? 0;
    expect(timelineStatus(neverStarted)).toBe(`Not Started · ${working} Working`);
    expect(timelineStatus({ ...neverStarted, never_started: false })).toBe('Paused · 2 Sessions');
    expect(timelineStatus({ ...neverStarted, waves: [] })).toBe('Not Started · 0 Working');
    expect([
      dryRunPlan(neverStarted),
      dryRunPlan(demoTimeline('paused')),
      dryRunPlan(demoTimeline('proposed')),
    ]).toEqual([true, false, true]);
    // A running plan is never a dry run, whatever the field says.
    expect(dryRunPlan({ ...demoTimeline('running'), never_started: true })).toBe(false);
    expect(timelineStatus(demoTimeline('proposed'))).toBe('Proposed Plan · Not Started');
    expect(timelineStatus(demoTimeline('no_plan'))).toBe('No Plan Yet');
    expect(timelineStatus(demoTimeline('no_projects'))).toBe('No Projects Yet');
    expect(timelineStatus({ ...demoTimeline('running'), running_since_ms: undefined, sessions: 1 })).toBe(
      'Running · 1 Session',
    );
    const blocked = (start_blocked: NonNullable<ReturnType<typeof demoTimeline>['kick_off']['start_blocked']>) =>
      startBlockedReason({
        ...demoTimeline('proposed'),
        kick_off: { can_start: false, can_pause: false, start_blocked },
      });
    expect(blocked('project_software')).toMatch(/Project Type to Orchestration/);
    expect(blocked('no_plan')).toMatch(/no plan/);
    expect(blocked('no_projects')).toMatch(/Add projects/);
    expect(blocked('running')).toBeUndefined();
    const inSettings = (start_blocked?: Parameters<typeof blocked>[0]) =>
      startBlockedInSettings({
        ...demoTimeline('proposed'),
        kick_off: { can_start: false, can_pause: false, start_blocked },
      });
    expect(
      ['no_projects', 'project_software', 'no_plan', 'running', undefined].map((code) => inSettings(code as never)),
    ).toEqual([true, false, false, false, false]);
    expect(startBlockedReason(demoTimeline('proposed'))).toBeUndefined();
    expect(findTimelineItem(undefined, 'step-1')).toBeUndefined();
    expect(findTimelineItem(demoTimeline('running'), 'step-404')).toBeUndefined();
  });
});

describe('a target ticket’s orchestration line in its own project (HS2-S3FKJ7)', () => {
  it('reuses the card’s line and names the orchestration project on the goal', () => {
    const waiting = item('step-6');
    const line = { owner_checkout_id: 'launch-0123', owner_label: 'Launch', proposed: false, item: waiting };
    const shown = targetLineOrchestration(line);
    expect(shown).toEqual({ ...orchestrationLine(waiting, false), goalTitle: `${waiting.goal_title!}, in Launch` });
    expect(shown.state).toBe('Waits for permission');
    // A proposed plan reads as a dry run, as on the timeline.
    const after = item('step-9', 'proposed');
    expect(targetLineOrchestration({ ...line, proposed: true, item: after }).state).toMatch(/^After /);
    expect(targetLineOrchestration({ ...line, item: { ...waiting, goal_title: undefined } }).goalTitle).toBe(
      'In Launch',
    );
    // The timeline card carries the same line.
    const timeline = demoTimeline('running');
    expect(timelineCard(waiting, timeline, undefined).orchestration).toEqual(orchestrationLine(waiting, false));
  });

  it('keys lines by connection and native id', () => {
    const first = item('step-6'),
      second = item('step-7');
    const map = targetLineMap([
      { owner_checkout_id: 'a', owner_label: 'A', proposed: false, item: first },
      { owner_checkout_id: 'a', owner_label: 'A', proposed: false, item: second },
    ]);
    expect(map.size).toBe(2);
    expect(map.get(targetLineKey(first.target.connection_id, first.target.native_id))?.goalTitle).toMatch(/in A$/);
    expect(map.get(targetLineKey('other', first.target.native_id))).toBeUndefined();
    expect(targetLineMap([]).size).toBe(0);
  });
});

describe('provisional work words its ledger state as a worker’s card does (HS2-H5H9QP)', () => {
  const provisional = item('step-5');
  const gate = (
    result: string,
    slug: string | null = 'API-12',
  ): { target_id: string; connection_id: string; native_id: string; slug?: string; result: string } => ({
    target_id: 'api',
    connection_id: 'api-source',
    native_id: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
    ...(slug === null ? {} : { slug }),
    result,
  });

  const words = (state: string, gates: ReturnType<typeof gate>[]) => {
    const line = orchestrationLine({ ...provisional, provisional: { state, gates } }, false);
    return [line.state, line.stateTone];
  };
  const worker = (state: string, gates: ReturnType<typeof gate>[]) => {
    const line = provisionalLine(
      {
        qualified: 'x',
        slug: 'x',
        state,
        gates: gates.map((entry) => ({ slug: entry.slug ?? entry.native_id.slice(-6), result: entry.result })),
      },
      gates.filter((entry) => entry.result === 'red').map((entry) => entry.slug ?? entry.native_id.slice(-6)),
    );
    return [line?.state, line?.stateTone];
  };

  it('uses the Workers view’s line for provisional, at-risk and reopened work', () => {
    expect(words('provisional', [gate('pending')])).toEqual(['Provisional on API-12 · CI running', 'info']);
    expect(words('at_risk', [gate('red')])).toEqual(['At risk · repair API-12 first', 'warning']);
    expect(words('reopened', [gate('red')])).toEqual(['Reopened · the fix invalidated it', 'warning']);
    // The same function, so the same words, as the Workers view.
    for (const [state, gates] of [
      ['provisional', [gate('pending'), gate('unknown', 'WEB-3')]],
      ['at_risk', [gate('red'), gate('pending', 'WEB-3')]],
      ['reopened', [gate('green')]],
    ] as const)
      expect(words(state, [...gates])).toEqual(worker(state, [...gates]));
    // Only red prerequisites are named for repair; an unlabelled one goes by its short id.
    expect(words('at_risk', [gate('red', null), gate('pending', 'WEB-3')])[0]).toBe('At risk · repair 9G5FAV first');
  });

  it('keeps the plan’s words for confirmed work and an item without a record', () => {
    expect(words('confirmed', [gate('green')])).toEqual([
      stateWords(provisional, false)?.label,
      stateWords(provisional, false)?.tone,
    ]);
    expect(orchestrationLine(provisional, false).state).toBe('Provisional on WEB-33');
  });

  it('reaches the timeline card and a target ticket’s line in its own project', () => {
    const atRisk: TimelineItem = { ...provisional, provisional: { state: 'at_risk', gates: [gate('red')] } };
    expect(timelineCard(atRisk, demoTimeline('running'), undefined).orchestration?.state).toBe(
      'At risk · repair API-12 first',
    );
    expect(
      targetLineOrchestration({ owner_checkout_id: 'a', owner_label: 'A', proposed: false, item: atRisk }).stateTone,
    ).toBe('warning');
  });
});

describe('HS2-FXY5QM a run nobody is plainly working on says so', () => {
  const running = item('step-3');
  const withRun = (run: Partial<NonNullable<TimelineItem['run']>>, state: TimelineItem['state'] = 'running') => ({
    ...running,
    state,
    run: { ...running.run!, ...run },
  });

  it('words each run condition with its tone, the most urgent first', () => {
    expect(running.state).toBe('running');
    expect(runAttention(running.run)).toBeUndefined();
    expect(runAttention(undefined)).toBeUndefined();
    const rows = [
      [{ state: 'intervened' }, 'No worker holds it', 'warning'],
      [{ paused: true }, 'Paused', 'neutral'],
      [{ cancel_requested: true }, 'Cancelling', 'neutral'],
      [{ unreachable_since_ms: 5 }, 'Project unreachable', 'warning'],
      // Several at once: unreachable, then cancelling, then no worker, then paused.
      [
        { state: 'intervened', paused: true, cancel_requested: true, unreachable_since_ms: 5 },
        'Project unreachable',
        'warning',
      ],
      [{ state: 'intervened', paused: true, cancel_requested: true }, 'Cancelling', 'neutral'],
      [{ state: 'intervened', paused: true }, 'No worker holds it', 'warning'],
    ] as const;
    for (const [run, label, tone] of rows) {
      const attention = runAttention({ ...running.run!, ...run });
      expect([attention?.label, attention?.tone]).toEqual([label, tone]);
      expect(attention?.next).toBeTruthy();
    }
    // A finished run asks nothing of a person, whatever flags it kept.
    for (const state of ['completed', 'failed', 'cancelled'])
      expect(runAttention({ ...running.run!, state, paused: true, unreachable_since_ms: 5 })).toBeUndefined();
  });

  it('puts it on the card line instead of the busy Working state, and back once the worker returns', () => {
    const timeline = demoTimeline('running');
    const healthy = timelineCard(running, timeline, undefined, demoClock);
    expect([healthy.busy, healthy.startedPhase, healthy.orchestration?.state]).toEqual([true, 'working', undefined]);
    const gone = timelineCard(withRun({ state: 'intervened' }), timeline, undefined, demoClock);
    expect([gone.busy, gone.startedPhase, gone.status]).toEqual([false, undefined, 'started']);
    expect([gone.orchestration?.state, gone.orchestration?.stateTone]).toEqual(['No worker holds it', 'warning']);
    // Its worker claims it again: dispatched, working, no line.
    const back = timelineCard(withRun({ state: 'dispatched' }), timeline, undefined, demoClock);
    expect([back.busy, back.orchestration?.state]).toEqual([true, undefined]);
    // Provisional work: the run's condition comes before the ledger's words.
    expect(orchestrationLine(withRun({ paused: true }, 'provisional'), false).state).toBe('Paused');
    // A step that is not running ignores a stale run.
    expect(orchestrationLine(withRun({ state: 'intervened' }, 'ready'), false).state).toBe('Ready');
  });

  it('words every run state for the inspector, never the code', () => {
    expect(
      ['requested', 'queued', 'dispatched', 'intervened', 'completed', 'failed', 'cancelled'].map((state) =>
        runStateText({ state, paused: false }),
      ),
    ).toEqual([
      'Requested; its project has not been asked yet',
      'Queued for its project’s dispatcher',
      'Dispatched to a worker',
      'No worker holds it',
      'Completed',
      'Failed',
      'Cancelled',
    ]);
    expect(runStateText({ state: 'dispatched', paused: true })).toBe('Dispatched to a worker, paused');
    expect(runStateText({ state: 'new_state', paused: false })).toBe('New state');
  });
});

describe('HS2-9FS7YX a running card whose step the scheduler holds back reads held, not working', () => {
  const running = item('step-3');
  const MINUTE = 60_000;
  const lost = (placement: Partial<NonNullable<TimelineItem['placement']>>, rest: Partial<TimelineItem> = {}) =>
    ({
      ...running,
      session_state: 'lost',
      placement: {
        at_ms: DEMO_NOW,
        decision: 'unplaced',
        reason: 'never_joined',
        detail: 'replacements_exhausted',
        retry_at_ms: DEMO_NOW + 60 * MINUTE,
        ...placement,
      },
      ...rest,
    }) as TimelineItem;
  const timeline = demoTimeline('running');

  it('holds an unplaced step with no live session, with its retry time, on the card and the badge', () => {
    const held = lost({});
    expect(schedulerHold(held, demoClock)?.label).toBe('Held until 15:00');
    expect(schedulerHold(held, demoClock)?.next).toContain('by 15:00');
    const card = timelineCard(held, timeline, undefined, demoClock);
    expect([card.busy, card.startedPhase, card.status, card.remoteClaim]).toEqual([false, undefined, 'started', false]);
    expect([card.orchestration?.state, card.orchestration?.stateTone]).toEqual(['Held until 15:00', 'warning']);
    expect(stateBadge(held)).toEqual({ label: 'Held', tone: 'warning' });
    // Another reason a step is left unplaced, such as a held resource, holds it too; without a retry it says so.
    const resource = lost({ reason: 'resource_held', detail: undefined, retry_at_ms: undefined });
    expect(schedulerHold(resource, demoClock)?.label).toBe('Held');
    expect(schedulerHold(resource, demoClock)?.next).toContain('when what it waits on changes');
    // No session record at all is no live session either.
    expect(schedulerHold(lost({}, { session_state: undefined, session_id: undefined }), demoClock)).toBeDefined();
  });

  it('says a kept session that never joined is left for a person', () => {
    const kept = lost({ decision: 'keep', retry_at_ms: undefined }, { session_state: 'running' });
    const card = timelineCard(kept, timeline, undefined, demoClock);
    expect([card.busy, card.orchestration?.state]).toEqual([false, 'Session did not join']);
    expect(schedulerHold(kept)?.next).toContain('left for you');
    // A plain keep, or a never-joined keep that still has replacements, is work going on.
    expect(
      schedulerHold(lost({ decision: 'keep', reason: 'running', detail: undefined }, { session_state: 'running' })),
    ).toBeUndefined();
    expect(schedulerHold(lost({ decision: 'keep', detail: undefined }, { session_state: 'running' }))).toBeUndefined();
  });

  it('is not held for work under way, a stale decision, a finished run or a step not running', () => {
    expect(schedulerHold(running)).toBeUndefined();
    expect(timelineCard(running, timeline, undefined, demoClock).busy).toBe(true);
    // A session runs it (a person started one by hand after the hold).
    expect(schedulerHold(lost({}, { session_state: 'running' }))).toBeUndefined();
    // A start or keep is not a hold.
    expect(schedulerHold(lost({ decision: 'start', reason: 'ready' }))).toBeUndefined();
    // The decision is older than the run, so it was about an earlier run.
    expect(schedulerHold(lost({ at_ms: running.run!.created_at_ms - 1 }))).toBeUndefined();
    expect(schedulerHold(lost({ at_ms: running.run!.created_at_ms }))).toBeDefined();
    for (const state of ['completed', 'failed', 'cancelled'])
      expect(schedulerHold(lost({}, { run: { ...running.run!, state } }))).toBeUndefined();
    expect(schedulerHold(lost({}, { run: undefined }))).toBeUndefined();
    expect(schedulerHold(lost({}, { placement: undefined }))).toBeUndefined();
    for (const state of ['ready', 'done', 'waits_for_prerequisite'] as const)
      expect(schedulerHold(lost({}, { state }))).toBeUndefined();
    expect(stateBadge(lost({}, { state: 'ready' }))).toEqual({ label: 'Ready', tone: 'success' });
  });

  it('comes after the run’s own condition and before the ledger, and lets go once a session starts again', () => {
    expect(runNotice(lost({}, { run: { ...running.run!, paused: true } }), demoClock)?.label).toBe('Paused');
    expect(orchestrationLine(lost({}, { state: 'provisional' }), false, demoClock).state).toBe('Held until 15:00');
    expect(runNotice(lost({}, { state: 'ready' }))).toBeUndefined();
    // Held, then the scheduler starts a session at the hour: a newer start, a running session, working again.
    const again = lost(
      { at_ms: DEMO_NOW + 60 * MINUTE, decision: 'start', reason: 'ready', detail: undefined, retry_at_ms: undefined },
      { session_state: 'running' },
    );
    const card = timelineCard(again, timeline, undefined, demoClock);
    expect([card.busy, card.startedPhase, card.orchestration?.state]).toEqual([true, 'working', undefined]);
    expect(stateBadge(again).label).toBe('Running');
    // It crashes too and is held again.
    expect(
      timelineCard(
        lost({ at_ms: DEMO_NOW + 61 * MINUTE, retry_at_ms: DEMO_NOW + 121 * MINUTE }),
        timeline,
        undefined,
        demoClock,
      ).orchestration?.state,
    ).toBe('Held until 16:01');
  });
});

describe('HS2-2751D6 a card whose prerequisite’s CI failed says so', () => {
  const waiting = item('step-9');
  const wait = waiting.waits_on![0];
  const withReason = (reason: string) => ({ ...waiting, waits_on: [{ ...wait, reason }] });

  it('reads CI failed with a warning while running, and plain after a prerequisite otherwise', () => {
    expect(stateWords(withReason('gate_failed'), false)).toEqual({
      label: `Waits on ${wait.slug} · CI failed`,
      tone: 'warning',
    });
    expect(stateWords(withReason('gate_pending'), false)).toEqual({ label: `Waits on ${wait.slug}`, tone: 'neutral' });
    expect(stateWords(withReason('prerequisite_not_finished'), false)?.label).toBe(`Waits on ${wait.slug}`);
    // A proposed plan names the order, not a verdict.
    expect(stateWords(withReason('gate_failed'), true)?.label).toBe(`After ${wait.slug}`);
    // Without a slug or a wait it falls back as before.
    expect(stateWords({ ...waiting, waits_on: [] }, false)?.label).toBe('Waits on a prerequisite');
    // The repair lands: the same card reads plainly again.
    const timeline = demoTimeline('running');
    const card = (reason: string) => timelineCard(withReason(reason), timeline, undefined, demoClock).orchestration;
    expect(card('gate_failed')?.state).toBe(`Waits on ${wait.slug} · CI failed`);
    expect(card('gate_pending')?.stateTone).toBe('neutral');
  });
});

describe('HS2-89578K a Done card whose CI gate failed reads as needing repair', () => {
  const done = item('step-1');
  const withCi = (ci_gate?: string) => ({ ...done, run: { ...done.run!, ci_gate } });
  const timeline = demoTimeline('running');
  const card = (ci?: string) => timelineCard(withCi(ci), timeline, undefined, demoClock).orchestration;

  it('says CI failed with a warning and a next step, and reads plainly done otherwise', () => {
    expect(done.state).toBe('done');
    expect(doneRepair(withCi('failed'))).toMatchObject({ label: 'CI failed', tone: 'warning' });
    expect(doneRepair(withCi('failed'))?.next).toContain('repair it as the Gates section says');
    expect(card('failed')).toMatchObject({ state: 'CI failed', stateTone: 'warning' });
    expect(stateBadge(withCi('failed'))).toEqual({ label: 'Done · CI failed', tone: 'warning' });
    expect(orchestrationLine(withCi('failed'), false).state).toBe('CI failed');
    // Every other verdict, and no gate, keeps Done as it was.
    for (const ci of [undefined, 'passed', 'pending', 'unknown', 'superseded']) {
      expect(doneRepair(withCi(ci)), String(ci)).toBeUndefined();
      expect(card(ci)?.state, String(ci)).toBeUndefined();
      expect(stateBadge(withCi(ci)), String(ci)).toEqual({ label: 'Done', tone: 'neutral' });
    }
    // A finished run with no run record, and unfinished work with a failed gate, are not Done repairs.
    expect(doneRepair({ ...done, run: undefined })).toBeUndefined();
    const running = item('step-3');
    expect(doneRepair({ ...running, run: { ...running.run!, ci_gate: 'failed' } })).toBeUndefined();
  });

  it('follows the verdict through red, repair and green, and red again', () => {
    expect(['pending', 'failed', 'pending', 'passed', 'failed'].map((ci) => card(ci)?.state)).toEqual([
      undefined,
      'CI failed',
      undefined,
      undefined,
      'CI failed',
    ]);
  });

  it('is the same line in the target project’s own List', () => {
    const line = targetLineOrchestration({
      item: withCi('failed'),
      proposed: false,
      owner_label: 'Launch',
    } as Parameters<typeof targetLineOrchestration>[0]);
    expect(line).toMatchObject({ state: 'CI failed', stateTone: 'warning' });
  });
});

describe('HS2-GPABYY a Done card whose deployment failed or whose review asks for changes reads as needing repair', () => {
  const done = item('step-1');
  type Gates = { ci_gate?: string; deployment_gate?: string; review_gate?: string };
  const withGates = (gates: Gates) => ({ ...done, run: { ...done.run!, ...gates } });
  const timeline = demoTimeline('running');
  const card = (gates: Gates) => timelineCard(withGates(gates), timeline, undefined, demoClock).orchestration;

  it('says Deploy failed or Changes requested with a warning and a next step', () => {
    const deploy = withGates({ deployment_gate: 'failed' });
    expect(doneRepair(deploy)).toMatchObject({ label: 'Deploy failed', tone: 'warning' });
    expect(doneRepair(deploy)?.next).toContain('fix the deployment and request it again');
    expect(card({ deployment_gate: 'failed' })).toMatchObject({ state: 'Deploy failed', stateTone: 'warning' });
    expect(stateBadge(deploy)).toEqual({ label: 'Done · Deploy failed', tone: 'warning' });
    const review = withGates({ review_gate: 'failed' });
    expect(doneRepair(review)).toMatchObject({ label: 'Changes requested', tone: 'warning' });
    expect(doneRepair(review)?.next).toContain('request the review again');
    expect(card({ review_gate: 'failed' })).toMatchObject({ state: 'Changes requested', stateTone: 'warning' });
    expect(stateBadge(review)).toEqual({ label: 'Done · Changes requested', tone: 'warning' });
    expect(orchestrationLine(review, false).state).toBe('Changes requested');
  });

  it('reads plainly Done for every other verdict, and never for unfinished work', () => {
    for (const verdict of [undefined, 'passed', 'pending', 'unknown', 'awaiting_approval', 'superseded']) {
      for (const gates of [{ deployment_gate: verdict }, { review_gate: verdict }]) {
        expect(doneRepair(withGates(gates)), JSON.stringify(gates)).toBeUndefined();
        expect(card(gates)?.state, JSON.stringify(gates)).toBeUndefined();
        expect(stateBadge(withGates(gates))).toEqual({ label: 'Done', tone: 'neutral' });
      }
    }
    const running = item('step-3');
    for (const gates of [{ deployment_gate: 'failed' }, { review_gate: 'failed' }])
      expect(doneRepair({ ...running, run: { ...running.run!, ...gates } })).toBeUndefined();
  });

  it('names CI first, then the deployment, then the review, and follows each repair', () => {
    const all = { ci_gate: 'failed', deployment_gate: 'failed', review_gate: 'failed' };
    expect(card(all)?.state).toBe('CI failed');
    expect(card({ ...all, ci_gate: 'passed' })?.state).toBe('Deploy failed');
    expect(card({ ...all, ci_gate: 'passed', deployment_gate: 'pending' })?.state).toBe('Changes requested');
    expect(card({ ci_gate: 'passed', deployment_gate: 'passed', review_gate: 'passed' })?.state).toBeUndefined();
    // A review sent back, requested again, approved, then sent back again.
    expect(
      ['failed', 'awaiting_approval', 'passed', 'failed'].map((review_gate) => card({ review_gate })?.state),
    ).toEqual(['Changes requested', undefined, undefined, 'Changes requested']);
  });

  it('is the same line in the target project’s own List', () => {
    const line = targetLineOrchestration({
      item: withGates({ deployment_gate: 'failed' }),
      proposed: false,
      owner_label: 'Launch',
    } as Parameters<typeof targetLineOrchestration>[0]);
    expect(line).toMatchObject({ state: 'Deploy failed', stateTone: 'warning' });
  });
});

describe('HS2-JGNCAR a running card whose ticket another worker took names no session of this machine', () => {
  const running = item('step-3');
  const elsewhere = { ...running, session_id: undefined, session_state: undefined, worked_elsewhere: true };

  it('reads the Session row plainly in every state', () => {
    expect(sessionText(elsewhere)).toBe('None here: another worker took the ticket');
    // Before HS2-JGNCAR's flag, or once a session of this machine carries the run, its own session reads.
    expect(sessionText(running)).toBe('Claude session, running');
    expect(sessionText({ ...running, tool: 'codex', session_state: undefined })).toBe('Codex session, state unknown');
    expect(sessionText({ ...running, session_id: undefined, state: 'done' })).toBe('Ended');
    expect(sessionText({ ...running, session_id: undefined, state: 'ready', run: undefined })).toBe(
      'Starts when the work is placed',
    );
    // The flag wins over a stale session id a fixture or an older server might still carry.
    expect(sessionText({ ...running, worked_elsewhere: true })).toBe('None here: another worker took the ticket');
  });

  it('reads the Tool row as not known here unless the step names one', () => {
    expect(toolText({ ...elsewhere, tool: undefined })).toBe('Not known here');
    expect(toolText({ ...elsewhere, tool: 'gemini' })).toBe('Gemini');
    expect(toolText({ ...running, tool: undefined, worked_elsewhere: undefined })).toBe(
      'Chosen when the work is placed',
    );
  });

  it('is never held back, whatever the decision that placed the freed session says', () => {
    const unplaced = {
      ...elsewhere,
      placement: { at_ms: DEMO_NOW, decision: 'unplaced', reason: 'resource_held' },
    } as TimelineItem;
    expect(schedulerHold(unplaced, demoClock)).toBeUndefined();
    expect(stateBadge(unplaced).label).not.toBe('Held');
    // The same card without the flag is held, so the flag is what clears it.
    expect(schedulerHold({ ...unplaced, worked_elsewhere: false }, demoClock)?.label).toBe('Held');
  });
});

describe('held template plans (HS2-0YHA2S)', () => {
  it('counts each held step once and names its templates in timeline order', () => {
    const timeline = structuredClone(demoTimeline('running'));
    expect(heldTemplatePlans(timeline)).toBeUndefined();
    const items = timeline.waves.flatMap((wave) => wave.items);
    const names = ['Repair the release', 'Document a feature', 'Repair the release'];
    items.slice(0, 3).forEach((item, n) => {
      item.state = 'waits_for_accept';
      item.accept = { operation_id: `tpl-${n}`, template_name: names[n] };
    });
    // The same step in two waves counts once; a held item without its apply still counts.
    timeline.waves[0].items.push({ ...items[0] });
    items[3].state = 'waits_for_accept';
    expect(heldTemplatePlans(timeline)).toEqual({
      steps: 4,
      templates: ['Repair the release', 'Document a feature'],
    });
  });

  it('words the status and the tooltip for one, two and more templates', () => {
    expect(heldTemplateStatus({ steps: 1 })).toBe(' · 1 step waits for an accept');
    expect(heldTemplateStatus({ steps: 4 })).toBe(' · 4 steps wait for an accept');
    expect(heldTemplateTitle({ steps: 1, templates: ['A'] })).toBe(
      'Accept the planned steps of A (1 step); placement starts them within the Orchestration settings',
    );
    expect(heldTemplateTitle({ steps: 2, templates: ['A', 'B'] })).toContain('of A and B (2 steps)');
    expect(heldTemplateTitle({ steps: 5, templates: ['A', 'B', 'C'] })).toContain('of A, B and C (5 steps)');
    expect(heldTemplateTitle({ steps: 2, templates: [] })).toContain('of the held templates (2 steps)');
  });
});
