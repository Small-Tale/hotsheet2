import './terminal-ticket-rail.css';

import { Select } from '@kerfjs/ui/select';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import { ContentTransition, type ContentTransitionDirection } from './content-transition';
import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';

export interface TerminalTicketRailProps {
  projects: readonly { id: string; name: string }[];
  selectedProjectId: string;
  views?: readonly { id: string; label: string }[];
  selectedViewId?: string;
  controls: SafeHtml;
  content: SafeHtml;
  inspector: SafeHtml;
  active: 'root' | 'ticket';
  direction?: ContentTransitionDirection;
  title?: string;
  action?: SafeHtml;
}

/**
 * The workspace grid's ticket rail as Workbench panel parts (HS2-QQW6CT): the panel toolbar holds the
 * project selector and the standard rail toggle, and the content keeps the rail's push navigation
 * between the ticket collection and the pushed ticket detail, which renders standalone inside it.
 */
export function terminalTicketRailPanel({
  projects,
  selectedProjectId,
  views = [],
  selectedViewId = 'all',
  controls,
  content,
  inspector,
  active,
  direction = 'forward',
  title = 'Queue',
  action,
}: TerminalTicketRailProps): SidebarPanelParts {
  const heading = views.length ? (
    <div class="terminal-ticket-rail__view">
      <Select
        presentation="toolbar-borderless"
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
  const root = (
    <section class="terminal-ticket-rail__root" aria-label="Project tickets">
      {/* The shared WorkspaceControls groups are genuine Toolbar zone content here too (HS2-K9KWJJ). The
          rail is always narrower than Kerf's stack breakpoint, so the zone stacks and wraps at group
          granularity: the full-width view switcher takes the first row; sort, selection actions, and the
          collapsible search share the next; an expanded search wraps onto its own full row. */}
      <Toolbar
        className="terminal-ticket-rail__controls"
        label="Ticket rail controls"
        dividerSides=""
        responsive="stack"
        responsiveAt="narrow"
        trailing={controls}
      />
      <div class="terminal-ticket-rail__heading">
        <Toolbar dividerSides="" leading={heading} trailing={action} />
      </div>
      <div class="terminal-ticket-rail__content">
        <SunkenPanel shape="square">{content}</SunkenPanel>
      </div>
    </section>
  );
  return {
    label: 'Ticket rail',
    toolbar: {
      label: 'Ticket rail toolbar',
      dividerSides: '',
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
    toggle: inspectorToggle('ticket rail'),
    content: (
      <section
        class="terminal-ticket-rail"
        data-component="terminal-ticket-rail"
        data-screen={active}
        aria-label="Ticket rail"
      >
        <ContentTransition
          active={active === 'ticket' ? 'b' : 'a'}
          direction={direction}
          a={root}
          b={inspector}
          label="Ticket navigation"
        />
      </section>
    ),
    pane: {},
  };
}

/** The ticket rail rendered standalone from its panel parts (the UX catalog). */
export function TerminalTicketRail({
  collapseControl = false,
  ...props
}: TerminalTicketRailProps & { collapseControl?: boolean }) {
  return <SidebarPane parts={terminalTicketRailPanel(props)} side="right" collapseControl={collapseControl} />;
}
