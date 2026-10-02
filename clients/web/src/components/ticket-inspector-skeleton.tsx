import '@awesome.me/webawesome/dist/components/button/button.js';
import '@kerfjs/ui/list-inset-control.css';
import '@kerfjs/ui/tab-bar.css';
import './ticket-inspector.css';
import './ticket-info-panel.css';
import './ticket-inspector-skeleton.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { pct, rem } from '@kerfjs/ui/css-values';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListInsetControl } from '@kerfjs/ui/list-inset-control';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Skeleton } from '@kerfjs/ui/skeleton';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { Activity, BookOpen, Info, ListTree, MessageSquareCode, MessageSquareText, Paperclip, Plus } from 'lucide';

import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketInspectorPanel } from './ticket-inspector-panel';
import { TicketPrioritySelect } from './ticket-priority-select';
import { TicketStatusMenu } from './ticket-status-menu';

const TABS = [
  { id: 'info', label: 'Info', icon: Info, iconName: 'info' },
  { id: 'timeline', label: 'Timeline', icon: ListTree, iconName: 'list-tree' },
  { id: 'code-review', label: 'Code Review', icon: MessageSquareCode, iconName: 'message-square-code' },
  { id: 'attachments', label: 'Attachments', icon: Paperclip, iconName: 'paperclip' },
] as const;

/** One placeholder note entry mirroring a note card's header (kind + time) with a skeleton body. */
function PlaceholderNote({ kind, card = false }: { kind: 'activity' | 'regular'; card?: boolean }) {
  const presentation =
    kind === 'activity'
      ? { label: 'Activity', icon: Activity, iconName: 'activity' }
      : { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' };
  return (
    <div class={`ticket-inspector-skeleton__note${card ? ' ticket-inspector-skeleton__note--card' : ''}`}>
      <div class="ticket-inspector-skeleton__note-header">
        <span class="ticket-inspector-skeleton__note-kind">
          <LucideIcon icon={presentation.icon} name={presentation.iconName} />
          {presentation.label}
        </span>
        <Skeleton width={rem(2.5)} height={rem(0.6875)} />
      </div>
      <div class="ticket-inspector-skeleton__note-body">
        <Skeleton />
        <Skeleton width={pct(45)} />
      </div>
    </div>
  );
}

/**
 * Loading placeholder for the ticket inspector (HS2-REG3A2): it renders the REAL inspector
 * chrome — header, Kerf tab bar, metadata controls, and section headers — with the unknown
 * ticket values shown as subtle, unanimated placeholder blocks. Since kerf-ui 5.0.0-beta.7 the
 * value slots use the framework's native component `placeholder` mode and `Skeleton` block, so the
 * metadata controls track the real Select sizes automatically instead of hand-maintained CSS
 * (HS2-KWWSWY). The inspector still looks like the inspector; only the per-ticket values are absent.
 * Like the inspector it exposes Workbench panel parts (HS2-QQW6CT): the same toolbar, standard
 * toggle, fixed header, and scrolling content.
 */
export function ticketInspectorSkeletonPanel({ slug }: { slug?: string } = {}): SidebarPanelParts {
  return {
    label: 'Loading ticket',
    toolbar: {
      label: 'Ticket inspector toolbar',
      dividerSides: '',
      center: slug ? <ToolbarText text={slug} size="small" /> : <ToolbarText text="" size="small" placeholder />,
      trailing: (
        <ToolbarControlGroup appearance="borderless" label="Ticket actions">
          <button type="button" aria-label="Open ticket reader" title="Open ticket reader" tabIndex={-1}>
            <LucideIcon icon={BookOpen} name="book-open" />
          </button>
        </ToolbarControlGroup>
      ),
    },
    toggle: inspectorToggle(),
    header: (
      // The chrome is shown for continuity only; `inert` keeps all of it non-interactive while loading.
      <div class="ticket-inspector__header ticket-inspector--placeholder" aria-hidden="true" inert>
        <div class="ticket-inspector-skeleton__title">
          <Skeleton height={rem(1.25)} />
          <Skeleton width={pct(62)} height={rem(1.25)} />
        </div>
        <div class="ticket-inspector__tabs-frame">
          <TabBar id="ticket-inspector-loading" label="Ticket inspector sections" className="ticket-inspector__tabs">
            {TABS.map((tab) => (
              <AppTab
                id={tab.id}
                name={tab.label}
                selected={tab.id === 'info'}
                closable={false}
                placeholder
                leading={<LucideIcon icon={tab.icon} name={tab.iconName} size={14.4} />}
              />
            ))}
          </TabBar>
        </div>
      </div>
    ),
    content: (
      <div
        class="ticket-inspector__body ticket-inspector--placeholder"
        data-component="ticket-inspector-skeleton-body"
        aria-busy="true"
      >
        <TicketInspectorPanel
          component="ticket-inspector-skeleton-panel"
          attributes={{ 'aria-hidden': 'true', inert: '' }}
        >
          <section class="ticket-info-panel__metadata" aria-label="Ticket metadata">
            <TicketCategorySelect name="inspector-category" value="" placeholder />
            <TicketPrioritySelect name="inspector-priority" value="default" placeholder />
            <div class="ticket-info-panel__status-field">
              <ListHeader label="Status" />
              <ListInsetControl>
                <div class="ticket-info-panel__status-line">
                  <TicketStatusMenu value="not_started" placeholder />
                </div>
              </ListInsetControl>
            </div>
          </section>
          <section class="ticket-info-panel__section ticket-info-panel__blocked-section">
            <ListItem
              action="block-ticket"
              icon={<LucideIcon icon={Plus} name="plus" />}
              label="Block ticket"
              tabIndex={-1}
            />
          </section>
          <section class="ticket-info-panel__section ticket-info-panel__details-section">
            <ListHeader label="Details" />
            <div class="ticket-info-panel__details-surface">
              <div class="ticket-inspector-skeleton__lines">
                <Skeleton lines={3} />
              </div>
            </div>
          </section>
          <section class="ticket-info-panel__section">
            <ListHeader
              label="Tags"
              actionDisabled
              action="add-tag"
              actionLabel="Add tag"
              actionIcon={<LucideIcon icon={Plus} name="plus" />}
            />
          </section>
          <section class="ticket-info-panel__section">
            <ListHeader
              label="Notes"
              actionDisabled
              action="add-note"
              actionLabel="Add note"
              actionIcon={<LucideIcon icon={Plus} name="plus" />}
            />
            <div class="ticket-inspector-skeleton__notes">
              <PlaceholderNote kind="activity" />
              <PlaceholderNote kind="activity" />
              <PlaceholderNote kind="regular" card />
            </div>
          </section>
          <footer class="ticket-inspector-skeleton__provenance">
            <Skeleton width={rem(6)} height={rem(0.6875)} />
            <Skeleton width={rem(4)} height={rem(0.6875)} />
          </footer>
        </TicketInspectorPanel>
      </div>
    ),
    pane: {},
  };
}

/** The skeleton rendered standalone (the terminal rail's pushed detail and the UX catalog). */
export function TicketInspectorSkeleton({
  slug,
  collapseControl = false,
}: { slug?: string; collapseControl?: boolean } = {}) {
  return (
    <aside
      class="ticket-inspector ticket-inspector--placeholder"
      data-component="ticket-inspector-skeleton"
      aria-busy="true"
      aria-label="Loading ticket"
    >
      <SidebarPane
        parts={ticketInspectorSkeletonPanel({ slug })}
        element="div"
        side="right"
        collapseControl={collapseControl}
      />
    </aside>
  );
}
