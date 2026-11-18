import { signal } from 'kerfjs';

import type {
  Api,
  HostedOrchestrationProjectChoice,
  OrchestrationAdmission,
  OrchestrationBinding,
  OrchestrationCapacity,
  OrchestrationManifest,
  OrchestrationPolicyView,
  OrchestrationResolution,
  OrchestrationSession,
  OrchestrationToolCapacity,
  ProjectType,
  ProjectTypeView,
} from '../api';
import { ApiHttpError } from '../api';
import type { Confirm } from '../confirmation';
import {
  type AddDestination,
  addPolicyAccount,
  changePolicyDraft,
  changeTargetLimits,
  destinationOf,
  newUlid,
  type PolicyDraft,
  policyDraftFrom,
  policyFromDraft,
  policyWithTargetLimits,
  removePolicyAccount,
  tabProjectIdOf,
  type TargetLimitsDraft,
  targetLimitsDraftFrom,
} from '../orchestration-settings-model';
import { isRemoteProjectId } from '../remote-projects/identity';

/** The orchestration reads and writes the settings use; `Api` satisfies it, and tests fake it. */
export type OrchestrationApi = Pick<
  Api,
  | 'projectType'
  | 'setProjectType'
  | 'orchestrationTargets'
  | 'orchestrationBinding'
  | 'orchestrationResolve'
  | 'addOrchestrationProject'
  | 'removeOrchestrationProject'
  | 'hostedOrchestrationProjects'
  | 'addHostedOrchestrationProject'
  | 'orchestrationCapacity'
  | 'orchestrationAdmission'
  | 'orchestrationToolCapacity'
  | 'orchestrationSessions'
  | 'stopOrchestrationSession'
  | 'takeOverOrchestrationSession'
  | 'orchestrationPolicy'
  | 'setOrchestrationPolicy'
>;

/** An open project tab as these settings need it. */
export interface OrchestrationOpenProject {
  id: string;
  name: string;
}

export interface ProjectTypeState {
  view?: ProjectTypeView;
  loading: boolean;
  saving: boolean;
  message: string;
}

/** Everything Settings → Orchestration shows for one orchestration project. */
export interface OrchestrationSettingsState {
  manifest?: OrchestrationManifest;
  bindings: Record<string, OrchestrationBinding | undefined>;
  policy?: OrchestrationPolicyView;
  draft?: PolicyDraft;
  /** The person changed the draft since it was read or saved; a reload keeps it. */
  draftDirty?: boolean;
  loading: boolean;
  /** The last failure, until the next successful change or read. */
  message: string;
  /** The last success, for the polite status line. */
  status: string;
  /** Targets, sessions, `add` or `policy` with a change in flight. */
  busy: readonly string[];
  /**
   * HS2-92FJXM: for an orchestration project open from another server, that server's open
   * checkouts, which + Add Project offers. Absent for a project served here.
   */
  hosted?: readonly HostedOrchestrationProjectChoice[];
  /** Why that list could not be read. */
  hostedError?: string;
}

/** A selected project's detail, shown in the inspector. Each part is read on its own. */
export interface OrchestrationDetailState {
  targetId: string;
  resolution?: OrchestrationResolution;
  resolutionError?: string;
  capacity?: OrchestrationCapacity;
  capacityError?: string;
  admissions: OrchestrationAdmission[];
  /** The tools the policy lets sessions use on this project, asked for admission (HS2-Y3N3PH). */
  admissionTools?: readonly string[];
  admissionError?: string;
  toolCapacity?: OrchestrationToolCapacity;
  toolCapacityError?: string;
  sessions?: OrchestrationSession[];
  sessionsError?: string;
  loading: boolean;
  /** The project's Limits form (HS2-7WF6JD); kept while the person edits it. */
  limits?: TargetLimitsDraft;
  limitsDirty?: boolean;
  limitsSaving?: boolean;
  /** The last Limits failure, shown beside the form until the next change. */
  limitsMessage?: string;
}

export interface OrchestrationSettingsDependencies {
  api: (projectId: string) => OrchestrationApi | undefined;
  openProjects: () => readonly OrchestrationOpenProject[];
  confirm: Confirm;
  /** The regular open-project flow (HS2-BT4KN6). */
  openProjectPicker: () => void;
  /** Whether that flow is still showing a dialog or opening a project. */
  openFlowBusy: () => boolean;
  /** Show the orchestration project's settings again after Open Another Project… opened a tab. */
  activateProject: (projectId: string) => void;
  /** Show a terminal of an open project in its drawer; false when that tab does not show it. */
  showTerminal: (projectId: string, terminalId: string) => boolean;
  /** Whether that open project's drawer can show the terminal now. */
  hasTerminal: (projectId: string, terminalId: string) => boolean;
  /** The AI tools this app knows, for the policy form's tool rows. */
  knownTools: () => readonly string[];
  /** Open the inspector, where a selected project's detail shows. */
  showInspector: () => void;
  /** Run after a microtask, for settling the open flow; tests run it at once. */
  defer?: (run: () => void) => void;
}

const messageOf = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));
/**
 * A detail part's failure in words, by the refusal's code (HS2-Y3N3PH). Adding a project is the
 * consent to read it (HS2-XHQ1EH), so a read is refused only by the policy's limits, an unreachable
 * project, or a person's pending answer; none of them is "not allowed yet".
 */
const REFUSALS: Readonly<Record<string, string>> = {
  policy_denied: 'The policy does not allow it for this project.',
  target_unavailable: 'This project cannot be reached now.',
  permission_required: 'It waits for a person to answer a permission request.',
  permission_denied: 'A person denied it.',
};
const readFailure = (reason: unknown) => {
  if (reason instanceof ApiHttpError && reason.status === 403)
    return REFUSALS[reason.code ?? ''] ?? `It was refused (${reason.code ?? 'forbidden'}).`;
  if (reason instanceof ApiHttpError && reason.status === 502)
    return `Its machine did not answer (${reason.code ?? 'unreachable'}).`;
  return messageOf(reason);
};
const EMPTY: OrchestrationSettingsState = { bindings: {}, loading: false, message: '', status: '', busy: [] };

/**
 * Project Settings → General → Project Type and Settings → Orchestration (HS2-W27N1D). Reads happen
 * when a category shows and when the server announces an `orchestration_changed` event for the
 * project; nothing polls on a timer. Every change goes through the shared orchestration workflows.
 */
export function createOrchestrationSettings(dependencies: OrchestrationSettingsDependencies) {
  const defer = dependencies.defer ?? ((run: () => void) => setTimeout(run, 0));
  const types = signal<Record<string, ProjectTypeState | undefined>>({}),
    settings = signal<Record<string, OrchestrationSettingsState | undefined>>({}),
    selected = signal<Record<string, string | undefined>>({}),
    details = signal<Record<string, OrchestrationDetailState | undefined>>({}),
    remoteLabels = signal<Record<string, string>>({});
  /** Read generations, so an older answer never overwrites a newer one. */
  const generations = new Map<string, number>();
  const next = (key: string) => {
    const value = (generations.get(key) ?? 0) + 1;
    generations.set(key, value);
    return value;
  };
  const current = (key: string, value: number) => generations.get(key) === value;

  const typeOf = (projectId: string): ProjectTypeState =>
    types.value[projectId] ?? { loading: false, saving: false, message: '' };
  const putType = (projectId: string, change: Partial<ProjectTypeState>) => {
    types.value = { ...types.value, [projectId]: { ...typeOf(projectId), ...change } };
  };
  const stateOf = (projectId: string): OrchestrationSettingsState => settings.value[projectId] ?? EMPTY;
  const put = (projectId: string, change: Partial<OrchestrationSettingsState>) => {
    settings.value = { ...settings.value, [projectId]: { ...stateOf(projectId), ...change } };
  };
  const busy = (projectId: string, key: string, on: boolean) => {
    const list = stateOf(projectId).busy.filter((item) => item !== key);
    put(projectId, { busy: on ? [...list, key] : list });
  };

  /** The project's type, read when Settings shows and after any change announcement. */
  async function loadType(projectId: string) {
    const api = dependencies.api(projectId);
    if (!api) return;
    const generation = next(`type:${projectId}`);
    putType(projectId, { loading: !typeOf(projectId).view });
    try {
      const view = await api.projectType(projectId);
      if (current(`type:${projectId}`, generation)) putType(projectId, { view, loading: false, message: '' });
    } catch (reason) {
      if (current(`type:${projectId}`, generation))
        putType(projectId, { loading: false, message: `Could not read the project type. ${messageOf(reason)}` });
    }
  }

  /** Switch the type. Back to Software with projects added asks first and keeps them. */
  async function setType(projectId: string, requested: ProjectType) {
    const api = dependencies.api(projectId),
      state = typeOf(projectId);
    if (!api || state.saving || state.view?.project_type === requested) return;
    const keeping = requested === 'software' ? (state.view?.active_projects ?? 0) : 0;
    if (
      keeping > 0 &&
      !(await dependencies.confirm({
        title: 'Change this project to Software?',
        message: `${keeping === 1 ? 'The 1 added project stays' : `The ${String(keeping)} added projects stay`} in its orchestration settings, and so does the policy. They come back if you change the type to Orchestration again. Sessions already running there keep running until you stop them.`,
        confirmLabel: 'Change to Software',
      }))
    ) {
      // The select already shows the new choice; rendering the stored type puts it back.
      putType(projectId, { view: state.view ? { ...state.view } : undefined });
      return;
    }
    next(`type:${projectId}`);
    putType(projectId, { saving: true, message: '' });
    try {
      const view = await api.setProjectType(projectId, requested, keeping > 0);
      putType(projectId, { view, saving: false });
      if (view.project_type === 'orchestration') void loadSettings(projectId);
    } catch (reason) {
      putType(projectId, { saving: false, message: messageOf(reason) });
    }
  }

  /** The declaration, this machine's bindings and the policy, read when the category shows. */
  async function loadSettings(projectId: string) {
    const api = dependencies.api(projectId);
    if (!api) return;
    const generation = next(`settings:${projectId}`);
    put(projectId, { loading: !stateOf(projectId).manifest });
    try {
      const [manifest, policy, hosted] = await Promise.all([
        api.orchestrationTargets(projectId),
        api.orchestrationPolicy(projectId),
        // HS2-92FJXM: a remote-served project adds from its server's open checkouts.
        isRemoteProjectId(projectId)
          ? api.hostedOrchestrationProjects(projectId).then(
              (choices) => ({ choices, error: undefined }),
              (reason: unknown) => ({ choices: undefined, error: messageOf(reason) }),
            )
          : Promise.resolve(undefined),
      ]);
      const active = manifest.targets.filter((target) => target.state === 'active');
      const bindings = Object.fromEntries(
        await Promise.all(
          active.map(async (target) => {
            try {
              return [target.target_id, await api.orchestrationBinding(projectId, target.target_id)] as const;
            } catch {
              return [target.target_id, undefined] as const;
            }
          }),
        ),
      );
      if (!current(`settings:${projectId}`, generation)) return;
      const previous = stateOf(projectId),
        // A draft the person is editing stays; a clean one follows the stored policy.
        clean = !previous.draft || !previous.draftDirty;
      put(projectId, {
        manifest,
        bindings,
        policy,
        draft: clean ? policyDraftFrom(policy, dependencies.knownTools()) : previous.draft,
        hosted: hosted?.choices ?? (hosted ? previous.hosted : undefined),
        hostedError: hosted?.error,
        loading: false,
        message: previous.message.startsWith('Could not read') ? '' : previous.message,
      });
      const target = selected.value[projectId];
      if (target && active.every((item) => item.target_id !== target)) closeDetail(projectId);
      else if (target) syncLimits(projectId, target);
    } catch (reason) {
      if (current(`settings:${projectId}`, generation))
        put(projectId, { loading: false, message: `Could not read the orchestration settings. ${messageOf(reason)}` });
    }
  }

  /** Remote server names for the rows and the + menu; read once per showing, never polled. */
  async function loadRemoteLabels(read: () => Promise<Record<string, string>>) {
    try {
      remoteLabels.value = { ...remoteLabels.value, ...(await read()) };
    } catch {
      // Rows fall back to "Remote Server"; nothing else depends on the names.
    }
  }

  async function addDestination(projectId: string, destination: AddDestination, label: string) {
    const api = dependencies.api(projectId);
    if (!api || stateOf(projectId).busy.includes('add')) return;
    busy(projectId, 'add', true);
    put(projectId, { message: '', status: '' });
    try {
      const added = await api.addOrchestrationProject(projectId, { destination, label, target_id: newUlid() });
      put(projectId, { status: added.added ? `${added.label} added.` : `${added.label} is already added.` });
      await loadSettings(projectId);
    } catch (reason) {
      put(projectId, { message: `Could not add ${label}. ${messageOf(reason)}` });
    } finally {
      busy(projectId, 'add', false);
    }
  }

  /**
   * HS2-92FJXM: + Add Project → one of the serving machine's open checkouts, for an orchestration
   * project open from another server. That server binds it; a repeat is the same project.
   */
  async function addHostedProject(ownerId: string, checkoutId: string) {
    const api = dependencies.api(ownerId),
      choice = stateOf(ownerId).hosted?.find((item) => item.checkout_id === checkoutId);
    if (!api || !choice || stateOf(ownerId).busy.includes('add')) return;
    busy(ownerId, 'add', true);
    put(ownerId, { message: '', status: '' });
    try {
      const added = await api.addHostedOrchestrationProject(ownerId, {
        checkout_id: checkoutId,
        label: choice.name,
        target_id: newUlid(),
      });
      put(ownerId, { status: added.added ? `${added.label} added.` : `${added.label} is already added.` });
      await loadSettings(ownerId);
    } catch (reason) {
      put(ownerId, { message: `Could not add ${choice.name}. ${messageOf(reason)}` });
    } finally {
      busy(ownerId, 'add', false);
    }
  }

  /** + Add Project → an open project. */
  function addOpenProject(ownerId: string, projectId: string) {
    const open = dependencies.openProjects().find((project) => project.id === projectId);
    if (!open || projectId === ownerId) return Promise.resolve();
    return addDestination(ownerId, destinationOf(projectId), open.name);
  }

  let pendingOpen: { ownerId: string; before: Set<string>; armed: boolean } | undefined;
  /** + Add Project → Open Another Project…: the regular open flow, then the new tab is added. */
  function openAnother(ownerId: string) {
    pendingOpen = {
      ownerId,
      before: new Set(dependencies.openProjects().map((project) => project.id)),
      armed: false,
    };
    dependencies.openProjectPicker();
    noteOpenFlow();
  }
  /**
   * Call when the open flow's dialogs or loading change. Once it was busy and is idle again, the
   * project it opened (if any) is added, and the orchestration project's tab is shown again. A
   * cancelled flow adds nothing. Settled after a microtask, so one dialog closing as the next opens
   * does not read as the end of the flow.
   */
  function noteOpenFlow() {
    const pending = pendingOpen;
    if (!pending) return;
    if (dependencies.openFlowBusy()) {
      pending.armed = true;
      return;
    }
    if (!pending.armed) return;
    defer(() => {
      if (pendingOpen !== pending || dependencies.openFlowBusy()) return;
      pendingOpen = undefined;
      const opened = dependencies.openProjects().find((project) => !pending.before.has(project.id));
      if (!opened || opened.id === pending.ownerId) return;
      dependencies.activateProject(pending.ownerId);
      void addOpenProject(pending.ownerId, opened.id);
    });
  }

  async function removeProject(projectId: string, targetId: string) {
    const api = dependencies.api(projectId),
      label = stateOf(projectId).manifest?.targets.find((target) => target.target_id === targetId)?.label ?? 'project';
    if (!api || stateOf(projectId).busy.includes(targetId)) return;
    if (
      !(await dependencies.confirm({
        title: `Stop orchestrating ${label}?`,
        message:
          'This project stops reading and following it, and plans nothing more there. Sessions already running there keep running until you stop them. You can add it again later.',
        confirmLabel: 'Stop Orchestrating',
        tone: 'danger',
      }))
    )
      return;
    busy(projectId, targetId, true);
    put(projectId, { message: '', status: '' });
    try {
      await api.removeOrchestrationProject(projectId, targetId);
      put(projectId, { status: `${label} removed.` });
      if (selected.value[projectId] === targetId) closeDetail(projectId);
      await loadSettings(projectId);
    } catch (reason) {
      put(projectId, { message: `Could not remove ${label}. ${messageOf(reason)}` });
    } finally {
      busy(projectId, targetId, false);
    }
  }

  const detailKey = (projectId: string, targetId: string) => `${projectId}:${targetId}`;
  const detailOf = (projectId: string) => {
    const target = selected.value[projectId];
    return target ? details.value[detailKey(projectId, target)] : undefined;
  };
  const putDetail = (projectId: string, targetId: string, change: Partial<OrchestrationDetailState>) => {
    const key = detailKey(projectId, targetId);
    details.value = {
      ...details.value,
      [key]: { targetId, admissions: [], loading: false, ...details.value[key], ...change },
    };
  };

  /** The Limits form follows the stored policy unless the person is editing it. */
  function syncLimits(projectId: string, targetId: string) {
    const detail = details.value[detailKey(projectId, targetId)];
    if (detail?.limitsDirty) return;
    putDetail(projectId, targetId, { limits: targetLimitsDraftFrom(stateOf(projectId).policy, targetId) });
  }

  /** A row opens its project's detail in the inspector. */
  function openDetail(projectId: string, targetId: string) {
    selected.value = { ...selected.value, [projectId]: targetId };
    syncLimits(projectId, targetId);
    dependencies.showInspector();
    void loadDetail(projectId, targetId);
  }
  function closeDetail(projectId: string) {
    selected.value = { ...selected.value, [projectId]: undefined };
  }

  /** Each part is read on its own, so one refusal (a read the gate denies) hides nothing else. */
  async function loadDetail(projectId: string, targetId: string) {
    const api = dependencies.api(projectId);
    if (!api) return;
    const generation = next(`detail:${projectId}:${targetId}`),
      fresh = () => current(`detail:${projectId}:${targetId}`, generation),
      policy = stateOf(projectId).policy?.policy,
      namespaces = [
        ...new Set(
          (policy?.placement?.resources ?? [])
            .filter((entry) => entry.target_id === targetId)
            .flatMap((entry) => entry.resources.map((resource) => resource.namespace)),
        ),
      ],
      // HS2-Y3N3PH: only the tools the policy lets sessions use here; the policy's limits refuse
      // admission for any other, which is not something to ask about.
      configured = new Set((policy?.tools ?? []).map((tool) => tool.tool)),
      tools = [
        ...new Set(
          (policy?.targets?.find((entry) => entry.target_id === targetId)?.tools ?? [])
            .map((scope) => scope.tool)
            .filter((tool) => configured.has(tool)),
        ),
      ];
    putDetail(projectId, targetId, { loading: !details.value[detailKey(projectId, targetId)]?.resolution });
    const part = async <T>(
      read: () => Promise<T>,
      done: (value: T) => Partial<OrchestrationDetailState>,
      failed: (message: string) => Partial<OrchestrationDetailState>,
    ) => {
      try {
        const value = await read();
        if (fresh()) putDetail(projectId, targetId, done(value));
      } catch (reason) {
        if (fresh()) putDetail(projectId, targetId, failed(readFailure(reason)));
      }
    };
    // The fresh check first: the reads after it pass the authorization gate, which checks the same
    // binding, and two checks of one binding at once make the older one conflict.
    await part(
      async () => {
        try {
          return await api.orchestrationResolve(projectId, targetId);
        } catch (reason) {
          if (!(reason instanceof ApiHttpError && reason.code === 'conflict')) throw reason;
          return api.orchestrationResolve(projectId, targetId);
        }
      },
      (resolution) => ({ resolution, resolutionError: undefined }),
      (message) => ({ resolutionError: message }),
    );
    await Promise.all([
      part(
        () => api.orchestrationCapacity(projectId, targetId, namespaces),
        (capacity) => ({ capacity, capacityError: undefined }),
        (message) => ({ capacity: undefined, capacityError: message }),
      ),
      part(
        () => Promise.all(tools.map((tool) => api.orchestrationAdmission(projectId, targetId, tool))),
        (admissions) => ({ admissions, admissionTools: tools, admissionError: undefined }),
        (message) => ({ admissions: [], admissionTools: tools, admissionError: message }),
      ),
      part(
        () => api.orchestrationToolCapacity(projectId),
        (toolCapacity) => ({ toolCapacity, toolCapacityError: undefined }),
        (message) => ({ toolCapacityError: message }),
      ),
      part(
        () => api.orchestrationSessions(projectId),
        (sessions) => ({
          sessions: sessions.filter((session) => session.target_id === targetId),
          sessionsError: undefined,
        }),
        (message) => ({ sessionsError: message }),
      ),
    ]);
    if (fresh()) putDetail(projectId, targetId, { loading: false });
  }

  const sessionOf = (projectId: string, sessionId: string) =>
    detailOf(projectId)?.sessions?.find((session) => session.session_id === sessionId);
  /** The open tab whose drawer shows a session, when that tab is open. */
  function sessionTab(projectId: string, session: OrchestrationSession): string | undefined {
    const destination = stateOf(projectId).bindings[session.target_id]?.destination;
    if (!destination || !session.hosted) return undefined;
    const tab = tabProjectIdOf(destination);
    return dependencies.openProjects().some((project) => project.id === tab) &&
      dependencies.hasTerminal(tab, session.hosted.session_id)
      ? tab
      : undefined;
  }

  function showSession(projectId: string, sessionId: string) {
    const session = sessionOf(projectId, sessionId),
      tab = session ? sessionTab(projectId, session) : undefined;
    if (session?.hosted && tab) dependencies.showTerminal(tab, session.hosted.session_id);
  }

  /** Stop at a safe point; mid-turn asks first and then stops anyway. */
  async function stopSession(projectId: string, sessionId: string) {
    const api = dependencies.api(projectId),
      session = sessionOf(projectId, sessionId);
    if (!api || !session || stateOf(projectId).busy.includes(sessionId)) return;
    busy(projectId, sessionId, true);
    put(projectId, { message: '', status: '' });
    try {
      try {
        await api.stopOrchestrationSession(projectId, sessionId, false);
      } catch (reason) {
        if (!(reason instanceof ApiHttpError && reason.code === 'session_not_safe')) throw reason;
        const force = await dependencies.confirm({
          title: 'Stop this session now?',
          message:
            'It is in the middle of work: a turn, a claim, uncommitted changes or a handoff. Stopping now may leave that work unfinished.',
          confirmLabel: 'Stop Now',
          tone: 'danger',
        });
        if (!force) return;
        await api.stopOrchestrationSession(projectId, sessionId, true);
      }
      put(projectId, { status: 'Session stopped.' });
    } catch (reason) {
      put(projectId, { message: `Could not stop the session. ${messageOf(reason)}` });
    } finally {
      busy(projectId, sessionId, false);
      const target = selected.value[projectId];
      if (target) void loadDetail(projectId, target);
    }
  }

  async function takeOverSession(projectId: string, sessionId: string) {
    const api = dependencies.api(projectId);
    if (!api || !sessionOf(projectId, sessionId) || stateOf(projectId).busy.includes(sessionId)) return;
    if (
      !(await dependencies.confirm({
        title: 'Take over this session?',
        message: 'It becomes yours: the orchestrator gives it no new work and never stops it.',
        confirmLabel: 'Take Over',
      }))
    )
      return;
    busy(projectId, sessionId, true);
    put(projectId, { message: '', status: '' });
    try {
      await api.takeOverOrchestrationSession(projectId, sessionId);
      put(projectId, { status: 'Session taken over.' });
    } catch (reason) {
      put(projectId, { message: `Could not take over the session. ${messageOf(reason)}` });
    } finally {
      busy(projectId, sessionId, false);
      const target = selected.value[projectId];
      if (target) void loadDetail(projectId, target);
    }
  }

  function changePolicy(projectId: string, field: string, value: string | boolean) {
    const state = stateOf(projectId);
    if (!state.draft) return;
    put(projectId, { draft: changePolicyDraft(state.draft, field, value), draftDirty: true, message: '', status: '' });
  }

  /** + Add Account and an account row's remove button on a tool of the policy form (HS2-7WF6JD). */
  function addAccount(projectId: string, tool: string) {
    const state = stateOf(projectId);
    if (!state.draft) return;
    put(projectId, { draft: addPolicyAccount(state.draft, tool), draftDirty: true, message: '', status: '' });
  }
  function removeAccount(projectId: string, tool: string, index: number) {
    const state = stateOf(projectId);
    if (!state.draft) return;
    put(projectId, { draft: removePolicyAccount(state.draft, tool, index), draftDirty: true, message: '', status: '' });
  }

  /** One control of the selected project's Limits form. */
  function changeLimits(projectId: string, field: string, value: string | boolean, checked?: boolean) {
    const target = selected.value[projectId],
      limits = target ? details.value[detailKey(projectId, target)]?.limits : undefined;
    if (!target || !limits) return;
    putDetail(projectId, target, {
      limits: changeTargetLimits(limits, field, value, checked),
      limitsDirty: true,
      limitsMessage: undefined,
    });
  }

  /** Apply Limits: store the policy with only this project's entry replaced, at the stored revision. */
  async function applyLimits(projectId: string) {
    const api = dependencies.api(projectId),
      target = selected.value[projectId],
      detail = target ? details.value[detailKey(projectId, target)] : undefined,
      state = stateOf(projectId);
    if (!api || !target || !detail?.limits || detail.limitsSaving) return;
    const built = policyWithTargetLimits(state.policy, detail.limits);
    if ('error' in built) {
      putDetail(projectId, target, { limitsMessage: built.error });
      return;
    }
    putDetail(projectId, target, { limitsSaving: true, limitsMessage: undefined });
    try {
      const policy = await api.setOrchestrationPolicy(projectId, state.policy?.revision ?? null, built.policy);
      const label = state.manifest?.targets.find((item) => item.target_id === target)?.label ?? 'project';
      put(projectId, {
        policy,
        // A clean policy draft follows the stored policy; one being edited stays.
        draft: state.draftDirty ? state.draft : policyDraftFrom(policy, dependencies.knownTools()),
        status: `Limits for ${label} saved.`,
        message: '',
      });
      putDetail(projectId, target, {
        limits: targetLimitsDraftFrom(policy, target),
        limitsDirty: false,
        limitsSaving: false,
      });
      void loadDetail(projectId, target);
    } catch (reason) {
      putDetail(projectId, target, {
        limitsSaving: false,
        limitsMessage: `Could not save the limits. ${messageOf(reason)}`,
      });
    }
  }

  async function applyPolicy(projectId: string) {
    const api = dependencies.api(projectId),
      state = stateOf(projectId);
    if (!api || !state.draft || state.busy.includes('policy')) return;
    const built = policyFromDraft(state.policy, state.draft);
    if ('error' in built) {
      put(projectId, { message: built.error, status: '' });
      return;
    }
    busy(projectId, 'policy', true);
    put(projectId, { message: '', status: '' });
    try {
      const policy = await api.setOrchestrationPolicy(projectId, state.policy?.revision ?? null, built.policy);
      put(projectId, {
        policy,
        draft: policyDraftFrom(policy, dependencies.knownTools()),
        draftDirty: false,
        status: 'Policy saved.',
      });
      const target = selected.value[projectId];
      if (target) syncLimits(projectId, target);
    } catch (reason) {
      put(projectId, { message: `Could not save the policy. ${messageOf(reason)}` });
    } finally {
      busy(projectId, 'policy', false);
    }
  }

  /** An `orchestration_changed` event (or a resync) for a project: read what shows again. */
  function changed(projectId: string, showing: { type: boolean; orchestration: boolean }) {
    if (showing.type || showing.orchestration) void loadType(projectId);
    if (!showing.orchestration) return;
    void loadSettings(projectId);
    const target = selected.value[projectId];
    if (target) void loadDetail(projectId, target);
  }

  return {
    types,
    settings,
    selected,
    details,
    remoteLabels,
    loadType,
    setType,
    loadSettings,
    loadRemoteLabels,
    addOpenProject,
    addHostedProject,
    openAnother,
    noteOpenFlow,
    removeProject,
    openDetail,
    closeDetail,
    loadDetail,
    detailOf,
    sessionTab,
    showSession,
    stopSession,
    takeOverSession,
    changePolicy,
    applyPolicy,
    addAccount,
    removeAccount,
    changeLimits,
    applyLimits,
    changed,
  };
}

export type OrchestrationSettings = ReturnType<typeof createOrchestrationSettings>;
