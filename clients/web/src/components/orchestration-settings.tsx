import { Badge } from '@kerfjs/ui/badge';
import { rem } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import {
  Cloud,
  FolderOpen,
  Laptop,
  MoreHorizontal,
  Plus,
  SquareArrowOutUpRight,
  Trash2,
  Unlink,
  Workflow,
} from 'lucide';

import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import type { AddChoice, OrchestrationProjectRow } from '../orchestration-settings-model';
import { OrchestrationPolicySettings, type OrchestrationPolicySettingsProps } from './orchestration-policy-settings';

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
  loading?: boolean;
  /** Targets, `add` or `policy` with a change in flight. */
  busy?: readonly string[];
  message?: string;
  status?: string;
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
}

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;
const placeIcon = (location: 'local' | 'remote' | 'unbound') =>
  location === 'remote' ? (
    <LucideIcon icon={Cloud} name="cloud" />
  ) : location === 'unbound' ? (
    <LucideIcon icon={Unlink} name="unlink" />
  ) : (
    <LucideIcon icon={Laptop} name="laptop" />
  );

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
  loading = false,
  busy = [],
  message = '',
  status = '',
  unavailableReason,
  hostedChoices,
  hostedServer = 'the server',
  policy = {},
}: OrchestrationSettingsProps) {
  const adding = busy.includes('add');
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
            <Text tone="quiet" size="compact" role="status" flush>
              {status}
            </Text>
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
              <List gap={rem(0.125)}>
                {rows.map((row) => {
                  const attributes = { 'data-target-id': row.targetId },
                    tabOpen = Boolean(row.tabProjectId && openTabs.includes(row.tabProjectId));
                  return (
                    <ListActionRow
                      label={row.label}
                      description={row.where || undefined}
                      status={
                        <Badge tone={row.connection.tone} appearance="quiet" size="compact">
                          {row.connection.label}
                        </Badge>
                      }
                      icon={placeIcon(row.location)}
                      iconTile="neutral"
                      action={ACTIONS.openProjectDetail.value}
                      itemId={row.targetId}
                      selected={row.targetId === selectedTarget}
                      busy={busy.includes(row.targetId)}
                      rootAttributes={{ 'data-orchestration-project': row.targetId }}
                      trailingMenu={{
                        label: `Actions for ${row.label}`,
                        icon: menuIcon,
                        items: [
                          {
                            label: 'Show Project Tab',
                            action: ACTIONS.showProjectTab.value,
                            icon: <LucideIcon icon={SquareArrowOutUpRight} name="square-arrow-out-up-right" />,
                            disabled: !tabOpen,
                            disabledReason: tabOpen ? undefined : 'Open this project in a tab first.',
                            attributes: { ...attributes, 'data-project-id': row.tabProjectId },
                          },
                          { kind: 'divider' },
                          {
                            label: 'Stop Orchestrating…',
                            action: ACTIONS.removeProject.value,
                            tone: 'danger',
                            icon: <LucideIcon icon={Trash2} name="trash-2" />,
                            attributes,
                          },
                        ],
                      }}
                    />
                  );
                })}
              </List>
            )}
          </List>
        </section>
        <OrchestrationPolicySettings {...policy} saving={busy.includes('policy') || policy.saving} />
      </List>
    </section>
  );
}
