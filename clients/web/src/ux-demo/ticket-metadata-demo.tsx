import { TicketAttachments } from '../components/ticket-attachments';
import { TicketCategorySelect } from '../components/ticket-category-select';
import { TicketInfoPanel } from '../components/ticket-info-panel';
import { TicketPrioritySelect } from '../components/ticket-priority-select';
import { TicketTimeline } from '../components/ticket-timeline';

export function TicketCategorySelectDemo() { return <section class="metadata-control-demo" aria-label="TicketCategorySelect demo"><TicketCategorySelect name="demo-category" value="feature" /></section>; }
export function TicketPrioritySelectDemo() { return <section class="metadata-control-demo" aria-label="TicketPrioritySelect demo"><TicketPrioritySelect name="demo-priority" value="urgent" /></section>; }
export function TicketInfoPanelDemo() { return <section class="inspector-panel-demo" aria-label="TicketInfoPanel demo"><TicketInfoPanel status="started" priority="high" category="feature" tags={['client', 'ux']} details="Build the reusable metadata and details presentation independently from the inspector shell." /></section>; }
export function TicketTimelineDemo() { return <section class="inspector-panel-demo" aria-label="TicketTimeline demo"><TicketTimeline /></section>; }
export function TicketAttachmentsDemo() { return <section class="inspector-panel-demo" aria-label="TicketAttachments demo"><TicketAttachments /></section>; }
