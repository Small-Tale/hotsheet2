import './ticket-inspector-placeholder.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { PanelRightClose } from 'lucide';

export function TicketInspectorPlaceholder({ selectionCount }: { selectionCount: number }) {
  return <aside class="ticket-inspector-placeholder" aria-label="Ticket inspector">
    <Toolbar divider={false} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="close-ticket-inspector" aria-label="Hide ticket inspector" title="Hide ticket inspector"><LucideIcon icon={PanelRightClose} name="panel-right-close" /></button></ToolbarControlGroup>} />
    <p>{selectionCount === 0 ? 'Select a ticket to see and edit its details' : `${selectionCount} items selected — use batch actions to edit them together`}</p>
  </aside>;
}
