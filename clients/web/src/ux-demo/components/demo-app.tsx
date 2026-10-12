import { Catalog } from '@kerfjs/ui/catalog';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';

import { ProjectTabContextMenu } from '../../components/project-tab-context-menu';
import { TicketRowContextMenu } from '../../components/ticket-row-context-menu';
import { AIConversationSettings } from '../ai-conversation-demo';
import { AppShellSettings } from '../app-shell-demo';
import { BulkTicketDialogSettings } from '../bulk-ticket-dialog-demo';
import { demoCatalog, demoKind, findDemo, kerfCatalogSections } from '../catalog';
import { CommandRunDialogSettings } from '../command-run-dialog-demo';
import { ConfidenceBadgeSettings } from '../confidence-badge-demo';
import { ConfidenceCalibrationSettings } from '../confidence-calibration-demo';
import { ConnectionDetailsDialogSettings } from '../connection-details-demo';
import { MarkdownEditorSettings } from '../content-components-demo';
import { ContentTransitionSettings } from '../content-transition-demo';
import { PermissionRequestSettings } from '../permission-components-demo';
import { RepositoryStatusPopoverSettings } from '../repository-status-demo';
import { SettingsWorkspaceSettings } from '../settings-workspace-demo';
import { StatusBadgeSettings } from '../status-badge-demo';
import { TagChipSettings } from '../tag-chip-demo';
import { collectionTickets } from '../ticket-collections-demo';
import { TICKET_ROW_DEMO_SLUG, TicketRowSettings, ticketRowSettings } from '../ticket-row-demo';
import { QuickTicketComposerSettings, TicketInspectorSettings } from '../workspace-components-demo';
import { demoContent } from './demo-content';
import {
  backgroundStyle,
  collapsed,
  contextMenu,
  DEFAULT_DEMO,
  increasedContrast,
  reducedMotion,
  selectedId,
  settingsOpen,
  tabContextMenu,
  theme,
} from './demo-state';

export function DemoApp() {
  const selected = findDemo(selectedId.value) ?? findDemo(DEFAULT_DEMO)!;
  // The standalone TicketRow demo row isn't a collection ticket, so resolve its context
  // menu from the live demo settings; collection rows resolve from their fixtures (HS2-AFB17W).
  const menuTicket =
    contextMenu.value?.ticketSlug === TICKET_ROW_DEMO_SLUG
      ? {
          category: ticketRowSettings.category.value,
          priority: ticketRowSettings.priority.value,
          status: ticketRowSettings.status.value,
        }
      : contextMenu.value?.ticketSlug
        ? collectionTickets.value.find((ticket) => ticket.slug === contextMenu.value?.ticketSlug)
        : undefined;
  const hasSettings =
    selected.id === 'app-shell' ||
    selected.id === 'tag-chip' ||
    selected.id === 'status-badge' ||
    selected.id === 'confidence-badge' ||
    selected.id === 'confidence-calibration' ||
    selected.id === 'ticket-row' ||
    selected.id === 'repository-status-popover' ||
    selected.id === 'connection-details-dialog' ||
    selected.id === 'content-transition' ||
    selected.id === 'permission-request' ||
    selected.id === 'ai-conversation' ||
    selected.id === 'quick-ticket-composer' ||
    selected.id === 'ticket-inspector' ||
    selected.id === 'markdown-editor' ||
    selected.id === 'bulk-ticket-dialog' ||
    selected.id === 'settings-workspace' ||
    selected.id === 'command-run-dialog';
  return (
    <>
      {/* The demo-owned shell narrows beside the settings inspector; Kerf's Catalog root stays unstyled. */}
      <div class="demo-shell" data-settings-open={String(settingsOpen.value)}>
        <Catalog
          brand={{ title: 'UX components', subtitle: 'Hot Sheet · deterministic production components' }}
          sections={kerfCatalogSections(demoCatalog)}
          active={selected.id}
          collapsed={collapsed.value}
          theme={theme.value}
          increasedContrast={increasedContrast.value}
          reducedMotion={reducedMotion.value}
          backgroundControl
          backgroundStyle={backgroundStyle.value}
          content={
            <section
              class="demo-catalog-examples"
              data-catalog-example-stack
              data-catalog-geometry-skip={demoKind(selected.id) === 'composition' ? '' : undefined}
              aria-label={`${selected.name} examples`}
            >
              {demoContent(selected)}
            </section>
          }
          status={
            <span>
              <strong>{selected.phase.replace('-', ' ')}</strong>
              {selected.implemented ? ' · Implemented' : ' · Planned'}
            </span>
          }
          headerActions={
            hasSettings && !settingsOpen.value ? (
              <ToolbarControlGroup label="Demo tools" content="mixed">
                <button type="button" data-action="toggle-settings" aria-expanded="false" title="Open demo settings">
                  <span>Settings</span>
                </button>
              </ToolbarControlGroup>
            ) : undefined
          }
        />
      </div>
      {settingsOpen.value && (
        <aside class="settings-inspector" aria-label={`${selected.name} settings`}>
          <header>
            <div>
              <p class="eyebrow">Demo settings</p>
              <h2 class="settings-inspector__title">{selected.name}</h2>
            </div>
            <wa-button class="settings-toggle" data-action="toggle-settings" aria-expanded="true">
              Close settings
            </wa-button>
          </header>
          {(() => {
            switch (selected.id) {
              case 'app-shell':
                return <AppShellSettings />;
              case 'tag-chip':
                return <TagChipSettings />;
              case 'status-badge':
                return <StatusBadgeSettings />;
              case 'confidence-badge':
                return <ConfidenceBadgeSettings />;
              case 'confidence-calibration':
                return <ConfidenceCalibrationSettings />;
              case 'ticket-row':
                return <TicketRowSettings />;
              case 'repository-status-popover':
                return <RepositoryStatusPopoverSettings />;
              case 'connection-details-dialog':
                return <ConnectionDetailsDialogSettings />;
              case 'content-transition':
                return <ContentTransitionSettings />;
              case 'permission-request':
                return <PermissionRequestSettings />;
              case 'ai-conversation':
                return <AIConversationSettings />;
              case 'quick-ticket-composer':
                return <QuickTicketComposerSettings />;
              case 'ticket-inspector':
                return <TicketInspectorSettings />;
              case 'markdown-editor':
                return <MarkdownEditorSettings />;
              case 'command-run-dialog':
                return <CommandRunDialogSettings />;
              case 'bulk-ticket-dialog':
                return <BulkTicketDialogSettings />;
              case 'settings-workspace':
                return <SettingsWorkspaceSettings />;
              default:
                return <p>This demo has no adjustable settings.</p>;
            }
          })()}
        </aside>
      )}
      {contextMenu.value && (
        <TicketRowContextMenu
          x={contextMenu.value.x}
          y={contextMenu.value.y}
          category={menuTicket?.category}
          priority={menuTicket?.priority}
          status={menuTicket?.status}
          upNextEligible={menuTicket?.status === 'not_started' || menuTicket?.status === 'started'}
          verifyAction={menuTicket?.status === 'completed'}
          notWorkingAction={menuTicket?.status === 'completed'}
          reopenAction={menuTicket?.status === 'verified' || menuTicket?.status === 'archive'}
          selectionCount={collectionTickets.value.filter((ticket) => ticket.selected).length || 1}
        />
      )}
      {tabContextMenu.value && <ProjectTabContextMenu {...tabContextMenu.value} />}
    </>
  );
}
