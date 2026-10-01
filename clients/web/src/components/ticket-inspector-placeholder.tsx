import './ticket-inspector-placeholder.css';

import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';

/** The empty or multi-selection inspector's panel parts for the Workbench's right rail (HS2-QQW6CT). */
export function ticketInspectorPlaceholderPanel({ selectionCount }: { selectionCount: number }): SidebarPanelParts {
  return {
    label: 'Ticket inspector',
    toolbar: { label: 'Ticket inspector toolbar', dividerSides: '' },
    toggle: inspectorToggle(),
    content: (
      <div class="ticket-inspector-placeholder" data-component="ticket-inspector-placeholder">
        <p>
          {selectionCount === 0
            ? 'Select a ticket to see and edit its details'
            : `${selectionCount} items selected — use batch actions to edit them together`}
        </p>
      </div>
    ),
    pane: {},
  };
}

export function TicketInspectorPlaceholder({
  selectionCount,
  collapseControl = false,
}: {
  selectionCount: number;
  collapseControl?: boolean;
}) {
  return (
    <SidebarPane
      parts={ticketInspectorPlaceholderPanel({ selectionCount })}
      side="right"
      collapseControl={collapseControl}
    />
  );
}
