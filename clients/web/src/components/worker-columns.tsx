import './worker-columns.css';

import { Badge } from '@kerfjs/ui/badge';
import { ColumnTrack } from '@kerfjs/ui/column-track';
import { px } from '@kerfjs/ui/css-values';
import { Disclosure } from '@kerfjs/ui/disclosure';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { Bot, CircleHelp, UserRound } from 'lucide';

import type { TicketRow as WireTicketRow } from '../api';
import { VIEWS_AND_SAVED_VIEWS_TARGETS } from '../interaction-attrs/views-and-saved-views';
import {
  integrationStrategyLabel,
  mergeWorkerViews,
  qualifiedWorkerTicket,
  type WorkerCoverage,
  workerCoverage,
  type WorkerCoverageItem,
  type WorkerCoverageSource,
  workerGroups,
  workerPreferenceChips,
  workerRequestStatus,
  type WorkerSourceError,
  type WorkerTicketDetails,
  workerTicketOperationKey,
  type WorkerView,
} from '../worker-view-model';
import { TicketRow, type TicketRowProps } from './ticket-row';

export interface WorkerColumnsProps {
  views: readonly WorkerView[];
  details: WorkerTicketDetails;
  ticketRow: (ticket: WireTicketRow) => TicketRowProps;
  selectedQualifiedId?: string;
  loading: boolean;
  /** Git sources whose worker view failed to load. */
  sourceErrors: readonly WorkerSourceError[];
  /** The project's ticket sources; undefined while they are not loaded. */
  sources: readonly WorkerCoverageSource[] | undefined;
  layout: 'grid' | 'paged';
}

const sourceCount = (count: number) => `${count} ${count === 1 ? 'source' : 'sources'}`;

/** One plain line per source: why its workers are not shown (HS2-7WE7CQ). */
function coverageLine(item: WorkerCoverageItem): string {
  if (item.reason === 'load_failed')
    return `${item.name}: ${(item.message ?? 'could not be loaded').replace(/[.\s]+$/u, '')}`;
  if (item.reason === 'failed_read') return `${item.name}: its settings could not be read`;
  if (item.reason === 'unobserved') return `${item.name}: not watched by this project's worker pool`;
  return `${item.name}: worker coordination is not available for this provider`;
}

/**
 * HS2-7WE7CQ: real failures get a warning banner; sources that cannot be covered by design, and
 * explicit unknowns, sit in a collapsed disclosure. Neither says a worker is missing or that
 * ticket access failed: only that source's workers, claims and assignments are not shown.
 */
function WorkerCoverageSummary({ coverage }: { coverage: WorkerCoverage }) {
  const unknowns = [
    ...(coverage.sourcesUnknown ? ["Ticket sources haven't loaded, so coverage is unknown."] : []),
    ...(coverage.connectionUnknown ? ['Live connection status is unknown for some workers.'] : []),
  ];
  const notes = coverage.notes.map(coverageLine),
    hidden = coverage.notes.length;
  return (
    <div class="worker-columns__coverage" data-worker-coverage="">
      {coverage.problems.length > 0 && (
        <StateBanner
          tone="warning"
          urgency="status"
          copyLayout="stacked"
          title="Workers Not Shown for Some Sources"
          detail={`${coverage.problems.map(coverageLine).join('. ')}. Tickets are unaffected.`}
          rootAttributes={{ 'data-worker-coverage-problems': '' }}
        />
      )}
      {notes.length + unknowns.length > 0 && (
        <Disclosure
          title={hidden > 0 ? `Workers not shown for ${sourceCount(hidden)}` : 'Worker coverage unknown'}
          detail="Tickets are unaffected"
          rootAttributes={{ 'data-worker-coverage-notes': '' }}
        >
          <List gap="xs">
            {[...notes, ...unknowns].map((line) => (
              <Text variant="p" tone="quiet" size="compact" flush>
                {line}
              </Text>
            ))}
          </List>
        </Disclosure>
      )}
    </div>
  );
}

function workerAvailability(worker: ReturnType<typeof mergeWorkerViews>[number]): string {
  if (worker.availability === 'unresolved_claim_holder') return 'Unresolved claim holder';
  if (worker.availability === 'unbound_connection') return 'Connection unbound';
  if (worker.availability === 'disconnected') return 'Disconnected';
  if (worker.connection_available === false || worker.observed_alive === false) return 'Connection unavailable';
  if (worker.connection_available === null || worker.observed_alive === null || worker.process_busy === null)
    return 'Availability unknown';
  return worker.process_busy ? 'Working' : 'Available';
}

function WorkerCard({ worker }: { worker: ReturnType<typeof mergeWorkerViews>[number] }) {
  const availability = workerAvailability(worker),
    kind = worker.role ?? worker.connection_kind ?? 'worker',
    requestStatus = workerRequestStatus(worker),
    idleReason = worker.availability === 'live' ? (worker.matching?.idle_reason ?? null) : null;
  return (
    <header class="worker-columns__card" data-worker-id={worker.id}>
      <div class="worker-columns__card-title">
        <LucideIcon
          icon={worker.role === 'integrator' ? UserRound : Bot}
          name={worker.role === 'integrator' ? 'user-round' : 'bot'}
        />
        <h2 title={worker.label}>{worker.label}</h2>
      </div>
      <div class="worker-columns__card-meta">
        <Badge size="compact" appearance="outline">
          {[worker.tool, kind].filter(Boolean).join(' · ') || 'Tool unknown'}
        </Badge>
        <Badge
          size="compact"
          tone={availability === 'Working' ? 'info' : availability === 'Available' ? 'neutral' : 'warning'}
        >
          {availability}
        </Badge>
        {worker.role === 'integrator' && worker.integration_strategy && (
          <Badge
            size="compact"
            appearance="outline"
            rootAttributes={{ 'data-integration-strategy': worker.integration_strategy }}
          >
            {integrationStrategyLabel(worker.integration_strategy)}
          </Badge>
        )}
        {workerPreferenceChips(worker.preferences).map((chip) => (
          <Badge
            size="compact"
            appearance="outline"
            tone={chip.kind === 'prefer' ? 'neutral' : 'info'}
            rootAttributes={{ 'data-worker-preference': chip.kind, 'data-worker-filter': chip.filter }}
          >
            {chip.label}
          </Badge>
        ))}
      </div>
      {requestStatus && (
        <Text variant="p" tone={worker.preferences?.refusal ? 'danger' : 'quiet'} size="compact">
          {requestStatus}
        </Text>
      )}
      {idleReason && (
        <Text variant="p" tone="quiet" size="compact">
          Idle: {idleReason}
        </Text>
      )}
      {worker.parent && (
        <Text variant="p" tone="quiet" size="compact">
          Background agent of {worker.parent}
        </Text>
      )}
      {worker.availability === 'live' && worker.lease_expires_at !== null && (
        <Text variant="p" tone="quiet" size="compact">
          Lease until {new Date(worker.lease_expires_at * 1000).toLocaleTimeString()}
        </Text>
      )}
    </header>
  );
}

/** Production Workers workspace: source-qualified tickets retain the shared ticket-summary contract. */
export function WorkerColumns({
  views,
  details,
  ticketRow,
  selectedQualifiedId,
  loading,
  sourceErrors,
  sources,
  layout,
}: WorkerColumnsProps) {
  // HS2-KX87JC: do not render stale availability from a source that failed after its last snapshot.
  const failedSources = new Set(sourceErrors.map((error) => error.connectionId)),
    currentViews = views.filter((view) => !failedSources.has(view.source_id)),
    workers = mergeWorkerViews(currentViews),
    coverage = workerCoverage(currentViews, sources, sourceErrors),
    coverageIncomplete =
      coverage.problems.length > 0 ||
      coverage.notes.length > 0 ||
      coverage.sourcesUnknown ||
      coverage.connectionUnknown;
  return (
    <section
      class="worker-columns"
      data-component="ticket-board"
      data-worker-view="true"
      data-layout={layout}
      {...VIEWS_AND_SAVED_VIEWS_TARGETS.ticketSelectionRoot.attrs}
      role="listbox"
      aria-multiselectable="false"
      aria-label="Project Workers"
    >
      {coverageIncomplete && <WorkerCoverageSummary coverage={coverage} />}
      {workers.length > 0 ? (
        <ColumnTrack layout={layout === 'grid' ? 'scroll' : 'paged'} minColumnWidth={px(250)} peek={px(24)}>
          {workers.map((worker) => {
            const groups = workerGroups(worker, Date.now() / 1000, details);
            return (
              <section
                class="worker-columns__column"
                data-key={`worker-column:${worker.id}`}
                data-worker-column={worker.id}
                aria-label={`${worker.label} Worker Column`}
              >
                <WorkerCard worker={worker} />
                <div
                  class="worker-columns__tickets"
                  data-key={`worker-scroll:${worker.id}`}
                  data-ticket-scroll-owner={`worker:${worker.id}`}
                  role="group"
                  aria-label={`${worker.label} tickets`}
                >
                  {groups.length === 0 && (
                    <Text variant="p" tone="quiet" size="compact">
                      No assigned or planned tickets
                    </Text>
                  )}
                  {groups.map((group) => (
                    <section
                      class="worker-columns__group"
                      data-key={`${worker.id}:${group.kind}`}
                      aria-label={`${group.label} Tickets`}
                    >
                      <h3>
                        {group.label} · {group.tickets.length}
                      </h3>
                      {group.tickets.map((item) => {
                        const qualified = qualifiedWorkerTicket(item.ref),
                          detail = details[qualified];
                        return (
                          <div
                            class="worker-columns__ticket"
                            data-key={`${worker.id}:${item.assignment ? workerTicketOperationKey(item.assignment) : `plan:${item.plan?.entry.operation ?? qualified}`}`}
                            data-worker-ticket={qualified}
                          >
                            {detail ? (
                              <TicketRow
                                {...ticketRow(detail)}
                                qualifiedId={qualified}
                                selected={selectedQualifiedId === qualified}
                                presentation="column"
                                busy={group.kind === 'working'}
                                agentName={worker.label}
                                upNextEligible={false}
                                draggable={false}
                              />
                            ) : (
                              <div class="worker-columns__unavailable">
                                <LucideIcon icon={CircleHelp} name="circle-help" />
                                <span>{item.slug}: ticket details unavailable</span>
                              </div>
                            )}
                            {item.plan && (
                              <Text variant="p" tone="quiet" size="compact">
                                {item.plan.reasons.length
                                  ? `Cannot dispatch: ${item.plan.reasons.join('; ')}`
                                  : 'Tentative plan; no claim'}
                              </Text>
                            )}
                            {item.assignment?.eta_at && group.kind === 'working' && (
                              <Text variant="p" tone="quiet" size="compact">
                                ETA {new Date(item.assignment.eta_at).toLocaleString()}
                              </Text>
                            )}
                          </div>
                        );
                      })}
                    </section>
                  ))}
                </div>
              </section>
            );
          })}
        </ColumnTrack>
      ) : (
        <div class="worker-columns__zero" role="status">
          {loading
            ? 'Loading workers…'
            : coverageIncomplete
              ? 'Worker availability is unknown.'
              : 'No workers are registered for this project.'}
        </div>
      )}
    </section>
  );
}
