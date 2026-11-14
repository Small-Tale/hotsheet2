import { describe, expect, it } from 'vitest';

import type { TimelineItem } from './orchestration-timeline';
import {
  cardTime,
  findTimelineItem,
  machineLabel,
  orchestrationLine,
  startBlockedReason,
  startColumnId,
  stateWords,
  stepOfCard,
  targetLineKey,
  targetLineMap,
  targetLineOrchestration,
  timelineCard,
  timelineColumns,
  timelineStatus,
  toolLabel,
  waveTitle,
} from './orchestration-timeline-board';
import { DEMO_NOW, demoClock, demoTimeline } from './ux-demo/orchestration-fixtures';

const item = (step: string, phase: Parameters<typeof demoTimeline>[0] = 'running') =>
  findTimelineItem(demoTimeline(phase), step)!.item;

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
    expect(blocked('policy_absent')).toMatch(/Orchestration settings first/);
    expect(blocked('project_software')).toMatch(/Project Type to Orchestration/);
    expect(blocked('no_plan')).toMatch(/no plan/);
    expect(blocked('no_projects')).toMatch(/Add projects/);
    expect(blocked('running')).toBeUndefined();
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
