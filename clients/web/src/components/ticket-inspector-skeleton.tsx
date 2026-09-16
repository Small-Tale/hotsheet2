import '@awesome.me/webawesome/dist/components/button/button.js';
import './ticket-inspector.css';
import './ticket-inspector-panel.css';
import './ticket-inspector-skeleton.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { MenuHeader } from '@kerfjs/ui/menu-header';
import { MenuItem } from '@kerfjs/ui/menu-item';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { Activity, BookOpen, Info, ListTree, MessageSquareCode, MessageSquareText, PanelRightClose, Paperclip, Plus } from 'lucide';

const TABS = [
  { id: 'info', label: 'Info', icon: Info, iconName: 'info' },
  { id: 'timeline', label: 'Timeline', icon: ListTree, iconName: 'list-tree' },
  { id: 'code-review', label: 'Code Review', icon: MessageSquareCode, iconName: 'message-square-code' },
  { id: 'attachments', label: 'Attachments', icon: Paperclip, iconName: 'paperclip' },
] as const;

/** One placeholder note entry mirroring a note card's header (kind + time) with a placeholder body. */
function PlaceholderNote({ kind, card = false }: { kind: 'activity' | 'regular'; card?: boolean }) {
  const presentation = kind === 'activity' ? { label: 'Activity', icon: Activity, iconName: 'activity' } : { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' };
  return <div class={`ticket-inspector__ph-note${card ? ' ticket-inspector__ph-note--card' : ''}`}>
    <div class="ticket-inspector__ph-note-header">
      <span class="ticket-inspector__ph-note-kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{presentation.label}</span>
      <span class="ticket-inspector__ph ticket-inspector__ph-note-time"></span>
    </div>
    <div class="ticket-inspector__ph-note-body"><span class="ticket-inspector__ph"></span><span class="ticket-inspector__ph"></span></div>
  </div>;
}

/**
 * Loading placeholder for the ticket inspector (HS2-REG3A2): it renders the REAL inspector
 * chrome — header, segmented tab bar, metadata controls, and section headers — with the
 * unknown ticket values shown as subtle, unanimated placeholder blocks. The inspector still
 * looks like the inspector; only the specific per-ticket values are absent while the next
 * ticket loads. (A general component-level `placeholder` mode is tracked upstream in kerf; see
 * the completing note.)
 */
export function TicketInspectorSkeleton({ slug }: { slug?: string } = {}) {
  const actions = <ToolbarControlGroup appearance="borderless" label="Ticket actions">
    <button type="button" aria-label="Open ticket reader" title="Open ticket reader" tabIndex={-1}><LucideIcon icon={BookOpen} name="book-open" /></button>
    <button type="button" data-action="close-ticket-inspector" aria-label="Hide inspector" title="Hide inspector"><LucideIcon icon={PanelRightClose} name="panel-right-close" /></button>
  </ToolbarControlGroup>;
  return <aside class="ticket-inspector ticket-inspector--placeholder" data-component="ticket-inspector-skeleton" aria-busy="true" aria-label="Loading ticket">
    <header class="ticket-inspector__header">
      <Toolbar divider={false} center={slug ? <ToolbarText text={slug} size="small" /> : <span class="ticket-inspector__ph ticket-inspector__ph-slug" aria-hidden="true"></span>} trailing={actions} />
      <div class="ticket-inspector__ph-title" aria-hidden="true"><span class="ticket-inspector__ph"></span><span class="ticket-inspector__ph"></span></div>
    </header>
    <nav class="ticket-inspector__tabs" aria-label="Ticket inspector sections" aria-hidden="true">{TABS.map(tab => <button type="button" tabIndex={-1} aria-current={tab.id === 'info' ? 'page' : undefined} data-key={tab.id}><LucideIcon icon={tab.icon} name={tab.iconName} /><span class="ticket-inspector__tab-label">{tab.label}</span></button>)}</nav>
    <div class="ticket-inspector__content" aria-hidden="true">
      <section class="ticket-inspector__ph-metadata" aria-label="Ticket metadata">
        <div class="ticket-inspector__ph-field"><span>Category</span><div class="ticket-inspector__ph ticket-inspector__ph-control"></div></div>
        <div class="ticket-inspector__ph-field"><span>Priority</span><div class="ticket-inspector__ph ticket-inspector__ph-control"></div></div>
        <div class="ticket-inspector__ph-field ticket-inspector__ph-field--status"><span>Status</span><div class="ticket-inspector__ph ticket-inspector__ph-status"></div></div>
      </section>
      <section class="ticket-inspector__section"><MenuItem className="ticket-inspector__block-action" action="block-ticket" icon={<LucideIcon icon={Plus} name="plus" />} label="Block ticket" tabIndex={-1} /></section>
      <section class="ticket-inspector__section"><MenuHeader label="Details" /><div class="ticket-inspector__details-surface"><div class="ticket-inspector__ph-lines"><span class="ticket-inspector__ph ticket-inspector__ph-line"></span><span class="ticket-inspector__ph ticket-inspector__ph-line"></span><span class="ticket-inspector__ph ticket-inspector__ph-line"></span></div></div></section>
      <section class="ticket-inspector__section"><MenuHeader label="Tags" action="add-tag" actionLabel="Add tag" actionIcon={<LucideIcon icon={Plus} name="plus" />} /></section>
      <section class="ticket-inspector__section"><MenuHeader label="Notes" action="add-note" actionLabel="Add note" actionIcon={<LucideIcon icon={Plus} name="plus" />} /><div class="ticket-inspector__ph-notes"><PlaceholderNote kind="activity" /><PlaceholderNote kind="activity" /><PlaceholderNote kind="regular" card /></div></section>
      <footer class="ticket-inspector__ph-provenance"><span class="ticket-inspector__ph ticket-inspector__ph-prov-a"></span><span class="ticket-inspector__ph ticket-inspector__ph-prov-b"></span></footer>
    </div>
  </aside>;
}
