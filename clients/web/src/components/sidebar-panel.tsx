import { collapsiblePanelToggleIcon } from '@kerfjs/ui/collapsible-panel';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane, type PaneConfig } from '@kerfjs/ui/pane';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import type { WorkbenchPanel, WorkbenchPanelToggle, WorkbenchPanelToolbar } from '@kerfjs/ui/workbench';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

/**
 * The parts of a shell side panel (HS2-RWGQWN). The application shell hands them to Kerf's
 * `Workbench`, which composes the panel's toolbar with the standard collapse `toggle` (carrying
 * `aria-controls`/`aria-expanded`) and relocates that toggle into the work area's toolbar while the
 * panel is collapsed; a standalone surface (the UX catalog) renders the same parts as a Kerf `Pane`
 * through {@link SidebarPane}.
 */
export interface SidebarPanelParts {
  /** Accessible name of the standalone Pane landmark. */
  label: string;
  /** The panel's top toolbar without its toggle (the Workbench and {@link SidebarPane} add it). */
  toolbar: Omit<WorkbenchPanelToolbar, 'toggle'>;
  /** The standard collapse toggle: its `data-action` and short name ("project sidebar"). */
  toggle: WorkbenchPanelToggle;
  content: SafeHtml;
  footer?: SafeHtml;
  /** The panel Pane's content semantics and safe-area edges. */
  pane: PaneConfig;
}

/** The Workbench panel fields a {@link SidebarPanelParts} provides; the shell adds state and sizing. */
export function workbenchSidebarPanel(
  parts: SidebarPanelParts,
): Pick<WorkbenchPanel, 'content' | 'toolbar' | 'footer' | 'pane'> {
  return {
    content: parts.content,
    toolbar: { ...parts.toolbar, toggle: parts.toggle },
    footer: parts.footer,
    pane: parts.pane,
  };
}

/**
 * Renders a side panel standalone as a Kerf `Pane`: the toolbar zones as its header (plus the
 * left-rail collapse toggle when `collapseControl` is set, mirroring the Workbench's standard toggle
 * while the panel is open), the content, and the footer.
 */
export function SidebarPane({
  parts,
  className,
  collapseControl = false,
}: {
  parts: SidebarPanelParts;
  className: string;
  collapseControl?: boolean;
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
        <LucideIcon {...collapsiblePanelToggleIcon('left', false)} />
      </button>
    </ToolbarControlGroup>
  ) : undefined;
  const header =
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
  return (
    <Pane
      element="aside"
      className={className}
      label={parts.label}
      header={header}
      footer={parts.footer}
      {...parts.pane}
    >
      {parts.content}
    </Pane>
  );
}
