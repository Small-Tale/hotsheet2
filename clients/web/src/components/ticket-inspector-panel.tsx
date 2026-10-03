import './ticket-inspector-panel.css';

export type TicketInspectorPanelPresentation = 'sidebar' | 'reader';

export interface TicketInspectorPanelProps {
  /** The composing tab panel's `data-component` identity (for example `ticket-info-panel`). */
  component: string;
  /** `reader` gives the reader modal's generous content inset instead of the sidebar's 8px column. */
  presentation?: TicketInspectorPanelPresentation;
  /** The composing panel's own root class, merged onto this root. */
  className?: string;
  /** Extra root attributes the composing panel owns (drop-target flags, ARIA). */
  attributes?: Record<string, string | undefined>;
  children?: unknown;
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
  const classes = ['ticket-inspector-panel'];
  if (presentation === 'reader') classes.push('ticket-inspector-panel--reader');
  if (className) classes.push(className);
  return (
    <div class={classes.join(' ')} data-component={component} {...attributes}>
      {children}
    </div>
  );
}
