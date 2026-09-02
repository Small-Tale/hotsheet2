import './ticket-inspector-placeholder.css';

import { PanelRightClose } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';

export function TicketInspectorPlaceholder({ selectionCount }: { selectionCount: number }) {
  return <aside class="ticket-inspector-placeholder" aria-label="Ticket inspector">
    <Toolbar divider={selectionCount > 0} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="close-ticket-inspector" aria-label="Hide ticket inspector" title="Hide ticket inspector"><LucideIcon icon={PanelRightClose} name="panel-right-close" /></button></ToolbarControlGroup>} />
    <p>{selectionCount === 0 ? 'Select a ticket to see and edit its details' : `${selectionCount} items selected — use batch actions to edit them together`}</p>
  </aside>;
}
