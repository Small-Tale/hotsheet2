import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import { BookOpen, Info, ListTree, Paperclip, Star, X } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';
import type { TicketPriority } from './ticket-row';
import './ticket-inspector.css';

export type InspectorTab = 'info' | 'timeline' | 'attachments';

export interface TicketInspectorProps {
  slug: string;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  details: string;
  activeTab?: InspectorTab;
  upNext?: boolean;
  attachmentCount?: number;
  noteCount?: number;
}

const tabs = [
  { id: 'info', label: 'Info', icon: Info, iconName: 'info' },
  { id: 'timeline', label: 'Timeline', icon: ListTree, iconName: 'list-tree' },
  { id: 'attachments', label: 'Attachments', icon: Paperclip, iconName: 'paperclip' },
] as const;

export function TicketInspector({ slug, title, status, priority, category, tags, details, activeTab = 'info', upNext = false, attachmentCount = 2, noteCount = 3 }: TicketInspectorProps) {
  const categories = [...new Set([category, 'task', 'feature', 'bug', 'investigation'])];
  return <aside class="ticket-inspector" data-component="ticket-inspector" aria-label={`${slug} inspector`}>
    <header class="ticket-inspector__header">
      <div><span>{slug}</span><h1>{title}</h1></div>
      <div><button type="button" class={`ticket-inspector__star${upNext ? ' ticket-inspector__star--active' : ''}`} data-action="toggle-inspector-up-next" aria-label={upNext ? 'Remove from Up Next' : 'Add to Up Next'}><LucideIcon icon={Star} name="star" /></button><button type="button" data-action="close-ticket-inspector" aria-label="Close inspector"><LucideIcon icon={X} name="x" /></button></div>
    </header>
    <nav class="ticket-inspector__tabs" aria-label="Ticket inspector sections">{tabs.map(tab => <button type="button" data-action="set-inspector-tab" data-inspector-tab={tab.id} aria-label={tab.label} aria-current={activeTab === tab.id ? 'page' : undefined}><LucideIcon icon={tab.icon} name={tab.iconName} /><span>{tab.label}</span></button>)}</nav>
    {activeTab === 'info' && <div class="ticket-inspector__content">
      <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
        <wa-select name="inspector-category" label="Category" value={category}>{categories.map(value => <wa-option value={value}>{value}</wa-option>)}</wa-select>
        <wa-select name="inspector-priority" label="Priority" value={priority}>{(['low', 'default', 'high', 'urgent'] as const).map(value => <wa-option value={value}>{value}</wa-option>)}</wa-select>
        <div><span>Status</span><StatusBadge status={status} /></div>
      </section>
      <section><header><h2>Details</h2><wa-button appearance="plain" data-action="open-ticket-reader" aria-label="Open ticket reader"><LucideIcon icon={BookOpen} name="book-open" /></wa-button></header><p>{details}</p></section>
      <section><header><h2>Tags</h2></header><div class="ticket-inspector__tags">{tags.map((tag, index) => TagChip({ id: `inspector-tag-${index}`, label: tag }))}</div></section>
    </div>}
    {activeTab === 'timeline' && <div class="ticket-inspector__content"><section><h2>Timeline</h2><ol class="ticket-inspector__timeline"><li><time>Now</time><p>Development is active on this ticket.</p></li><li><time>1 hour ago</time><p>Ticket metadata was updated.</p></li></ol><p>{noteCount} notes total</p></section></div>}
    {activeTab === 'attachments' && <div class="ticket-inspector__content"><section><h2>Attachments</h2><div class="ticket-inspector__attachment"><LucideIcon icon={Paperclip} name="paperclip" /><span>wireframe.png</span></div><p>{attachmentCount} attachments total</p></section></div>}
  </aside>;
}
