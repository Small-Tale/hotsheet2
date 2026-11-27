import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import type { PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { Text } from '@kerfjs/ui/text';
import { Cloud, FolderOpen, Group, House, Laptop, SquareArrowOutUpRight, Trash2, Ungroup, Unlink } from 'lucide';

import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import type { OrchestrationProjectRow, SiteChoice } from '../orchestration-settings-model';

/** Where a project or a choice lives: this computer, another machine, or not bound here. */
export const placeIcon = (location: 'local' | 'remote' | 'unbound') =>
  location === 'remote' ? (
    <LucideIcon icon={Cloud} name="cloud" />
  ) : location === 'unbound' ? (
    <LucideIcon icon={Unlink} name="unlink" />
  ) : (
    <LucideIcon icon={Laptop} name="laptop" />
  );

/**
 * HS2-E9VVQF: a project's Add a Site submenu: the open projects of its ticket store, Show All Projects for the
 * edge cases, and the regular open flow.
 */
export function siteMenuItems(
  row: OrchestrationProjectRow,
  choices: readonly SiteChoice[],
  showAll: boolean,
  busy: boolean,
  hosted = false,
): PopupMenuEntry[] {
  const attributes = { 'data-target-id': row.targetId };
  return [
    { kind: 'heading', label: showAll ? 'Open Projects' : 'Open Projects in the Same Ticket Store' },
    ...(choices.length
      ? choices.map((choice) => ({
          label: choice.name,
          action: ACTIONS.addSite.value,
          icon: placeIcon(choice.location),
          disabled: busy,
          details: (
            <Text variant="span" tone="quiet" size="compact">
              {[
                `${choice.location === 'remote' ? 'Remote' : 'Local'} · ${choice.machine}`,
                choice.sameStore === false
                  ? 'Another ticket store'
                  : choice.sameStore === undefined
                    ? 'Ticket store unknown'
                    : '',
              ]
                .filter(Boolean)
                .join(' · ')}
            </Text>
          ),
          attributes: { ...attributes, 'data-project-id': choice.projectId },
        }))
      : [
          {
            label: showAll ? 'No other projects are open' : 'No open project uses this ticket store',
            disabled: true,
          },
        ]),
    { kind: 'divider' },
    {
      label: 'Show All Projects',
      action: ACTIONS.showAllSites.value,
      checked: showAll,
      attributes,
    },
    ...(hosted
      ? []
      : [
          {
            label: 'Open Another Project…',
            action: ACTIONS.openAnotherSite.value,
            icon: <LucideIcon icon={FolderOpen} name="folder-open" />,
            disabled: busy,
            attributes,
          },
        ]),
  ];
}

/** What a project's or a site's actions need besides its row. */
export interface ProjectActionContext {
  /** Its project tab is open here, so Show Project Tab can show it. */
  tabOpen: boolean;
  /** A change to it is on its way. */
  busy: boolean;
  /** For a site, its project's label, which Separate from <project> names. */
  projectLabel?: string;
  /** Add a Site's choices; absent where no site can be added (a project open from another server, unread). */
  siteChoices?: readonly SiteChoice[];
  showAllSites?: boolean;
  /** An add is on its way. */
  adding?: boolean;
  /** HS2-89YNGN: the choices are another server's open checkouts, so there is no Open Another Project…. */
  hostedSites?: boolean;
}

/**
 * A project's or a site's actions, one list for its context menu in Settings → Orchestration → Projects and its
 * detail's actions menu in the inspector (HS2-M1T6KN): Show Project Tab; Add a Site on a project, or Make <site>
 * the Home Site and Separate from <project> on a site (HS2-E9VVQF, HS2-NAA3HV); then Stop Orchestrating….
 */
export function projectActionItems(row: OrchestrationProjectRow, context: ProjectActionContext): PopupMenuEntry[] {
  const attributes = { 'data-target-id': row.targetId };
  return [
    {
      label: 'Show Project Tab',
      action: ACTIONS.showProjectTab.value,
      icon: <LucideIcon icon={SquareArrowOutUpRight} name="square-arrow-out-up-right" />,
      disabled: !context.tabOpen,
      disabledReason: context.tabOpen ? undefined : 'Open this project in a tab first.',
      attributes: { ...attributes, 'data-project-id': row.tabProjectId },
    },
    ...(row.siteOf
      ? [
          {
            label: `Make ${row.label} the Home Site`,
            action: ACTIONS.makeHomeSite.value,
            icon: <LucideIcon icon={House} name="house" />,
            disabled: context.busy,
            attributes,
          },
          {
            label: `Separate from ${context.projectLabel ?? 'Project'}`,
            action: ACTIONS.separateSite.value,
            icon: <LucideIcon icon={Ungroup} name="ungroup" />,
            disabled: context.busy,
            attributes,
          },
        ]
      : context.siteChoices
        ? [
            {
              label: 'Add a Site',
              icon: <LucideIcon icon={Group} name="group" />,
              attributes,
              submenu: siteMenuItems(
                row,
                context.siteChoices,
                Boolean(context.showAllSites),
                Boolean(context.adding),
                context.hostedSites,
              ),
            },
          ]
        : []),
    { kind: 'divider' },
    {
      label: 'Stop Orchestrating…',
      action: ACTIONS.removeProject.value,
      tone: 'danger',
      icon: <LucideIcon icon={Trash2} name="trash-2" />,
      attributes,
    },
  ];
}
