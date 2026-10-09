import '@kerfjs/ui/nav-stack.css';
import './terminal-ticket-rail.css';

import { NavStack, type NavStackView } from '@kerfjs/ui/nav-stack';
import { Select } from '@kerfjs/ui/select';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import {
  inspectorToggle,
  type NavigationPanelParts,
  PanelCollapseControl,
  type SidebarPanelParts,
} from './sidebar-panel';
import { TicketViewAction, type TicketViewActionSpec } from './workspace-controls';

/** The NavStack id of the workspace grid's ticket rail (`wireNavStack` finds it by this). */
export const TERMINAL_TICKET_RAIL_STACK_ID = 'terminal-ticket-rail-stack';
/** The rail's root view key; a pushed ticket's key is `ticket:<slug>`. */
export const TERMINAL_TICKET_RAIL_ROOT_KEY = 'root';

export interface TerminalTicketRailProps {
  projects: readonly { id: string; name: string }[];
  selectedProjectId: string;
  views?: readonly { id: string; label: string }[];
  selectedViewId?: string;
  controls: SafeHtml;
  content: SafeHtml;
  /**
   * The pushed ticket detail, when one is open: a stable view key and the detail surface's panel
   * parts (its toolbar `center`/`leading` and `trailing` zones, fixed `header`, and `content`).
   */
  detail?: { key: string; parts: SidebarPanelParts };
  title?: string;
  /** The rail view's primary action, rendered as literal Toolbar `trailing` content (HS2-F4P7ZA). */
  action?: TicketViewActionSpec;
}

/** A static surface's panel parts as one pushed NavStack view (HS2-FY06N4). */
function detailView(key: string, parts: SidebarPanelParts): NavStackView {
  return {
    key,
    toolbar: {
      leading: parts.toolbar.leading,
      center: parts.toolbar.center,
      trailing: parts.toolbar.trailing,
    },
    header: parts.header,
    content: parts.content,
  };
}

/**
 * The workspace grid's ticket rail as a Workbench **navigation** panel (HS2-FY06N4, Kerf
 * `KF-WW33YJ`): a controlled NavStack whose root view lists the project's tickets and whose pushed
 * view is the selected ticket's detail. The active view's toolbar is the rail's only toolbar row —
 * Kerf's back control, the view's groups, then the panel's standard toggle, which relocates to the
 * workspace toolbar while the rail is collapsed — above that view's pinned header and its one
 * scrolling content. The app owns the view array; `wireNavStack` animates push and pop.
 */
export function terminalTicketRailPanel({
  projects,
  selectedProjectId,
  views = [],
  selectedViewId = 'all',
  controls,
  content,
  detail,
  title = 'Queue',
  action,
}: TerminalTicketRailProps): NavigationPanelParts {
  const heading = views.length ? (
    <div class="terminal-ticket-rail__view">
      {/* Kerf's view-title Select: a 36px trigger flush with the list below, its caret beside the
          label, and an inset focus ring the rail's clipping ancestors never crop (HS2-HEYASQ,
          HS2-DAMHD1). */}
      <Select
        presentation="title"
        focusRingInset
        name="terminal-rail-view"
        value={selectedViewId}
        ariaLabel="Ticket rail view"
        choices={views.map((view) => ({ value: view.id, label: view.label }))}
        renderSelected={(choice) => <span>{choice.label}</span>}
      />
    </div>
  ) : (
    <ToolbarText text={title} size="xlarge-fixed" />
  );
  const root: NavStackView = {
    key: TERMINAL_TICKET_RAIL_ROOT_KEY,
    toolbar: {
      leading: (
        <ToolbarControlGroup single appearance="borderless">
          <Select
            className="terminal-ticket-rail__project"
            presentation="toolbar-borderless"
            name="terminal-rail-project"
            value={selectedProjectId}
            ariaLabel="Ticket rail project"
            triggerWidth="max-content"
            choices={projects.map((project) => ({ value: project.id, label: project.name }))}
            renderSelected={(choice) => <span>{choice.label}</span>}
          />
        </ToolbarControlGroup>
      ),
    },
    header: (
      <div class="terminal-ticket-rail__header" data-component="terminal-ticket-rail-header">
        {/* The shared WorkspaceControls groups stay on one row while expanded search fits the
            available rail width; lower-priority actions move into More when space runs out. */}
        <Toolbar
          className="terminal-ticket-rail__controls"
          label="Ticket rail controls"
          dividerSides=""
          responsive="none"
          trailing={controls}
        />
        <div class="terminal-ticket-rail__heading">
          <Toolbar dividerSides="" leading={heading} trailing={action && <TicketViewAction action={action} />} />
        </div>
      </div>
    ),
    // The view's sunken scroll surface replaces the app's own scroller and SunkenPanel.
    pane: { appearance: 'sunken' },
    content: (
      <section
        class="terminal-ticket-rail__list"
        data-component="terminal-ticket-rail-list"
        aria-label="Project tickets"
      >
        {content}
      </section>
    ),
  };
  return {
    label: 'Ticket rail',
    toolbar: { label: 'Ticket rail toolbar', dividerSides: '' },
    toggle: inspectorToggle('ticket rail'),
    navStack: {
      id: TERMINAL_TICKET_RAIL_STACK_ID,
      label: 'Ticket navigation',
      backLabel: 'Back to ticket list',
      // The pushed detail's ticket number follows Kerf's back control in the leading zone, so the back
      // control, the number, the actions, and the toggle share one row at the rail's narrowest width.
      toolbarConfig: { label: 'Ticket rail toolbar', dividerSides: '' },
      views: detail ? [root, detailView(detail.key, detail.parts)] : [root],
    },
  };
}

/**
 * The ticket rail rendered standalone from its navigation parts (the UX catalog): the same NavStack,
 * with the rail's collapse control as its persistent last toolbar group when `collapseControl` is set.
 */
export function TerminalTicketRail({
  collapseControl = false,
  ...props
}: TerminalTicketRailProps & { collapseControl?: boolean }) {
  const parts = terminalTicketRailPanel(props);
  return (
    <aside class="terminal-ticket-rail" data-component="terminal-ticket-rail" aria-label={parts.label}>
      <NavStack
        {...parts.navStack}
        panelToggle={collapseControl ? <PanelCollapseControl toggle={parts.toggle} side="right" /> : undefined}
      />
    </aside>
  );
}
