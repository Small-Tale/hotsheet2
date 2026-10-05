import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';

import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';

/** The empty or multi-selection inspector's panel parts for the Workbench's right rail (HS2-QQW6CT). */
export function ticketInspectorPlaceholderPanel({ selectionCount }: { selectionCount: number }): SidebarPanelParts {
  return {
    label: 'Ticket inspector',
    toolbar: { label: 'Ticket inspector toolbar', dividerSides: '' },
    toggle: inspectorToggle(),
    content: (
      <List
        fill
        hAlign="center"
        vAlign="middle"
        className="ticket-inspector-placeholder"
        rootAttributes={{ 'data-ticket-inspector-placeholder': 'true' }}
      >
        <EmptyState
          title={
            selectionCount === 0
              ? 'Select a ticket to see and edit its details'
              : `${selectionCount} items selected — use batch actions to edit them together`
          }
        />
      </List>
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
