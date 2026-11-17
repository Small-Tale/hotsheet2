import { afterEach, describe, expect, it, vi } from 'vitest';

import { findTimelineItem } from '../orchestration-timeline-board';
import { demoClock, demoTimeline } from '../ux-demo/orchestration-fixtures';
import { OrchestrationInspector, orchestrationInspectorPanel } from './orchestration-inspector';
import { OrchestrationTimelineView } from './orchestration-timeline-view';
import { OrchestrationToolbar } from './orchestration-toolbar';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
}));

const item = (step: string) => findTimelineItem(demoTimeline('running'), step)!.item;

describe('OrchestrationTimelineView (HS2-J89JC7)', () => {
  it('renders the board with Done rightmost inside the view root that other handlers leave alone', () => {
    const markup = String(OrchestrationTimelineView({ timeline: demoTimeline('running'), clock: demoClock }));
    expect(markup).toContain('data-orchestration-view="true"');
    expect(markup).toContain('data-phase="running"');
    const order = [...markup.matchAll(/data-column-id="([^"]+)"/g)].map((match) => match[1]);
    expect(order).toEqual([
      'orchestration-later',
      'orchestration-then',
      'orchestration-next',
      'orchestration-working_now',
      'orchestration-done',
    ]);
    expect(markup).toContain('aria-label="Orchestration timeline"');
    expect(markup).toContain('data-ticket-group="api"');
  });

  it('distinguishes loading, no projects, no plan and a failed read', () => {
    expect(String(OrchestrationTimelineView({ loading: true }))).toContain('Loading the Orchestration Timeline');
    const none = String(OrchestrationTimelineView({ timeline: demoTimeline('no_projects') }));
    expect(none).toContain('No Projects to Orchestrate Yet');
    expect(none).toContain('data-action="choose-orchestration-projects"');
    const noPlan = String(OrchestrationTimelineView({ timeline: demoTimeline('no_plan') }));
    expect(noPlan).toContain('No Plan Yet');
    expect(noPlan).toMatch(
      /data-action="propose-orchestration-plan"[^>]*disabled|disabled[^>]*data-action="propose-orchestration-plan"/,
    );
    const ready = String(
      OrchestrationTimelineView({ timeline: demoTimeline('no_plan'), propose: { available: true } }),
    );
    expect(ready).not.toMatch(/data-action="propose-orchestration-plan"[^>]*disabled/);
    const failed = String(OrchestrationTimelineView({ timeline: demoTimeline('running'), error: 'Offline.' }));
    expect(failed).toContain('The Orchestration Timeline Could Not Be Read');
    expect(failed).toContain('data-column-id="orchestration-done"');
  });
});

describe('OrchestrationToolbar (HS2-J89JC7)', () => {
  it('offers one kick-off action per phase and says why Start is refused', () => {
    const toolbar = (phase: Parameters<typeof demoTimeline>[0], extra = {}) =>
      String(OrchestrationToolbar({ timeline: demoTimeline(phase), clock: demoClock, ...extra }));
    expect(toolbar('proposed')).toContain('This Plan Is a Dry Run');
    expect(toolbar('proposed')).toContain('data-action="start-orchestration"');
    expect(toolbar('running')).toContain('data-action="pause-orchestration"');
    expect(toolbar('running')).toContain('Running Since 12:02 · 3 Sessions');
    expect(toolbar('paused')).toContain('data-action="start-orchestration"');
    expect(toolbar('no_plan')).toContain('data-action="propose-orchestration-plan"');
    expect(toolbar('no_plan', { planning: 'planning' })).toContain('No Plan Yet · Planning');
    expect(toolbar('no_projects')).not.toContain('data-action=');
    const blocked = String(
      OrchestrationToolbar({
        timeline: {
          ...demoTimeline('proposed'),
          kick_off: { can_start: false, can_pause: false, start_blocked: 'policy_absent' },
        },
      }),
    );
    expect(blocked).toContain('Set the orchestration limits in Orchestration settings first');
    expect(blocked).toMatch(
      /data-action="start-orchestration"[^>]*disabled|disabled[^>]*data-action="start-orchestration"/,
    );
  });
});

describe('OrchestrationInspector (HS2-J89JC7)', () => {
  const inspector = (step: string, extra = {}) =>
    String(OrchestrationInspector({ item: item(step), timeline: demoTimeline('running'), clock: demoClock, ...extra }));

  it('shows where it runs, the run with the person’s controls, and its evidence', () => {
    const markup = inspector('step-3', {
      details: {
        loading: false,
        gates: [{ kind: 'ci', state: 'pending', commit: '3f9c2a1d00' }],
        validations: [{ step_id: 'contract-check', state: 'running' }],
        events: [{ at_ms: 0, kind: 'state', state: 'dispatched', source: 'target' }],
      },
    });
    for (const section of ['Where It Runs', 'Run', 'Gates', 'Validation', 'Events'])
      expect(markup).toContain(`data-orchestration-section="${section}"`);
    expect(markup).toContain('Build host');
    for (const control of ['pause', 'handoff', 'cancel']) expect(markup).toContain(`data-control="${control}"`);
    expect(markup).toContain('Correct…');
    expect(markup).toContain('CI · 3f9c2a1');
    expect(markup).toContain('learned from the project');
    expect(inspector('step-3', { busy: true })).toMatch(
      /data-control="pause"[^>]*disabled|disabled[^>]*data-control="pause"/,
    );
    const paused = String(
      OrchestrationInspector({
        item: { ...item('step-3'), run: { ...item('step-3').run!, paused: true } },
        timeline: demoTimeline('running'),
      }),
    );
    expect(paused).toContain('data-control="resume"');
  });

  it('links a waiting card to its request or its goal decision, and names prerequisites', () => {
    const permission = inspector('step-6');
    expect(permission).toContain('data-orchestration-section="Waits for Permission"');
    expect(permission).toContain('data-action="show-orchestration-request"');
    expect(permission).toContain('data-request-id="request-6"');
    // HS2-DBQSWB: the action reads as words a person uses, not its code.
    expect(permission).toContain('The orchestrator asked to start a session in');
    expect(permission).not.toContain('session start');
    const decision = inspector('step-7');
    expect(decision).toContain('data-action="open-orchestration-goal"');
    expect(decision).toContain('Open LCH-1');
    expect(inspector('step-9')).toContain('API-15');
    expect(inspector('step-8')).toContain('target cap');
    // HS2-0D60T8: provisional work has its Built Ahead section instead of a banner.
    expect(inspector('step-5')).toContain('data-orchestration-section="Built Ahead"');
    // A finished or unstarted item has no run controls.
    expect(inspector('step-8')).not.toContain('data-control=');
    expect(inspector('step-1')).not.toContain('data-control=');
  });

  it('HS2-15G5T4 places the Plan section after what the card waits on, before its Run', () => {
    const sections = (markup: string) =>
      [...markup.matchAll(/data-orchestration-section="([^"]+)"/g)].map((match) => match[1]);
    expect(sections(inspector('step-3'))).toEqual(['Where It Runs', 'Plan', 'Run']);
    expect(sections(inspector('step-8'))).toEqual(['Where It Runs', 'Why It Is Not Running', 'Plan']);
    expect(sections(inspector('step-9'))).toEqual(['Where It Runs', 'Prerequisites', 'Plan']);
    const running = inspector('step-3');
    expect(running).toContain('In progress');
    expect(running).toContain('WEB-30');
    const ready = inspector('step-8');
    expect(ready).toContain('Confirmed');
    expect(ready).toContain('Nothing waits on it');
    expect(inspector('step-7')).toContain('API-12: integrated; its CI is still running');
    // Prerequisites already lists API-15; Plan does not repeat it as a Why row.
    expect(inspector('step-9').match(/>Why</g)).toBeNull();
    const cyclic = String(
      OrchestrationInspector({
        item: { ...item('step-8'), state: 'waits_for_prerequisite', level: 'cyclic', reasons: ['plan_cycle'] },
        timeline: demoTimeline('running'),
      }),
    );
    expect(cyclic).toContain('In a cycle');
    expect(cyclic).toContain('Its prerequisites lead back to this step');
  });

  it('HS2-XM692S adds the last reconciliation to the Run section beside the shipped controls', () => {
    const run = (details: object) => {
      const markup = inspector('step-3', { details: { loading: false, ...details } });
      return markup.slice(markup.indexOf('data-orchestration-section="Run"'));
    };
    const checked = run({
      runRead: true,
      reconciliation: {
        at_ms: demoTimeline('running').now_ms,
        disposition: 'hold_unverified',
        findings: [
          { source: 'ci_run', verification: 'unverifiable', reason: 'provider_unreachable' },
          { source: 'target_claim', verification: 'mismatched', reason: 'different_live_work' },
        ],
      },
    });
    expect(checked).toContain('Held: something it depends on could not be verified, since 14:00');
    expect(checked).toContain('CI (provider unreachable)');
    expect(checked).toContain('Claim (different live work)');
    for (const control of ['pause', 'handoff', 'cancel']) expect(checked).toContain(`data-control="${control}"`);
    expect(run({ runRead: true })).toContain('Not checked since its last change');
    // Until the run record is read, no Checked row claims anything.
    expect(run({})).not.toContain('>Checked<');
  });

  it('HS2-9XJEZC shows each validation step’s evidence and one Run Validation, and a wait for permission', () => {
    const steps = [
      { step_id: 'contract-check', check: 'profile', state: 'passed', at_ms: 0, exit_code: 0, evidence: '4f2a91c' },
      { step_id: 'smoke', check: 'command', state: 'failed', reason: 'exit_code_mismatch', exit_code: 1 },
    ];
    const section = (details: object, extra = {}) => {
      const markup = inspector('step-3', { details: { loading: false, validations: steps, ...details }, ...extra });
      return markup.slice(markup.indexOf('data-orchestration-section="Validation"'));
    };
    const shown = section({});
    expect(shown).toContain('evidence 4f2a91c');
    expect(shown).toContain('smoke (command)');
    expect(shown).toContain('Failed · exit code mismatch · exit 1');
    expect(shown).toContain('data-action="run-orchestration-validation"');
    expect(shown).not.toMatch(/data-action="run-orchestration-validation"[^>]*disabled/);
    expect(shown).not.toContain('data-action="show-orchestration-request"');
    const waiting = section({ pendingValidations: [{ step_id: 'smoke', operation_id: 'op', request_id: 'req-1' }] });
    expect(waiting).toContain('Waits for permission');
    expect(waiting).toContain('data-request-id="req-1"');
    // Busy, or nothing left to run, disables it.
    expect(section({}, { busy: true })).toMatch(
      /data-action="run-orchestration-validation"[^>]*disabled|disabled[^>]*data-action="run-orchestration-validation"/,
    );
    const done = String(
      OrchestrationInspector({
        item: item('step-3'),
        timeline: demoTimeline('running'),
        details: { loading: false, validations: [steps[0]] },
      }),
    );
    expect(done).toContain('Every step passed or is running');
  });

  it('HS2-0D60T8 shows built-ahead work with its gates, the At Risk banner and Reopen… for a person', () => {
    const work = {
      state: 'at_risk',
      depth: 1,
      max_depth: 2,
      recovery: 'rebase_onto_fix',
      held: true,
      gates: [
        {
          target_id: 'web',
          connection_id: 'web-tickets',
          native_id: 'ticket-WEB-33',
          commit: '3f9c2a1d00',
          result: 'red',
        },
      ],
    };
    const section = (details: object) => {
      const markup = inspector('step-5', { details: { loading: false, ...details } });
      return markup.slice(markup.indexOf('data-orchestration-section="Built Ahead"'));
    };
    const risky = section({ readinessRead: true, builtAhead: work });
    expect(risky).toContain('At Risk');
    expect(risky).toContain('WEB-33’s CI failed. Repair WEB-33 first');
    expect(risky).toContain('WEB-33 · CI failed on 3f9c2a1');
    expect(risky).toContain('1 of 2 allowed');
    expect(risky).toContain('Held off the shipping branch for now');
    expect(risky).toContain('Reopen…');
    expect(risky).toContain('data-reopen-reason="fix_changed_contract"');
    const reopened = section({
      readinessRead: true,
      builtAhead: { ...work, state: 'reopened', reopen_reason: 'work_needs_redo' },
    });
    expect(reopened).toContain('Reopened (work needs redo)');
    expect(reopened).not.toContain('Reopen…');
    expect(reopened).not.toContain('At Risk');
    // Before the record is read, the section says only what the card is.
    expect(section({})).toContain('rests on a prerequisite whose CI is still running');
    // Plan comes first, then Built Ahead, then the Run.
    const order = [...inspector('step-5').matchAll(/data-orchestration-section="([^"]+)"/g)].map((match) => match[1]);
    expect(order).toEqual(['Where It Runs', 'Plan', 'Built Ahead', 'Run']);
    // A running card shows the section only once a record says it was built ahead.
    expect(inspector('step-3')).not.toContain('Built Ahead');
    expect(inspector('step-3', { details: { loading: false, builtAhead: { ...work, state: 'confirmed' } } })).toContain(
      'data-orchestration-section="Built Ahead"',
    );
  });

  it('is a right-rail panel with an empty selection', () => {
    expect(String(OrchestrationInspector({}))).toContain('Select a card');
    const panel = orchestrationInspectorPanel({ item: item('step-3'), timeline: demoTimeline('running') });
    expect(panel.label).toBe('Orchestration Inspector');
    expect(String(panel.toolbar.center)).toContain('API-12 in API');
    expect(String(orchestrationInspectorPanel({}).toolbar.center)).toContain('Nothing selected');
  });
});

describe('wireOrchestrationTimeline (HS2-J89JC7)', () => {
  afterEach(() => {
    registrations.length = 0;
  });
  it('selects plan steps, collapses groups in its own scope, and routes every action', async () => {
    const { wireOrchestrationTimeline } = await import('./orchestration-timeline-interactions');
    const calls: string[] = [];
    const bindings = {
      select: (step: string) => calls.push(`select ${step}`),
      toggleGroup: (group: string) => calls.push(`group ${group}`),
      start: () => calls.push('start'),
      pause: () => calls.push('pause'),
      propose: () => calls.push('propose'),
      chooseProjects: () => calls.push('choose'),
      runControl: (control: string, state?: string) => calls.push(`run ${control} ${state ?? ''}`),
      showRequest: (id: string) => calls.push(`request ${id}`),
      openGoal: () => calls.push('goal'),
      runValidation: () => calls.push('validate'),
      reopen: (reason: string) => calls.push(`reopen ${reason}`),
    };
    wireOrchestrationTimeline({} as HTMLElement, bindings);
    const view = { dataset: { orchestrationView: 'true' } };
    const target = (dataset: Record<string, string>, inside = true) =>
      ({
        dataset,
        closest: (selector: string) => (inside && selector.includes('orchestration-view') ? view : null),
      }) as unknown as Element;
    const run = (event: string, selector: string, element: Element, init: Partial<KeyboardEvent> = {}) => {
      const found = registrations.find((item) => item.event === event && item.selector.includes(selector))!;
      found.handle({ preventDefault: () => undefined, ...init } as unknown as Event, element);
    };
    run('click', 'select-ticket-row', target({ ticketQualifiedId: 'orchestration:step-3' }));
    run('click', 'select-ticket-row', target({ ticketQualifiedId: 'orchestration:step-4' }, false));
    run('click', 'select-ticket-row', target({ ticketQualifiedId: 'HS2-1:x' }));
    run('keydown', 'select-ticket-row', target({ ticketQualifiedId: 'orchestration:step-5' }), { key: 'Enter' });
    run('keydown', 'select-ticket-row', target({ ticketQualifiedId: 'orchestration:step-5' }), { key: 'a' });
    run('click', 'toggle-ticket-group', target({ ticketGroup: 'api' }));
    run('click', 'start-orchestration', target({}));
    run('click', 'pause-orchestration', target({}));
    run('click', 'propose-orchestration-plan', target({}));
    run('click', 'choose-orchestration-projects', target({}));
    run('click', 'orchestration-run-control', target({ control: 'correct', runState: 'failed' }));
    run('click', 'show-orchestration-request', target({ requestId: 'request-6' }));
    run('click', 'open-orchestration-goal', target({}));
    run('click', 'run-orchestration-validation', target({}));
    run('click', 'reopen-orchestration-work', target({ reopenReason: 'fix_changed_contract' }));
    run('click', 'reopen-orchestration-work', target({}));
    expect(calls).toEqual([
      'select step-3',
      'select step-5',
      'group api',
      'start',
      'pause',
      'propose',
      'choose',
      'run correct failed',
      'request request-6',
      'goal',
      'validate',
      'reopen fix_changed_contract',
    ]);
  });
});

describe('arrow keys on the Orchestration board (HS2-T4V1TX)', () => {
  // Later (empty), Then, Next (empty: its group collapsed), Working Now, Done.
  const board = [[], ['t1', 't2', 't3'], [], ['w1'], ['d1', 'd2']] as const;

  it.each([
    ['ArrowDown', 't1', 't2'],
    ['ArrowDown', 't3', 't3'],
    ['ArrowUp', 't2', 't1'],
    ['ArrowUp', 't1', 't1'],
    // Across an empty column, at the same position or the column's last card.
    ['ArrowRight', 't1', 'w1'],
    ['ArrowRight', 't3', 'w1'],
    ['ArrowRight', 'w1', 'd1'],
    ['ArrowLeft', 'd2', 'w1'],
    ['ArrowLeft', 'w1', 't1'],
    // The board's edges stop the move; the empty Later column is skipped.
    ['ArrowLeft', 't2', 't2'],
    ['ArrowRight', 'd2', 'd2'],
    // A card the board does not draw (its group was collapsed, or it moved away) starts at the first.
    ['ArrowDown', 'gone', 't1'],
    ['ArrowRight', undefined, 't1'],
  ] as const)('%s from %s reaches %s', async (key, current, expected) => {
    const { adjacentOrchestrationCard } = await import('./orchestration-timeline-interactions');
    expect(adjacentOrchestrationCard(board, current, key)).toBe(expected);
  });

  it('walks repeatedly, out and back, and reads an empty board as nothing to move to', async () => {
    const { adjacentOrchestrationCard } = await import('./orchestration-timeline-interactions');
    let at: string | undefined = 't1';
    for (const key of ['ArrowDown', 'ArrowDown', 'ArrowDown', 'ArrowRight', 'ArrowRight', 'ArrowDown'] as const)
      at = adjacentOrchestrationCard(board, at, key);
    expect(at).toBe('d2');
    for (const key of ['ArrowLeft', 'ArrowLeft', 'ArrowUp'] as const) at = adjacentOrchestrationCard(board, at, key);
    // Done's second card returns to Working Now's only card, then to Then's first.
    expect(at).toBe('t1');
    expect(adjacentOrchestrationCard([], 't1', 'ArrowDown')).toBeUndefined();
    expect(adjacentOrchestrationCard([[], []], undefined, 'ArrowLeft')).toBeUndefined();
    // Empty, then refilled: the same read works on the new board.
    expect(adjacentOrchestrationCard([['n1'], ['n2']], 't1', 'ArrowRight')).toBe('n1');
  });

  it('focuses and selects the next card, and leaves modified arrows and other views alone', async () => {
    registrations.length = 0;
    const { wireOrchestrationTimeline } = await import('./orchestration-timeline-interactions');
    const selected: string[] = [];
    const noop = () => undefined;
    wireOrchestrationTimeline({} as HTMLElement, {
      select: (step, moved) => selected.push(`${step}${moved ? ' moved' : ''}`),
      toggleGroup: noop,
      start: noop,
      pause: noop,
      propose: noop,
      chooseProjects: noop,
      runControl: noop,
      showRequest: noop,
      openGoal: noop,
      runValidation: noop,
      reopen: noop,
    });
    const focused: string[] = [];
    const column = { scrollIntoView: noop };
    const card = (id: string) => {
      const node = {
        dataset: { ticketQualifiedId: `orchestration:${id}` },
        focus: () => focused.push(id),
        scrollIntoView: noop,
        closest: (selector: string) =>
          selector.includes('orchestration-view') ? view : selector.includes('board-column') ? column : null,
      };
      return node as unknown as HTMLElement;
    };
    const columns = [[card('a1'), card('a2')], [card('b1')]];
    const view = {
      dataset: { orchestrationView: 'true' },
      querySelectorAll: () => columns.map((cards) => ({ querySelectorAll: () => cards })),
    };
    const keydown = registrations.find((item) => item.event === 'keydown')!;
    let prevented = 0;
    const press = (target: HTMLElement, key: string, init: Partial<KeyboardEvent> = {}) =>
      keydown.handle({ key, preventDefault: () => (prevented += 1), ...init } as unknown as Event, target);
    press(columns[0][0], 'ArrowDown');
    press(columns[0][1], 'ArrowRight');
    press(columns[1][0], 'ArrowRight');
    press(columns[1][0], 'ArrowLeft', { shiftKey: true });
    press(columns[1][0], 'Enter');
    expect(focused).toEqual(['a2', 'b1']);
    expect(selected).toEqual(['a2 moved', 'b1 moved', 'b1']);
    // The edge press is consumed (no page scroll) but moves nothing; the Shift press is not consumed.
    expect(prevented).toBe(4);
  });
});

describe('the Orchestration view fills its area (HS2-3JH0QV)', () => {
  it('pins the body to the remaining row whether or not a failure banner shows', async () => {
    const { readFileSync } = await import('node:fs');
    const css = readFileSync(new URL('./orchestration-timeline-view.css', import.meta.url), 'utf8');
    expect(css).toContain('grid-template-rows: auto minmax(0, 1fr);');
    expect(css).toMatch(/\.orchestration-timeline-view__body \{[^}]*grid-row: 2;/);
  });
});
