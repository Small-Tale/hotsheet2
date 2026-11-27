import './ticket-inspector-panel.css';

import { List, type ListProps } from '@kerfjs/ui/list';

export type TicketInspectorPanelPresentation = 'sidebar' | 'reader';

/** The composing tab panels' own root classes; a literal union so the Kerf analyzer can classify the root (HS2-5FFT4J). */
export type TicketInspectorPanelRootClass =
  'ticket-info-panel' | 'ticket-timeline' | 'ticket-code-review ticket-code-review--panel' | 'ticket-attachments';

export interface TicketInspectorPanelProps {
  /** The composing tab panel's `data-component` identity (for example `ticket-info-panel`). */
  component: string;
  /** `reader` gives the reader modal's generous content inset instead of the sidebar's 8px column. */
  presentation?: TicketInspectorPanelPresentation;
  /** The composing panel's own root class, merged onto this root. */
  className?: TicketInspectorPanelRootClass;
  /** Extra root attributes the composing panel owns (drop-target flags, ARIA). */
  attributes?: Record<string, string | undefined>;
  children?: ListProps['children'];
}

/**
 * The scrolling content column every ticket inspector tab panel composes (HS2-MGVE50): Info, Timeline,
 * Code Review, Attachments, and the Info placeholder variant. It owns the 8px-grid inset model (HS2-EQEGGG,
 * HS2-R64ETQ) — each direct child sits 8px from the inspector edge, the reader variant uses the modal's
 * inset instead — so the panels never restyle one another's markup.
 */
export function TicketInspectorPanel({
  component,
  presentation = 'sidebar',
  className,
  attributes,
  children,
}: TicketInspectorPanelProps) {
  // HS2-5FFT4J: one finite template (KF-R55A9X) so the Kerf analyzer proves every root class is app-owned.
  return (
    <div
      class={`ticket-inspector-panel${presentation === 'reader' ? ' ticket-inspector-panel--reader' : ''}${className ? ` ${className}` : ''}`}
      data-component={component}
      {...attributes}
    >
      <List gap="l">{children}</List>
    </div>
  );
}
