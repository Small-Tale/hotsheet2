import { TicketAttachments } from '../components/ticket-attachments';
import { TicketCategorySelect } from '../components/ticket-category-select';
import { TicketInfoPanel } from '../components/ticket-info-panel';
import { TicketPrioritySelect } from '../components/ticket-priority-select';
import { TicketStatusMenu } from '../components/ticket-status-menu';
import { TicketTimeline } from '../components/ticket-timeline';

export function TicketCategorySelectDemo() { return <section class="metadata-control-demo" aria-label="TicketCategorySelect demo"><TicketCategorySelect name="demo-category" value="feature" /></section>; }
export function TicketPrioritySelectDemo() { return <section class="metadata-control-demo" aria-label="TicketPrioritySelect demo"><TicketPrioritySelect name="demo-priority" value="urgent" /></section>; }
export function TicketStatusMenuDemo() { return <section class="metadata-control-demo" aria-label="TicketStatusMenu demo"><div><span>Status</span><TicketStatusMenu value="started" /></div></section>; }
export function TicketInfoPanelDemo() { return <section class="inspector-panel-demo" aria-label="TicketInfoPanel demo"><TicketInfoPanel status="started" priority="high" category="feature" tags={['client', 'ux']} details={'## Implementation notes\n\nBuild the reusable metadata and details presentation independently from the inspector shell.'} blockedReason="Waiting for final design review." notes={[{ id: 'review', kind: 'regular', author: 'Claude', time: '10 minutes ago', body: 'The metadata and notes now share the inspector’s controlled state.' }]} providerName="Hot Sheet git" updatedLabel="Updated 2 minutes ago" /></section>; }
export function TicketTimelineDemo() { return <section class="inspector-panel-demo" aria-label="TicketTimeline demo"><TicketTimeline /></section>; }
export function TicketAttachmentsDemo() { return <section class="inspector-panel-demo" aria-label="TicketAttachments demo"><TicketAttachments /></section>; }
