import { describe, expect, it, vi } from 'vitest';

import { ApiHttpError, type OrchestrationSession } from '../api';
import type { ConfirmationOptions } from '../confirmation';
import { remoteProjectId } from '../remote-projects/identity';
import { remoteTerminalList } from '../remote-projects/terminals';
import { terminalProjectOwner } from '../terminal-project-scope';
import {
  createDemoOrchestrationApi,
  DEMO_OPEN_PROJECTS,
  DEMO_OWNER,
  DEMO_REMOTE,
  DEMO_TARGETS,
} from '../ux-demo/orchestration-settings-fixtures';
import { createOrchestrationSettings, type OrchestrationApi } from './orchestration-settings';

function setup({ empty = false, answers = [] as boolean[] } = {}) {
  const fake = createDemoOrchestrationApi({ empty });
  const api: OrchestrationApi = Object.fromEntries(
    Object.entries(fake.api).map(([name, call]) => [name, vi.fn(call as (...args: unknown[]) => unknown)]),
  ) as unknown as OrchestrationApi;
  const asked: ConfirmationOptions[] = [];
  let open: { id: string; name: string }[] = [...DEMO_OPEN_PROJECTS];
  let busy = false;
  const deferred: (() => void)[] = [];
  const activated: string[] = [];
  const shown: string[] = [];
  const settings = createOrchestrationSettings({
    api: () => api,
    openProjects: () => open,
    confirm: (options) => {
      asked.push(options);
      return Promise.resolve(answers.shift() ?? true);
    },
    openProjectPicker: vi.fn(),
    openFlowBusy: () => busy,
    activateProject: (id) => activated.push(id),
    showTerminal: (project, terminal) => {
      shown.push(`${project}:${terminal}`);
      return true;
    },
    hasTerminal: (_project, terminal) => terminal === 'synthetic-terminal-1',
    knownTools: () => ['claude', 'codex'],
    showInspector: vi.fn(),
    defer: (run) => deferred.push(run),
  });
  return {
    settings,
    api,
    asked,
    activated,
    shown,
    flush: () => {
      for (const run of deferred.splice(0)) run();
    },
    setBusy: (value: boolean) => {
      busy = value;
      settings.noteOpenFlow();
    },
    openTab: (project: { id: string; name: string }) => {
      open = [...open, project];
    },
  };
}
const state = (t: ReturnType<typeof setup>) => t.settings.settings.value[DEMO_OWNER]!;

describe('Project Type (HS2-W27N1D)', () => {
  it('reads the type, and asks before switching back to Software with projects added', async () => {
    const t = setup({ answers: [false, true] });
    await t.settings.loadType(DEMO_OWNER);
    expect(t.settings.types.value[DEMO_OWNER]?.view).toMatchObject({
      project_type: 'orchestration',
      active_projects: 3,
    });
    // Cancelled: nothing is written and the stored type still shows.
    await t.settings.setType(DEMO_OWNER, 'software');
    expect(t.asked[0].title).toBe('Change this project to Software?');
    expect(t.asked[0].message).toContain('The 3 added projects stay');
    expect(t.api.setProjectType).not.toHaveBeenCalled();
    // Confirmed: kept, and the confirmation travels with the request.
    await t.settings.setType(DEMO_OWNER, 'software');
    expect(t.api.setProjectType).toHaveBeenCalledWith(DEMO_OWNER, 'software', true);
    expect(t.settings.types.value[DEMO_OWNER]?.view?.project_type).toBe('software');
    // Repeated: the same type is a no-op; back to Orchestration needs no question and loads settings.
    await t.settings.setType(DEMO_OWNER, 'software');
    expect(t.api.setProjectType).toHaveBeenCalledTimes(1);
    await t.settings.setType(DEMO_OWNER, 'orchestration');
    expect(t.asked).toHaveLength(2);
    expect(t.api.setProjectType).toHaveBeenLastCalledWith(DEMO_OWNER, 'orchestration', false);
  });

  it('switches an empty Orchestration project to Software without asking', async () => {
    const t = setup({ empty: true });
    await t.settings.loadType(DEMO_OWNER);
    await t.settings.setType(DEMO_OWNER, 'software');
    expect(t.asked).toHaveLength(0);
    expect(t.api.setProjectType).toHaveBeenCalledWith(DEMO_OWNER, 'software', false);
  });
});

describe('Settings → Orchestration (HS2-W27N1D)', () => {
  it('loads the declaration, bindings and policy, and keeps an edited draft across reloads', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    expect(state(t).manifest?.targets).toHaveLength(3);
    expect(Object.keys(state(t).bindings)).toHaveLength(3);
    expect(state(t).draft?.automatic).toBe(true);
    t.settings.changePolicy(DEMO_OWNER, 'machine.max_sessions', '4');
    await t.settings.loadSettings(DEMO_OWNER);
    expect(state(t).draft?.machine.max_sessions).toBe('4');
  });

  it('ignores an older read that answers after a newer one', async () => {
    const t = setup();
    let release: (() => void) | undefined;
    (t.api.orchestrationTargets as ReturnType<typeof vi.fn>).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = () => {
            resolve({ enabled: true, revision: 'old', targets: [] });
          };
        }) as never,
    );
    const older = t.settings.loadSettings(DEMO_OWNER);
    await t.settings.loadSettings(DEMO_OWNER);
    release!();
    await older;
    expect(state(t).manifest?.targets).toHaveLength(3);
  });

  it('adds an open project once, then says it is already added', async () => {
    const t = setup({ empty: true });
    await t.settings.loadSettings(DEMO_OWNER);
    await t.settings.addOpenProject(DEMO_OWNER, 'web-0000000000ab');
    expect(t.api.addOrchestrationProject).toHaveBeenCalledWith(
      DEMO_OWNER,
      expect.objectContaining({ destination: { kind: 'local', checkout_id: 'web-0000000000ab' }, label: 'Web' }),
    );
    expect(state(t).status).toBe('Web added.');
    expect(state(t).manifest?.targets).toHaveLength(1);
    await t.settings.addOpenProject(DEMO_OWNER, 'web-0000000000ab');
    expect(state(t).status).toBe('Web is already added.');
    // The owner itself and an unknown tab add nothing.
    await t.settings.addOpenProject(DEMO_OWNER, DEMO_OWNER);
    await t.settings.addOpenProject(DEMO_OWNER, 'not-open');
    expect(t.api.addOrchestrationProject).toHaveBeenCalledTimes(2);
  });

  it('adds the project Open Another Project… opened, and nothing when the flow is cancelled', async () => {
    const t = setup({ empty: true });
    await t.settings.loadSettings(DEMO_OWNER);
    // Cancelled: the flow was busy, then idle with no new tab.
    t.settings.openAnother(DEMO_OWNER);
    t.setBusy(true);
    t.setBusy(false);
    t.flush();
    expect(t.api.addOrchestrationProject).not.toHaveBeenCalled();
    // A dialog closing as the next opens does not settle the flow early.
    t.settings.openAnother(DEMO_OWNER);
    t.setBusy(true);
    t.setBusy(false);
    t.setBusy(true);
    t.flush();
    t.openTab({ id: 'infra-0000000000ab', name: 'Infra' });
    t.setBusy(false);
    t.flush();
    await vi.waitFor(() => {
      expect(state(t).status).toBe('Infra added.');
    });
    expect(t.activated).toEqual([DEMO_OWNER]);
    // A later ordinary open is not added.
    t.openTab({ id: 'later-0000000000ab', name: 'Later' });
    t.setBusy(true);
    t.setBusy(false);
    t.flush();
  });

  it('removes a project only after confirmation and closes its detail', async () => {
    const t = setup({ answers: [false, true] });
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    await t.settings.removeProject(DEMO_OWNER, DEMO_TARGETS.web);
    expect(t.api.removeOrchestrationProject).not.toHaveBeenCalled();
    await t.settings.removeProject(DEMO_OWNER, DEMO_TARGETS.web);
    expect(t.asked[1]).toMatchObject({ title: 'Stop orchestrating Web?', tone: 'danger' });
    expect(t.settings.selected.value[DEMO_OWNER]).toBeUndefined();
    expect(state(t).manifest?.targets.filter((target) => target.state === 'active')).toHaveLength(2);
    expect(state(t).status).toBe('Web removed.');
  });
});

describe('a project detail (HS2-W27N1D)', () => {
  it('reads each part on its own, so a refused part hides nothing else', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.docs);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.loading).toBe(false);
    });
    const detail = t.settings.detailOf(DEMO_OWNER)!;
    expect(detail.capacityError).toContain('unreachable');
    expect(detail.resolution?.usable).toBe(false);
    expect(detail.toolCapacity?.tools).toHaveLength(2);
    expect(detail.admissions.map((admission) => admission.outcome)).toEqual(['reduced', 'accepted']);
    // No policy resource names this target, so no claim namespace is asked for.
    expect(t.api.orchestrationCapacity).toHaveBeenCalledWith(DEMO_OWNER, DEMO_TARGETS.docs, []);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    await vi.waitFor(() => {
      expect(t.api.orchestrationCapacity).toHaveBeenLastCalledWith(DEMO_OWNER, DEMO_TARGETS.web, ['vm']);
    });
  });

  it('asks admission only for the tools the policy allows on the project, and words refusals by code (HS2-Y3N3PH)', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.api);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.loading).toBe(false);
    });
    expect(t.api.orchestrationAdmission).not.toHaveBeenCalled();
    expect(t.settings.detailOf(DEMO_OWNER)).toMatchObject({ admissions: [], admissionTools: [] });
    // A refusal names its cause; an added project is never "not allowed yet".
    const refusals: [string, RegExp][] = [
      ['policy_denied', /policy does not allow it/],
      ['target_unavailable', /cannot be reached/],
      ['permission_required', /permission request/],
      ['something_new', /refused \(something_new\)/],
    ];
    for (const [code, words] of refusals) {
      vi.mocked(t.api.orchestrationCapacity).mockRejectedValueOnce(new ApiHttpError('refused', 403, code));
      await t.settings.loadDetail(DEMO_OWNER, DEMO_TARGETS.api);
      expect(t.settings.detailOf(DEMO_OWNER)?.capacityError).toMatch(words);
      expect(t.settings.detailOf(DEMO_OWNER)?.capacityError).not.toMatch(/not allowed it yet/);
    }
    // Listed tools are asked, in the project's order.
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.admissionTools).toEqual(['codex', 'claude']);
    });
  });

  it('stops a session at a safe point, and asks before stopping mid-turn', async () => {
    const t = setup({ answers: [false, true] });
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.sessions).toHaveLength(2);
    });
    const worker = '01JSYNTHSESSION00000000001';
    await t.settings.stopSession(DEMO_OWNER, worker);
    expect(t.asked[0].title).toBe('Stop this session now?');
    expect(t.api.stopOrchestrationSession).toHaveBeenCalledTimes(1);
    await t.settings.stopSession(DEMO_OWNER, worker);
    expect(t.api.stopOrchestrationSession).toHaveBeenLastCalledWith(DEMO_OWNER, worker, true);
    expect(state(t).status).toBe('Session stopped.');
  });

  it('takes a session over after confirmation and shows one in its drawer', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.sessions).toHaveLength(2);
    });
    t.settings.showSession(DEMO_OWNER, '01JSYNTHSESSION00000000001');
    expect(t.shown).toEqual(['web-0000000000ab:synthetic-terminal-1']);
    // A session whose terminal no open tab shows is not shown.
    t.settings.showSession(DEMO_OWNER, '01JSYNTHSESSION00000000002');
    expect(t.shown).toHaveLength(1);
    await t.settings.takeOverSession(DEMO_OWNER, '01JSYNTHSESSION00000000001');
    expect(t.asked[0].title).toBe('Take over this session?');
    expect(state(t).status).toBe('Session taken over.');
  });
});

describe('the policy form (HS2-W27N1D)', () => {
  it('refuses an invalid draft locally and saves a valid one at the stored revision', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.changePolicy(DEMO_OWNER, 'machine.max_cpu_percent', '0');
    await t.settings.applyPolicy(DEMO_OWNER);
    expect(state(t).message).toBe('CPU Ceiling must be 1 to 100.');
    expect(t.api.setOrchestrationPolicy).not.toHaveBeenCalled();
    t.settings.changePolicy(DEMO_OWNER, 'machine.max_cpu_percent', '80');
    await t.settings.applyPolicy(DEMO_OWNER);
    expect(t.api.setOrchestrationPolicy).toHaveBeenCalledWith(
      DEMO_OWNER,
      '01JSYNTHREVISION0000000002',
      expect.objectContaining({ machine: expect.objectContaining({ max_cpu_percent: 80 }) }),
    );
    expect(state(t).status).toBe('Policy saved.');
    expect(state(t).draft?.machine.max_cpu_percent).toBe('80');
  });
});

describe('live updates (HS2-W27N1D)', () => {
  it('reads again only what shows', async () => {
    const t = setup();
    t.settings.changed(DEMO_OWNER, { type: false, orchestration: false });
    expect(t.api.projectType).not.toHaveBeenCalled();
    t.settings.changed(DEMO_OWNER, { type: true, orchestration: false });
    expect(t.api.projectType).toHaveBeenCalledTimes(1);
    expect(t.api.orchestrationTargets).not.toHaveBeenCalled();
    t.settings.changed(DEMO_OWNER, { type: true, orchestration: true });
    expect(t.api.orchestrationTargets).toHaveBeenCalledTimes(1);
  });
});

describe("a project's limits and tool accounts (HS2-7WF6JD)", () => {
  it('applies only the selected project entry, and keeps an edit in progress across a reload', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.api);
    // The policy does not list API yet: the form starts from the defaults.
    expect(t.settings.detailOf(DEMO_OWNER)?.limits).toMatchObject({ listed: false, tools: [] });
    t.settings.changeLimits(DEMO_OWNER, 'limits.tools', 'claude', true);
    t.settings.changeLimits(DEMO_OWNER, 'limits.max_sessions', '2');
    // An announcement reloads the settings; the edit in progress stays.
    await t.settings.loadSettings(DEMO_OWNER);
    expect(t.settings.detailOf(DEMO_OWNER)?.limits).toMatchObject({ tools: ['claude'], max_sessions: '2' });
    await t.settings.applyLimits(DEMO_OWNER);
    const [, revision, body] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[0];
    expect(revision).toBe('01JSYNTHREVISION0000000002');
    expect(body.targets?.map((entry) => entry.target_id)).toEqual([
      DEMO_TARGETS.web,
      DEMO_TARGETS.docs,
      DEMO_TARGETS.api,
    ]);
    expect(body.targets?.[2]).toEqual({
      target_id: DEMO_TARGETS.api,
      enabled: true,
      tools: [{ tool: 'claude' }],
      max_sessions: 2,
    });
    expect(state(t).status).toBe('Limits for API saved.');
    // Saved: the form follows the stored policy again, and admission is asked for the new tool.
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.admissionTools).toEqual(['claude']);
    });
    expect(t.settings.detailOf(DEMO_OWNER)).toMatchObject({ limitsDirty: false, limits: { listed: true } });
  });

  it('says why limits are refused, locally or by the server, and keeps the draft', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    t.settings.changeLimits(DEMO_OWNER, 'limits.max_sessions', '9');
    await t.settings.applyLimits(DEMO_OWNER);
    expect(t.settings.detailOf(DEMO_OWNER)?.limitsMessage).toBe(
      "Sessions Here must be at most 2, the machine's session limit.",
    );
    expect(t.api.setOrchestrationPolicy).not.toHaveBeenCalled();
    t.settings.changeLimits(DEMO_OWNER, 'limits.max_sessions', '1');
    expect(t.settings.detailOf(DEMO_OWNER)?.limitsMessage).toBeUndefined();
    vi.mocked(t.api.setOrchestrationPolicy).mockRejectedValueOnce(new ApiHttpError('state changed', 409, 'conflict'));
    await t.settings.applyLimits(DEMO_OWNER);
    expect(t.settings.detailOf(DEMO_OWNER)).toMatchObject({
      limitsSaving: false,
      limitsDirty: true,
      limitsMessage: 'Could not save the limits. state changed',
      limits: { max_sessions: '1' },
    });
  });

  it('adds, edits and removes a tool account in the policy form', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.addAccount(DEMO_OWNER, 'codex');
    t.settings.changePolicy(DEMO_OWNER, 'account.codex.0.account', 'team');
    t.settings.changePolicy(DEMO_OWNER, 'account.codex.0.budget', '12.50');
    t.settings.addAccount(DEMO_OWNER, 'codex');
    t.settings.removeAccount(DEMO_OWNER, 'codex', 1);
    await t.settings.applyPolicy(DEMO_OWNER);
    const [, , body] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[0];
    expect(body.tools?.find((tool) => tool.tool === 'codex')?.accounts).toEqual([
      { account: 'team', max_sessions: 1, budget_cents_per_day: 1250 },
    ]);
    expect(state(t).draft?.tools.find((tool) => tool.tool === 'codex')?.accounts).toEqual([
      { account: 'team', max_sessions: '1', budget: '12.50' },
    ]);
  });
});

describe("a project's models and required resources (HS2-YSY7AR)", () => {
  it('applies them with the limits, and reads held resources in the new namespaces', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.docs);
    t.settings.changeLimits(DEMO_OWNER, 'limits.models.codex', 'gpt-5-codex');
    t.settings.changeLimits(DEMO_OWNER, 'limits.resources', 'sim/iphone-16');
    await t.settings.applyLimits(DEMO_OWNER);
    const [, , body] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[0];
    expect(body.targets?.find((entry) => entry.target_id === DEMO_TARGETS.docs)?.tools).toEqual([
      { tool: 'codex', models: ['gpt-5-codex'] },
      { tool: 'claude' },
    ]);
    expect(body.placement?.resources).toEqual([
      { target_id: DEMO_TARGETS.web, resources: [{ namespace: 'vm', resource: 'macos-vm' }] },
      { target_id: DEMO_TARGETS.docs, resources: [{ namespace: 'sim', resource: 'iphone-16' }] },
    ]);
    await vi.waitFor(() => {
      expect(t.api.orchestrationCapacity).toHaveBeenLastCalledWith(DEMO_OWNER, DEMO_TARGETS.docs, ['sim']);
    });
    expect(t.settings.detailOf(DEMO_OWNER)?.limits).toMatchObject({
      models: { codex: 'gpt-5-codex', claude: '' },
      resources: 'sim/iphone-16',
    });
  });
});

describe("Show in Drawer for a remote project's session (HS2-6E32N4)", () => {
  // The peer's own shapes, as its `/terminals` and `/checkouts/{id}` answer (the real-server half is
  // `remote_servers/sessions.rs` `show_in_drawer_route`). Two checkouts on one host, and a stray.
  const apiTab = remoteProjectId(DEMO_REMOTE, 'api-0000000000ab'),
    mobileTab = remoteProjectId(DEMO_REMOTE, 'mobile-0000000000ab'),
    scopes = [
      { id: apiTab, root: '/srv/peer/api' },
      { id: mobileTab, root: '/srv/peer/mobile' },
    ],
    peerTerminals = [
      { id: 'peer-terminal-api', kind: 'ai', launch_cwd: '/srv/peer/api', cwd: '/srv/peer/api/src' },
      { id: 'peer-terminal-mobile', kind: 'ai', launch_cwd: '/srv/peer/mobile', cwd: '/srv/peer/mobile' },
      { id: 'peer-terminal-stray', kind: 'shell', launch_cwd: '/srv/peer/api-old', cwd: '/srv/peer/api-old' },
    ];
  const remoteSession = (id: string, terminal: string): OrchestrationSession => ({
    session_id: id,
    target_id: DEMO_TARGETS.api,
    role: 'worker',
    tool: 'claude',
    state: 'running',
    owner: 'orchestrator',
    paused: false,
    hosted: { server_id: '01JSYNTHPEER0000000000001', session_id: terminal, worker_id: `claude-${terminal}` },
    created_at_ms: 1,
    updated_at_ms: 1,
  });

  function remoteSetup() {
    const fake = createDemoOrchestrationApi();
    const api: OrchestrationApi = {
      ...fake.api,
      orchestrationSessions: () =>
        Promise.resolve([
          remoteSession('01JSYNTHSESSION00000000101', 'peer-terminal-api'),
          // Recorded against this target, but its terminal runs in the sibling checkout.
          remoteSession('01JSYNTHSESSION00000000102', 'peer-terminal-mobile'),
          remoteSession('01JSYNTHSESSION00000000103', 'peer-terminal-stray'),
        ]),
    };
    let open: { id: string; name: string }[] = [...DEMO_OPEN_PROJECTS];
    // What the drawer holds: each open tab's slice of the peer list as the bridge presents it,
    // owned and keyed as the terminal dashboard does (`${projectId}:${terminal.id}`).
    let drawer = new Set<string>();
    const load = (list: unknown[]) => {
      drawer = new Set(
        open.flatMap((project) =>
          (remoteTerminalList(list, project.id, scopes) as { id: string; launch_cwd?: string }[])
            .filter(
              (info) =>
                terminalProjectOwner(
                  open.map((item) => ({ id: item.id, root: item.id })),
                  info.launch_cwd,
                ) === project.id,
            )
            .map((info) => `${project.id}:${info.id}`),
        ),
      );
    };
    const shown: string[] = [];
    const settings = createOrchestrationSettings({
      api: () => api,
      openProjects: () => open,
      confirm: () => Promise.resolve(true),
      openProjectPicker: vi.fn(),
      openFlowBusy: () => false,
      activateProject: vi.fn(),
      showTerminal: (project, terminal) => {
        if (!drawer.has(`${project}:${terminal}`)) return false;
        shown.push(`${project}:${terminal}`);
        return true;
      },
      hasTerminal: (project, terminal) => drawer.has(`${project}:${terminal}`),
      knownTools: () => ['claude', 'codex'],
      showInspector: vi.fn(),
      defer: (run) => {
        run();
      },
    });
    return {
      settings,
      shown,
      load,
      close: (id: string) => {
        open = open.filter((project) => project.id !== id);
      },
      reopen: (project: { id: string; name: string }) => {
        open = [...open, project];
      },
    };
  }
  const session = (t: ReturnType<typeof remoteSetup>, id: string) =>
    t.settings.detailOf(DEMO_OWNER)!.sessions!.find((item) => item.session_id === id)!;

  it('shows the session in the drawer of its remote tab, and only there', async () => {
    const t = remoteSetup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.api);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.sessions).toHaveLength(3);
    });
    const [worker, sibling, stray] = [
      '01JSYNTHSESSION00000000101',
      '01JSYNTHSESSION00000000102',
      '01JSYNTHSESSION00000000103',
    ];
    // Empty: the remote tab's terminal list has not arrived, so nothing is showable yet.
    t.load([]);
    expect(t.settings.sessionTab(DEMO_OWNER, session(t, worker))).toBeUndefined();
    t.settings.showSession(DEMO_OWNER, worker);
    expect(t.shown).toEqual([]);
    // Refilled from the peer: the bridge gives the API tab its own terminal only.
    t.load(peerTerminals);
    expect(t.settings.sessionTab(DEMO_OWNER, session(t, worker))).toBe(apiTab);
    t.settings.showSession(DEMO_OWNER, worker);
    expect(t.shown).toEqual([`${apiTab}:peer-terminal-api`]);
    // A terminal of the sibling checkout belongs to the Mobile tab, and a stray one to no tab, so
    // neither is shown through the API project's row.
    expect(t.settings.sessionTab(DEMO_OWNER, session(t, sibling))).toBeUndefined();
    expect(t.settings.sessionTab(DEMO_OWNER, session(t, stray))).toBeUndefined();
    t.settings.showSession(DEMO_OWNER, sibling);
    t.settings.showSession(DEMO_OWNER, stray);
    expect(t.shown).toHaveLength(1);
    // Repeated: the same terminal again.
    t.settings.showSession(DEMO_OWNER, worker);
    expect(t.shown).toEqual([`${apiTab}:peer-terminal-api`, `${apiTab}:peer-terminal-api`]);
    // The tab closed: its drawer is gone, so the action is not offered.
    t.close(apiTab);
    t.load(peerTerminals);
    expect(t.settings.sessionTab(DEMO_OWNER, session(t, worker))).toBeUndefined();
    t.settings.showSession(DEMO_OWNER, worker);
    expect(t.shown).toHaveLength(2);
    // Reopened and its list read again: offered once more.
    t.reopen({ id: apiTab, name: 'API' });
    t.load(peerTerminals);
    expect(t.settings.sessionTab(DEMO_OWNER, session(t, worker))).toBe(apiTab);
  });
});

describe("a project's coordination sessions (HS2-TK9ADC)", () => {
  it('applies the roles with the limits, and a further edit takes them off again', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.docs);
    expect(t.settings.detailOf(DEMO_OWNER)?.limits?.roleSessions).toEqual([]);
    t.settings.changeLimits(DEMO_OWNER, 'limits.role_sessions', 'integrator', true);
    t.settings.changeLimits(DEMO_OWNER, 'limits.role_sessions', 'dispatcher', true);
    expect(t.settings.detailOf(DEMO_OWNER)?.limits?.roleSessions).toEqual(['dispatcher', 'integrator']);
    await t.settings.applyLimits(DEMO_OWNER);
    const [, , body] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[0];
    expect(body.placement?.role_sessions).toEqual([
      { target_id: DEMO_TARGETS.docs, roles: ['dispatcher', 'integrator'] },
    ]);
    // The detail reads back what was stored.
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.limits?.roleSessions).toEqual(['dispatcher', 'integrator']);
    });
    t.settings.changeLimits(DEMO_OWNER, 'limits.role_sessions', 'dispatcher', false);
    t.settings.changeLimits(DEMO_OWNER, 'limits.role_sessions', 'integrator', false);
    await t.settings.applyLimits(DEMO_OWNER);
    const [, , cleared] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[1];
    expect(cleared.placement?.role_sessions).toBeUndefined();
  });
});

describe("adding a remote server's open projects (HS2-92FJXM)", () => {
  const REMOTE_OWNER = `remote:${DEMO_REMOTE}:launch-0000000000ab`;

  it('reads them only for a project open from another server, and adds one there', async () => {
    const t = setup();
    // A project served here reads no list.
    await t.settings.loadSettings(DEMO_OWNER);
    expect(t.api.hostedOrchestrationProjects).not.toHaveBeenCalled();
    expect(state(t).hosted).toBeUndefined();
    // Open from another server: that server's open projects, Web already added.
    await t.settings.loadSettings(REMOTE_OWNER);
    expect(t.api.hostedOrchestrationProjects).toHaveBeenCalledWith(REMOTE_OWNER);
    const remote = () => t.settings.settings.value[REMOTE_OWNER]!;
    expect(remote().hosted?.map((choice) => [choice.name, choice.added])).toEqual([
      ['Web', true],
      ['Billing', false],
      ['Docs', true],
    ]);
    // Added through the hosted route, then read again: Billing is checked.
    await t.settings.addHostedProject(REMOTE_OWNER, 'billing-0000000000ab');
    expect(t.api.addHostedOrchestrationProject).toHaveBeenCalledWith(
      REMOTE_OWNER,
      expect.objectContaining({ checkout_id: 'billing-0000000000ab', label: 'Billing' }),
    );
    expect(remote().status).toBe('Billing added.');
    expect(remote().hosted?.find((choice) => choice.name === 'Billing')?.added).toBe(true);
    // Repeated: the same project, said so.
    await t.settings.addHostedProject(REMOTE_OWNER, 'billing-0000000000ab');
    expect(remote().status).toBe('Billing is already added.');
    // A checkout the list does not offer is never asked for.
    const calls = vi.mocked(t.api.addHostedOrchestrationProject).mock.calls.length;
    await t.settings.addHostedProject(REMOTE_OWNER, 'elsewhere-0000000000ab');
    expect(vi.mocked(t.api.addHostedOrchestrationProject).mock.calls).toHaveLength(calls);
  });

  it('keeps the last list when a read fails, and says why; a refused add is shown in place', async () => {
    const t = setup();
    await t.settings.loadSettings(REMOTE_OWNER);
    const remote = () => t.settings.settings.value[REMOTE_OWNER]!;
    vi.mocked(t.api.hostedOrchestrationProjects).mockRejectedValueOnce(new Error('The server did not answer.'));
    await t.settings.loadSettings(REMOTE_OWNER);
    expect(remote().hostedError).toBe('The server did not answer.');
    expect(remote().hosted).toHaveLength(3);
    // The next read succeeds and clears it.
    await t.settings.loadSettings(REMOTE_OWNER);
    expect(remote().hostedError).toBeUndefined();
    vi.mocked(t.api.addHostedOrchestrationProject).mockRejectedValueOnce(
      new ApiHttpError('client certificate is read-only', 403, 'forbidden'),
    );
    await t.settings.addHostedProject(REMOTE_OWNER, 'billing-0000000000ab');
    expect(remote().message).toContain('Could not add Billing.');
    expect(remote().busy).not.toContain('add');
  });
});

describe('removing a project from another machine (HS2-0G80WD)', () => {
  it('asks first, then removes through the hosted route; a project served here keeps the local one', async () => {
    const remoteOwner = `remote:${DEMO_REMOTE}:launch-0000000000ab`;
    const t = setup({ answers: [false, true, true] });
    await t.settings.loadSettings(remoteOwner);
    // Cancelled: nothing is asked of the server.
    await t.settings.removeProject(remoteOwner, DEMO_TARGETS.web);
    expect(t.asked[0].title).toBe('Stop orchestrating Web?');
    expect(t.api.removeHostedOrchestrationProject).not.toHaveBeenCalled();
    // Confirmed: the hosted route, never the loopback-only one.
    await t.settings.removeProject(remoteOwner, DEMO_TARGETS.web);
    expect(t.api.removeHostedOrchestrationProject).toHaveBeenCalledWith(remoteOwner, DEMO_TARGETS.web);
    expect(t.api.removeOrchestrationProject).not.toHaveBeenCalled();
    expect(t.settings.settings.value[remoteOwner]?.status).toBe('Web removed.');
    // A project served here removes through its own route.
    await t.settings.loadSettings(DEMO_OWNER);
    await t.settings.removeProject(DEMO_OWNER, DEMO_TARGETS.docs);
    expect(t.api.removeOrchestrationProject).toHaveBeenCalledWith(DEMO_OWNER, DEMO_TARGETS.docs);
    expect(t.api.removeHostedOrchestrationProject).toHaveBeenCalledTimes(1);
  });
});
