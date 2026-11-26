import { describe, expect, it } from 'vitest';

import {
  chosenSite,
  decisionText,
  explanationRows,
  integratorRows,
  lastPassText,
  latestDecisions,
  parseDecisionPage,
  parseExplanation,
  parseIntegratorStanding,
  parseRoleStandings,
  roleReasonPhrase,
  roleStandingRows,
  type RoleStandingView,
  roleStatusText,
  type SchedulerDecision,
  schedulerReasonText,
  schedulerReasonWithModel,
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

  it('words a role session’s start, keep and wind-down by its role and project, not as a step (HS2-2YCHRC)', () => {
    const say = (rest: Partial<SchedulerDecision>) => decisionText(decision(rest), timeline);
    const integrator = { target_id: 'api', reason: 'integrator_started', detail: 'handoff_pending' };
    expect(say({ ...integrator, tool: 'claude' })).toBe(
      'Started the integrator of API on Claude: a handoff waits to be integrated',
    );
    expect(say({ ...integrator, mode: 'dry_run' })).toBe(
      'Would start the integrator of API: a handoff waits to be integrated',
    );
    expect(say({ ...integrator, outcome: 'permission_required' })).toBe('The integrator of API waits for permission');
    expect(say({ target_id: 'web', reason: 'role_needed', detail: 'dispatcher', tool: 'claude' })).toBe(
      'Started the dispatcher of Web on Claude',
    );
    expect(say({ kind: 'keep', target_id: 'web', reason: 'role_needed', detail: 'dispatcher' })).toBe(
      'Kept the dispatcher of Web',
    );
    expect(say({ kind: 'keep', target_id: 'api', reason: 'integrator_kept', detail: 'ci_pending' })).toBe(
      'Kept the integrator of API: an integrated commit waits on its CI gate',
    );
    expect(say({ kind: 'wind_down', target_id: 'api', reason: 'integrator_wound_down', detail: 'idle' })).toBe(
      'Wound down the integrator of API: nothing has needed it for a while',
    );
    expect(say({ kind: 'wind_down', reason: 'integrator_wound_down', mode: 'dry_run' })).toBe(
      'Would wind down the integrator of a project: nothing has needed it for a while',
    );
    // A `role_needed` decision naming no known role reads as before.
    expect(say({ kind: 'keep', step_id: 'step-4', reason: 'role_needed' })).toBe(
      'Kept the session of WEB-33: role needed',
    );
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

  it('names the site the scheduler chose in Where It Runs, with the step’s own site after it (HS2-PR984R)', () => {
    const step = timeline.waves.flatMap((wave) => wave.items).find((item) => item.site_id === 'web')!;
    // Placed on its own site, or no explanation yet: the card's site, with no second row.
    expect(chosenSite(step, timeline, undefined)).toEqual({
      label: 'Web',
      location: 'local',
      machine_label: undefined,
    });
    expect(chosenSite(step, timeline, { standing: 'start', reason: 'ready_work', site: 'web' }).own).toBeUndefined();
    // Placed on another site of the project: that site and its machine lead.
    expect(chosenSite(step, timeline, { standing: 'start', reason: 'ready_work', site: 'api' })).toEqual({
      label: 'API',
      location: 'remote',
      machine_label: 'Build host',
      own: { label: 'Web', location: 'local' },
    });
    // A site the timeline does not list cannot be named, so the card's own site stays.
    expect(chosenSite(step, timeline, { standing: 'start', reason: 'ready_work', site: 'gone' }).label).toBe('Web');
  });
});

describe('why integration is or is not running (HS2-65AD6J)', () => {
  const standing = (rest: Partial<RoleStandingView>): RoleStandingView => ({
    target_id: 'api',
    role: 'integrator',
    standing: 'none',
    ...rest,
  });
  const why = (rest: Partial<RoleStandingView>) => roleReasonPhrase(standing(rest));

  it('names each standing, with a dry run saying it would act', () => {
    expect(roleStatusText(standing({ standing: 'start' }))).toBe('Starts on the next pass');
    expect(roleStatusText(standing({ standing: 'start', mode: 'dry_run' }))).toBe(
      'Would start on the next pass (dry run)',
    );
    expect(roleStatusText(standing({ standing: 'keep', mode: 'dry_run' }))).toBe('Running');
    expect(roleStatusText(standing({ standing: 'wind_down' }))).toBe('Winding down');
    expect(roleStatusText(standing({ standing: 'wind_down', mode: 'dry_run' }))).toBe('Would wind down (dry run)');
    expect(roleStatusText(standing({ standing: 'not_started', reason: 'integrator_not_started' }))).toBe('Not started');
    expect(roleStatusText(standing({ standing: 'not_started', reason: 'integrator_unavailable' }))).toBe('Unavailable');
    expect(roleStatusText(standing({ standing: 'not_started', reason: 'dispatcher_unavailable' }))).toBe('Unavailable');
    expect(roleStatusText(standing({ standing: 'none' }))).toBe('Not running');
    expect(roleStatusText(standing({ standing: 'brand_new_standing' }))).toBe('Brand new standing');
  });

  it('says why an integrator starts, is kept or winds down', () => {
    expect(why({ standing: 'none' })).toBe('no handoff waits to be integrated');
    expect(why({ standing: 'start', reason: 'integrator_started', detail: 'handoff_pending' })).toBe(
      'a handoff waits to be integrated',
    );
    expect(why({ standing: 'keep', reason: 'integrator_kept', detail: 'queue' })).toBe(
      'handoffs wait to be integrated',
    );
    expect(why({ standing: 'keep', reason: 'integrator_kept', detail: 'ci_pending' })).toBe(
      'an integrated commit waits on its CI gate',
    );
    expect(why({ standing: 'keep', reason: 'integrator_kept', detail: 'worker_run' })).toBe(
      'a worker it serves still has a run',
    );
    expect(why({ standing: 'keep', reason: 'integrator_kept', detail: 'new_need' })).toBe('its project still needs it');
    expect(why({ standing: 'wind_down', reason: 'integrator_wound_down', detail: 'idle' })).toBe(
      'nothing has needed it for a while',
    );
    expect(why({ standing: 'keep', reason: 'awaiting_safe_point' })).toBe('it stops at its next safe point');
    expect(why({ standing: 'keep', reason: 'idle_grace' })).toContain('kept a while');
    expect(why({ standing: 'wind_down', reason: 'role_prompt_failed', detail: 'integrator' })).toContain(
      'role prompt failed',
    );
    expect(why({ standing: 'wind_down', reason: 'never_joined', detail: 'integrator' })).toContain('in time');
    expect(why({ standing: 'keep', reason: 'never_joined', detail: 'replacements_exhausted' })).toContain(
      'left for a person',
    );
    expect(why({ standing: 'keep' })).toBeUndefined();
  });

  it('says why an integrator or dispatcher is not started, each detail in words', () => {
    const notStarted = (detail?: string) => why({ standing: 'not_started', reason: 'integrator_not_started', detail });
    expect(notStarted('target_cap')).toBe('the project’s session limit is reached');
    expect(notStarted('refusal_backoff')).toBe('its machine refused recently and is asked again later');
    expect(notStarted('cooldown')).toBe('its project is cooling down after a recent start');
    expect(notStarted('target_unavailable')).toBe('its project could not be reached');
    expect(notStarted('no_tool_available')).toBe('no AI tool account can take it now');
    expect(notStarted('machine_refused')).toBe('its machine has no room');
    expect(notStarted('target_disabled')).toBe('its project is turned off in the policy');
    expect(notStarted('policy_opt_out')).toContain('integrates by hand');
    expect(notStarted(undefined)).toBe('this pass could not start one');
    expect(notStarted('brand_new_detail')).toBe('brand new detail');
    expect(why({ standing: 'not_started', reason: 'integrator_unavailable', detail: 'replacements_exhausted' })).toBe(
      'the integrators the scheduler started did not take up their role, so none starts for a while. ' +
        'Open the AI tool in a terminal of this project to check that it starts and is signed in, or integrate ' +
        'handoffs by hand until then',
    );
    expect(
      why({
        role: 'dispatcher',
        standing: 'not_started',
        reason: 'dispatcher_unavailable',
        detail: 'replacements_exhausted',
      }),
    ).toBe(
      'the dispatchers the scheduler started did not take up their role, so none starts for a while. ' +
        'Open the AI tool in a terminal of this project to check that it starts and is signed in, or assign ready ' +
        'work by hand until then',
    );
    // HS2-91AF8K: each unavailable role says what a person can do meanwhile.
    for (const reason of ['integrator_unavailable', 'dispatcher_unavailable'])
      expect(why({ standing: 'not_started', reason })).toMatch(/Open the AI tool .+ by hand until then$/);
    expect(why({ standing: 'not_started', reason: 'brand_new_reason' })).toBe('brand new reason');
    // No code ever reaches a person as a code.
    for (const detail of ['target_cap', 'cooldown', 'policy_opt_out', 'target_disabled', 'machine_refused'])
      expect(notStarted(detail)).not.toContain('_');
  });

  it('lists a running card’s Integration rows, naming the home site only when it is another site', () => {
    expect(
      integratorRows(
        standing({ standing: 'keep', reason: 'integrator_kept', detail: 'queue', session_id: 'sched-role-1' }),
        clock,
        timeline,
        'api',
      ),
    ).toEqual([
      { label: 'Integrator', value: 'Running' },
      { label: 'Why', value: 'Handoffs wait to be integrated' },
      { label: 'Session', value: 'sched-role-1' },
    ]);
    expect(
      integratorRows(
        standing({ standing: 'not_started', reason: 'integrator_not_started', detail: 'cooldown', retry_at_ms: 7 }),
        clock,
        timeline,
        'web',
      ),
    ).toEqual([
      { label: 'Integrator', value: 'Not started' },
      { label: 'Why', value: 'Its project is cooling down after a recent start' },
      { label: 'Home Site', value: 'API' },
      { label: 'Next', value: 'Looked at again by t7' },
    ]);
    expect(integratorRows(standing({}), clock)).toEqual([
      { label: 'Integrator', value: 'Not running' },
      { label: 'Why', value: 'No handoff waits to be integrated' },
    ]);
  });

  it('lists the overview’s Coordination Sessions rows per project role session', () => {
    expect(
      roleStandingRows(
        [
          standing({ standing: 'start', reason: 'integrator_started', detail: 'handoff_pending', mode: 'dry_run' }),
          standing({
            target_id: 'web',
            role: 'dispatcher',
            standing: 'not_started',
            reason: 'dispatcher_unavailable',
            detail: 'replacements_exhausted',
            retry_at_ms: 3,
          }),
          standing({ target_id: 'gone-site-123456', standing: 'keep' }),
        ],
        clock,
        timeline,
      ),
    ).toEqual([
      { label: 'API Integrator', value: 'Would start on the next pass (dry run): a handoff waits to be integrated' },
      {
        label: 'Web Dispatcher',
        value:
          'Unavailable: the dispatchers the scheduler started did not take up their role, so none starts for a while. ' +
          'Open the AI tool in a terminal of this project to check that it starts and is signed in, or assign ready ' +
          'work by hand until then; looked at again by t3',
      },
      { label: 'site 123456 Integrator', value: 'Running' },
    ]);
  });

  it('reads `scheduler explain`’s integrator, leaving out what is not that shape', () => {
    expect(
      parseIntegratorStanding({
        standing: 'running',
        reason: 'in_progress',
        mode: 'applied',
        integrator: {
          target_id: 'api',
          standing: 'keep',
          reason: 'integrator_kept',
          detail: 'ci_pending',
          session_id: 'sched-role-integrator-api',
          retry_at_ms: 9,
        },
      }),
    ).toEqual({
      target_id: 'api',
      role: 'integrator',
      standing: 'keep',
      reason: 'integrator_kept',
      detail: 'ci_pending',
      session_id: 'sched-role-integrator-api',
      retry_at_ms: 9,
      mode: 'applied',
    });
    expect(
      parseIntegratorStanding({ standing: 'running', integrator: { target_id: 'api', standing: 'none' } }),
    ).toEqual({
      target_id: 'api',
      role: 'integrator',
      standing: 'none',
      reason: undefined,
      detail: undefined,
      session_id: undefined,
      retry_at_ms: undefined,
      mode: undefined,
    });
    expect(parseIntegratorStanding(undefined)).toBeUndefined();
    expect(parseIntegratorStanding({ standing: 'running' })).toBeUndefined();
    expect(parseIntegratorStanding({ integrator: { standing: 'keep' } })).toBeUndefined();
    expect(
      parseIntegratorStanding({ integrator: { target_id: 'api', standing: 'keep', retry_at_ms: -1 } })?.retry_at_ms,
    ).toBeUndefined();
  });

  it('reads `scheduler plan`’s integrator placements and roles not started, leaving out malformed entries', () => {
    expect(
      parseRoleStandings({
        mode: 'dry_run',
        placements: [
          {
            action: 'start',
            target_id: 'api',
            role: 'integrator',
            reason: 'integrator_started',
            detail: 'handoff_pending',
          },
          { action: 'keep', target_id: 'web', role: 'dispatcher', reason: 'role_needed' },
          { action: 'start', target_id: 'docs', reason: 'ready_work' },
          { target_id: 'docs', role: 'integrator' },
        ],
        roles_unplaced: [
          { target_id: 'web', role: 'integrator', reason: 'integrator_not_started', detail: 'policy_opt_out' },
          { target_id: 'docs', role: 'dispatcher', reason: 'dispatcher_unavailable', retry_at_ms: 5 },
          { target_id: 'docs', reason: 'integrator_unavailable' },
          'not an entry',
        ],
      }),
    ).toEqual([
      {
        target_id: 'api',
        role: 'integrator',
        standing: 'start',
        reason: 'integrator_started',
        detail: 'handoff_pending',
        session_id: undefined,
        retry_at_ms: undefined,
        mode: 'dry_run',
      },
      {
        target_id: 'web',
        role: 'integrator',
        standing: 'not_started',
        reason: 'integrator_not_started',
        detail: 'policy_opt_out',
        retry_at_ms: undefined,
        mode: 'dry_run',
      },
      {
        target_id: 'docs',
        role: 'dispatcher',
        standing: 'not_started',
        reason: 'dispatcher_unavailable',
        detail: undefined,
        retry_at_ms: 5,
        mode: 'dry_run',
      },
    ]);
    expect(parseRoleStandings(undefined)).toEqual([]);
    expect(parseRoleStandings({ placements: 'x', roles_unplaced: {} })).toEqual([]);
  });

  it('words a `role_unplaced` decision of the log', () => {
    const role = (rest: Partial<SchedulerDecision>) =>
      decisionText(decision({ kind: 'role_unplaced', ...rest }), timeline);
    expect(role({ target_id: 'api', reason: 'integrator_not_started', detail: 'target_cap' })).toBe(
      'Did not start the integrator of API: the project’s session limit is reached',
    );
    expect(role({ target_id: 'web', reason: 'dispatcher_unavailable', detail: 'replacements_exhausted' })).toBe(
      'Did not start the dispatcher of Web: the dispatchers the scheduler started did not take up their role, so ' +
        'none starts for a while. Open the AI tool in a terminal of this project to check that it starts and is ' +
        'signed in, or assign ready work by hand until then',
    );
    expect(role({})).toBe('Did not start the integrator of a project');
    expect(
      parseDecisionPage({
        entries: [
          { seq: 2, record: { at_ms: 1, kind: 'role_unplaced', target_id: 'api', reason: 'integrator_unavailable' } },
        ],
      }).entries[0].target_id,
    ).toBe('api');
  });
});

describe('a pattern-only Models list that resolves to no model, in words (HS2-747WQA)', () => {
  const unresolved = 'its machine has not reported the AI tool’s models, so no allowed model can be picked';
  const noMatch = 'no model its machine reports is an allowed model';

  it('words both codes in place of no_tool_available’s phrase, and leaves other details alone', () => {
    expect(schedulerReasonWithModel('no_tool_available', 'model_unresolved')).toBe(unresolved);
    expect(schedulerReasonWithModel('no_tool_available', 'no_matching_model')).toBe(noMatch);
    expect(schedulerReasonWithModel('no_tool_available', undefined)).toBe('no AI tool account can take it now');
    expect(schedulerReasonWithModel('requirement_unmet', 'os:windows')).toBe(schedulerReasonText('requirement_unmet'));
  });

  it('says why a passed-over site started nothing', () => {
    expect(sitePassDetail({ target_id: 'x', reason: 'no_tool_available', detail: 'model_unresolved' })).toBe(
      unresolved,
    );
    expect(sitePassText({ target_id: 'x', reason: 'no_tool_available', detail: 'no_matching_model' })).toBe(noMatch);
  });

  it('says it in the Why It Is Not Running Scheduler row and keeps the other rows', () => {
    const rows = explanationRows(
      { standing: 'not_placed', reason: 'no_tool_available', detail: 'model_unresolved', retry_at_ms: 9 },
      clock,
      5,
    );
    expect(rows).toEqual([
      { label: 'Scheduler', value: `Waits: ${unresolved} (last pass t5)` },
      { label: 'Next', value: 'Looked at again by t9' },
    ]);
    const sites = explanationRows(
      {
        standing: 'not_placed',
        reason: 'no_tool_available',
        detail: 'no_matching_model',
        site: 'web',
        sites: [{ target_id: 'api', reason: 'no_tool_available', detail: 'model_unresolved' }],
      },
      clock,
      undefined,
      timeline,
    );
    expect(sites[0]).toEqual({ label: 'Scheduler', value: `Waits: ${noMatch}` });
    expect(sites).toContainEqual({
      label: 'Not on API',
      value: 'Its machine has not reported the AI tool’s models, so no allowed model can be picked',
    });
    for (const row of [...rows, ...sites]) expect(row.value).not.toContain('_');
  });

  it('says it for a deferred decision and a role session that is not started', () => {
    expect(
      decisionText(
        decision({ kind: 'unplaced', step_id: 'step-8', reason: 'no_tool_available', detail: 'no_matching_model' }),
        timeline,
      ),
    ).toBe(`Deferred DOC-9: ${noMatch}`);
    for (const [detail, text] of [
      ['model_unresolved', unresolved],
      ['no_matching_model', noMatch],
    ] as const)
      expect(roleReasonPhrase({ standing: 'not_started', reason: 'integrator_not_started', detail })).toBe(text);
    const [row] = roleStandingRows(
      [
        {
          target_id: 'web',
          role: 'integrator',
          standing: 'not_started',
          reason: 'integrator_not_started',
          detail: 'model_unresolved',
        },
      ],
      clock,
      timeline,
    );
    expect(row).toEqual({
      label: 'Web Integrator',
      value: `Not started: ${unresolved}`,
    });
  });
});

describe('HS2-2Y535C the explain answer’s own codes and a policy block, in words', () => {
  it('words a step with no saved policy and one the pass did not rank, with no code left', () => {
    const absent = explanationRows({ standing: 'not_placed', reason: 'policy_absent' }, clock);
    expect(absent[0].value).toBe('Waits: no policy is saved yet; apply one in Settings › Orchestration');
    const unranked = explanationRows({ standing: 'not_placed', reason: 'not_considered' }, clock);
    expect(unranked[0].value).toBe('Waits: the scheduler has not looked at it yet');
    for (const rows of [absent, unranked]) for (const row of rows) expect(row.value).not.toMatch(/_|policy absent/);
  });

  it('names the limit behind a policy block, and keeps the bare phrase for a limit it cannot word', () => {
    expect(schedulerReasonWithModel('policy_denied', 'policy_machine_cap')).toBe(
      'the policy does not allow it (the machine’s session limit is reached)',
    );
    expect(schedulerReasonWithModel('policy_denied', 'policy_future_rule')).toBe('the policy does not allow it');
    expect(schedulerReasonWithModel('policy_denied', undefined)).toBe('the policy does not allow it');
    // Another reason's detail is not a policy limit.
    expect(schedulerReasonWithModel('target_cap', 'policy_machine_cap')).toBe('the project’s session limit is reached');
  });
});
