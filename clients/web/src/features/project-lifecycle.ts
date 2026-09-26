import { type Signal, signal } from 'kerfjs';

import { Api, type Capabilities, type Checkout, type CustomView, type ProviderConnection } from '../api';
import { isRemoteClient } from '../client-origin';
import { type ProjectRestoreFailure, rememberedProjectName } from '../components/project-restore-error';
import { type ExternalProviderKind, type GithubAuthState } from '../components/provider-setup-form';
import { type Control, type Project, type UnhealthyServerRecovery } from '../interactions/types';
import { type MigrationJobClient, MigrationJobClient as MigrationJobs } from '../migration-job-client';
import { type MigrationJob } from '../migration-progress';
import type { PermissionAutomation } from '../permission-notifications';
import { openProjectFetch, type ProjectOpenResult } from '../project-startup';
import { insertTabByRank, replaceTabInPlace } from '../tab-order';
import { customTicketViewKey, type TicketView } from '../ticket-views';
import { dismissHs1CleanupPrompt, hs1MigrationPromptDismissed, saveActiveProjectRoot } from '../workspace-session';

interface DefaultProvider {
  name: string;
  capabilities: Capabilities;
}

interface ProjectActivation {
  project: Project;
}

export interface ProjectLifecycleDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  selectedView: Signal<TicketView>;
  loading: Signal<boolean>;
  error: Signal<string>;
  hideVerifiedByProject: Signal<Record<string, boolean>>;
  providerCapabilities: Signal<Record<string, Capabilities>>;
  defaultProviders: Signal<Record<string, DefaultProvider | undefined>>;
  terminalDrawerSelected: Signal<string>;
  terminalDrawerVisible: Signal<boolean>;
  project: () => Project | undefined;
  rememberedProjectRoots: () => string[];
  activateOpenProject: (projectId: string) => ProjectActivation | undefined;
  loadPermissionAutomation: (projectId: string) => PermissionAutomation;
  setPermissionAutomation: (projectId: string, automation: PermissionAutomation) => void;
  startPermissionUpdates: () => void;
  syncProjectChangeStreams: () => void;
  refreshProject: () => Promise<unknown>;
  refreshCommands: (project: Project) => Promise<unknown>;
  refreshCustomViews: (project: Project) => Promise<unknown>;
  refreshDriveConnections: (project: Project, restoreDrawerTabs: boolean) => Promise<unknown>;
  refreshTerminalDashboard: () => Promise<unknown>;
  restoreProjectSession: (project: Project) => Promise<unknown>;
  customViewFor: (view: TicketView, projectId: string) => CustomView | undefined;
  applyCustomViewQuery: (view: CustomView) => void;
  observeTerminalDrawer: () => void;
  requestProjectRefresh: (project: Project) => Promise<unknown>;
  showToast: (message: string) => void;
}

/** Own project opening, restoration, migration, and ticket-provider setup state and actions. */
export function createProjectLifecycleController(dependencies: ProjectLifecycleDependencies) {
  const {
    projects,
    selectedProjectId,
    selectedView,
    loading,
    error,
    hideVerifiedByProject,
    providerCapabilities,
    defaultProviders,
    terminalDrawerSelected,
    terminalDrawerVisible,
  } = dependencies;
  const projectDialogOpen = signal(false),
    projectDialogError = signal(''),
    remoteProjectDialogOpen = signal(false),
    remoteProjectCheckouts = signal<Checkout[]>([]),
    remoteProjectLoading = signal(false),
    remoteProjectError = signal(''),
    unhealthyServerRecovery = signal<UnhealthyServerRecovery | undefined>(undefined),
    unhealthyServerRecoveryBusy = signal(false),
    hs1MigrationProject = signal<Project | undefined>(undefined),
    hs1MigrationBusy = signal(false),
    hs1MigrationError = signal(''),
    migrationJobsByRoot = signal<Partial<Record<string, MigrationJob>>>({}),
    migrationConnectionErrors = signal<Record<string, string>>({}),
    migrationJobDetails = signal<Record<string, boolean>>({}),
    ticketSourceSetupProject = signal<Project | undefined>(undefined),
    ticketSourceSetupError = signal(''),
    providerConnections = signal<ProviderConnection[]>([]),
    providerSetupKind = signal<ExternalProviderKind | undefined>(undefined),
    providerEditingId = signal<string | undefined>(undefined),
    providerSettingsBusy = signal(false),
    providerSettingsError = signal(''),
    githubAuth = signal<GithubAuthState | undefined>(undefined),
    ticketSourceSetupNavigation = signal<'none' | 'push' | 'pop'>('none'),
    createdGitTicketStore = signal(''),
    ticketSourceRemoteBusy = signal(false),
    ticketSourceRemoteError = signal(''),
    projectRestoreFailures = signal<ProjectRestoreFailure[]>([]),
    selectedProjectRestoreRoot = signal('');
  const projectsPendingActivation = new Set<string>();
  /** Remembered startup position of each restored project, for out-of-order registration. */
  const restoreRanks = new Map<string, number>();

  const migrationJobs: MigrationJobClient = new MigrationJobs(
    (job) => {
      migrationJobsByRoot.value = { ...migrationJobsByRoot.value, [job.root]: job };
      migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [job.root]: '' };
      if (job.status === 'running')
        projects.value = projects.value.map((item) =>
          item.root === job.root ? { ...item, hs1CleanupEligible: false } : item,
        );
    },
    async (job) => {
      const target = projects.value.find((item) => item.root === job.root);
      if (!target) return;
      const response = await fetch('/__hotsheet/projects/open', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ root: target.root }),
      });
      const current = (await response.json()) as Project & { error?: string };
      if (!response.ok) throw new Error(current.error ?? 'Could not refresh the completed migration.');
      if (migrationJobsByRoot.value[job.root]?.attempt !== job.attempt) return;
      migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [job.root]: '' };
      projects.value = projects.value.map((item) => (item.root === target.root ? { ...item, ...current } : item));
      if (job.kind === 'import') {
        const descriptors = await new Api(target.apiPath).providers(),
          selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
        if (migrationJobsByRoot.value[job.root]?.attempt !== job.attempt) return;
        defaultProviders.value = {
          ...defaultProviders.value,
          [target.id]: selected ? { name: selected.display_name, capabilities: selected.capabilities } : undefined,
        };
        providerCapabilities.value = {
          ...providerCapabilities.value,
          ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
        };
        await dependencies.requestProjectRefresh(target);
      }
    },
    (root, message) => {
      migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [root]: message };
    },
  );

  function hs1SourceIdentity(value: Project) {
    return [value.hs1DatabasePath, value.hs1PostgresVersion].filter(Boolean).join('\u0000');
  }

  function retainProjectRestoreFailure(root: string, message: string, recoveryPid?: number, busy = false) {
    const failure: ProjectRestoreFailure = {
      root,
      name: rememberedProjectName(root),
      error: message,
      recoveryPid,
      busy,
    };
    projectRestoreFailures.value = [...projectRestoreFailures.value.filter((item) => item.root !== root), failure];
  }

  async function wireOpenedProject(
    root: string,
    opened: Extract<ProjectOpenResult, { ok: true }>,
    restoreIndex?: number,
  ) {
    const { project: value, providers: descriptors } = opened;
    projectRestoreFailures.value = projectRestoreFailures.value.filter((item) => item.root !== root);
    if (selectedProjectRestoreRoot.value === root) selectedProjectRestoreRoot.value = '';
    if (restoreIndex === undefined) projects.value = replaceTabInPlace(projects.value, (item) => item.id, value);
    else {
      // Startup registers the active project first; keep every tab in its remembered position.
      if (!restoreRanks.has(value.id)) restoreRanks.set(value.id, restoreIndex);
      projects.value = insertTabByRank(
        projects.value,
        (item) => item.id,
        value,
        restoreRanks.get(value.id)!,
        (item) => restoreRanks.get(item.id),
      );
    }
    hideVerifiedByProject.value = {
      ...hideVerifiedByProject.value,
      [value.id]: localStorage.getItem(`hotsheet.project.${value.id}.hide-verified-column`) === 'true',
    };
    dependencies.setPermissionAutomation(value.id, dependencies.loadPermissionAutomation(value.id));
    const selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
    defaultProviders.value = {
      ...defaultProviders.value,
      [value.id]: selected ? { name: selected.display_name, capabilities: selected.capabilities } : undefined,
    };
    providerCapabilities.value = {
      ...providerCapabilities.value,
      ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
    };
    projectsPendingActivation.add(value.id);
    await migrationJobs
      .join(value.root)
      .then((job) => {
        if (!job)
          migrationJobsByRoot.value = Object.fromEntries(
            Object.entries(migrationJobsByRoot.value).filter(([candidate]) => candidate !== value.root),
          );
      })
      .catch(() => undefined);
  }

  function presentOpenedProjectSetup(value: Project) {
    projectsPendingActivation.delete(value.id);
    const migrationTarget =
        value.needsHs1Migration &&
        !migrationJobsByRoot.value[value.root] &&
        !hs1MigrationPromptDismissed(localStorage, value.id, hs1SourceIdentity(value))
          ? value
          : undefined,
      setupTarget = value.needsTicketSetup && !value.needsHs1Migration ? value : undefined,
      openingDialog = document.querySelector<Control>('[data-project-dialog]'),
      waitForProjectDialog = Boolean((migrationTarget || setupTarget) && openingDialog?.open);
    ticketSourceSetupNavigation.value = 'none';
    createdGitTicketStore.value = '';
    ticketSourceRemoteError.value = '';
    hs1MigrationError.value = '';
    const presentSetup = () => {
      if (dependencies.project()?.id !== value.id) return;
      hs1MigrationProject.value = migrationTarget;
      ticketSourceSetupProject.value = setupTarget;
    };
    if (waitForProjectDialog) openingDialog!.addEventListener('wa-after-hide', presentSetup, { once: true });
    projectDialogOpen.value = false;
    if (!waitForProjectDialog) presentSetup();
    else {
      hs1MigrationProject.value = undefined;
      ticketSourceSetupProject.value = undefined;
    }
    ticketSourceSetupError.value = '';
  }

  async function activateOpenedProject(value: Project) {
    if (selectedProjectId.value !== value.id) {
      if (!dependencies.activateOpenProject(value.id)) throw new Error('Could not activate the opened project.');
    } else {
      saveActiveProjectRoot(localStorage, value.root);
      terminalDrawerSelected.value =
        localStorage.getItem(`hotsheet.project.${value.id}.terminal-drawer-selection`) || 'grid';
    }
    presentOpenedProjectSetup(value);
    await Promise.all([
      dependencies.refreshProject(),
      dependencies.refreshCommands(value),
      dependencies.refreshCustomViews(value),
      dependencies.refreshDriveConnections(value, true),
      ...(terminalDrawerVisible.value ? [dependencies.refreshTerminalDashboard()] : []),
    ]);
    await dependencies.restoreProjectSession(value);
    const restored = dependencies.customViewFor(selectedView.value, value.id);
    if (restored) dependencies.applyCustomViewQuery(restored);
    else if (customTicketViewKey(selectedView.value)) selectedView.value = 'all';
    if (terminalDrawerVisible.value) dependencies.observeTerminalDrawer();
  }

  async function openProject(
    root: string,
    ticketStore?: string,
    remember = true,
    reportError = true,
    retainFailure = false,
  ) {
    loading.value = true;
    unhealthyServerRecovery.value = undefined;
    if (reportError) error.value = '';
    try {
      const opened = await openProjectFetch(root, ticketStore);
      if (!opened.ok) {
        unhealthyServerRecovery.value = opened.recovery;
        throw new Error(opened.error);
      }
      await wireOpenedProject(root, opened);
      if (remember)
        localStorage.setItem('hotsheet.open-projects', JSON.stringify(dependencies.rememberedProjectRoots()));
      dependencies.startPermissionUpdates();
      dependencies.syncProjectChangeStreams();
      await activateOpenedProject(opened.project);
      return true;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      if (retainFailure) retainProjectRestoreFailure(root, message, unhealthyServerRecovery.value?.expected.pid);
      if (reportError) error.value = message;
      return false;
    } finally {
      loading.value = false;
    }
  }

  async function retryProjectRestore(root: string) {
    const failure = projectRestoreFailures.value.find((item) => item.root === root);
    if (!failure || failure.busy) return;
    retainProjectRestoreFailure(root, failure.error, failure.recoveryPid, true);
    await openProject(root, undefined, false, false, true);
  }

  async function recoverUnhealthyProjectServer() {
    const recovery = unhealthyServerRecovery.value,
      form = document.querySelector<HTMLFormElement>('[data-action="open-project-form"]');
    if (!recovery || !form || unhealthyServerRecoveryBusy.value) return;
    unhealthyServerRecoveryBusy.value = true;
    projectDialogError.value = '';
    try {
      const response = await fetch('/__hotsheet/server/recover-unhealthy', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(recovery),
        }),
        result = (await response.json()) as { recovered?: boolean; error?: string };
      if (!response.ok || !result.recovered)
        throw new Error(result.error ?? 'Could not recover the unresponsive server.');
      const root = (form.querySelector('[name="project-root"]') as Control).value,
        store = (form.querySelector('[name="ticket-store"]') as Control).value;
      await openProject(root, store || undefined);
    } catch (reason) {
      projectDialogError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      unhealthyServerRecoveryBusy.value = false;
    }
  }

  async function chooseHs1TicketStore() {
    const choice = await fetch('/__hotsheet/folders/choose', { method: 'POST' }),
      chosen = (await choice.json()) as { path?: string; error?: string };
    if (!choice.ok) {
      hs1MigrationError.value = chosen.error ?? 'Could not choose a ticket repository folder.';
      return;
    }
    const input = document.querySelector<Control>('[name="hs1-ticket-store"]');
    if (input && chosen.path) input.value = chosen.path;
  }

  async function importHs1Project(form: HTMLFormElement) {
    const target = hs1MigrationProject.value,
      location = form.querySelector<Control>('[name="hs1-ticket-store"]')?.value.trim();
    if (!target || !location || hs1MigrationBusy.value) return;
    hs1MigrationBusy.value = true;
    hs1MigrationError.value = '';
    try {
      await migrationJobs.start({ projectId: target.id, root: target.root, location, kind: 'import' });
      if (hs1MigrationProject.value?.id === target.id) hs1MigrationProject.value = undefined;
    } catch (reason) {
      hs1MigrationError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      hs1MigrationBusy.value = false;
    }
  }

  async function removeOldHs1Data() {
    const current = dependencies.project();
    if (
      !current ||
      !current.hs1CleanupEligible ||
      !window.confirm('Delete the old Hot Sheet 1 files from this project? Backups will be kept.')
    )
      return;
    try {
      const response = await fetch(`/__hotsheet/projects/${encodeURIComponent(current.id)}/hs1-data`, {
          method: 'DELETE',
        }),
        result = (await response.json()) as { removed?: string[]; error?: string };
      if (!response.ok) throw new Error(result.error ?? 'Could not remove the old Hot Sheet 1 files.');
      dismissHs1CleanupPrompt(localStorage, current.id, hs1SourceIdentity(current));
      projects.value = projects.value.map((item) =>
        item.id === current.id
          ? {
              ...item,
              hs1CleanupEligible: false,
              needsHs1Migration: false,
              hs1DatabasePath: undefined,
              hs1SourcePath: undefined,
              hs1PostgresVersion: undefined,
            }
          : item,
      );
      dependencies.showToast(
        `Removed ${result.removed?.length ?? 0} old Hot Sheet 1 item${result.removed?.length === 1 ? '' : 's'}; backups were kept.`,
      );
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function createProjectGitSource(custom = false) {
    const target = ticketSourceSetupProject.value;
    if (!target) return;
    ticketSourceSetupError.value = '';
    loading.value = true;
    try {
      let location: string | undefined;
      if (custom) {
        const choice = await fetch('/__hotsheet/folders/choose', { method: 'POST' }),
          chosen = (await choice.json()) as { path?: string; error?: string };
        if (!choice.ok) throw new Error(chosen.error ?? 'Could not choose a ticket repository folder.');
        if (!chosen.path) return;
        location = chosen.path;
      }
      const response = await fetch('/__hotsheet/projects/setup-git', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ root: target.root, location }),
        }),
        result = (await response.json()) as { ticketStore?: string; connectionId?: string; error?: string };
      if (!response.ok || !result.ticketStore || !result.connectionId)
        throw new Error(result.error ?? 'Could not create the git ticket store.');
      const client = new Api(target.apiPath),
        checkout = await client.addCheckoutSource(
          target.id,
          {
            id: result.connectionId,
            provider: 'git',
            locator: result.ticketStore,
            name: 'Hot Sheet git',
            default: Boolean(target.needsTicketSetup),
            settings: {},
          },
          Boolean(target.needsTicketSetup),
        ),
        updated = { ...target, stores: checkout.stores, needsTicketSetup: false };
      projects.value = projects.value.map((item) => (item.id === target.id ? updated : item));
      ticketSourceSetupProject.value = updated;
      createdGitTicketStore.value = result.ticketStore;
      ticketSourceSetupNavigation.value = 'push';
      requestAnimationFrame(() => document.querySelector<HTMLElement>('[name="ticket-store-remote"]')?.focus());
      const descriptors = await client.providers(),
        selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
      defaultProviders.value = {
        ...defaultProviders.value,
        [target.id]: selected ? { name: selected.display_name, capabilities: selected.capabilities } : undefined,
      };
      providerCapabilities.value = {
        ...providerCapabilities.value,
        ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
      };
      await dependencies.refreshProject();
    } catch (reason) {
      ticketSourceSetupError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      loading.value = false;
    }
  }

  async function connectCreatedGitRemote(form: HTMLFormElement) {
    const target = ticketSourceSetupProject.value,
      store = createdGitTicketStore.value,
      remote = form.querySelector<Control>('[name="ticket-store-remote"]')?.value.trim();
    if (!target || !store || !remote || ticketSourceRemoteBusy.value) return;
    ticketSourceRemoteBusy.value = true;
    ticketSourceRemoteError.value = '';
    try {
      if (target.hs1ImportCompleted) {
        await migrationJobs.start({ projectId: target.id, root: target.root, location: store, kind: 'backup', remote });
        if (ticketSourceSetupProject.value?.id === target.id) {
          ticketSourceSetupProject.value = undefined;
          createdGitTicketStore.value = '';
        }
        return;
      }
      const response = await fetch('/__hotsheet/projects/setup-git-remote', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ store, remote }),
        }),
        result = (await response.json()) as { connected?: boolean; error?: string };
      if (!response.ok || !result.connected) throw new Error(result.error ?? 'Could not connect the Git remote.');
      dependencies.showToast('Ticket repository connected and backed up.');
      ticketSourceSetupProject.value = undefined;
      createdGitTicketStore.value = '';
    } catch (reason) {
      ticketSourceRemoteError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      ticketSourceRemoteBusy.value = false;
    }
  }

  async function refreshProviderConnections(current = dependencies.project(), quiet = false) {
    if (!current) return;
    try {
      const connections = await new Api(current.apiPath, '', { trackBusy: !quiet }).connections();
      if (dependencies.project()?.id !== current.id) return;
      providerConnections.value = connections;
      providerSettingsError.value = '';
    } catch (reason) {
      if (dependencies.project()?.id === current.id)
        providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function saveExternalProvider(form: HTMLFormElement) {
    const current = ticketSourceSetupProject.value ?? dependencies.project(),
      kind = providerSetupKind.value,
      editingId = providerEditingId.value;
    if (!current || !kind || providerSettingsBusy.value) return;
    const values = new FormData(form),
      read = (field: string) => {
        const value = values.get(field);
        return typeof value === 'string' ? value.trim() : '';
      },
      id = editingId ?? read('connection-id'),
      name = read('connection-name'),
      locator = read('connection-locator'),
      credential =
        kind === 'github'
          ? (githubAuth.value?.credential ?? read('credential-reference'))
          : read('credential-reference'),
      makeDefault = values.get('make-default') === 'on';
    if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
      providerSettingsError.value = 'Connection ID must use lowercase letters, numbers, and hyphens.';
      return;
    }
    if (!locator || (kind !== 'jira' && !locator.includes('/'))) {
      providerSettingsError.value =
        kind === 'jira' ? 'Enter the Jira project key.' : 'Enter a namespace/repository path.';
      return;
    }
    if (!credential) {
      providerSettingsError.value = 'Enter an existing OS-keychain credential reference.';
      return;
    }
    const settings: Record<string, unknown> = { credential: { secret: credential } },
      apiBase = read('api-base'),
      email = read('jira-email');
    if (apiBase) settings[kind === 'jira' ? 'base_url' : 'api_base'] = apiBase;
    if (kind === 'jira') {
      if (!email || !apiBase) {
        providerSettingsError.value = 'Jira requires the account email and site URL.';
        return;
      }
      settings.email = email;
    }
    const connection: ProviderConnection = {
      id,
      provider: kind,
      locator,
      name: name || null,
      default: makeDefault,
      settings,
    };
    providerSettingsBusy.value = true;
    providerSettingsError.value = '';
    try {
      const client = new Api(current.apiPath),
        saved = editingId
          ? await client.updateConnection(editingId, connection)
          : await client.createConnection(connection);
      await client.addCheckoutSource(current.id, saved, makeDefault);
      if (editingId && !makeDefault && providerConnections.value.find((item) => item.id === editingId)?.default)
        await client.setCheckoutDefaultSource(current.id, null);
      providerConnections.value = await client.connections();
      const descriptors = await client.providers(),
        selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
      defaultProviders.value = {
        ...defaultProviders.value,
        [current.id]: selected ? { name: selected.display_name, capabilities: selected.capabilities } : undefined,
      };
      providerCapabilities.value = {
        ...providerCapabilities.value,
        ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
      };
      projects.value = projects.value.map((item) =>
        item.id === current.id ? { ...item, needsTicketSetup: false } : item,
      );
      ticketSourceSetupProject.value = undefined;
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      await dependencies.refreshProject();
      dependencies.showToast(editingId ? `${saved.name ?? saved.id} updated.` : `${saved.name ?? saved.id} connected.`);
    } catch (reason) {
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      providerSettingsBusy.value = false;
    }
  }

  function githubWebBase(apiBase: string) {
    const normalized = apiBase.trim().replace(/\/$/, '');
    if (!normalized || normalized === 'https://api.github.com') return 'https://github.com';
    return normalized.replace(/\/api\/v3$/, '');
  }

  async function startGitHubSignIn(form: HTMLFormElement) {
    const target = ticketSourceSetupProject.value;
    if (!target || githubAuth.value?.state === 'waiting') return;
    providerSettingsError.value = '';
    try {
      const apiBase = form.querySelector<Control>('[name="api-base"]')?.value ?? '',
        client = new Api(target.apiPath),
        started = await client.startGitHubAuth(githubWebBase(apiBase));
      githubAuth.value = {
        session: started.session_id,
        userCode: started.user_code,
        verificationUri: started.verification_uri,
        state: 'waiting',
      };
      const result = await client.waitGitHubAuth(started.session_id);
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Async cancellation may replace the live auth signal while this request is pending.
      if (githubAuth.value?.session !== started.session_id || result.state === 'pending') return;
      if (result.state === 'authorized') {
        githubAuth.value = { ...githubAuth.value, state: 'authorized', credential: result.credential_reference };
        try {
          const listed = await client.githubAuthRepositories(started.session_id);
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Async cancellation may replace the live auth signal while this request is pending.
          if (githubAuth.value?.session === started.session_id)
            githubAuth.value = { ...githubAuth.value, repositories: listed.repositories };
        } catch (reason) {
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Async cancellation may replace the live auth signal while this request is pending.
          if (githubAuth.value?.session === started.session_id)
            githubAuth.value = {
              ...githubAuth.value,
              message: reason instanceof Error ? reason.message : String(reason),
            };
        }
      } else {
        githubAuth.value = {
          ...githubAuth.value,
          state: result.state,
          message: result.state === 'error' ? result.message : undefined,
        };
      }
    } catch (reason) {
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function cancelGitHubSignIn() {
    const target = ticketSourceSetupProject.value,
      current = githubAuth.value;
    if (target && current?.state === 'waiting') void new Api(target.apiPath).cancelGitHubAuth(current.session);
    if (current) githubAuth.value = { ...current, state: 'cancelled' };
  }

  async function chooseProjectPath(button: Element) {
    projectDialogError.value = '';
    try {
      const input = button.closest('.project-dialog__path')?.querySelector<Control>('wa-input');
      if (!input) throw new Error('Could not find the project path field.');
      const endpoint = new URL('/__hotsheet/folders/choose', window.location.href),
        response = await fetch(endpoint, { method: 'POST' }),
        text = await response.text();
      let result: { path?: string; error?: string } = {};
      try {
        result = text ? (JSON.parse(text) as typeof result) : {};
      } catch {
        throw new Error(`The folder chooser returned an invalid response (${response.status}).`);
      }
      if (!response.ok) throw new Error(result.error ?? 'Could not open the folder chooser.');
      if (!result.path) return;
      input.value = result.path;
      input.focus();
    } catch (reason) {
      projectDialogError.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function chooseAndOpenProject() {
    error.value = '';
    projectDialogError.value = '';
    try {
      const response = await fetch(new URL('/__hotsheet/folders/choose', window.location.href), { method: 'POST' }),
        text = await response.text();
      let result: { path?: string; error?: string } = {};
      try {
        result = text ? (JSON.parse(text) as typeof result) : {};
      } catch {
        throw new Error(`The folder chooser returned an invalid response (${response.status}).`);
      }
      if (!response.ok) throw new Error(result.error ?? 'Could not open the folder chooser.');
      if (result.path) await openProject(result.path);
    } catch (reason) {
      projectDialogError.value = reason instanceof Error ? reason.message : String(reason);
      projectDialogOpen.value = true;
    }
  }

  function openProjectPicker() {
    error.value = '';
    projectDialogError.value = '';
    unhealthyServerRecovery.value = undefined;
    if (isRemoteClient()) void openRemoteProjectDialog();
    else projectDialogOpen.value = true;
  }

  async function openRemoteProjectDialog() {
    remoteProjectError.value = '';
    remoteProjectLoading.value = true;
    remoteProjectDialogOpen.value = true;
    try {
      const response = await fetch('/__hotsheet/checkouts');
      if (!response.ok) throw new Error(`the server responded with ${response.status}`);
      remoteProjectCheckouts.value = (await response.json()) as Checkout[];
    } catch (reason) {
      console.error('Could not load the server open-projects list', reason);
      remoteProjectError.value =
        'Could not load the projects open on the Hot Sheet server. Check the connection to the server and try again.';
    } finally {
      remoteProjectLoading.value = false;
    }
  }

  async function openRemoteCheckout(root: string) {
    remoteProjectDialogOpen.value = false;
    await openProject(root);
  }

  return {
    projectDialogOpen,
    projectDialogError,
    remoteProjectDialogOpen,
    remoteProjectCheckouts,
    remoteProjectLoading,
    remoteProjectError,
    unhealthyServerRecovery,
    unhealthyServerRecoveryBusy,
    hs1MigrationProject,
    hs1MigrationBusy,
    hs1MigrationError,
    migrationJobsByRoot,
    migrationConnectionErrors,
    migrationJobDetails,
    migrationJobs,
    ticketSourceSetupProject,
    ticketSourceSetupError,
    providerConnections,
    providerSetupKind,
    providerEditingId,
    providerSettingsBusy,
    providerSettingsError,
    githubAuth,
    ticketSourceSetupNavigation,
    createdGitTicketStore,
    ticketSourceRemoteBusy,
    ticketSourceRemoteError,
    projectRestoreFailures,
    selectedProjectRestoreRoot,
    projectsPendingActivation,
    /** A restored project's remembered startup position, when it was registered by startup restore. */
    projectRestoreRank: (id: string) => restoreRanks.get(id),
    hs1SourceIdentity,
    retainProjectRestoreFailure,
    wireOpenedProject,
    presentOpenedProjectSetup,
    activateOpenedProject,
    openProject,
    retryProjectRestore,
    recoverUnhealthyProjectServer,
    chooseHs1TicketStore,
    importHs1Project,
    removeOldHs1Data,
    createProjectGitSource,
    connectCreatedGitRemote,
    refreshProviderConnections,
    saveExternalProvider,
    startGitHubSignIn,
    cancelGitHubSignIn,
    chooseProjectPath,
    chooseAndOpenProject,
    openProjectPicker,
    openRemoteProjectDialog,
    openRemoteCheckout,
  };
}
