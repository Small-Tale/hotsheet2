import '@awesome.me/webawesome/dist/components/skeleton/skeleton.js';
import './ticket-inspector-skeleton.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { PanelRightClose } from 'lucide';

/**
 * Loading placeholder for the ticket inspector: it keeps the inspector's shape (header,
 * title, tab bar, metadata fields, and body lines) while the next ticket loads, instead of
 * showing the previous ticket's stale data disabled (HS2-REG3A2).
 */
export function TicketInspectorSkeleton() {
  return <aside class="ticket-inspector-skeleton" data-component="ticket-inspector-skeleton" aria-busy="true" aria-label="Loading ticket">
    <header class="ticket-inspector-skeleton__header">
      <Toolbar divider={false} center={<wa-skeleton class="ticket-inspector-skeleton__slug" effect="sheen"></wa-skeleton>} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="close-ticket-inspector" aria-label="Hide ticket inspector" title="Hide ticket inspector"><LucideIcon icon={PanelRightClose} name="panel-right-close" /></button></ToolbarControlGroup>} />
      <wa-skeleton class="ticket-inspector-skeleton__title" effect="sheen"></wa-skeleton>
    </header>
    <div class="ticket-inspector-skeleton__tabs" aria-hidden="true">{[0, 1, 2, 3].map(index => <wa-skeleton class="ticket-inspector-skeleton__tab" effect="sheen" data-key={index}></wa-skeleton>)}</div>
    <div class="ticket-inspector-skeleton__content" aria-hidden="true">
      {[0, 1, 2].map(index => <div class="ticket-inspector-skeleton__field" data-key={index}><wa-skeleton class="ticket-inspector-skeleton__label" effect="sheen"></wa-skeleton><wa-skeleton class="ticket-inspector-skeleton__pill" effect="sheen"></wa-skeleton></div>)}
      <div class="ticket-inspector-skeleton__lines">{[0, 1, 2, 3].map(index => <wa-skeleton class="ticket-inspector-skeleton__line" effect="sheen" data-key={index}></wa-skeleton>)}</div>
    </div>
  </aside>;
}
