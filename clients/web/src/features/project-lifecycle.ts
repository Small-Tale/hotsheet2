import { type Signal, signal } from 'kerfjs';

import {
  Api,
  type Capabilities,
  type Checkout,
  type CustomView,
  type ProviderAccount,
  type ProviderConnection,
  type ProviderDescriptor,
} from '../api';
import { isRemoteClient } from '../client-origin';
import { copyWhenReady } from '../clipboard-when-ready';
import { type ProjectRestoreFailure, rememberedProjectName } from '../components/project-restore-error';
import { type ExternalProviderKind, type GithubAuthState, providerName } from '../components/provider-setup-form';
import { carryGithubAttachmentSettings } from '../github-attachment-settings';
import { type Control, type Project, type UnhealthyServerRecovery } from '../interactions/types';
import { type MigrationJobClient, MigrationJobClient as MigrationJobs } from '../migration-job-client';
import { type MigrationJob } from '../migration-progress';
import { type ProjectTicketSources, projectTicketSources } from '../new-ticket-source';
import type { PermissionAutomation } from '../permission-notifications';
import { openedProjectYieldsToSelection } from '../project-activation';
import { openProjectFetch, type ProjectOpenResult } from '../project-startup';
import { insertTabByRank, replaceTabInPlace } from '../tab-order';
import { customTicketViewKey, type TicketView } from '../ticket-views';
import { dismissHs1CleanupPrompt, hs1MigrationPromptDismissed, saveActiveProjectRoot } from '../workspace-session';

type DefaultProvider = ProjectTicketSources;

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
  /** Bumped on every explicit project-tab selection, so an in-flight open can yield to it (HS2-YVBGW3). */
  projectSelectionGeneration?: () => number;
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
  restoreCustomView: (view: CustomView) => void;
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
    /** Machine-wide provider sign-ins and the projects using each (HS2-SM9PM8). */
    providerAccounts = signal<ProviderAccount[]>([]),
    providerAccountsError = signal(''),
    /** The account whose sign-out is in flight. */
    signingOutAccount = signal<string | undefined>(undefined),
    /** An unlinked source selected for removal from App Settings → Accounts. */
    unusedAccountSourceChoice = signal<string | undefined>(undefined),
    removingUnusedAccountSource = signal<string | undefined>(undefined),
    /** The GitLab or Jira account whose details prefill a new source's form (HS2-F5HNJN). */
    providerAccountChoice = signal<string | undefined>(undefined),
    providerSettingsBusy = signal(false),
    providerSettingsError = signal(''),
    providerRemovingId = signal<string | undefined>(undefined),
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
        const descriptors = await new Api(target.apiPath).providers();
        if (migrationJobsByRoot.value[job.root]?.attempt !== job.attempt) return;
        defaultProviders.value = {
          ...defaultProviders.value,
          [target.id]: projectTicketSources(descriptors),
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
    defaultProviders.value = {
      ...defaultProviders.value,
      [value.id]: projectTicketSources(descriptors),
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
    if (restored) dependencies.restoreCustomView(restored);
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
    const selectionAtStart = dependencies.projectSelectionGeneration?.();
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
      // The opened tab is visible while the open finishes; if the user picked a tab meanwhile, that
      // explicit choice wins instead of being overridden when the open completes (HS2-YVBGW3).
      if (
        openedProjectYieldsToSelection(
          selectionAtStart,
          dependencies.projectSelectionGeneration?.(),
          dependencies.project()?.id,
          opened.project.id,
        )
      )
        presentOpenedProjectSetup(opened.project);
      else await activateOpenedProject(opened.project);
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
      const descriptors = await client.providers();
      defaultProviders.value = {
        ...defaultProviders.value,
        [target.id]: projectTicketSources(descriptors),
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
      const client = new Api(current.apiPath, '', { trackBusy: !quiet }),
        [connections, descriptors] = await Promise.all([client.connections(), client.providers()]);
      if (dependencies.project()?.id !== current.id) return;
      providerConnections.value = connections;
      applyProviderDescriptors(current, descriptors);
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
      existing = editingId ? providerConnections.value.find((item) => item.id === editingId) : undefined,
      // New connections get a server-generated id; an empty display name uses the provider's name (HS2-48GA17).
      id = editingId ?? '',
      name = read('connection-name') || providerName(kind),
      locator = read('connection-locator'),
      existingCredential = (existing?.settings.credential as { secret?: unknown } | undefined)?.secret,
      credential =
        kind === 'github'
          ? (githubAuth.value?.credential ?? (typeof existingCredential === 'string' ? existingCredential : ''))
          : read('credential-reference'),
      makeDefault = values.get('make-default') === 'on';
    if (!locator || (kind !== 'jira' && !locator.includes('/'))) {
      providerSettingsError.value =
        kind === 'jira' ? 'Enter the Jira project key.' : 'Enter a namespace/repository path.';
      return;
    }
    if (!credential) {
      providerSettingsError.value =
        kind === 'github' ? 'Sign in with GitHub first.' : 'Enter an existing OS-keychain credential reference.';
      return;
    }
    const existingApiBase = existing?.settings.api_base,
      settings: Record<string, unknown> = { credential: { secret: credential } },
      // GitHub has no API-base field: Enterprise derives it from the server signed in to, and an edit
      // keeps the connection's existing one (HS2-1JT25R).
      apiBase =
        kind === 'github'
          ? editingId
            ? typeof existingApiBase === 'string'
              ? existingApiBase
              : ''
            : githubAuth.value?.enterpriseUrl
              ? `${githubAuth.value.enterpriseUrl}/api/v3`
              : ''
          : read('api-base'),
      email = read('jira-email');
    if (apiBase) settings[kind === 'jira' ? 'base_url' : 'api_base'] = apiBase;
    // The dialog has no assets-repository fields yet; an edit keeps a GitHub connection's
    // attachment repository configured headlessly with `github-connect` (HS2-HSA64D).
    if (kind === 'github') carryGithubAttachmentSettings(existing?.settings, settings);
    if (kind === 'jira') {
      if (!email || !apiBase) {
        providerSettingsError.value = 'Jira requires the account email and site URL.';
        return;
      }
      settings.email = email;
    }
    const wasProjectDefault = Boolean(
        editingId &&
        defaultProviders.value[current.id]?.sources.some((item) => item.connectionId === editingId && item.default),
      ),
      connection: ProviderConnection = {
        id,
        provider: kind,
        locator,
        name,
        // A project's default is its checkout's, never a flag on the record (HS2-3SCH1K).
        default: existing?.default ?? false,
        settings,
      };
    providerSettingsBusy.value = true;
    providerSettingsError.value = '';
    try {
      const client = new Api(current.apiPath),
        // A new source is created owned by this project: record and link in one request (HS2-SM9PM8).
        saved = editingId
          ? await client.updateConnection(editingId, connection)
          : await client.createConnection(connection, makeDefault);
      if (editingId && makeDefault !== wasProjectDefault)
        await client.setCheckoutDefaultSource(current.id, makeDefault ? editingId : null);
      await reloadProviderDescriptors(client, current);
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

  async function reloadProviderDescriptors(client: Api, current: Project) {
    providerConnections.value = await client.connections();
    applyProviderDescriptors(current, await client.providers());
  }

  function applyProviderDescriptors(current: Project, descriptors: ProviderDescriptor[]) {
    defaultProviders.value = {
      ...defaultProviders.value,
      [current.id]: projectTicketSources(descriptors),
    };
    providerCapabilities.value = {
      ...providerCapabilities.value,
      ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
    };
  }

  /** Run one change to this project's checkout sources, then refresh what depends on them (HS2-3SCH1K). */
  async function changeProjectSources(change: (client: Api, current: Project) => Promise<unknown>, done: string) {
    const current = dependencies.project();
    if (!current || providerSettingsBusy.value) return;
    providerSettingsBusy.value = true;
    providerSettingsError.value = '';
    try {
      const client = new Api(current.apiPath);
      await change(client, current);
      await reloadProviderDescriptors(client, current);
      await dependencies.refreshProject();
      dependencies.showToast(done);
    } catch (reason) {
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      providerSettingsBusy.value = false;
    }
  }

  /**
   * Open the source's editor straight at its inline removal confirmation (HS2-SM9PM8): removing a source
   * deletes its connection when no other project uses it, so it is never a one-click row action.
   */
  function requestProjectSourceRemoval(id: string) {
    const current = dependencies.project(),
      connection = providerConnections.value.find((item) => item.id === id);
    if (!current || !connection) return;
    ticketSourceSetupNavigation.value = 'none';
    ticketSourceSetupProject.value = current;
    providerSetupKind.value = connection.provider as ExternalProviderKind;
    providerEditingId.value = id;
    providerRemovingId.value = id;
    providerSettingsError.value = '';
  }

  /** Make one of this project's sources the default for new tickets. */
  function setProjectDefaultSource(id: string) {
    const name = defaultProviders.value[dependencies.project()?.id ?? '']?.sources.find(
      (item) => item.connectionId === id,
    )?.name;
    return changeProjectSources(
      (client, current) => client.setCheckoutDefaultSource(current.id, id),
      `${name ?? id} is now this project's default source.`,
    );
  }

  /**
   * Switch the connection open for editing off or back on (HS2-SF6W34). While disabled the server
   * neither reads nor writes it, so its tickets drop out of the refreshed views.
   */
  async function toggleProviderDisabled() {
    const current = ticketSourceSetupProject.value ?? dependencies.project(),
      id = providerEditingId.value,
      connection = providerConnections.value.find((item) => item.id === id);
    if (!current || !id || !connection || providerSettingsBusy.value) return;
    const disabled = !connection.disabled;
    providerSettingsBusy.value = true;
    providerSettingsError.value = '';
    try {
      const client = new Api(current.apiPath);
      await client.setConnectionDisabled(id, disabled);
      await reloadProviderDescriptors(client, current);
      ticketSourceSetupProject.value = undefined;
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      providerRemovingId.value = undefined;
      await dependencies.refreshProject();
      dependencies.showToast(`${connection.name ?? id} ${disabled ? 'disabled' : 'enabled'}.`);
    } catch (reason) {
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      providerSettingsBusy.value = false;
    }
  }

  /** Ask to confirm permanently removing the connection open for editing (HS2-724S9N). */
  function requestProviderRemoval() {
    providerRemovingId.value = providerEditingId.value;
    providerSettingsError.value = '';
  }
  function cancelProviderRemoval() {
    providerRemovingId.value = undefined;
  }
  /** Remove the confirmed connection and every local reference to it, then close the dialog. */
  async function removeExternalProvider() {
    const current = ticketSourceSetupProject.value ?? dependencies.project(),
      id = providerRemovingId.value;
    if (!current || !id || id !== providerEditingId.value || providerSettingsBusy.value) return;
    const connection = providerConnections.value.find((item) => item.id === id);
    providerSettingsBusy.value = true;
    providerSettingsError.value = '';
    try {
      const client = new Api(current.apiPath),
        // Removes it from this project only; the connection goes too once no project uses it (HS2-SM9PM8).
        result = await client.removeCheckoutSource(current.id, id),
        others = (connection?.projects ?? []).filter((project) => project.id !== current.id);
      await reloadProviderDescriptors(client, current);
      ticketSourceSetupProject.value = undefined;
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      providerRemovingId.value = undefined;
      await dependencies.refreshProject();
      dependencies.showToast(
        result.removed_connection || !others.length
          ? `${connection?.name ?? id} removed from this project.`
          : `${connection?.name ?? id} removed from this project; ${others.map((item) => item.alias).join(', ')} still use it.`,
      );
    } catch (reason) {
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      providerSettingsBusy.value = false;
    }
  }

  /** The Enterprise web origin typed in the form: `https://host[:port]`, or an error message. */
  function enterpriseOrigin(form: HTMLFormElement): { origin: string } | { error: string } {
    const raw = form.querySelector<Control>('[name="github-enterprise-url"]')?.value.trim() ?? '';
    try {
      const url = new URL(/^https?:\/\//i.test(raw) ? raw : `https://${raw}`);
      if (!raw || !url.hostname.includes('.')) throw new Error('missing host');
      if (url.hostname === 'github.com' || url.hostname === 'api.github.com') return { origin: 'https://github.com' };
      return { origin: url.origin };
    } catch {
      return { error: 'Enter your GitHub Enterprise server address, such as https://github.example.com.' };
    }
  }

  let githubPopup: Window | null = null;
  const GITHUB_POPUP = 'hotsheet-github-sign-in',
    GITHUB_POPUP_FEATURES = 'popup,width=560,height=760';

  /**
   * Sign in with GitHub's device flow (HS2-1JT25R). Called from the click itself: the sized popup and
   * the clipboard write both start before the first await, while the browser still treats them as
   * user-initiated. The popup is pointed at GitHub once the one-time code exists and is closed again
   * when GitHub authorizes Hot Sheet.
   */
  async function startGitHubSignIn(form: HTMLFormElement) {
    const target = ticketSourceSetupProject.value,
      current = githubAuth.value;
    if (!target || current?.state === 'waiting') return;
    providerSettingsError.value = '';
    const enterprise = current?.enterprise ? enterpriseOrigin(form) : undefined;
    if (enterprise && 'error' in enterprise) {
      githubAuth.value = { ...current!, state: 'idle', message: enterprise.error };
      return;
    }
    const webBase = enterprise?.origin ?? 'https://github.com',
      enterpriseUrl = webBase === 'https://github.com' ? undefined : webBase;
    githubPopup = window.open('', GITHUB_POPUP, GITHUB_POPUP_FEATURES);
    let provideCode!: (code: string) => void, withholdCode!: (reason: unknown) => void;
    const code = new Promise<string>((resolve, reject) => {
      provideCode = resolve;
      withholdCode = reject;
    });
    const copied = copyWhenReady(code);
    try {
      const client = new Api(target.apiPath),
        started = await client.startGitHubAuth(webBase);
      provideCode(started.user_code);
      if (githubPopup && !githubPopup.closed) githubPopup.location.href = started.verification_uri;
      githubAuth.value = {
        session: started.session_id,
        userCode: started.user_code,
        verificationUri: started.verification_uri,
        state: 'waiting',
        enterprise: current?.enterprise,
        enterpriseUrl,
      };
      void copied.then((ok) => {
        if (githubAuth.value?.session === started.session_id) githubAuth.value = { ...githubAuth.value, copied: ok };
      });
      const result = await client.waitGitHubAuth(started.session_id);
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Async cancellation may replace the live auth signal while this request is pending.
      if (githubAuth.value?.session !== started.session_id || result.state === 'pending') return;
      if (result.state === 'authorized') {
        closeGitHubPopup();
        githubAuth.value = { ...githubAuth.value, state: 'authorized', credential: result.credential_reference };
        await loadGitHubRepositories(client, started.session_id);
      } else {
        githubAuth.value = {
          ...githubAuth.value,
          state: result.state,
          message: result.state === 'error' ? result.message : undefined,
        };
      }
    } catch (reason) {
      withholdCode(reason);
      closeGitHubPopup();
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function closeGitHubPopup() {
    if (githubPopup && !githubPopup.closed) githubPopup.close();
    githubPopup = null;
  }

  /** Switch the not-yet-started sign-in between GitHub.com and GitHub Enterprise. */
  function chooseGitHubEnterprise(enterprise: boolean) {
    const current = githubAuth.value;
    if (current?.state === 'waiting') return;
    githubAuth.value = {
      session: '',
      userCode: '',
      verificationUri: '',
      state: 'idle',
      enterprise,
      enterpriseUrl: current?.enterpriseUrl,
    };
  }

  /** Copy the waiting code again, for when the automatic copy was refused or overwritten. */
  async function copyGitHubCode() {
    const current = githubAuth.value;
    if (current?.state !== 'waiting') return;
    const ok = await copyWhenReady(Promise.resolve(current.userCode));
    if (githubAuth.value?.session === current.session) githubAuth.value = { ...githubAuth.value, copied: ok };
    dependencies.showToast(ok ? 'Code copied.' : 'Copy the code shown in the dialog.');
  }

  /** Bring the GitHub window back if the user closed it before approving. */
  function reopenGitHubSignIn() {
    const current = githubAuth.value;
    if (current?.state !== 'waiting') return;
    githubPopup = window.open(current.verificationUri, GITHUB_POPUP, GITHUB_POPUP_FEATURES);
  }

  /**
   * List (or re-list) the repositories the signed-in session — or a reused account (HS2-SM9PM8) — can
   * reach, with installation grants.
   */
  async function loadGitHubRepositories(client: Api, session: string, account?: string) {
    try {
      const listed = account
        ? await client.accountGithubRepositories(account)
        : await client.githubAuthRepositories(session);
      if (githubAuth.value?.session !== session) return;
      githubAuth.value = {
        ...githubAuth.value,
        repositories: listed.repositories,
        installations: (listed.installations ?? []).map((item) => ({
          account: item.account,
          selection: item.selection,
          settingsUrl: item.settings_url ?? undefined,
        })),
        installUrl: listed.install_url ?? undefined,
        refreshing: false,
        message: undefined,
      };
    } catch (reason) {
      if (githubAuth.value?.session === session)
        githubAuth.value = {
          ...githubAuth.value,
          refreshing: false,
          message: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }

  /** Re-list after the user granted the app more repositories on GitHub (HS2-27T5WT). */
  async function refreshGitHubRepositories() {
    const target = ticketSourceSetupProject.value,
      current = githubAuth.value;
    if (!target || current?.state !== 'authorized' || current.refreshing) return;
    githubAuth.value = { ...current, refreshing: true };
    await loadGitHubRepositories(new Api(target.apiPath), current.session, current.account);
  }

  /**
   * Add a source in this project with a GitHub account already signed in on this machine (HS2-SM9PM8):
   * no new sign-in, just this project's own repository choice.
   */
  async function useGithubAccount(id: string) {
    const target = ticketSourceSetupProject.value,
      account = providerAccounts.value.find((item) => item.id === id && item.provider === 'github');
    if (!target || !account || githubAuth.value?.state === 'waiting') return;
    // An Enterprise account's site comes from its reported endpoint (also for an unused sign-in,
    // HS2-16MYXN), so the new source gets that server's api_base rather than github.com's.
    const session = `account:${id}`,
      enterpriseUrl = account.base_url
        ? new URL(account.base_url).origin
        : account.host && account.host !== 'github.com'
          ? `https://${account.host}`
          : undefined;
    providerSettingsError.value = '';
    githubAuth.value = {
      session,
      userCode: '',
      verificationUri: '',
      state: 'authorized',
      credential: id,
      account: id,
      enterprise: Boolean(enterpriseUrl),
      enterpriseUrl,
    };
    await loadGitHubRepositories(new Api(target.apiPath), session, id);
  }

  /**
   * Add a GitLab or Jira source with an account already signed in on this machine (HS2-F5HNJN): its
   * credential reference, Jira email, and site or API base prefill the form, while this project still
   * enters its own project path or key.
   */
  function useProviderAccount(id: string) {
    const kind = providerSetupKind.value;
    if (!kind || kind === 'github' || providerEditingId.value) return;
    if (!providerAccounts.value.some((item) => item.id === id && item.provider === kind)) return;
    providerSettingsError.value = '';
    providerAccountChoice.value = id;
  }

  /** Machine-wide sign-ins and the projects using each (HS2-SM9PM8). */
  async function refreshProviderAccounts(current = dependencies.project()) {
    if (!current) return;
    try {
      providerAccounts.value = await new Api(current.apiPath, '', { trackBusy: false }).accounts();
      providerAccountsError.value = '';
    } catch (reason) {
      providerAccountsError.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  /** Sign out of an account no ticket source uses; the server refuses one still in use. */
  async function signOutProviderAccount(id: string) {
    const current = dependencies.project();
    if (!current || signingOutAccount.value) return;
    signingOutAccount.value = id;
    providerAccountsError.value = '';
    try {
      await new Api(current.apiPath).signOutAccount(id);
      await refreshProviderAccounts(current);
      dependencies.showToast('Signed out.');
    } catch (reason) {
      providerAccountsError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      signingOutAccount.value = undefined;
    }
  }

  function requestUnusedAccountSourceRemoval(accountId: string, sourceId: string) {
    if (removingUnusedAccountSource.value) return;
    if (
      !providerAccounts.value.some(
        (account) =>
          account.id === accountId &&
          account.sources.some((source) => source.connection_id === sourceId && source.projects.length === 0),
      )
    )
      return;
    providerAccountsError.value = '';
    unusedAccountSourceChoice.value = sourceId;
  }

  function cancelUnusedAccountSourceRemoval() {
    if (!removingUnusedAccountSource.value) unusedAccountSourceChoice.value = undefined;
  }

  async function removeUnusedAccountSource(accountId: string, sourceId: string) {
    const current = dependencies.project();
    if (!current || unusedAccountSourceChoice.value !== sourceId || removingUnusedAccountSource.value) return;
    removingUnusedAccountSource.value = sourceId;
    providerAccountsError.value = '';
    try {
      await new Api(current.apiPath).removeUnusedAccountSource(accountId, sourceId);
      unusedAccountSourceChoice.value = undefined;
      await refreshProviderAccounts(current);
      dependencies.showToast('Unused ticket source removed.');
    } catch (reason) {
      providerAccountsError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      removingUnusedAccountSource.value = undefined;
    }
  }

  function cancelGitHubSignIn() {
    const target = ticketSourceSetupProject.value,
      current = githubAuth.value;
    if (target && current?.state === 'waiting') void new Api(target.apiPath).cancelGitHubAuth(current.session);
    closeGitHubPopup();
    if (current?.state === 'waiting') githubAuth.value = { ...current, state: 'cancelled' };
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
    providerAccounts,
    providerAccountsError,
    signingOutAccount,
    unusedAccountSourceChoice,
    removingUnusedAccountSource,
    refreshProviderAccounts,
    signOutProviderAccount,
    requestUnusedAccountSourceRemoval,
    cancelUnusedAccountSourceRemoval,
    removeUnusedAccountSource,
    useGithubAccount,
    providerAccountChoice,
    useProviderAccount,
    providerSettingsBusy,
    providerRemovingId,
    requestProjectSourceRemoval,
    setProjectDefaultSource,
    requestProviderRemoval,
    cancelProviderRemoval,
    removeExternalProvider,
    toggleProviderDisabled,
    refreshGitHubRepositories,
    chooseGitHubEnterprise,
    copyGitHubCode,
    reopenGitHubSignIn,
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
