import { createScope } from 'kerfjs/scope';

import { demoModule as aiConversation } from './ai-conversation-demo';
import { demoModule as appChrome } from './app-chrome-demo';
import { demoModule as appDialogs } from './app-dialogs-demo';
import { demoModule as appLoadingIndicator } from './app-loading-indicator-demo';
import { demoModule as appShell } from './app-shell-demo';
import { demoModule as bulkTicketDialog } from './bulk-ticket-dialog-demo';
import { demoModule as choiceCard } from './choice-card-demo';
import { demoModule as commandRunDialog } from './command-run-dialog-demo';
import { demoModule as confidenceBadge } from './confidence-badge-demo';
import { demoModule as confidenceCalibration } from './confidence-calibration-demo';
import { demoModule as connectionDetails } from './connection-details-demo';
import { demoModule as contentComponents } from './content-components-demo';
import { demoModule as contentTransition } from './content-transition-demo';
import { demoModule as corruptTicketInspector } from './corrupt-ticket-inspector-demo';
import type { DemoEntry, DemoModule } from './demo-module';
import { demoModule as dialogLayout } from './dialog-layout-demo';
import { demoModule as haltedSession } from './halted-session-demo';
import { demoModule as list } from './list-demo';
import { demoModule as listHeader } from './list-header-demo';
import { demoModule as listItem } from './list-item-demo';
import { demoModule as notWorkingDialog } from './not-working-dialog-demo';
import { demoModule as notificationCard } from './notification-card-demo';
import { demoModule as permissionComponents } from './permission-components-demo';
import { demoModule as projectDialog } from './project-dialog-demo';
import { demoModule as projectSidebar } from './project-sidebar-demo';
import { demoModule as providerIcon } from './provider-icon-demo';
import { demoModule as repositorySize } from './repository-size-demo';
import { demoModule as repositoryStatus } from './repository-status-demo';
import { demoModule as searchScopeToolbar } from './search-scope-toolbar-demo';
import { demoModule as select } from './select-demo';
import { demoModule as setupSettings } from './setup-settings-demo';
import { demoModule as statusBadge } from './status-badge-demo';
import { demoModule as tagChip } from './tag-chip-demo';
import { demoModule as terminalComponents } from './terminal-components-demo';
import { demoModule as terminalVisibility } from './terminal-visibility-demo';
import { demoModule as ticketCollections } from './ticket-collections-demo';
import { demoModule as ticketMetadata } from './ticket-metadata-demo';
import { demoModule as ticketRow } from './ticket-row-demo';
import { demoModule as ticketSearchField } from './ticket-search-field-demo';
import { demoModule as toolbarControlGroup } from './toolbar-control-group-demo';
import { demoModule as toolbar } from './toolbar-demo';
import { demoModule as toolbarText } from './toolbar-text-demo';
import { demoModule as workspaceComponents } from './workspace-components-demo';

/**
 * Every demo source file's catalog contribution. `wireDemoModules` installs their interactions in this
 * order, which follows the delegated listeners' original registration order, so capture/bubble and
 * stopPropagation interplay between modules is unchanged (HS2-HMVBZW).
 */
export const demoModules: readonly DemoModule[] = [
  projectDialog,
  appShell,
  workspaceComponents,
  ticketSearchField,
  notificationCard,
  corruptTicketInspector,
  haltedSession,
  permissionComponents,
  appChrome,
  projectSidebar,
  dialogLayout,
  terminalComponents,
  appDialogs,
  terminalVisibility,
  contentTransition,
  repositoryStatus,
  appLoadingIndicator,
  aiConversation,
  connectionDetails,
  setupSettings,
  tagChip,
  statusBadge,
  confidenceCalibration,
  repositorySize,
  confidenceBadge,
  toolbarControlGroup,
  contentComponents,
  bulkTicketDialog,
  ticketMetadata,
  ticketRow,
  ticketCollections,
  notWorkingDialog,
  commandRunDialog,
  toolbar,
  toolbarText,
  providerIcon,
  select,
  searchScopeToolbar,
  list,
  listItem,
  listHeader,
  choiceCard,
];

const entries = new Map<string, DemoEntry>();
for (const module of demoModules)
  for (const [id, entry] of Object.entries(module.entries)) {
    if (entries.has(id)) throw new Error(`Duplicate UX demo entry ${id}.`);
    entries.set(id, entry);
  }

/** The demo registered for a catalog entry, if its component demo exists yet. */
export function demoEntry(id: string): DemoEntry | undefined {
  return entries.get(id);
}

/** Every registered entry id, for catalog consistency checks. */
export function registeredDemoIds(): string[] {
  return [...entries.keys()];
}

/** Install every module's delegated interactions on the catalog root and return one disposer. */
export function wireDemoModules(root: HTMLElement): () => void {
  const scope = createScope();
  for (const module of demoModules) if (module.wire) scope.add(module.wire(root));
  return () => {
    scope.dispose();
  };
}
