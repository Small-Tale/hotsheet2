import { delegate, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { isRemoteClient } from '../client-origin';
import { type ExternalProviderKind } from '../components/provider-setup-form';
import { PROJECT_LIFECYCLE_ACTIONS, PROJECT_LIFECYCLE_TARGETS } from '../interaction-attrs/project-lifecycle';
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
  const lifetime = createScope();
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
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.addProject.selector, () => {
      openProjectPicker();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.chooseProject.selector, () => {
      if (isRemoteClient()) void openRemoteProjectDialog();
      else void chooseAndOpenProject();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.cancelOpenProject.selector, () => {
      unhealthyServerRecovery.value = undefined;
      projectDialogOpen.value = false;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.openRemoteCheckout.selector, (_event, target) => {
      const root = data(target.closest<HTMLElement>('[data-checkout-root]')!).checkoutRoot;
      if (root) void openRemoteCheckout(root);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.cancelRemoteProject.selector, () => {
      remoteProjectDialogOpen.value = false;
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-remote-project-dialog]', () => {
      remoteProjectDialogOpen.value = false;
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-project-dialog]', () => {
      unhealthyServerRecovery.value = undefined;
      projectDialogOpen.value = false;
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', PROJECT_LIFECYCLE_ACTIONS.importHs1Project.selector, (event, target) => {
      event.preventDefault();
      void importHs1Project(target as HTMLFormElement);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.browseHs1TicketStore.selector, () => {
      void chooseHs1TicketStore();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.dismissHs1Migration.selector, () => {
      const target = hs1MigrationProject.value;
      if (!target || hs1MigrationBusy.value) return;
      dismissHs1MigrationPrompt(localStorage, target.id, hs1SourceIdentity(target));
      hs1MigrationProject.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', PROJECT_LIFECYCLE_TARGETS.hs1MigrationDialog.selector, () => {
      const target = hs1MigrationProject.value;
      if (!target || hs1MigrationBusy.value) return;
      dismissHs1MigrationPrompt(localStorage, target.id, hs1SourceIdentity(target));
      hs1MigrationProject.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.openHs1Migration.selector, () => {
      const current = project();
      if (!current?.needsHs1Migration) return;
      hs1MigrationProject.value = current;
      requestAnimationFrame(() =>
        requestAnimationFrame(() =>
          document.querySelector<Control>('[data-component="hs1-migration-dialog"]')?.show?.(),
        ),
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.migrationJobDetails.selector, () => {
      const root = project()?.root;
      if (root) migrationJobDetails.value = { ...migrationJobDetails.value, [root]: !migrationJobDetails.value[root] };
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.reconnectMigrationJob.selector, () => {
      const root = project()?.root;
      if (root)
        void migrationJobs.join(root).catch((reason: unknown) => {
          migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [root]: String(reason) };
        });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.retryMigrationJob.selector, () => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.backupMigrationJob.selector, () => {
      const target = project(),
        job = target && migrationJobsByRoot.value[target.root];
      if (!target || !job || job.status === 'running' || (job.kind === 'import' && job.status !== 'succeeded')) return;
      ticketSourceSetupProject.value = target;
      createdGitTicketStore.value = job.store;
      ticketSourceSetupNavigation.value = 'push';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.removeHs1Data.selector, () => {
      void removeOldHs1Data();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.dismissProjectSetupWarning.selector, () => {
      const current = project();
      if (!current) return;
      projects.value = projects.value.map((item) =>
        item.id === current.id ? { ...item, setupWarning: undefined } : item,
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.dismissHs1Cleanup.selector, () => {
      const current = project();
      if (!current) return;
      dismissHs1CleanupPrompt(localStorage, current.id, hs1SourceIdentity(current));
      projects.value = projects.value.map((item) =>
        item.id === current.id ? { ...item, hs1CleanupEligible: false } : item,
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-ticket-source-setup-dialog]', () => {
      ticketSourceSetupProject.value = undefined;
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      providerSettingsError.value = '';
      createdGitTicketStore.value = '';
      ticketSourceRemoteError.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.dismissTicketSourceSetup.selector, () => {
      ticketSourceSetupProject.value = undefined;
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      providerSettingsError.value = '';
      createdGitTicketStore.value = '';
      ticketSourceRemoteError.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.submitTicketStoreRemote.selector, () =>
      document.querySelector<HTMLFormElement>('#ticket-source-remote-form')?.requestSubmit(),
    ),
  );
  lifetime.add(
    delegate(document.body, 'submit', PROJECT_LIFECYCLE_ACTIONS.connectTicketStoreRemote.selector, (event, target) => {
      event.preventDefault();
      void connectCreatedGitRemote(target as HTMLFormElement);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.backTicketStoreRemote.selector, () => {
      ticketSourceSetupNavigation.value = 'pop';
      createdGitTicketStore.value = '';
      ticketSourceRemoteError.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.createProjectGitSource.selector, () => {
      void createProjectGitSource();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.createProjectGitSourceCustom.selector, () => {
      void createProjectGitSource(true);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.browseProjectPath.selector, (_event, target) => {
      void chooseProjectPath(target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.recoverUnhealthyServer.selector, () => {
      void recoverUnhealthyProjectServer();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', PROJECT_LIFECYCLE_ACTIONS.reloadClient.selector, () => {
      window.location.reload();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
