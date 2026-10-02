import { collapsiblePanelToggleIcon } from '@kerfjs/ui/collapsible-panel';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane, type PaneConfig, type PaneElement } from '@kerfjs/ui/pane';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import type { WorkbenchPanelToggle, WorkbenchPanelToolbar, WorkbenchStaticPanel } from '@kerfjs/ui/workbench';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

/**
 * The parts of a shell side panel (HS2-RWGQWN, HS2-QQW6CT). The application shell hands them to
 * Kerf's `Workbench`, which composes the panel's toolbar with the standard collapse `toggle`
 * (carrying `aria-controls`/`aria-expanded`), pins the optional `header` under it, and relocates the
 * toggle into the work area's toolbar while the panel is collapsed (the leading edge for the left
 * rail, the trailing edge for the right rail). A standalone surface (the UX catalog, the ticket
 * reader modal, the terminal rail's pushed inspector) renders the same parts as a Kerf `Pane` through
 * {@link SidebarPane}.
 */
export interface SidebarPanelParts {
  /** Accessible name of the standalone Pane landmark. */
  label: string;
  /** The panel's top toolbar without its toggle (the Workbench and {@link SidebarPane} add it). */
  toolbar: Omit<WorkbenchPanelToolbar, 'toggle'>;
  /** The standard collapse toggle: its `data-action` and short name ("project sidebar"). */
  toggle: WorkbenchPanelToggle;
  /** Fixed chrome under the toolbar, above the scrolling content (an inspector's title and tabs). */
  header?: SafeHtml;
  content: SafeHtml;
  footer?: SafeHtml;
  /** The panel Pane's content semantics and safe-area edges. */
  pane: PaneConfig;
}

/**
 * The standard collapse toggle every right-rail surface shares (HS2-QQW6CT): the Workbench renders it
 * in the open rail's toolbar and relocates it to the trailing edge of the workspace toolbar while the
 * rail is collapsed, so one action both hides and shows the rail.
 */
export const INSPECTOR_TOGGLE_ACTION = 'toggle-ticket-inspector';
export const inspectorToggle = (name = 'ticket inspector'): WorkbenchPanelToggle => ({
  action: INSPECTOR_TOGGLE_ACTION,
  name,
});

/** The Workbench panel fields a {@link SidebarPanelParts} provides; the shell adds state and sizing. */
export function workbenchSidebarPanel(
  parts: SidebarPanelParts,
): Pick<WorkbenchStaticPanel, 'content' | 'toolbar' | 'header' | 'footer' | 'pane'> {
  return {
    content: parts.content,
    toolbar: { ...parts.toolbar, toggle: parts.toggle },
    header: parts.header,
    footer: parts.footer,
    pane: parts.pane,
  };
}

/**
 * Renders a side panel standalone as a Kerf `Pane`: the toolbar zones and the fixed `header` as its
 * header (plus the collapse toggle for the panel's `side` when `collapseControl` is set, mirroring
 * the Workbench's standard toggle while the panel is open), the content, and the footer.
 */
export function SidebarPane({
  parts,
  className,
  collapseControl = false,
  side = 'left',
  element = 'aside',
}: {
  parts: SidebarPanelParts;
  className?: string;
  collapseControl?: boolean;
  /** The rail the panel docks to, which picks the collapse toggle's glyph. */
  side?: 'left' | 'right';
  /** The Pane's root element; an app-owned landmark wrapper passes `div`. */
  element?: PaneElement;
}) {
  const { label: toolbarLabel, title, leading, center, trailing, ...toolbarConfig } = parts.toolbar;
  const toggle = collapseControl ? (
    <ToolbarControlGroup appearance="borderless" single label={parts.toggle.name}>
      <button
        type="button"
        data-action={parts.toggle.action}
        aria-expanded="true"
        aria-label={parts.toggle.hideLabel ?? `Hide ${parts.toggle.name}`}
        title={parts.toggle.hideLabel ?? `Hide ${parts.toggle.name}`}
      >
        <LucideIcon {...collapsiblePanelToggleIcon(side, false)} />
      </button>
    </ToolbarControlGroup>
  ) : undefined;
  const toolbar =
    title || leading || center || trailing || toggle ? (
      <Toolbar
        {...toolbarConfig}
        label={toolbarLabel}
        leading={
          title || leading ? (
            <>
              {title}
              {leading}
            </>
          ) : undefined
        }
        center={center}
        trailing={
          trailing || toggle ? (
            <>
              {trailing}
              {toggle}
            </>
          ) : undefined
        }
      />
    ) : undefined;
  const header =
    toolbar || parts.header ? (
      <>
        {toolbar}
        {parts.header}
      </>
    ) : undefined;
  return (
    <Pane
      element={element}
      className={className}
      label={element === 'div' ? undefined : parts.label}
      header={header}
      footer={parts.footer}
      {...parts.pane}
    >
      {parts.content}
    </Pane>
  );
}
