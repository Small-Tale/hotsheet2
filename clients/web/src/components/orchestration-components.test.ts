import { afterEach, describe, expect, it, vi } from 'vitest';

import { findTimelineItem } from '../orchestration-timeline-board';
import { demoClock, demoDecisions, demoSummary, demoTimeline } from '../ux-demo/orchestration-fixtures';
import { OrchestrationDecisionsDialog } from './orchestration-decisions-dialog';
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

  it('HS2-F1BKQ0 replaces the failure with what is wrong with the plan, and offers the person’s fix', () => {
    const conflict = String(
      OrchestrationTimelineView({
        error: 'plan_conflict',
        planHealth: { state: 'conflict', conflicts: ['S1'], local: ['S1'], discard_resolves: true },
        stepName: () => 'API-12',
      }),
    );
    expect(conflict).toContain('data-orchestration-plan-problem="conflict"');
    expect(conflict).toContain('This Machine’s Plan Differs From the Checked-In Plan');
    expect(conflict).toContain('data-action="keep-checked-in-orchestration-plan"');
    expect(conflict).not.toContain('The Orchestration Timeline Could Not Be Read');
    expect(conflict).not.toContain('No Orchestration Timeline');
    const damaged = String(
      OrchestrationTimelineView({
        error: 'corrupt',
        planHealth: {
          state: 'damaged',
          file: 'checked_in',
          reason: 'corrupt',
          conflicts: [],
          local: [],
          discard_resolves: false,
        },
      }),
    );
    expect(damaged).toContain('The Checked-In Plan Cannot Be Read');
    expect(damaged).not.toContain('keep-checked-in-orchestration-plan');
    // A healthy plan leaves the ordinary failure.
    const other = String(
      OrchestrationTimelineView({
        error: 'Offline.',
        planHealth: { state: 'ok', conflicts: [], local: [], discard_resolves: false },
      }),
    );
    expect(other).toContain('The Orchestration Timeline Could Not Be Read');
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
    // HS2-GSRFG0: explicit goals are planned from; with none open, goal tickets are optional.
    expect(ready).toContain('Propose a plan from the open goal tickets in this project.');
    expect(ready).toContain('title="Propose a plan from the goal tickets"');
    const allOpen = String(
      OrchestrationTimelineView({ timeline: demoTimeline('no_plan'), propose: { available: true }, defaultGoal: true }),
    );
    expect(allOpen).toContain('Propose a plan to work through all open tickets of the chosen projects');
    expect(allOpen).toContain('title="Propose a plan that works through all open tickets"');
    expect(allOpen).not.toContain('Write goal tickets');
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
    // HS2-5RYXNN: a planner stopped on a first-run question says where a person answers it.
    expect(toolbar('no_plan', { planning: 'waiting' })).toContain('No Plan Yet · Planner waiting in its terminal');
    expect(toolbar('no_plan', { planning: 'ended' })).not.toContain(' · ');
    // HS2-FDS95D: a planner that finished its turn still reads as planning.
    expect(toolbar('no_plan', { planning: 'replannable' })).toContain('No Plan Yet · Planning');
    expect(toolbar('no_projects')).not.toContain('data-action=');
    // HS2-2Z4M6M: a press on its way, a starting or working planner take Propose Plan's place, busy.
    for (const [extra, label] of [
      [{ busy: true, propose: { available: true } }, 'Starting Planner'],
      [{ planning: 'planning', propose: { available: false, reason: 'A planning session is working' } }, 'Planning'],
      [{ planning: 'waiting' }, 'Planner Waiting'],
    ] as const) {
      const markup = toolbar('no_plan', extra);
      expect(markup).not.toContain('data-action="propose-orchestration-plan"');
      expect(markup).toContain(`data-planner-state="${label}"`);
      expect(markup).toContain('disabled');
    }
    expect(
      toolbar('no_plan', { planning: 'planning', propose: { available: false, reason: 'Working now' } }),
    ).toContain('title="Working now"');
    // A planner that finished its turn keeps the button, so a press re-plans (HS2-FDS95D).
    expect(toolbar('no_plan', { planning: 'replannable', propose: { available: true } })).toContain(
      'data-action="propose-orchestration-plan"',
    );
    const working = String(
      OrchestrationTimelineView({
        timeline: demoTimeline('no_plan'),
        planning: 'planning',
        propose: { available: false },
      }),
    );
    expect(working).toContain('data-planner-state="Planning"');
    expect(working).not.toContain('data-action="propose-orchestration-plan"');
    // HS2-D9KP86: no saved policy does not block Start; the banner and title say Start saves the defaults,
    // and the banner leads to them.
    const defaults = String(
      OrchestrationToolbar({
        timeline: {
          ...demoTimeline('proposed'),
          kick_off: { can_start: true, can_pause: false, default_policy: true },
        },
      }),
    );
    expect(defaults).toContain(
      'Start Orchestration saves the default policy shown in Orchestration settings and starts sessions within it.',
    );
    expect(defaults).toContain('title="Save the default policy and place sessions automatically within it"');
    expect(defaults).not.toMatch(
      /data-action="start-orchestration"[^>]*disabled|disabled[^>]*data-action="start-orchestration"/,
    );
    expect(defaults).toContain('data-action="open-orchestration-settings"');
    expect(defaults).toContain('Open Orchestration Settings');
    // With a saved policy it starts within it, and no settings action.
    const saved = String(OrchestrationToolbar({ timeline: demoTimeline('proposed') }));
    expect(saved).toContain('Start Orchestration starts sessions within the Orchestration settings.');
    expect(saved).not.toContain('open-orchestration-settings');
  });

  it('HS2-1DC8X1 says why a paused plan cannot start, with the way to the setting', () => {
    const paused = (start_blocked?: 'project_software') =>
      String(
        OrchestrationToolbar({
          timeline: {
            ...demoTimeline('paused'),
            kick_off: { can_start: start_blocked === undefined, can_pause: false, start_blocked },
          },
        }),
      );
    // A reason changed elsewhere names it, with no Settings action.
    const software = paused('project_software');
    expect(software).toContain('Start Orchestration Is Unavailable');
    expect(software).not.toContain('open-orchestration-settings');
    // Nothing blocks: no banner, and a proposed plan with a policy has no settings action.
    expect(paused()).not.toContain('Start Orchestration Is Unavailable');
    expect(String(OrchestrationToolbar({ timeline: demoTimeline('proposed') }))).not.toContain(
      'open-orchestration-settings',
    );
  });
});

describe('Update Plan in the Orchestration toolbar (HS2-190RDX)', () => {
  const toolbar = (phase: Parameters<typeof demoTimeline>[0], extra = {}) =>
    String(OrchestrationToolbar({ timeline: demoTimeline(phase), clock: demoClock, ...extra }));
  const enabled = (markup: string, action: string) =>
    new RegExp(`<[^>]*data-action="${action}"[^>]*>`).exec(markup)?.[0].includes('disabled') === false;

  it('offers Update Plan beside the kick-off action once there is a plan, with no pause needed', () => {
    for (const phase of ['proposed', 'running', 'paused'] as const) {
      const markup = toolbar(phase, { update: { available: true } });
      expect(markup).toContain('Update Plan');
      expect(enabled(markup, 'update-orchestration-plan')).toBe(true);
      // The kick-off action stays: Pause is still offered while running, never required.
      expect(markup).toContain(phase === 'running' ? 'pause-orchestration' : 'start-orchestration');
    }
    for (const phase of ['no_plan', 'no_projects'] as const) {
      expect(toolbar(phase, { update: { available: true } })).not.toContain('update-orchestration-plan');
    }
  });

  it('is disabled with its reason, or while a change is on its way', () => {
    const reason = 'Choose an AI tool for this project in Settings first';
    const unavailable = toolbar('running', { update: { available: false, reason } });
    expect(enabled(unavailable, 'update-orchestration-plan')).toBe(false);
    expect(unavailable).toContain(reason);
    expect(enabled(toolbar('running', { update: { available: true }, busy: true }), 'update-orchestration-plan')).toBe(
      false,
    );
    // Without an availability it is disabled, never silently enabled.
    expect(enabled(toolbar('running'), 'update-orchestration-plan')).toBe(false);
  });

  it('shows the update in progress and offers to cancel it while the plan keeps running', () => {
    const markup = toolbar('running', { update: { available: true }, planning: 'updating' });
    expect(markup).toContain('· Updating Plan');
    expect(markup).not.toContain('update-orchestration-plan"');
    expect(enabled(markup, 'cancel-orchestration-plan-update')).toBe(true);
    expect(markup).toContain('pause-orchestration');
    expect(enabled(toolbar('running', { planning: 'updating', busy: true }), 'cancel-orchestration-plan-update')).toBe(
      false,
    );
  });
});

describe('A planner question in the Orchestration toolbar (HS2-N5BDE4)', () => {
  const toolbar = (phase: Parameters<typeof demoTimeline>[0], extra = {}) =>
    String(OrchestrationToolbar({ timeline: demoTimeline(phase), clock: demoClock, ...extra }));

  it('says the planner asked and links to the goal ticket that holds the question', () => {
    for (const phase of ['no_plan', 'proposed', 'running'] as const) {
      const markup = toolbar(phase, { planning: 'asked', question: 'LCH-1', update: { available: true } });
      expect(markup).toContain('· Planner asked a question');
      expect(markup).toContain('data-action="answer-orchestration-question"');
      expect(markup).toContain('Open LCH-1, where the planning session asked you a question');
    }
    // Without a question, or once answered, there is nothing to open.
    expect(toolbar('running', { planning: 'asked' })).not.toContain('answer-orchestration-question');
    expect(toolbar('running', { planning: 'planning', question: 'LCH-1' })).not.toContain(
      'answer-orchestration-question',
    );
  });
});

describe('The default goal in the Orchestration toolbar (HS2-YSRT6Y)', () => {
  const toolbar = (phase: Parameters<typeof demoTimeline>[0], extra = {}) =>
    String(OrchestrationToolbar({ timeline: demoTimeline(phase), clock: demoClock, ...extra }));

  it('says planning works through all open tickets while no goal ticket is open', () => {
    expect(toolbar('no_plan', { defaultGoal: true, propose: { available: true } })).toContain(
      'No Plan Yet · Working through all open tickets',
    );
    expect(toolbar('no_plan', { defaultGoal: true, propose: { available: true } })).toContain(
      'Propose a plan that works through all open tickets',
    );
    expect(toolbar('running', { defaultGoal: true, update: { available: true } })).toContain(
      'Plan again from all open tickets while the current plan keeps running',
    );
    // With the planner working, both are said.
    expect(toolbar('no_plan', { defaultGoal: true, planning: 'planning' })).toContain(
      '· Working through all open tickets · Planning',
    );
    // An explicit goal: nothing is added.
    expect(toolbar('running')).not.toContain('Working through all open tickets');
    expect(toolbar('no_plan', { propose: { available: true } })).toContain('Propose a plan from the goal tickets');
  });
});

describe('Automatic placement by default in the Orchestration toolbar (HS2-R2N33M)', () => {
  it('says sessions start automatically and offers Pause while placement runs by default', () => {
    const byDefault = String(
      OrchestrationToolbar({ timeline: { ...demoTimeline('running'), automatic_by_default: true }, clock: demoClock }),
    );
    expect(byDefault).toContain('Sessions Start Automatically');
    expect(byDefault).toContain('A plan exists, so Hot Sheet starts sessions for ready work');
    expect(byDefault).toMatch(/data-action="pause-orchestration"/);
    expect(byDefault).not.toMatch(/<[^>]*data-action="pause-orchestration"[^>]*disabled/);
    // Started by a person, paused, or only proposed: no banner.
    expect(String(OrchestrationToolbar({ timeline: demoTimeline('running'), clock: demoClock }))).not.toContain(
      'Sessions Start Automatically',
    );
    for (const phase of ['paused', 'proposed', 'no_plan'] as const) {
      expect(
        String(
          OrchestrationToolbar({ timeline: { ...demoTimeline(phase), automatic_by_default: true }, clock: demoClock }),
        ),
      ).not.toContain('Sessions Start Automatically');
    }
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
    // HS2-FXY5QM: the run's state in words, and what a person can do when no worker holds it.
    expect(paused).toContain('Dispatched to a worker, paused');
    const gone = String(
      OrchestrationInspector({
        item: { ...item('step-3'), run: { ...item('step-3').run!, state: 'intervened' } },
        timeline: demoTimeline('running'),
      }),
    );
    expect(gone).toContain('No worker holds it');
    expect(gone).toContain('Hand it off, record what happened with Correct…, or cancel the run.');
    expect(gone).not.toContain('Intervened');
  });

  it('HS2-PR984R Where It Runs names the site the scheduler chose and its machine', () => {
    const own = item('step-3');
    const other = demoTimeline('running').targets.find((target) => target.target_id !== (own.site_id ?? ''))!;
    const markup = inspector('step-3', {
      details: { loading: false, explanation: { standing: 'start', reason: 'ready_work', site: other.target_id } },
    });
    const where = markup.slice(
      markup.indexOf('data-orchestration-section="Where It Runs"'),
      markup.indexOf('data-orchestration-section=', markup.indexOf('data-orchestration-section="Where It Runs"') + 1),
    );
    expect(where).toContain(`${other.label} (${other.location})`);
    expect(where).toContain("Step's site");
    expect(where).toContain(`${own.project_label} (${own.location})`);
    expect(inspector('step-3')).not.toContain("Step's site");
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

  it('HS2-77SRHC explains a ready card and shows the overview with the latest decisions', () => {
    const ready = inspector('step-8', {
      details: { loading: false, explanation: { standing: 'not_placed', reason: 'target_cap', retry_at_ms: 0 } },
    });
    expect(ready).toContain('Waits: the project’s session limit is reached (last pass 14:00)');
    expect(ready).toContain('Looked at again by');
    // Until the explanation is read, the last logged reason stands in.
    expect(inspector('step-8')).toContain('target cap');
    const overview = String(
      OrchestrationInspector({
        timeline: demoTimeline('running'),
        projectName: 'Launch',
        clock: demoClock,
        overview: { decisions: demoDecisions() },
      }),
    );
    expect(overview).toContain('Launch Orchestration');
    for (const section of ['Scheduler', 'Latest Decisions'])
      expect(overview).toContain(`data-orchestration-section="${section}"`);
    expect(overview).toContain('Automatic');
    expect(overview).toContain('13:58 · started 1, deferred 1');
    expect(overview).toContain('Started API-12 on Claude');
    expect(overview).toContain('API-15 waits for permission');
    expect(overview).not.toContain('Kept the session');
    expect(overview).toContain('data-action="show-orchestration-decisions"');
    const unread = String(OrchestrationInspector({ timeline: demoTimeline('paused') }));
    expect(unread).toContain('Paused; running work continues');
    expect(unread).toContain('Reading the decision log…');
    expect(unread).toMatch(
      /data-action="show-orchestration-decisions"[^>]*disabled|disabled[^>]*data-action="show-orchestration-decisions"/,
    );
    const empty = String(
      OrchestrationInspector({
        timeline: demoTimeline('proposed'),
        overview: { decisions: { entries: [], cursor: 0, more: false } },
      }),
    );
    expect(empty).toContain('The scheduler has decided nothing yet.');
    expect(empty).toContain('Not started; the plan is a dry run');
  });

  it('HS2-5GRXFN shows the resource a deferred card needs and the overview’s contention', () => {
    const now = demoTimeline('running').now_ms;
    const why = inspector('step-8', {
      details: {
        loading: false,
        explanation: {
          standing: 'not_placed',
          reason: 'resource_held',
          retry_at_ms: now,
          resources: [{ namespace: 'vm', resource: 'macos-vm', state: 'held', expires_at_ms: now }],
        },
      },
    });
    expect(why).toContain('Waits: a resource it needs is held');
    expect(why).toContain('held until about 14:00 unless renewed');
    const overview = (contention?: object[]) =>
      String(
        OrchestrationInspector({
          timeline: demoTimeline('running'),
          clock: demoClock,
          overview: { contention } as never,
        }),
      );
    expect(
      overview([{ namespace: 'vm', resource: 'macos-vm', state: 'held', expires_at_ms: now, waiting: ['step-8'] }]),
    ).toContain('Held until about 14:00 unless renewed · DOC-9 waits');
    expect(overview([])).toContain('No work waits on a resource.');
    expect(overview()).toContain('Reading who holds the resources work needs…');
  });

  it('HS2-XJEH08 shows progress and cost in the overview, never an unknown as zero', () => {
    const markup = String(
      OrchestrationInspector({
        timeline: demoTimeline('running'),
        clock: demoClock,
        overview: { summary: demoSummary() },
      }),
    );
    const order = [...markup.matchAll(/data-orchestration-section="([^"]+)"/g)].map((match) => match[1]);
    expect(order).toEqual([
      'Progress',
      'Projects',
      'Scheduler',
      'Coordination Sessions',
      'Resources',
      'Latest Decisions',
    ]);
    expect(markup).toContain('2 of 10');
    expect(markup).toContain('5 (2 for you)');
    expect(markup).toContain('$12.40 so far · partial: 1 work item has no cost (no usage recorded)');
    expect(markup).toContain('cost unknown (no usage recorded)');
    expect(markup).not.toContain('$0.00');
  });

  it('HS2-65AD6J says whether a running card’s integrator runs and lists each project’s coordination sessions', () => {
    const now = demoTimeline('running').now_ms;
    // A card with a run: Integration after Run, with the home site when the card's site is another one.
    const running = inspector('step-3', {
      details: {
        loading: false,
        integrator: {
          target_id: 'web',
          role: 'integrator',
          standing: 'not_started',
          reason: 'integrator_not_started',
          detail: 'policy_opt_out',
        },
      },
    });
    const order = [...running.matchAll(/data-orchestration-section="([^"]+)"/g)].map((match) => match[1]);
    expect(order.indexOf('Integration')).toBe(order.indexOf('Run') + 1);
    expect(running).toContain('Not started');
    expect(running).toContain('integrates by hand');
    expect(running).toContain('Home Site');
    // Not read, or a card without a run: no Integration section.
    expect(inspector('step-3', { details: { loading: false } })).not.toContain('"Integration"');
    expect(
      inspector('step-8', {
        details: { loading: false, integrator: { target_id: 'web', role: 'integrator', standing: 'none' } },
      }),
    ).not.toContain('"Integration"');
    const overview = (roles?: object[]) =>
      String(
        OrchestrationInspector({ timeline: demoTimeline('running'), clock: demoClock, overview: { roles } as never }),
      );
    const listed = overview([
      { target_id: 'api', role: 'integrator', standing: 'keep', reason: 'integrator_kept', detail: 'ci_pending' },
      {
        target_id: 'web',
        role: 'dispatcher',
        standing: 'not_started',
        reason: 'dispatcher_unavailable',
        detail: 'replacements_exhausted',
        retry_at_ms: now,
      },
    ]);
    expect(listed).toContain('API Integrator');
    expect(listed).toContain('Running: an integrated commit waits on its CI gate');
    expect(listed).toContain('Web Dispatcher');
    expect(listed).toContain('Unavailable: the dispatchers the scheduler started did not take up their role');
    expect(listed).toContain('looked at again by 14:00');
    expect(overview([])).toContain('No integrator runs now, and no coordination session waits to start.');
    expect(overview()).toContain('Reading whether each project’s integrator runs…');
  });

  it('HS2-77SRHC lists the whole log newest first with Load Earlier while older decisions remain', () => {
    expect(String(OrchestrationDecisionsDialog({}))).toBe('');
    const open = String(
      OrchestrationDecisionsDialog({
        state: { entries: demoDecisions().entries, more: true, loading: false },
        timeline: demoTimeline('running'),
        clock: demoClock,
      }),
    );
    expect(open).toContain('Scheduler Decisions');
    expect(open).toContain('Deferred DOC-9: a resource it needs is held (vm/macos-vm)');
    expect(open.indexOf('Started API-12')).toBeLessThan(open.indexOf('Kept the session'));
    expect(open).toContain('data-action="load-earlier-orchestration-decisions"');
    const all = String(
      OrchestrationDecisionsDialog({ state: { entries: [], more: false, loading: false, error: 'Offline.' } }),
    );
    expect(all).not.toContain('load-earlier');
    expect(all).toContain('The scheduler has decided nothing yet.');
    expect(all).toContain('Offline.');
    expect(String(OrchestrationDecisionsDialog({ state: { entries: [], more: true, loading: true } }))).toContain(
      'Reading the decision log…',
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
      updatePlan: () => calls.push('update plan'),
      cancelPlanUpdate: () => calls.push('cancel update'),
      answerQuestion: () => calls.push('answer'),
      chooseProjects: () => calls.push('choose'),
      runControl: (control: string, state?: string) => calls.push(`run ${control} ${state ?? ''}`),
      showRequest: (id: string) => calls.push(`request ${id}`),
      openGoal: () => calls.push('goal'),
      runValidation: () => calls.push('validate'),
      reopen: (reason: string) => calls.push(`reopen ${reason}`),
      showDecisions: () => calls.push('decisions'),
      loadEarlierDecisions: () => calls.push('earlier'),
      closeDecisions: () => calls.push('close'),
      keepCheckedInPlan: () => calls.push('keep plan'),
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
    run('click', 'update-orchestration-plan', target({}));
    run('click', 'cancel-orchestration-plan-update', target({}));
    run('click', 'answer-orchestration-question', target({}));
    run('click', 'choose-orchestration-projects', target({}));
    run('click', 'orchestration-run-control', target({ control: 'correct', runState: 'failed' }));
    run('click', 'show-orchestration-request', target({ requestId: 'request-6' }));
    run('click', 'open-orchestration-goal', target({}));
    run('click', 'run-orchestration-validation', target({}));
    run('click', 'reopen-orchestration-work', target({ reopenReason: 'fix_changed_contract' }));
    run('click', 'reopen-orchestration-work', target({}));
    run('click', 'show-orchestration-decisions', target({}));
    run('click', 'load-earlier-orchestration-decisions', target({}));
    run('click', 'keep-checked-in-orchestration-plan', target({}));
    const dialog = target({});
    registrations
      .find((item) => item.event === 'kui-request-close')!
      .handle({ target: dialog } as unknown as Event, dialog);
    registrations
      .find((item) => item.event === 'kui-request-close')!
      .handle({ target: target({}) } as unknown as Event, dialog);
    expect(calls).toEqual([
      'select step-3',
      'select step-5',
      'group api',
      'start',
      'pause',
      'propose',
      'update plan',
      'cancel update',
      'answer',
      'choose',
      'run correct failed',
      'request request-6',
      'goal',
      'validate',
      'reopen fix_changed_contract',
      'decisions',
      'earlier',
      'keep plan',
      'close',
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
      updatePlan: noop,
      cancelPlanUpdate: noop,
      answerQuestion: noop,
      chooseProjects: noop,
      runControl: noop,
      showRequest: noop,
      openGoal: noop,
      runValidation: noop,
      reopen: noop,
      showDecisions: noop,
      loadEarlierDecisions: noop,
      closeDecisions: noop,
      keepCheckedInPlan: noop,
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
