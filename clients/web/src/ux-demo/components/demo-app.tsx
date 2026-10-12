import { Catalog } from '@kerfjs/ui/catalog';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';

import { ProjectTabContextMenu } from '../../components/project-tab-context-menu';
import { TicketRowContextMenu } from '../../components/ticket-row-context-menu';
import { demoCatalog, type DemoDefinition, demoKind, findDemo, kerfCatalogSections } from '../catalog';
import { demoEntry } from '../registry';
import { SettingsInspector } from '../settings-inspector';
import { collectionTickets } from '../ticket-collections-demo';
import { TICKET_ROW_DEMO_SLUG, ticketRowSettings } from '../ticket-row-demo';
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

function PlannedDemo({ item }: { item: DemoDefinition }) {
  return (
    <section class="planned-demo" aria-label={`${item.name} planned demo`}>
      <span>Planned component</span>
      <p>The catalog entry and navigation are ready. Its real component demo will be added in a later slice.</p>
    </section>
  );
}

export function DemoApp() {
  const selected = findDemo(selectedId.value) ?? findDemo(DEFAULT_DEMO)!;
  const entry = demoEntry(selected.id),
    Demo = entry?.Demo;
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
              {Demo ? <Demo /> : <PlannedDemo item={selected} />}
            </section>
          }
          status={
            <span>
              <strong>{selected.phase.replace('-', ' ')}</strong>
              {selected.implemented ? ' · Implemented' : ' · Planned'}
            </span>
          }
          headerActions={
            entry?.Settings && !settingsOpen.value ? (
              <ToolbarControlGroup label="Demo tools" content="mixed">
                <button type="button" data-action="toggle-settings" aria-expanded="false" title="Open demo settings">
                  <span>Settings</span>
                </button>
              </ToolbarControlGroup>
            ) : undefined
          }
        />
      </div>
      {settingsOpen.value && <SettingsInspector name={selected.name} entry={entry} />}
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
