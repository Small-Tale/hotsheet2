import './context-menu-anchor.css';

import type { SafeHtml } from 'kerfjs';

import { contextPopupMenuAnchor } from '../context-menu-position';

/**
 * The fixed, 1px wrapper a context-mode Kerf PopupMenu opens from: it names the menu for delegated
 * dismissal and tests, and records the viewport anchor `revealContextPopupMenu` hands to Kerf
 * (HS2-2EHD8R). Render it inside the active surface so the menu is never inert beneath a dialog.
 */
export function ContextMenuAnchor({
  x,
  y,
  component,
  label,
  attributes,
  children,
}: {
  x: number;
  y: number;
  /** `data-component` name that identifies the menu for delegated dismissal and tests. */
  component: string;
  label: string;
  /** Extra identifying `data-*` attributes (the menu's target ids). */
  attributes?: Record<string, string | undefined>;
  children: SafeHtml;
}) {
  return (
    <div
      class="context-menu-anchor"
      data-component={component}
      role="menu"
      aria-label={label}
      {...attributes}
      {...contextPopupMenuAnchor(x, y)}
    >
      {children}
    </div>
  );
}
