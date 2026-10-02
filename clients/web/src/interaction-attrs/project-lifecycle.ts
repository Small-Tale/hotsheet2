import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: project opening, HS1 migration, ticket-source setup, and server recovery.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/project-lifecycle.ts` register `.selector`.
 */
export const PROJECT_LIFECYCLE_ACTIONS = {
  addProject: action('add-project'),
  chooseProject: action('choose-project'),
  cancelOpenProject: action('cancel-open-project'),
  openRemoteCheckout: action('open-remote-checkout'),
  cancelRemoteProject: action('cancel-remote-project'),
  importHs1Project: action('import-hs1-project'),
  browseHs1TicketStore: action('browse-hs1-ticket-store'),
  dismissHs1Migration: action('dismiss-hs1-migration'),
  openHs1Migration: action('open-hs1-migration'),
  migrationJobDetails: action('migration-job-details'),
  reconnectMigrationJob: action('reconnect-migration-job'),
  retryMigrationJob: action('retry-migration-job'),
  backupMigrationJob: action('backup-migration-job'),
  removeHs1Data: action('remove-hs1-data'),
  dismissProjectSetupWarning: action('dismiss-project-setup-warning'),
  dismissHs1Cleanup: action('dismiss-hs1-cleanup'),
  dismissTicketSourceSetup: action('dismiss-ticket-source-setup'),
  submitTicketStoreRemote: action('submit-ticket-store-remote'),
  connectTicketStoreRemote: action('connect-ticket-store-remote'),
  backTicketStoreRemote: action('back-ticket-store-remote'),
  createProjectGitSource: action('create-project-git-source'),
  createProjectGitSourceCustom: action('create-project-git-source-custom'),
  browseProjectPath: action('browse-project-path'),
  recoverUnhealthyServer: action('recover-unhealthy-server'),
  reloadClient: action('reload-client'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * project opening, HS1 migration, ticket-source setup, and server recovery.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const PROJECT_LIFECYCLE_TARGETS = {
  hs1MigrationDialog: attr('data-component', 'hs1-migration-dialog'),
} as const;
