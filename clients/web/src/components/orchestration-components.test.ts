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
    expect(inspector('step-5')).toContain('Building Ahead');
    // A finished or unstarted item has no run controls.
    expect(inspector('step-8')).not.toContain('data-control=');
    expect(inspector('step-1')).not.toContain('data-control=');
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
    ]);
  });
});
