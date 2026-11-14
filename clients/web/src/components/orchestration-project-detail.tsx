import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Bot, CircleStop, Hand, MoreHorizontal, PanelBottomOpen, Trash2 } from 'lucide';

import type { OrchestrationAdmission, OrchestrationBinding, OrchestrationSession } from '../api';
import type { OrchestrationDetailState } from '../features/orchestration-settings';
import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import {
  type ConnectionStatus,
  connectionStatus,
  formatMegabytes,
  sessionStatus,
  sessionTitle,
} from '../orchestration-settings-model';
import { inspectorToggle, type SidebarPanelParts } from './sidebar-panel';

export interface OrchestrationProjectDetailSession {
  session: OrchestrationSession;
  /** Its project's tab is open and its drawer shows the session's terminal. */
  showable: boolean;
  busy: boolean;
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
  removing?: boolean;
  toolLabel?: (tool: string) => string;
  /** Formats a time in milliseconds; the catalog passes a fixed clock. */
  formatTime?: (ms: number) => string;
}

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;
const defaultTime = (ms: number) => new Date(ms).toLocaleString();

const ADMISSION_REASONS: Readonly<Record<string, string>> = {
  tool_not_installed: 'not installed there',
  tool_not_configured: 'turned off in that project',
  session_cap_reached: 'session cap reached',
  above_cpu_ceiling: 'CPU above the ceiling',
  below_disk_floor: 'disk below the floor',
  below_ram_floor: 'memory below the floor',
  capacity_unknown: 'capacity unknown',
};

function admissionValue(admission: OrchestrationAdmission): string {
  const reason = admission.reason ? ` · ${ADMISSION_REASONS[admission.reason] ?? admission.reason}` : '';
  if (admission.outcome === 'accepted') return `Can start ${String(admission.admitted)} now`;
  if (admission.outcome === 'reduced')
    return `Can start ${String(admission.admitted)} of ${String(admission.requested)}${reason}`;
  return `Refused${reason}`;
}

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
 * binding and connection, capacity and admission, AI tool capacity, held resources and the sessions
 * the orchestrator runs there, with Show in Drawer, Take Over and Stop. Every part is a read through
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
  removing = false,
  toolLabel = (tool) => tool,
  formatTime = defaultTime,
}: OrchestrationProjectDetailProps) {
  const connection = connectionStatus(binding, detail?.resolution),
    capacity = detail?.capacity,
    loading = Boolean(detail?.loading),
    checked = detail?.resolution?.binding.checked_at_ms ?? binding?.checked_at_ms,
    tools = detail?.toolCapacity?.tools ?? [];
  return (
    <div data-component="orchestration-project-detail" data-target-id={targetId}>
      <List gap="l" controlInsets="rbl">
        <section aria-label="Binding">
          <ListHeader label="Binding" />
          <ValueTable label={`${label} binding`} density="compact" stackBelow="compact">
            <ValueTableRow label="Where" value={where || 'Not bound on this computer'} />
            <ValueTableRow label="Connection" value={badge(connection)} placeholder={loading && !detail?.resolution} />
            {checked ? <ValueTableRow label="Last Checked" value={formatTime(checked)} /> : null}
          </ValueTable>
          {detail?.resolutionError ? unreadable('The connection', detail.resolutionError) : null}
        </section>
        <section aria-label="Capacity and Admission">
          <ListHeader label="Capacity and Admission" />
          {capacity ? (
            <ValueTable label={`${label} capacity`} density="compact" stackBelow="compact">
              <ValueTableRow
                label="Free Memory"
                value={`${formatMegabytes(capacity.memory.free_mb)} of ${formatMegabytes(capacity.memory.total_mb)}`}
              />
              <ValueTableRow
                label="CPU Load"
                value={capacity.cpu.load_percent === null ? 'Unknown' : `${String(capacity.cpu.load_percent)}%`}
              />
              <ValueTableRow
                label="Free Disk"
                value={`${formatMegabytes(capacity.disk.free_mb)} of ${formatMegabytes(capacity.disk.total_mb)}`}
              />
              <ValueTableRow
                label="AI Sessions There"
                value={capacity.sessions === null ? 'Unknown' : String(capacity.sessions.length)}
              />
              {detail.admissions.map((admission) => (
                <ValueTableRow label={`${toolLabel(admission.tool)} Admission`} value={admissionValue(admission)} />
              ))}
            </ValueTable>
          ) : detail?.capacityError ? (
            unreadable('Capacity', detail.capacityError)
          ) : (
            <ValueTable label={`${label} capacity`} density="compact">
              <ValueTableRow label="Free Memory" value="" placeholder />
            </ValueTable>
          )}
          {detail?.admissionError ? unreadable('Admission', detail.admissionError) : null}
        </section>
        <section aria-label="AI Tool Capacity">
          <ListHeader label="AI Tool Capacity" />
          {detail?.toolCapacityError ? (
            unreadable('Tool capacity', detail.toolCapacityError)
          ) : tools.length ? (
            <ValueTable label="AI tool capacity" density="compact" stackBelow="compact">
              {tools.map((row) => (
                <ValueTableRow
                  label={row.account ? `${toolLabel(row.tool)} · ${row.account}` : toolLabel(row.tool)}
                  value={
                    row.state === 'limited' && row.until_ms
                      ? `Limited until ${formatTime(row.until_ms)}`
                      : row.state === 'available'
                        ? 'Available'
                        : row.state === 'degraded'
                          ? 'Degraded'
                          : 'Unknown'
                  }
                />
              ))}
            </ValueTable>
          ) : (
            quiet(detail?.toolCapacity ? 'The policy configures no AI tools.' : 'Loading…')
          )}
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
                    description={`${toolLabel(session.tool)}${session.model ? ` · ${session.model}` : ''} · since ${formatTime(session.created_at_ms)}`}
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

/** The detail as the inspector's panel parts, in the Workbench's right rail. */
export function orchestrationProjectPanel(props: OrchestrationProjectDetailProps): SidebarPanelParts {
  return {
    label: `${props.label} Detail`,
    toolbar: {
      label: 'Project detail toolbar',
      dividerSides: '',
      title: <ToolbarText text={props.label} description={props.where || undefined} />,
    },
    toggle: inspectorToggle('Project Detail'),
    content: <OrchestrationProjectDetail {...props} />,
    pane: {},
  };
}
