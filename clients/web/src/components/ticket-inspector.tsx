import '@awesome.me/webawesome/dist/components/button/button.js';
import { Info, ListTree, Paperclip, Star, X } from 'lucide';
import { LucideIcon } from './lucide-icon';
import type { TicketStatus } from './status-badge';
import type { TicketPriority } from './ticket-row';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketTimeline } from './ticket-timeline';
import { TicketAttachments } from './ticket-attachments';
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
  return <aside class="ticket-inspector" data-component="ticket-inspector" aria-label={`${slug} inspector`}>
    <header class="ticket-inspector__header">
      <div><span>{slug}</span><h1>{title}</h1></div>
      <div><button type="button" class={`ticket-inspector__star${upNext ? ' ticket-inspector__star--active' : ''}`} data-action="toggle-inspector-up-next" aria-label={upNext ? 'Remove from Up Next' : 'Add to Up Next'}><LucideIcon icon={Star} name="star" /></button><button type="button" data-action="close-ticket-inspector" aria-label="Close inspector"><LucideIcon icon={X} name="x" /></button></div>
    </header>
    <nav class="ticket-inspector__tabs" aria-label="Ticket inspector sections">{tabs.map(tab => <button type="button" data-action="set-inspector-tab" data-inspector-tab={tab.id} aria-label={tab.label} aria-current={activeTab === tab.id ? 'page' : undefined}><LucideIcon icon={tab.icon} name={tab.iconName} /><span>{tab.label}</span></button>)}</nav>
    {activeTab === 'info' && <TicketInfoPanel status={status} priority={priority} category={category} tags={tags} details={details} />}
    {activeTab === 'timeline' && <TicketTimeline noteCount={noteCount} />}
    {activeTab === 'attachments' && <TicketAttachments attachmentCount={attachmentCount} />}
  </aside>;
}
