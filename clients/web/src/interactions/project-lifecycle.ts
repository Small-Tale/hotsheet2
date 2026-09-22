import { delegate, type Signal } from 'kerfjs';

import { isRemoteClient } from '../client-origin';
import { type ExternalProviderKind } from '../components/provider-setup-form';
import { type MigrationJobClient } from '../migration-job-client';
import { type MigrationJob } from '../migration-progress';
import { dismissHs1CleanupPrompt, dismissHs1MigrationPrompt } from '../workspace-session';
import { data } from './dom';
import { type Control, type Project, type UnhealthyServerRecovery } from './types';

/** Live application bindings used by this handler group. */
export interface ProjectLifecycleInteractionsDependencies {
  readonly openProjectPicker: () => void;
  readonly openRemoteProjectDialog: () => Promise<void>;
  readonly chooseAndOpenProject: () => Promise<void>;
  readonly unhealthyServerRecovery: Signal<UnhealthyServerRecovery | undefined>;
  readonly projectDialogOpen: Signal<boolean>;
  readonly openRemoteCheckout: (root: string) => Promise<void>;
  readonly remoteProjectDialogOpen: Signal<boolean>;
  readonly importHs1Project: (form: HTMLFormElement) => Promise<void>;
  readonly chooseHs1TicketStore: () => Promise<void>;
  readonly hs1MigrationProject: Signal<Project | undefined>;
  readonly hs1MigrationBusy: Signal<boolean>;
  readonly hs1SourceIdentity: (value: Project) => string;
  readonly project: () => Project | undefined;
  readonly migrationJobDetails: Signal<Record<string, boolean>>;
  readonly migrationJobs: MigrationJobClient;
  readonly migrationConnectionErrors: Signal<Record<string, string>>;
  readonly migrationJobsByRoot: Signal<Partial<Record<string, MigrationJob>>>;
  readonly ticketSourceSetupProject: Signal<Project | undefined>;
  readonly createdGitTicketStore: Signal<string>;
  readonly ticketSourceSetupNavigation: Signal<'none' | 'push' | 'pop'>;
  readonly removeOldHs1Data: () => Promise<void>;
  readonly projects: Signal<Project[]>;
  readonly providerSetupKind: Signal<ExternalProviderKind | undefined>;
  readonly providerEditingId: Signal<string | undefined>;
  readonly providerSettingsError: Signal<string>;
  readonly ticketSourceRemoteError: Signal<string>;
  readonly connectCreatedGitRemote: (form: HTMLFormElement) => Promise<void>;
  readonly createProjectGitSource: (custom?: boolean) => Promise<void>;
  readonly chooseProjectPath: (button: Element) => Promise<void>;
  readonly recoverUnhealthyProjectServer: () => Promise<void>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireProjectLifecycleInteractions(dependencies: ProjectLifecycleInteractionsDependencies) {
  const {
    openProjectPicker,
    openRemoteProjectDialog,
    chooseAndOpenProject,
    unhealthyServerRecovery,
    projectDialogOpen,
    openRemoteCheckout,
    remoteProjectDialogOpen,
    importHs1Project,
    chooseHs1TicketStore,
    hs1MigrationProject,
    hs1MigrationBusy,
    hs1SourceIdentity,
    project,
    migrationJobDetails,
    migrationJobs,
    migrationConnectionErrors,
    migrationJobsByRoot,
    ticketSourceSetupProject,
    createdGitTicketStore,
    ticketSourceSetupNavigation,
    removeOldHs1Data,
    projects,
    providerSetupKind,
    providerEditingId,
    providerSettingsError,
    ticketSourceRemoteError,
    connectCreatedGitRemote,
    createProjectGitSource,
    chooseProjectPath,
    recoverUnhealthyProjectServer,
  } = dependencies;
  delegate(document.body, 'click', '[data-action="add-project"]', () => {
    openProjectPicker();
  });
  delegate(document.body, 'click', '[data-action="choose-project"]', () => {
    if (isRemoteClient()) void openRemoteProjectDialog();
    else void chooseAndOpenProject();
  });
  delegate(document.body, 'click', '[data-action="cancel-open-project"]', () => {
    unhealthyServerRecovery.value = undefined;
    projectDialogOpen.value = false;
  });
  delegate(document.body, 'click', '[data-action="open-remote-checkout"]', (_event, target) => {
    const root = data(target.closest<HTMLElement>('[data-checkout-root]')!).checkoutRoot;
    if (root) void openRemoteCheckout(root);
  });
  delegate(document.body, 'click', '[data-action="cancel-remote-project"]', () => {
    remoteProjectDialogOpen.value = false;
  });
  delegate(document.body, 'wa-hide', '[data-remote-project-dialog]', () => {
    remoteProjectDialogOpen.value = false;
  });
  delegate(document.body, 'wa-hide', '[data-project-dialog]', () => {
    unhealthyServerRecovery.value = undefined;
    projectDialogOpen.value = false;
  });
  delegate(document.body, 'submit', '[data-action="import-hs1-project"]', (event, target) => {
    event.preventDefault();
    void importHs1Project(target as HTMLFormElement);
  });
  delegate(document.body, 'click', '[data-action="browse-hs1-ticket-store"]', () => {
    void chooseHs1TicketStore();
  });
  delegate(document.body, 'click', '[data-action="dismiss-hs1-migration"]', () => {
    const target = hs1MigrationProject.value;
    if (!target || hs1MigrationBusy.value) return;
    dismissHs1MigrationPrompt(localStorage, target.id, hs1SourceIdentity(target));
    hs1MigrationProject.value = undefined;
  });
  delegate(document.body, 'wa-hide', '[data-component="hs1-migration-dialog"]', () => {
    const target = hs1MigrationProject.value;
    if (!target || hs1MigrationBusy.value) return;
    dismissHs1MigrationPrompt(localStorage, target.id, hs1SourceIdentity(target));
    hs1MigrationProject.value = undefined;
  });
  delegate(document.body, 'click', '[data-action="open-hs1-migration"]', () => {
    const current = project();
    if (!current?.needsHs1Migration) return;
    hs1MigrationProject.value = current;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => document.querySelector<Control>('[data-component="hs1-migration-dialog"]')?.show?.()),
    );
  });
  delegate(document.body, 'click', '[data-action="migration-job-details"]', () => {
    const root = project()?.root;
    if (root) migrationJobDetails.value = { ...migrationJobDetails.value, [root]: !migrationJobDetails.value[root] };
  });
  delegate(document.body, 'click', '[data-action="reconnect-migration-job"]', () => {
    const root = project()?.root;
    if (root)
      void migrationJobs.join(root).catch((reason: unknown) => {
        migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [root]: String(reason) };
      });
  });
  delegate(document.body, 'click', '[data-action="retry-migration-job"]', () => {
    const target = project(),
      job = target && migrationJobsByRoot.value[target.root];
    if (target && job)
      void migrationJobs
        .start({
          projectId: target.id,
          root: target.root,
          location: job.store,
          kind: job.kind,
          remote: job.remote,
          retryAttempt: job.attempt,
        })
        .catch((reason: unknown) => {
          migrationConnectionErrors.value = {
            ...migrationConnectionErrors.value,
            [target.root]: reason instanceof Error ? reason.message : String(reason),
          };
        });
  });
  delegate(document.body, 'click', '[data-action="backup-migration-job"]', () => {
    const target = project(),
      job = target && migrationJobsByRoot.value[target.root];
    if (!target || !job || job.status === 'running' || (job.kind === 'import' && job.status !== 'succeeded')) return;
    ticketSourceSetupProject.value = target;
    createdGitTicketStore.value = job.store;
    ticketSourceSetupNavigation.value = 'push';
  });
  delegate(document.body, 'click', '[data-action="remove-hs1-data"]', () => {
    void removeOldHs1Data();
  });
  delegate(document.body, 'click', '[data-action="dismiss-hs1-cleanup"]', () => {
    const current = project();
    if (!current) return;
    dismissHs1CleanupPrompt(localStorage, current.id, hs1SourceIdentity(current));
    projects.value = projects.value.map((item) =>
      item.id === current.id ? { ...item, hs1CleanupEligible: false } : item,
    );
  });
  delegate(document.body, 'wa-hide', '[data-ticket-source-setup-dialog]', () => {
    ticketSourceSetupProject.value = undefined;
    providerSetupKind.value = undefined;
    providerEditingId.value = undefined;
    providerSettingsError.value = '';
    createdGitTicketStore.value = '';
    ticketSourceRemoteError.value = '';
  });
  delegate(document.body, 'click', '[data-action="dismiss-ticket-source-setup"]', () => {
    ticketSourceSetupProject.value = undefined;
    providerSetupKind.value = undefined;
    providerEditingId.value = undefined;
    providerSettingsError.value = '';
    createdGitTicketStore.value = '';
    ticketSourceRemoteError.value = '';
  });
  delegate(document.body, 'click', '[data-action="submit-ticket-store-remote"]', () =>
    document.querySelector<HTMLFormElement>('#ticket-source-remote-form')?.requestSubmit(),
  );
  delegate(document.body, 'submit', '[data-action="connect-ticket-store-remote"]', (event, target) => {
    event.preventDefault();
    void connectCreatedGitRemote(target as HTMLFormElement);
  });
  delegate(document.body, 'click', '[data-action="back-ticket-store-remote"]', () => {
    ticketSourceSetupNavigation.value = 'pop';
    createdGitTicketStore.value = '';
    ticketSourceRemoteError.value = '';
  });
  delegate(document.body, 'click', '[data-action="create-project-git-source"]', () => {
    void createProjectGitSource();
  });
  delegate(document.body, 'click', '[data-action="create-project-git-source-custom"]', () => {
    void createProjectGitSource(true);
  });
  delegate(document.body, 'click', '[data-action="browse-project-path"]', (_event, target) => {
    void chooseProjectPath(target);
  });
  delegate(document.body, 'click', '[data-action="recover-unhealthy-server"]', () => {
    void recoverUnhealthyProjectServer();
  });
  delegate(document.body, 'click', '[data-action="reload-client"]', () => {
    window.location.reload();
  });
}
