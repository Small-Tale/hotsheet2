import { Badge } from '@kerfjs/ui/badge';
import { rem } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { TreeList, type TreeListNode } from '@kerfjs/ui/tree-list';
import { FolderOpen, House, Layers, MoreHorizontal, Plus, Workflow } from 'lucide';

import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import {
  expandedProjectGroups,
  flattenProjectTree,
  projectTree,
  type ProjectTreeNode,
  selectedProjectNode,
} from '../orchestration-project-tree';
import type { AddChoice, OrchestrationProjectRow, SiteChoice } from '../orchestration-settings-model';
import { OrchestrationPolicySettings, type OrchestrationPolicySettingsProps } from './orchestration-policy-settings';
import { placeIcon, projectActionItems } from './orchestration-project-actions';
import {
  OrchestrationTemplateSettings,
  type OrchestrationTemplateSettingsProps,
} from './orchestration-template-settings';

/** HS2-92FJXM: one checkout the serving machine has open. */
export interface HostedProjectChoiceView {
  checkoutId: string;
  name: string;
  added: boolean;
}

export interface OrchestrationSettingsProps {
  rows?: readonly OrchestrationProjectRow[];
  /** The + Add Project menu's open projects. */
  choices?: readonly AddChoice[];
  /** Open project tabs, so a row's Show Project Tab is enabled only when its tab is open. */
  openTabs?: readonly string[];
  /** The project whose detail the inspector shows. */
  selectedTarget?: string;
  /** HS2-M1T6KN: the tree row the person chose, when it is a project group standing for its home site. */
  selectedNode?: string;
  /** HS2-M1T6KN: the orchestration project, whose closed project groups `collapsedGroups` names. */
  ownerId?: string;
  /** HS2-M1T6KN: closed project groups as `projectCollapseKey`s; every other group is open. */
  collapsedGroups?: ReadonlySet<string>;
  loading?: boolean;
  /** Targets, `add` or `policy` with a change in flight. */
  busy?: readonly string[];
  /** A read failure, or the persistent record of a failed change; a change's outcome is a toast (HS2-1K6122). */
  message?: string;
  /** Why projects cannot be added here now, such as a remote server's list that could not be read. */
  unavailableReason?: string;
  /**
   * HS2-92FJXM: for an orchestration project open from another server, the checkouts that server has
   * open, which + Add Project offers instead of this machine's tabs. Absent for a project served here.
   */
  hostedChoices?: readonly HostedProjectChoiceView[];
  /** The name of that server, for the menu's heading. */
  hostedServer?: string;
  policy?: OrchestrationPolicySettingsProps;
  /** HS2-1XCW6N: Settings › Orchestration › Templates; absent where templates are not offered. */
  templates?: OrchestrationTemplateSettingsProps;
  /**
   * HS2-E9VVQF: each project row's Add a Site choices, by the project's target id. Absent for a
   * project open from another server, which offers no Add a Site.
   */
  siteChoices?: Readonly<Record<string, readonly SiteChoice[]>>;
  /** Add a Site lists every open project, not only those of the same ticket store. */
  showAllSites?: boolean;
  /**
   * HS2-89YNGN: the site choices are another server's open checkouts, so Add a Site has no Open Another
   * Project…: nothing new is opened on that server.
   */
  hostedSites?: boolean;
}

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;

function addMenuItems(choices: readonly AddChoice[], busy: boolean): PopupMenuEntry[] {
  return [
    { kind: 'heading', label: 'Open Projects' },
    ...(choices.length
      ? choices.map((choice) => ({
          label: choice.name,
          action: ACTIONS.addOpenProject.value,
          icon: placeIcon(choice.location),
          checked: choice.added,
          disabled: busy,
          details: (
            <Text variant="span" tone="quiet" size="compact">
              {`${choice.location === 'remote' ? 'Remote' : 'Local'} · ${choice.machine}`}
            </Text>
          ),
          attributes: { 'data-project-id': choice.projectId },
        }))
      : [{ label: 'No other projects are open', disabled: true }]),
    { kind: 'divider' },
    {
      label: 'Open Another Project…',
      action: ACTIONS.openAnotherProject.value,
      icon: <LucideIcon icon={FolderOpen} name="folder-open" />,
      disabled: busy,
    },
  ];
}

/** HS2-92FJXM: + Add Project for a project open from another server: only that server's open projects. */
function hostedMenuItems(choices: readonly HostedProjectChoiceView[], server: string, busy: boolean): PopupMenuEntry[] {
  return [
    { kind: 'heading', label: `Open on ${server}` },
    ...(choices.length
      ? choices.map((choice) => ({
          label: choice.name,
          action: ACTIONS.addHostedProject.value,
          icon: placeIcon('remote'),
          checked: choice.added,
          disabled: busy,
          attributes: { 'data-checkout-id': choice.checkoutId },
        }))
      : [{ label: 'No other projects are open there', disabled: true }]),
  ];
}

interface ProjectsTreeInput {
  rows: readonly OrchestrationProjectRow[];
  labels: ReadonlyMap<string, string>;
  openTabs: readonly string[];
  selectedTarget?: string;
  selectedNode?: string;
  ownerId: string;
  collapsedGroups: ReadonlySet<string>;
  busy: readonly string[];
  adding: boolean;
  siteChoices?: Readonly<Record<string, readonly SiteChoice[]>>;
  showAllSites: boolean;
  hostedSites: boolean;
}

const nodeIcon = (node: ProjectTreeNode) =>
  node.kind === 'project' ? (
    <LucideIcon icon={Layers} name="layers" />
  ) : node.kind === 'home' ? (
    <LucideIcon icon={House} name="house" />
  ) : (
    placeIcon(node.row.location)
  );

/**
 * HS2-M1T6KN (Design v1 Concept A): the projects as a Kerf `TreeList`. A project with several sites is a top-level
 * node with its home site first and then its other sites; a project with one site is a leaf. Every group is open
 * until a person closes it. TreeList rows hold no "…" menu, so each project and site has a context-mode
 * `PopupMenu` beside the tree, opened at the pointer by a right-click or at the row by the context-menu key, and
 * the same actions in its detail's actions menu in the inspector; a group opens its home site's menu.
 */
function projectsTree(input: ProjectsTreeInput) {
  const tree = projectTree(input.rows),
    busyOf = (targetId: string) => input.busy.includes(targetId);
  const toNode = (node: ProjectTreeNode): TreeListNode => ({
    id: node.id,
    label: node.label,
    description: node.description,
    icon: nodeIcon(node),
    trailing:
      node.kind !== 'project' && busyOf(node.row.targetId) ? (
        <LoadingSpinner label={`Changing ${node.label}`} />
      ) : node.status ? (
        <Badge tone={node.status.tone} appearance="quiet" size="compact">
          {node.status.label}
        </Badge>
      ) : undefined,
    rootAttributes:
      node.kind === 'project'
        ? { 'data-orchestration-project-group': node.row.targetId }
        : { 'data-orchestration-project': node.row.targetId },
    ...(node.children ? { children: node.children.map(toNode) } : {}),
  });
  const actionRows = flattenProjectTree(tree).filter((node) => node.kind !== 'project');
  return (
    <div data-orchestration-project-tree>
      <TreeList
        id="orchestration-projects"
        label="Orchestrated Projects"
        nodes={tree.map(toNode)}
        expanded={expandedProjectGroups(tree, input.ownerId, input.collapsedGroups)}
        selected={selectedProjectNode(input.selectedTarget, input.selectedNode)}
        action={ACTIONS.openProjectDetail.value}
        toggleAction={ACTIONS.toggleProjectGroup.value}
      />
      {actionRows.map(({ row }) => {
        const tabOpen = Boolean(row.tabProjectId && input.openTabs.includes(row.tabProjectId));
        return (
          <PopupMenu
            context
            label={`Actions for ${row.label}`}
            rootAttributes={{ 'data-orchestration-project-menu': row.targetId }}
            items={projectActionItems(row, {
              tabOpen,
              busy: busyOf(row.targetId),
              projectLabel: row.siteOf ? input.labels.get(row.siteOf) : undefined,
              siteChoices: input.siteChoices ? (input.siteChoices[row.targetId] ?? []) : undefined,
              showAllSites: input.showAllSites,
              adding: input.adding,
              hostedSites: input.hostedSites,
            })}
          />
        );
      })}
    </div>
  );
}

/**
 * Settings → Orchestration (HS2-W27N1D), shown only for an Orchestration project: the projects it
 * orchestrates, added with + Add Project, then the limits-only policy. Adding a project is the consent
 * to read and follow it (HS2-XHQ1EH); a row opens that project's detail in the inspector.
 */
export function OrchestrationSettings({
  rows = [],
  choices = [],
  openTabs = [],
  selectedTarget,
  selectedNode,
  ownerId = '',
  collapsedGroups = new Set<string>(),
  loading = false,
  busy = [],
  message = '',
  unavailableReason,
  hostedChoices,
  hostedServer = 'the server',
  policy = {},
  templates,
  siteChoices,
  showAllSites = false,
  hostedSites = false,
}: OrchestrationSettingsProps) {
  const adding = busy.includes('add');
  const labels = new Map(rows.map((row) => [row.targetId, row.label]));
  return (
    <section data-component="orchestration-settings">
      <List gap="l">
        <section aria-label="Projects" data-orchestration-projects>
          <List gap="m">
            <List gap="xs">
              <Text variant="h2" size="large" flush>
                Projects
              </Text>
              <Text tone="quiet" flush>
                Adding a project lets this one read and follow its tickets. Anything the orchestrator does there asks
                first, through the permission prompt.
              </Text>
            </List>
            <Row>
              <PopupMenu
                text="Add Project"
                icon={<LucideIcon icon={Plus} name="plus" />}
                emphasis="secondary"
                disabled={Boolean(unavailableReason) || loading}
                popupNote={
                  hostedChoices
                    ? `Projects open on ${hostedServer} can be added. Open others on that computer.`
                    : 'A project you open from here also opens in its own tab.'
                }
                rootAttributes={{ 'data-orchestration-add-project': '' }}
                items={
                  hostedChoices ? hostedMenuItems(hostedChoices, hostedServer, adding) : addMenuItems(choices, adding)
                }
              />
            </Row>
            {unavailableReason ? (
              <Text tone="quiet" size="compact" flush>
                {unavailableReason}
              </Text>
            ) : null}
            {message ? (
              <Text tone="danger" size="compact" role="alert" flush>
                {message}
              </Text>
            ) : null}
            {loading && rows.length === 0 ? (
              <List gap={rem(0.125)}>
                <ListActionRow
                  label="Loading"
                  action={ACTIONS.openProjectDetail.value}
                  placeholder
                  trailingMenu={{ label: 'Project Actions', icon: menuIcon, items: [] }}
                />
              </List>
            ) : rows.length === 0 ? (
              <EmptyState
                icon={<LucideIcon icon={Workflow} name="workflow" />}
                title="No projects yet."
                detail={
                  hostedChoices
                    ? `Add the projects to orchestrate from those open on ${hostedServer}.`
                    : 'Add the projects to orchestrate. Projects open in tabs are listed first.'
                }
                rootAttributes={{ 'data-orchestration-empty': '' }}
              />
            ) : (
              projectsTree({
                rows,
                labels,
                openTabs,
                selectedTarget,
                selectedNode,
                ownerId,
                collapsedGroups,
                busy,
                adding,
                siteChoices,
                showAllSites,
                hostedSites,
              })
            )}
          </List>
        </section>
        {templates ? <OrchestrationTemplateSettings {...templates} /> : null}
        <OrchestrationPolicySettings {...policy} saving={busy.includes('policy') || policy.saving} />
      </List>
    </section>
  );
}
