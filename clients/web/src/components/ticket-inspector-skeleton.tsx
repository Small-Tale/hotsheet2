import '@awesome.me/webawesome/dist/components/button/button.js';
import '@kerfjs/ui/tab-bar.css';
import './ticket-inspector.css';
import './ticket-inspector-panel.css';
import './ticket-inspector-skeleton.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Skeleton } from '@kerfjs/ui/skeleton';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { Activity, BookOpen, Info, ListTree, MessageSquareCode, MessageSquareText, PanelRightClose, Paperclip, Plus } from 'lucide';

import { TicketCategorySelect } from './ticket-category-select';
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
  const presentation = kind === 'activity' ? { label: 'Activity', icon: Activity, iconName: 'activity' } : { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' };
  return <div class={`ticket-inspector__ph-note${card ? ' ticket-inspector__ph-note--card' : ''}`}>
    <div class="ticket-inspector__ph-note-header">
      <span class="ticket-inspector__ph-note-kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{presentation.label}</span>
      <Skeleton width="2.5rem" height="0.6875rem" />
    </div>
    <div class="ticket-inspector__ph-note-body"><Skeleton /><Skeleton width="45%" /></div>
  </div>;
}

/**
 * Loading placeholder for the ticket inspector (HS2-REG3A2): it renders the REAL inspector
 * chrome — header, Kerf tab bar, metadata controls, and section headers — with the unknown
 * ticket values shown as subtle, unanimated placeholder blocks. Since kerf-ui 5.0.0-beta.7 the
 * value slots use the framework's native component `placeholder` mode and `Skeleton` block, so the
 * metadata controls track the real Select sizes automatically instead of hand-maintained CSS
 * (HS2-KWWSWY). The inspector still looks like the inspector; only the per-ticket values are absent.
 */
export function TicketInspectorSkeleton({ slug }: { slug?: string } = {}) {
  const actions = <ToolbarControlGroup appearance="borderless" label="Ticket actions">
    <button type="button" aria-label="Open ticket reader" title="Open ticket reader" tabIndex={-1}><LucideIcon icon={BookOpen} name="book-open" /></button>
    <button type="button" data-action="close-ticket-inspector" aria-label="Hide inspector" title="Hide inspector"><LucideIcon icon={PanelRightClose} name="panel-right-close" /></button>
  </ToolbarControlGroup>;
  return <aside class="ticket-inspector ticket-inspector--placeholder" data-component="ticket-inspector-skeleton" aria-busy="true" aria-label="Loading ticket">
    <header class="ticket-inspector__header">
      <Toolbar divider={false} center={slug ? <ToolbarText text={slug} size="small" /> : <Skeleton width="5.5rem" height="1rem" />} trailing={actions} />
      <div class="ticket-inspector__ph-title" aria-hidden="true"><Skeleton height="1.25rem" /><Skeleton width="62%" height="1.25rem" /></div>
    </header>
    <div aria-hidden="true"><TabBar id="ticket-inspector-loading" label="Ticket inspector sections" className="ticket-inspector__tabs">{TABS.map(tab => <AppTab id={tab.id} name={tab.label} selected={tab.id === 'info'} closable={false} placeholder leading={<LucideIcon icon={tab.icon} name={tab.iconName} />} />)}</TabBar></div>
    <div class="ticket-inspector__content" aria-hidden="true">
      <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
        <TicketCategorySelect name="inspector-category" value="" placeholder />
        <TicketPrioritySelect name="inspector-priority" value="default" placeholder />
        <div class="ticket-inspector__status-field"><span>Status</span><span class="ticket-inspector__status-line"><TicketStatusMenu value="not_started" placeholder /></span></div>
      </section>
      <section class="ticket-inspector__section"><ListItem className="ticket-inspector__block-action" action="block-ticket" icon={<LucideIcon icon={Plus} name="plus" />} label="Block ticket" tabIndex={-1} /></section>
      <section class="ticket-inspector__section"><ListHeader label="Details" /><div class="ticket-inspector__details-surface"><div class="ticket-inspector__ph-lines"><Skeleton lines={3} /></div></div></section>
      <section class="ticket-inspector__section"><ListHeader label="Tags" action="add-tag" actionLabel="Add tag" actionIcon={<LucideIcon icon={Plus} name="plus" />} /></section>
      <section class="ticket-inspector__section"><ListHeader label="Notes" action="add-note" actionLabel="Add note" actionIcon={<LucideIcon icon={Plus} name="plus" />} /><div class="ticket-inspector__ph-notes"><PlaceholderNote kind="activity" /><PlaceholderNote kind="activity" /><PlaceholderNote kind="regular" card /></div></section>
      <footer class="ticket-inspector__ph-provenance"><Skeleton width="6rem" height="0.6875rem" /><Skeleton width="4rem" height="0.6875rem" /></footer>
    </div>
  </aside>;
}
