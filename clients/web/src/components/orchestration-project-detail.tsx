import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { CheckboxGroup } from '@kerfjs/ui/checkbox-group';
import { rem } from '@kerfjs/ui/css-values';
import { Disclosure } from '@kerfjs/ui/disclosure';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Bot, CircleHelp, CircleStop, Hand, MoreHorizontal, PanelBottomOpen, Server, Trash2 } from 'lucide';

import type { OrchestrationBinding, OrchestrationSession } from '../api';
import type { OrchestrationDetailState } from '../features/orchestration-settings';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import {
  admissionValue,
  advancedLimitsChanged,
  BUILD_AHEAD_HINT,
  type ConnectionStatus,
  connectionStatus,
  listOf,
  MODELS_HINT,
  modelSuggestions,
  REQUIRED_RESOURCES_HINT,
  roleSessionsHint,
  roleSessionsLocked,
  sessionDescription,
  SESSIONS_HERE_HINT,
  sessionStatus,
  sessionTitle,
} from '../orchestration-settings-model';
import { OrchestrationPatternField } from './orchestration-pattern-field';
import { inspectorToggle, type SidebarPanelParts } from './sidebar-panel';

export interface OrchestrationProjectDetailSession {
  session: OrchestrationSession;
  /** Its project's tab is open and its drawer shows the session's terminal. */
  showable: boolean;
  busy: boolean;
}

/** HS2-D4G81Q: one site of the project this detail's target belongs to, and where it is bound. */
export interface OrchestrationProjectDetailSite {
  targetId: string;
  label: string;
  /** "Local on This Computer", "Remote on Build host", or "Not bound on this computer". */
  where: string;
  /** The project's own target, its home site. */
  home: boolean;
}

export interface OrchestrationProjectDetailProps {
  targetId: string;
  label: string;
  /** "Local on This Computer" or "Remote on Build host". */
  where: string;
  binding?: OrchestrationBinding;
  detail?: OrchestrationDetailState;
  sessions?: readonly OrchestrationProjectDetailSession[];
  /** Resource namespaces the policy requires on this project. */
  claimNamespaces?: readonly string[];
  /** The AI tools the policy sets up, which the Limits form offers (HS2-7WF6JD). */
  policyTools?: readonly string[];
  /**
   * HS2-SYZB33: when this project is another site of a project, the label of that project's home
   * site, where its coordination sessions start.
   */
  homeLabel?: string;
  /** HS2-D4G81Q: every site of the target's project, when it has more than one. */
  sites?: readonly OrchestrationProjectDetailSite[];
  /**
   * HS2-JF3EA3: the Systems machine this project runs on, for Open in Systems; absent when this
   * computer cannot tell, and Systems then opens on its current machine.
   */
  systemsMachineId?: string;
  /** HS2-1X1TXC: the models each AI tool reports, by tool, which its Models control offers. */
  toolModels?: Readonly<Record<string, readonly string[]>>;
  /** HS2-NPAZQW: the declared resources and patterns Required Resources offers. */
  resourceChoices?: readonly string[];
  removing?: boolean;
  /**
   * HS2-M1T6KN: the project's or site's actions, the same as its context menu in the Projects tree, for the
   * toolbar's actions menu.
   */
  actions?: readonly PopupMenuEntry[];
  toolLabel?: (tool: string) => string;
  /** Formats a time in milliseconds; the catalog passes a fixed clock. */
  formatTime?: (ms: number) => string;
}

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;
const defaultTime = (ms: number) => new Date(ms).toLocaleString();

const badge = (status: ConnectionStatus) => (
  <Badge tone={status.tone} appearance="quiet" size="compact">
    {status.label}
  </Badge>
);

/** A quiet line aligned with the section headers' text. */
const quiet = (text: string) => (
  <List textInsets="rl">
    <Text tone="quiet" size="compact" flush>
      {text}
    </Text>
  </List>
);

/** A section's read failed: say so in place, so the rest of the detail still shows. */
const unreadable = (what: string, message: string) => quiet(`${what} is not available. ${message}`);

/**
 * One orchestrated project's detail (HS2-W27N1D), opened from its row in Settings → Orchestration:
 * binding and connection, limits, this project's admission with Open in Systems (HS2-JF3EA3), held
 * resources and the sessions the orchestrator runs there, with Show in Drawer, Take Over and Stop. Every part is a read through
 * an existing orchestration route; a part the authorization gate refuses says so and hides nothing
 * else.
 */
export function OrchestrationProjectDetail({
  targetId,
  label,
  where,
  binding,
  detail,
  sessions = [],
  claimNamespaces = [],
  policyTools = [],
  homeLabel,
  sites = [],
  systemsMachineId,
  toolModels = {},
  resourceChoices = [],
  removing = false,
  toolLabel = (tool) => tool,
  formatTime = defaultTime,
}: OrchestrationProjectDetailProps) {
  const connection = connectionStatus(binding, detail?.resolution),
    capacity = detail?.capacity,
    loading = Boolean(detail?.loading),
    checked = detail?.resolution?.binding.checked_at_ms ?? binding?.checked_at_ms,
    several = sites.length > 1;
  return (
    <div data-component="orchestration-project-detail" data-target-id={targetId}>
      <List gap="l" controlInsets="rbl">
        <section aria-label="Binding">
          <ListHeader label="Binding" />
          <ValueTable label={`${label} binding`} density="compact" stackBelow="compact">
            {several ? (
              // HS2-NGP2AK: a project with several sites names where each one is, one row per site.
              <>
                <ValueTableRow label="Where" value={`${String(sites.length)} sites`} />
                {sites.map((site) => (
                  <ValueTableRow
                    label={site.home ? `${site.label} (Home)` : site.label}
                    value={site.where || 'Not bound on this computer'}
                  />
                ))}
              </>
            ) : (
              <ValueTableRow label="Where" value={where || 'Not bound on this computer'} />
            )}
            <ValueTableRow label="Connection" value={badge(connection)} placeholder={loading && !detail?.resolution} />
            {checked ? <ValueTableRow label="Last Checked" value={formatTime(checked)} /> : null}
          </ValueTable>
          {detail?.resolutionError ? unreadable('The connection', detail.resolutionError) : null}
        </section>
        {limitsSection({ detail, policyTools, toolLabel, homeLabel, toolModels, resourceChoices })}
        {/* HS2-JF3EA3: only this project's admission stays here; what the machine is and how busy it
            is moved to the Systems screen, which Open in Systems shows. */}
        <section aria-label="Admission" data-orchestration-admission>
          <ListHeader label="Admission" />
          <List gap="m">
            {detail?.admissions.length ? (
              <ValueTable label={`${label} admission`} density="compact" stackBelow="compact">
                {detail.admissions.map((admission) => (
                  <ValueTableRow label={toolLabel(admission.tool)} value={admissionValue(admission)} />
                ))}
              </ValueTable>
            ) : detail?.admissionTools?.length === 0 && !detail.admissionError ? (
              quiet('No AI tool is allowed here yet.')
            ) : detail?.admissionError ? (
              unreadable('Admission', detail.admissionError)
            ) : (
              <ValueTable label={`${label} admission`} density="compact">
                <ValueTableRow label="Admission" value="" placeholder />
              </ValueTable>
            )}
            <List textInsets="rl">
              <Row>
                <Button
                  emphasis="secondary"
                  size="compact"
                  action={NAVIGATION_AND_TABS_ACTIONS.selectSystemsMachine.value}
                  icon={<LucideIcon icon={Server} name="server" />}
                  rootAttributes={systemsMachineId ? { 'data-machine-id': systemsMachineId } : {}}
                >
                  Open in Systems
                </Button>
              </Row>
            </List>
          </List>
        </section>
        <section aria-label="Held Resources">
          <ListHeader label="Held Resources" />
          {claimNamespaces.length === 0 ? (
            quiet('The policy requires no resources here.')
          ) : capacity?.claims ? (
            capacity.claims.length ? (
              <ValueTable label="Held resources" density="compact" stackBelow="compact">
                {capacity.claims.map((claim) => (
                  <ValueTableRow
                    label={`${claim.namespace} · ${claim.resource}`}
                    value={`Held until ${formatTime(claim.expires_at_ms)}`}
                  />
                ))}
              </ValueTable>
            ) : (
              quiet('Nothing is held.')
            )
          ) : (
            quiet(capacity ? 'Held resources could not be read.' : 'Waiting for capacity.')
          )}
        </section>
        <section aria-label="Sessions">
          <ListHeader label="Sessions" />
          {detail?.sessionsError ? (
            unreadable('Sessions', detail.sessionsError)
          ) : sessions.length === 0 ? (
            quiet(detail?.sessions ? 'The orchestrator runs no sessions here.' : 'Loading…')
          ) : (
            <List gap={rem(0.125)}>
              {sessions.map(({ session, showable, busy }) => {
                const attributes = { 'data-session-id': session.session_id },
                  status = sessionStatus(session),
                  human = session.owner === 'human';
                return (
                  <ListActionRow
                    label={sessionTitle(session, toolLabel)}
                    description={sessionDescription(session, toolLabel, formatTime(session.created_at_ms))}
                    status={badge(status)}
                    icon={<LucideIcon icon={Bot} name="bot" />}
                    action={ACTIONS.openSessionActions.value}
                    itemId={session.session_id}
                    busy={busy}
                    multiline
                    rootAttributes={{ 'data-orchestration-session': session.session_id }}
                    trailingMenu={{
                      label: `Actions for ${sessionTitle(session, toolLabel)}`,
                      icon: menuIcon,
                      items: [
                        {
                          label: 'Show in Drawer',
                          action: ACTIONS.showSession.value,
                          icon: <LucideIcon icon={PanelBottomOpen} name="panel-bottom-open" />,
                          disabled: !showable,
                          disabledReason: showable ? undefined : 'Open this project in a tab to show its sessions.',
                          attributes,
                        },
                        {
                          label: 'Take Over…',
                          action: ACTIONS.takeOverSession.value,
                          icon: <LucideIcon icon={Hand} name="hand" />,
                          disabled: human || session.state !== 'running',
                          disabledReason: human ? 'You already took it over.' : undefined,
                          attributes,
                        },
                        { kind: 'divider' },
                        {
                          label: 'Stop',
                          action: ACTIONS.stopSession.value,
                          tone: 'danger',
                          icon: <LucideIcon icon={CircleStop} name="circle-stop" />,
                          disabled: human || !['running', 'requested'].includes(session.state),
                          disabledReason: human ? 'A session you took over is yours to stop.' : undefined,
                          attributes,
                        },
                      ],
                    }}
                  />
                );
              })}
            </List>
          )}
        </section>
        <section aria-label="Remove">
          <Button
            emphasis="secondary"
            tone="danger"
            action={ACTIONS.removeProject.value}
            loading={removing}
            disabled={removing}
            rootAttributes={{ 'data-target-id': targetId }}
            icon={<LucideIcon icon={Trash2} name="trash-2" />}
          >
            Stop Orchestrating…
          </Button>
        </section>
      </List>
    </div>
  );
}

interface LimitsSectionProps {
  detail?: OrchestrationDetailState;
  policyTools: readonly string[];
  toolLabel: (tool: string) => string;
  homeLabel?: string;
  toolModels: Readonly<Record<string, readonly string[]>>;
  resourceChoices: readonly string[];
}

function limitsSection({ detail, policyTools, toolLabel, homeLabel, toolModels, resourceChoices }: LimitsSectionProps) {
  const limits = detail?.limits,
    saving = Boolean(detail?.limitsSaving),
    disabled = saving || !limits,
    changed = advancedLimitsChanged(limits),
    resources = listOf(limits?.resources ?? '');
  return (
    <section aria-label="Limits" data-component="orchestration-project-limits">
      <ListHeader
        label="Limits"
        action={ACTIONS.showLimitsHelp.value}
        actionLabel="About Limits"
        actionIcon={<LucideIcon icon={CircleHelp} name="circle-help" />}
        triggerAttributes={{ 'aria-haspopup': 'dialog' }}
      />
      <List gap="m" textInsets="rl">
        <Checkbox
          name="limits.enabled"
          label="Orchestrate in This Project"
          hint="Off: the orchestrator starts nothing here. It can still stop sessions."
          checked={limits?.enabled ?? true}
          disabled={disabled}
        />
        {policyTools.length ? (
          <CheckboxGroup
            name="limits.tools"
            label="AI Tools Here"
            hint="None checked is Auto: sessions here use any of these tools installed and signed in where this project runs."
            choices={policyTools.map((tool) => ({ value: tool, label: toolLabel(tool) }))}
            value={(limits?.tools ?? []).filter((tool) => policyTools.includes(tool))}
            disabled={disabled}
          />
        ) : (
          <Text tone="quiet" size="compact" flush>
            Auto: sessions here use any AI tool installed and signed in where this project runs.
          </Text>
        )}
        {policyTools
          .filter((tool) => limits?.tools.includes(tool))
          .map((tool) => {
            const chosen = listOf(limits?.models[tool] ?? '');
            return (
              <OrchestrationPatternField
                field={`limits.models.${tool}`}
                label={`${toolLabel(tool)} Models`}
                values={chosen}
                suggestions={modelSuggestions(toolModels[tool] ?? [], chosen)}
                placeholder="Any model"
                hint={MODELS_HINT}
                disabled={disabled}
              />
            );
          })}
        {/* HS2-25KT42: every limit below is Auto until changed, so the default view needs no input. */}
        <Disclosure
          title="Advanced"
          detail={changed ? `${String(changed)} Changed` : 'All Auto'}
          rootAttributes={{ 'data-orchestration-advanced-limits': '' }}
        >
          <List gap="m">
            <CheckboxGroup
              name="limits.role_sessions"
              label="Coordination Sessions"
              hint={roleSessionsHint(homeLabel, limits?.roleSessions ?? [], limits?.roleSessionsAuto)}
              choices={[
                { value: 'dispatcher', label: 'Dispatcher' },
                { value: 'integrator', label: 'Integrator' },
              ]}
              value={limits?.roleSessions ?? []}
              disabled={disabled || roleSessionsLocked(homeLabel, limits.roleSessions, limits.roleSessionsAuto)}
            />
            <TextField
              name="limits.max_sessions"
              label="Sessions Here"
              type="number"
              inputMode="numeric"
              placeholder="Auto"
              min={0}
              max={64}
              hint={SESSIONS_HERE_HINT}
              value={limits?.max_sessions ?? ''}
              disabled={disabled}
            />
            <Select
              name="limits.build_ahead"
              label="Build Ahead"
              hint={BUILD_AHEAD_HINT}
              value={limits?.buildAhead ?? 'default'}
              disabled={disabled}
              choices={[
                { value: 'default', label: 'As the Policy Says' },
                { value: 'on', label: 'Build Ahead Here' },
                { value: 'off', label: 'Never Build Ahead Here' },
              ]}
            />
            <OrchestrationPatternField
              field="limits.resources"
              label="Required Resources"
              values={resources}
              suggestions={resourceChoices}
              placeholder="None"
              hint={REQUIRED_RESOURCES_HINT}
              disabled={disabled}
            />
            <Checkbox
              name="limits.machine_override"
              label="Use Different Machine Limits"
              hint="Off is Auto: this project's machine uses the policy's machine defaults."
              checked={limits?.machineOverride ?? false}
              disabled={disabled}
            />
            {limits?.machineOverride ? (
              <Grid minColumnWidth={rem(10)} gap="m">
                <TextField
                  name="limits.machine.max_sessions"
                  label="Sessions per Machine"
                  type="number"
                  inputMode="numeric"
                  value={limits.machine.max_sessions}
                  disabled={disabled}
                />
                <TextField
                  name="limits.machine.min_free_ram_mb"
                  label="Free Memory Floor (MB)"
                  type="number"
                  inputMode="numeric"
                  value={limits.machine.min_free_ram_mb}
                  disabled={disabled}
                />
                <TextField
                  name="limits.machine.max_cpu_percent"
                  label="CPU Ceiling (%)"
                  type="number"
                  inputMode="numeric"
                  value={limits.machine.max_cpu_percent}
                  disabled={disabled}
                />
                <TextField
                  name="limits.machine.min_free_disk_mb"
                  label="Free Disk Floor (MB)"
                  type="number"
                  inputMode="numeric"
                  value={limits.machine.min_free_disk_mb}
                  disabled={disabled}
                />
              </Grid>
            ) : null}
          </List>
        </Disclosure>
        {detail?.limitsMessage ? (
          <Text tone="danger" size="compact" flush>
            {detail.limitsMessage}
          </Text>
        ) : null}
        <Row gap="xs">
          <Button emphasis="secondary" action={ACTIONS.applyProjectLimits.value} disabled={disabled} loading={saving}>
            Apply Limits
          </Button>
        </Row>
      </List>
    </section>
  );
}

/** The detail as the inspector's panel parts, in the Workbench's right rail. */
export function orchestrationProjectPanel(props: OrchestrationProjectDetailProps): SidebarPanelParts {
  return {
    label: `${props.label} Detail`,
    toolbar: {
      label: 'Project detail toolbar',
      dividerSides: '',
      title: <ToolbarText text={props.label} description={props.where || undefined} />,
      // HS2-M1T6KN: the row's actions without a right-click.
      ...(props.actions?.length
        ? {
            trailing: (
              <ToolbarControlGroup appearance="borderless" single nestedDropdown label="Project Actions">
                <PopupMenu
                  label={`Actions for ${props.label}`}
                  icon={menuIcon}
                  caret={false}
                  placement="bottom-end"
                  rootAttributes={{ 'data-orchestration-detail-actions': props.targetId }}
                  items={props.actions}
                />
              </ToolbarControlGroup>
            ),
          }
        : {}),
    },
    toggle: inspectorToggle('Project Detail'),
    content: <OrchestrationProjectDetail {...props} />,
    pane: {},
  };
}
