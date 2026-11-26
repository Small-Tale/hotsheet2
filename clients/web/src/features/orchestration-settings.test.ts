import { describe, expect, it, vi } from 'vitest';

import { ApiHttpError, type OrchestrationSession } from '../api';
import { OrchestrationSettings } from '../components/orchestration-settings';
import type { ConfirmationAnswer, ConfirmationOptions } from '../confirmation';
import { hostedSiteSources, projectRows, siteChoicesByProject } from '../orchestration-settings-model';
import { remoteProjectId } from '../remote-projects/identity';
import { remoteTerminalList } from '../remote-projects/terminals';
import { terminalProjectOwner } from '../terminal-project-scope';
import {
  createDemoOrchestrationApi,
  DEMO_OPEN_PROJECTS,
  DEMO_OWNER,
  DEMO_REMOTE,
  DEMO_TARGETS,
  demoStore,
} from '../ux-demo/orchestration-settings-fixtures';
import { createOrchestrationSettings, type OrchestrationApi } from './orchestration-settings';

function setup({ empty = false, answers = [] as boolean[], choices = [] as ConfirmationAnswer[] } = {}) {
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
  const toasts: string[] = [];
  const settings = createOrchestrationSettings({
    api: () => api,
    openProjects: () => open,
    confirm: (options) => {
      asked.push(options);
      return Promise.resolve(answers.shift() ?? true);
    },
    // HS2-E9VVQF: unanswered duplicate questions add a separate project, as before.
    choose: (options) => {
      asked.push(options);
      return Promise.resolve(choices.shift() ?? 'alternate');
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
    notify: (message) => toasts.push(message),
  });
  return {
    settings,
    api,
    toasts,
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
    // HS2-1K6122: each outcome is announced through the toast, once.
    expect(t.toasts).toEqual(['Web added.', 'Web is already added.']);
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
    // HS2-40CTDS: the outcome is one toast.
    expect(t.toasts).toEqual(['Web removed.']);
  });
});

describe('what each site of a project is (HS2-D4G81Q)', () => {
  it('reads the sites view only while a project has several sites, and keeps the last one when a read fails', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    expect(t.api.orchestrationSites).not.toHaveBeenCalled();
    expect(state(t).sites).toBeUndefined();

    // Docs becomes another site of Web.
    const grouped = await t.api.orchestrationTargets(DEMO_OWNER);
    grouped.targets = grouped.targets.map((target) =>
      target.target_id === DEMO_TARGETS.docs ? { ...target, site_of: DEMO_TARGETS.web } : target,
    );
    vi.mocked(t.api.orchestrationTargets).mockResolvedValue(grouped);
    vi.mocked(t.api.orchestrationSites).mockResolvedValueOnce({
      revision: grouped.revision,
      projects: [
        {
          project_id: DEMO_TARGETS.web,
          label: 'Web',
          sites: [
            {
              target_id: DEMO_TARGETS.web,
              label: 'Web',
              project: true,
              destination: null,
              confirmed_at_ms: null,
              last_failure: null,
              stale: true,
              capabilities: { os: 'macos', os_version: '15.1', arch: 'arm64' },
            },
            {
              target_id: DEMO_TARGETS.docs,
              label: 'Docs',
              project: false,
              destination: null,
              confirmed_at_ms: null,
              last_failure: null,
              stale: true,
              capabilities_error: 'target_unavailable',
            },
          ],
        },
      ],
    });
    await t.settings.loadSettings(DEMO_OWNER);
    await vi.waitFor(() => {
      expect(state(t).sites?.projects[0].sites).toHaveLength(2);
    });
    expect(t.api.orchestrationSites).toHaveBeenCalledTimes(1);

    // A failed read keeps the last view rather than clearing the rows.
    vi.mocked(t.api.orchestrationSites).mockRejectedValueOnce(new Error('offline'));
    await t.settings.loadSettings(DEMO_OWNER);
    await vi.waitFor(() => {
      expect(t.api.orchestrationSites).toHaveBeenCalledTimes(2);
    });
    expect(state(t).sites?.projects[0].sites[0].capabilities?.os).toBe('macos');
    expect(state(t).message).toBe('');

    // Separated again: the view is dropped and not read.
    vi.mocked(t.api.orchestrationTargets).mockResolvedValue({
      ...grouped,
      targets: grouped.targets.map((target) => ({ ...target, site_of: undefined })),
    });
    await t.settings.loadSettings(DEMO_OWNER);
    expect(state(t).sites).toBeUndefined();
    expect(t.api.orchestrationSites).toHaveBeenCalledTimes(2);
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
    // HS2-N5BDE4: API is not listed, so it is Auto: admission is asked for every usable tool.
    expect(t.settings.detailOf(DEMO_OWNER)?.admissionTools).toEqual(['codex', 'claude']);
    expect(t.api.orchestrationAdmission).toHaveBeenCalled();
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
    // HS2-40CTDS: the cancelled stop says nothing; the one that stops is one toast.
    expect(t.toasts).toEqual(['Session stopped.']);
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
    expect(t.toasts).toEqual(['Session taken over.']);
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
    // HS2-40CTDS: a draft's own validation stays beside the form, not in a toast.
    expect(t.toasts).toEqual([]);
    t.settings.changePolicy(DEMO_OWNER, 'machine.max_cpu_percent', '80');
    await t.settings.applyPolicy(DEMO_OWNER);
    expect(t.api.setOrchestrationPolicy).toHaveBeenCalledWith(
      DEMO_OWNER,
      '01JSYNTHREVISION0000000002',
      expect.objectContaining({ machine: expect.objectContaining({ max_cpu_percent: 80 }) }),
    );
    expect(state(t).status).toBe('Policy saved.');
    expect(t.toasts).toEqual(['Policy saved.']);
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
    expect(t.toasts).toEqual(['Limits for API saved.']);
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
    // HS2-40CTDS: the local refusal stays beside the limits; the server's refusal is toasted and kept there too.
    expect(t.toasts).toEqual(['Could not save the limits. state changed']);
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
    // HS2-R2N33M: unlisted is Auto, shown as both; a change makes it explicit.
    expect(t.settings.detailOf(DEMO_OWNER)?.limits).toMatchObject({
      roleSessions: ['dispatcher', 'integrator'],
      roleSessionsAuto: true,
    });
    t.settings.changeLimits(DEMO_OWNER, 'limits.role_sessions', 'integrator', false);
    expect(t.settings.detailOf(DEMO_OWNER)?.limits?.roleSessions).toEqual(['dispatcher']);
    await t.settings.applyLimits(DEMO_OWNER);
    const [, , body] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[0];
    expect(body.placement?.role_sessions).toEqual([{ target_id: DEMO_TARGETS.docs, roles: ['dispatcher'] }]);
    // The detail reads back what was stored.
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.limits?.roleSessions).toEqual(['dispatcher']);
    });
    t.settings.changeLimits(DEMO_OWNER, 'limits.role_sessions', 'dispatcher', false);
    await t.settings.applyLimits(DEMO_OWNER);
    const [, , cleared] = vi.mocked(t.api.setOrchestrationPolicy).mock.calls[1];
    expect(cleared.placement?.role_sessions).toEqual([{ target_id: DEMO_TARGETS.docs, roles: [] }]);
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
      ['Mobile', false],
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
    expect(remote().hosted).toHaveLength(4);
    // The next read succeeds and clears it.
    await t.settings.loadSettings(REMOTE_OWNER);
    expect(remote().hostedError).toBeUndefined();
    vi.mocked(t.api.addHostedOrchestrationProject).mockRejectedValueOnce(
      new ApiHttpError('client certificate is read-only', 403, 'read_only'),
    );
    await t.settings.addHostedProject(REMOTE_OWNER, 'billing-0000000000ab');
    expect(remote().message).toContain('Could not add Billing.');
    expect(remote().busy).not.toContain('add');
  });
});

describe("adding a remote server's open project as a site (HS2-89YNGN)", () => {
  const REMOTE_OWNER = `remote:${DEMO_REMOTE}:launch-0000000000ab`;
  const remote = (t: ReturnType<typeof setup>) => t.settings.settings.value[REMOTE_OWNER]!;

  it('offers the same-store checkouts as sites and adds one through the hosted route', async () => {
    const t = setup();
    await t.settings.loadSettings(REMOTE_OWNER);
    const state = remote(t);
    const hosted = hostedSiteSources(state.hosted!, 'Build Host');
    expect(hosted.stores['mobile-0000000000ab']).toBe(demoStore('mobile-0000000000ab'));
    const rows = projectRows(state.manifest, state.bindings, {}, undefined, 'Build Host');
    const sites = siteChoicesByProject(rows, hosted.choices, hosted.stores, state.manifest, state.bindings, false);
    // API's store is Mobile's; Billing reaches another one and shows only under Show All Projects.
    expect(sites[DEMO_TARGETS.api].map((choice) => [choice.name, choice.machine, choice.sameStore])).toEqual([
      ['Mobile', 'Build Host', true],
    ]);
    await t.settings.addSite(REMOTE_OWNER, DEMO_TARGETS.api, 'mobile-0000000000ab');
    expect(t.api.addHostedOrchestrationProject).toHaveBeenLastCalledWith(
      REMOTE_OWNER,
      expect.objectContaining({ checkout_id: 'mobile-0000000000ab', label: 'Mobile', site_of: DEMO_TARGETS.api }),
    );
    expect(remote(t).status).toBe('Mobile added as a site of API.');
    expect(remote(t).manifest?.targets.find((target) => target.label === 'Mobile')?.site_of).toBe(DEMO_TARGETS.api);
    // Another store's checkout is refused by the server, in the message line.
    vi.mocked(t.api.addHostedOrchestrationProject).mockRejectedValueOnce(
      new ApiHttpError('store mismatch', 409, 'site_store_mismatch'),
    );
    await t.settings.addSite(REMOTE_OWNER, DEMO_TARGETS.api, 'billing-0000000000ab');
    expect(remote(t).message).toBe(
      'Could not add Billing as a site of API. It reaches a different ticket store than this project.',
    );
    // An unknown project row is a no-op.
    const calls = vi.mocked(t.api.addHostedOrchestrationProject).mock.calls.length;
    await t.settings.addSite(REMOTE_OWNER, 'missing', 'mobile-0000000000ab');
    expect(vi.mocked(t.api.addHostedOrchestrationProject).mock.calls).toHaveLength(calls);
  });

  it('asks about a duplicate from + Add Project: site, separate project, or nothing', async () => {
    const t = setup({ choices: ['cancel', 'confirm'] });
    await t.settings.loadSettings(REMOTE_OWNER);
    // Billing's store is no added project's: added at once, nothing asked.
    await t.settings.addHostedProject(REMOTE_OWNER, 'billing-0000000000ab');
    expect(t.asked).toHaveLength(0);
    expect(remote(t).status).toBe('Billing added.');
    // Mobile shares API's store: asked, and Cancel adds nothing.
    const calls = vi.mocked(t.api.addHostedOrchestrationProject).mock.calls.length;
    await t.settings.addHostedProject(REMOTE_OWNER, 'mobile-0000000000ab');
    expect(t.asked.at(-1)?.title).toBe('Is Mobile another site of API?');
    expect(vi.mocked(t.api.addHostedOrchestrationProject).mock.calls).toHaveLength(calls);
    // Confirmed: added as a site of API.
    await t.settings.addHostedProject(REMOTE_OWNER, 'mobile-0000000000ab');
    expect(t.api.addHostedOrchestrationProject).toHaveBeenLastCalledWith(
      REMOTE_OWNER,
      expect.objectContaining({ checkout_id: 'mobile-0000000000ab', site_of: DEMO_TARGETS.api }),
    );
    expect(remote(t).status).toBe('Mobile added as a site of API.');
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

describe('Add a Site and Separate from <project> (HS2-E9VVQF)', () => {
  const MOBILE = `remote:${DEMO_REMOTE}:mobile-0000000000ab`;
  const stores = (t: ReturnType<typeof setup>) => state(t).stores ?? {};

  it('inspects each open tab once for its ticket store, waiting for one already in flight', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    await t.settings.loadStores(DEMO_OWNER);
    // Every other open tab is inspected once: API, Web, Docs and Mobile.
    expect(t.api.inspectOrchestrationTarget).toHaveBeenCalledTimes(4);
    expect(stores(t)[MOBILE]).toBe('a'.repeat(64));
    // Read again (an orchestration_changed event): nothing is inspected twice.
    await t.settings.loadSettings(DEMO_OWNER);
    await t.settings.loadStores(DEMO_OWNER);
    expect(t.api.inspectOrchestrationTarget).toHaveBeenCalledTimes(4);
    // A new tab is inspected on its own; a failed inspection is remembered as unknown.
    t.openTab({ id: 'billing-0000000000ab', name: 'Billing' });
    (t.api.inspectOrchestrationTarget as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('gone'));
    await t.settings.loadStores(DEMO_OWNER, 'billing-0000000000ab');
    expect(stores(t)['billing-0000000000ab']).toBeNull();
    expect(t.api.inspectOrchestrationTarget).toHaveBeenCalledTimes(5);
  });

  it('reads no stores for a project open from another server', async () => {
    const t = setup();
    const owner = remoteProjectId(DEMO_REMOTE, 'launch-0000000000ab');
    await t.settings.loadStores(owner);
    expect(t.api.inspectOrchestrationTarget).not.toHaveBeenCalled();
  });

  it('adds an open project as a site of a project, and shows a refused store in the message line', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    // Billing reaches another store than API: the refusal says so and nothing is added.
    t.openTab({ id: 'billing-0000000000ab', name: 'Billing' });
    await t.settings.addSite(DEMO_OWNER, DEMO_TARGETS.api, 'billing-0000000000ab');
    expect(t.api.addOrchestrationProject).toHaveBeenLastCalledWith(
      DEMO_OWNER,
      expect.objectContaining({ site_of: DEMO_TARGETS.api, label: 'Billing' }),
    );
    expect(state(t).message).toBe(
      'Could not add Billing as a site of API. It reaches a different ticket store than this project.',
    );
    expect(state(t).manifest?.targets).toHaveLength(3);
    // Mobile reaches API's store: it is added as a site of API.
    await t.settings.addSite(DEMO_OWNER, DEMO_TARGETS.api, MOBILE);
    expect(state(t).message).toBe('');
    expect(state(t).status).toBe('Mobile added as a site of API.');
    const site = state(t).manifest?.targets.find((target) => target.label === 'Mobile');
    expect(site?.site_of).toBe(DEMO_TARGETS.api);
    // An unknown tab or project is a no-op.
    await t.settings.addSite(DEMO_OWNER, 'missing', MOBILE);
    await t.settings.addSite(DEMO_OWNER, DEMO_TARGETS.api, 'closed-0000000000ab');
    expect(t.api.addOrchestrationProject).toHaveBeenCalledTimes(2);

    // HS2-NAA3HV: Make Mobile the Home Site: API becomes Mobile's site, with no question asked.
    const before = t.asked.length;
    (t.api.makeOrchestrationHomeSite as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new ApiHttpError('plan conflict', 409, 'site_plan_conflict'),
    );
    await t.settings.makeHomeSite(DEMO_OWNER, site!.target_id);
    expect(state(t).message).toBe(
      'Could not make Mobile the home site of API. The plan would have two steps on one ticket of the project.',
    );
    expect(state(t).busy).not.toContain(site!.target_id);
    await t.settings.makeHomeSite(DEMO_OWNER, site!.target_id);
    expect(t.asked).toHaveLength(before);
    expect(t.api.makeOrchestrationHomeSite).toHaveBeenLastCalledWith(DEMO_OWNER, site!.target_id, expect.any(String));
    expect(state(t).message).toBe('');
    expect(state(t).status).toBe('Mobile is the home site of the project now.');
    const declared = (id: string) => state(t).manifest?.targets.find((target) => target.target_id === id);
    expect(declared(site!.target_id)?.site_of).toBeUndefined();
    expect(declared(DEMO_TARGETS.api)?.site_of).toBe(site!.target_id);
    // A home has nothing to move; and API is made the home again.
    await t.settings.makeHomeSite(DEMO_OWNER, site!.target_id);
    expect(t.api.makeOrchestrationHomeSite).toHaveBeenCalledTimes(2);
    await t.settings.makeHomeSite(DEMO_OWNER, DEMO_TARGETS.api);
    expect(declared(DEMO_TARGETS.api)?.site_of).toBeUndefined();
    expect(declared(site!.target_id)?.site_of).toBe(DEMO_TARGETS.api);

    // A stale revision: the refusal shows with the project's name, and the site stays.
    (t.api.separateOrchestrationSite as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new ApiHttpError('orchestration declaration changed', 409, 'conflict'),
    );
    await t.settings.separateSite(DEMO_OWNER, site!.target_id);
    expect(state(t).message).toBe('Could not separate Mobile from API. orchestration declaration changed');
    expect(state(t).busy).not.toContain(site!.target_id);
    // Separate from API: no question, and the site is a project of its own again.
    const asked = t.asked.length;
    await t.settings.separateSite(DEMO_OWNER, site!.target_id);
    expect(t.asked).toHaveLength(asked);
    expect(t.api.separateOrchestrationSite).toHaveBeenLastCalledWith(
      DEMO_OWNER,
      site!.target_id,
      expect.any(String),
      'Mobile',
    );
    expect(state(t).message).toBe('');
    expect(state(t).status).toBe('Mobile is its own project again.');
    expect(t.toasts.at(-1)).toBe('Mobile is its own project again.');
    expect(state(t).manifest?.targets.find((target) => target.label === 'Mobile')?.site_of).toBeUndefined();
    // A project that is not a site: nothing to separate.
    await t.settings.separateSite(DEMO_OWNER, DEMO_TARGETS.web);
    expect(t.api.separateOrchestrationSite).toHaveBeenCalledTimes(2);
  });

  it('asks only for a duplicate from + Add Project: site, separate project, or nothing', async () => {
    const t = setup({ choices: ['cancel', 'confirm', 'alternate'] });
    await t.settings.loadSettings(DEMO_OWNER);
    // Billing reaches a store no added project uses: added at once, no question.
    t.openTab({ id: 'billing-0000000000ab', name: 'Billing' });
    await t.settings.addOpenProject(DEMO_OWNER, 'billing-0000000000ab');
    expect(t.asked).toHaveLength(0);
    expect(state(t).status).toBe('Billing added.');
    // An open project that is already added is not a duplicate: no question either.
    await t.settings.addOpenProject(DEMO_OWNER, 'web-0000000000ab');
    expect(t.asked).toHaveLength(0);
    expect(state(t).status).toBe('Web is already added.');
    // Mobile reaches API's store. Cancel adds nothing.
    const before = (t.api.addOrchestrationProject as ReturnType<typeof vi.fn>).mock.calls.length;
    await t.settings.addOpenProject(DEMO_OWNER, MOBILE);
    expect(t.asked[0]).toMatchObject({
      title: 'Is Mobile another site of API?',
      confirmLabel: 'Add as a Site of API',
      alternateLabel: 'Add as a Separate Project',
    });
    expect(t.api.addOrchestrationProject).toHaveBeenCalledTimes(before);
    // Add as a Site of API.
    await t.settings.addOpenProject(DEMO_OWNER, MOBILE);
    expect(t.api.addOrchestrationProject).toHaveBeenLastCalledWith(
      DEMO_OWNER,
      expect.objectContaining({ site_of: DEMO_TARGETS.api }),
    );
    expect(state(t).status).toBe('Mobile added as a site of API.');
  });

  it('adds a duplicate as a separate project when the person says so', async () => {
    const t = setup({ choices: ['alternate'] });
    await t.settings.loadSettings(DEMO_OWNER);
    await t.settings.addOpenProject(DEMO_OWNER, MOBILE);
    expect(t.asked).toHaveLength(1);
    const sent = (t.api.addOrchestrationProject as ReturnType<typeof vi.fn>).mock.lastCall![1] as object;
    expect(sent).not.toHaveProperty('site_of');
    expect(state(t).status).toBe('Mobile added.');
  });

  it('Open Another Project… from Add a Site adds the opened tab as a site', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openAnother(DEMO_OWNER, DEMO_TARGETS.api);
    t.setBusy(true);
    t.openTab({ id: `remote:${DEMO_REMOTE}:mobile2-0000000000ab`, name: 'API Clone' });
    t.setBusy(false);
    t.flush();
    await vi.waitFor(() => {
      expect(t.api.addOrchestrationProject).toHaveBeenCalledWith(
        DEMO_OWNER,
        expect.objectContaining({ label: 'API Clone', site_of: DEMO_TARGETS.api }),
      );
    });
    expect(t.activated).toEqual([DEMO_OWNER]);
  });

  it('Show All Projects toggles per project', () => {
    const t = setup();
    expect(t.settings.settings.value[DEMO_OWNER]?.showAllSites).toBeUndefined();
    t.settings.toggleAllSites(DEMO_OWNER);
    expect(state(t).showAllSites).toBe(true);
    t.settings.toggleAllSites(DEMO_OWNER);
    expect(state(t).showAllSites).toBe(false);
  });
});

describe('the Add a Site and Separate menus (HS2-E9VVQF)', () => {
  const row = (targetId: string, label: string, siteOf?: string) => ({
    targetId,
    label,
    location: 'local' as const,
    where: '',
    place: '',
    connection: { label: 'Bound', tone: 'neutral' as const },
    ...(siteOf ? { siteOf } : {}),
  });
  const rows = [row('W', 'Widgets'), row('S', 'Widgets laptop', 'W'), row('D', 'Docs')];
  const clone = { projectId: 'clone', name: 'Widgets clone', location: 'local' as const, machine: 'This Computer' };

  it('offers Add a Site on a project row and Separate from <project> on a site row', () => {
    const markup = String(
      OrchestrationSettings({
        rows,
        siteChoices: { W: [{ ...clone, sameStore: true }], D: [] },
      }),
    );
    expect(markup.match(/>Add a Site</g)).toHaveLength(2);
    expect(markup).toContain('Open Projects in the Same Ticket Store');
    expect(markup).toContain('data-action="add-orchestration-site"');
    expect(markup).toContain('data-project-id="clone"');
    expect(markup).toContain('No open project uses this ticket store');
    expect(markup).toContain('>Show All Projects<');
    expect(markup).toContain('data-action="open-another-orchestration-site"');
    // One Separate entry, on the site row, naming its project.
    expect(markup.match(/Separate from Widgets/g)).toHaveLength(1);
    expect(markup).toContain('data-action="separate-orchestration-site"');
    expect(markup).toContain('data-lucide="group"');
    expect(markup).toContain('data-lucide="ungroup"');
  });

  it('marks another or an unknown store under Show All Projects, and offers no Add a Site from another server', () => {
    const all = String(
      OrchestrationSettings({
        rows,
        showAllSites: true,
        siteChoices: {
          W: [
            { ...clone, sameStore: true },
            { ...clone, projectId: 'other', name: 'Other', sameStore: false },
            { ...clone, projectId: 'unknown', name: 'Unknown', sameStore: undefined },
          ],
        },
      }),
    );
    expect(all).toContain('>Open Projects<');
    expect(all).toContain('Another ticket store');
    expect(all).toContain('Ticket store unknown');
    expect(all).toContain('aria-checked="true"');
    const hosted = String(OrchestrationSettings({ rows, hostedChoices: [] }));
    expect(hosted).not.toContain('Add a Site');
    // A site row still separates there.
    expect(hosted).toContain('Separate from Widgets');
    // HS2-89YNGN: once the server's open checkouts are read, Add a Site offers them, without opening a new one.
    const served = String(
      OrchestrationSettings({
        rows,
        hostedChoices: [],
        hostedSites: true,
        siteChoices: { W: [{ ...clone, sameStore: true }] },
      }),
    );
    expect(served).toContain('Add a Site');
    expect(served).not.toContain('Open Another Project…');
    expect(all).toContain('Open Another Project…');
  });
});

describe('a change’s outcome is a toast, and a failure keeps its record (HS2-40CTDS)', () => {
  const refused = () => new ApiHttpError('state changed', 409, 'conflict');

  it('toasts each failed change once and keeps it in the message line until the next change', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    t.settings.openDetail(DEMO_OWNER, DEMO_TARGETS.web);
    await vi.waitFor(() => {
      expect(t.settings.detailOf(DEMO_OWNER)?.sessions).toHaveLength(2);
    });
    const failures: Array<[() => Promise<unknown>, keyof OrchestrationApi, string]> = [
      [
        () => t.settings.removeProject(DEMO_OWNER, DEMO_TARGETS.docs),
        'removeOrchestrationProject',
        'Could not remove Docs. state changed',
      ],
      [
        () => t.settings.stopSession(DEMO_OWNER, '01JSYNTHSESSION00000000002'),
        'stopOrchestrationSession',
        'Could not stop the session. state changed',
      ],
      [
        () => t.settings.takeOverSession(DEMO_OWNER, '01JSYNTHSESSION00000000002'),
        'takeOverOrchestrationSession',
        'Could not take over the session. state changed',
      ],
      [() => t.settings.applyPolicy(DEMO_OWNER), 'setOrchestrationPolicy', 'Could not save the policy. state changed'],
    ];
    for (const [run, call, message] of failures) {
      vi.mocked(t.api[call] as (...args: unknown[]) => unknown).mockRejectedValueOnce(refused());
      await run();
      expect(state(t).message).toBe(message);
      expect(state(t).status).toBe('');
      expect(t.toasts.at(-1)).toBe(message);
    }
    expect(t.toasts).toEqual(failures.map(([, , message]) => message));
    // The next change clears the record, and its success is the only new toast.
    await t.settings.applyPolicy(DEMO_OWNER);
    expect(state(t).message).toBe('');
    expect(t.toasts.slice(failures.length)).toEqual(['Policy saved.']);
  });

  it('toasts a refused add, and a refused site change, in words', async () => {
    const t = setup();
    await t.settings.loadSettings(DEMO_OWNER);
    vi.mocked(t.api.addOrchestrationProject).mockRejectedValueOnce(refused());
    await t.settings.addOpenProject(DEMO_OWNER, 'web-0000000000ab');
    expect(state(t).message).toBe('Could not add Web. state changed');
    expect(t.toasts).toEqual(['Could not add Web. state changed']);
  });

  it('reads a failed read into the message line only', async () => {
    const t = setup();
    vi.mocked(t.api.orchestrationTargets).mockRejectedValueOnce(new Error('offline'));
    await t.settings.loadSettings(DEMO_OWNER);
    expect(state(t).message).toBe('Could not read the orchestration settings. offline');
    expect(t.toasts).toEqual([]);
  });
});
