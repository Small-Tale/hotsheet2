import { describe, expect, it, vi } from 'vitest';

import type { ConfirmationOptions } from '../confirmation';
import {
  createDemoOrchestrationApi,
  DEMO_OPEN_PROJECTS,
  DEMO_OWNER,
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
    expect(t.api.addOrchestrationProject).toHaveBeenCalledTimes(1);
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
