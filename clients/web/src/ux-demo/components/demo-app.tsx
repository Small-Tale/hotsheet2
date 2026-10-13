import { Catalog, CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Text } from '@kerfjs/ui/text';

import { ProjectTabContextMenu } from '../../components/project-tab-context-menu';
import { TicketRowContextMenu } from '../../components/ticket-row-context-menu';
import { demoCatalog, type DemoDefinition, demoKind, findDemo, kerfCatalogSections } from '../catalog';
import { demoEntry } from '../registry';
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
  settingsCollapsed,
  tabContextMenu,
  theme,
  uxReviewProject,
} from './demo-state';

function PlannedDemo({ item }: { item: DemoDefinition }) {
  return (
    <CatalogExample label={`${item.name} planned demo`}>
      <EmptyState
        title="Planned component"
        detail="The catalog entry and navigation are ready. Its real component demo will be added in a later slice."
      />
    </CatalogExample>
  );
}

export function DemoApp() {
  const selected = findDemo(selectedId.value) ?? findDemo(DEFAULT_DEMO)!;
  const entry = demoEntry(selected.id),
    Demo = entry?.Demo,
    Settings = entry?.Settings;
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
        // Kerf's settings pane hosts the entry's Settings form; entries without one get no pane or toggle.
        settings={Settings ? <Settings /> : undefined}
        settingsCollapsed={settingsCollapsed.value}
        settingsPanel={{ size: 368 }}
        toggleSettingsAction="toggle-settings"
        uxReview={uxReviewProject.value ? { projectDirectory: uxReviewProject.value } : undefined}
        content={
          // Converted demos render their own Kerf CatalogExamples inside Kerf's stack, full width
          // for application-width compositions (KF-HKVT5S). Demos not yet converted get one
          // full-width CatalogExample (HS2-Z3FMEZ).
          !entry?.catalogExamples ? (
            <CatalogExample viewport={{ width: 'full' }}>
              {Demo ? <Demo /> : <PlannedDemo item={selected} />}
            </CatalogExample>
          ) : (
            <CatalogExampleStack
              label={`${selected.name} examples`}
              width={demoKind(selected.id) === 'composition' ? 'full' : undefined}
            >
              {Demo ? <Demo /> : <PlannedDemo item={selected} />}
            </CatalogExampleStack>
          )
        }
        // Each entry's phase and Planned state are Catalog tags on its sidebar row (HS2-HMVBZW).
        status={
          <Text variant="span" size="compact" tone="quiet">
            Production components · deterministic fixtures
          </Text>
        }
      />
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
