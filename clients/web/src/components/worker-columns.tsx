import './worker-columns.css';

import { Badge } from '@kerfjs/ui/badge';
import { ColumnTrack } from '@kerfjs/ui/column-track';
import { px } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { Bot, CircleHelp, UserRound } from 'lucide';

import type { TicketRow as WireTicketRow } from '../api';
import { VIEWS_AND_SAVED_VIEWS_TARGETS } from '../interaction-attrs/views-and-saved-views';
import {
  mergeWorkerViews,
  qualifiedWorkerTicket,
  workerGroups,
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
  sourceErrors: readonly string[];
  unsupportedSources: readonly string[];
  layout: 'grid' | 'paged';
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
    kind = worker.role ?? worker.connection_kind ?? 'worker';
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
      </div>
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
  unsupportedSources,
  layout,
}: WorkerColumnsProps) {
  const workers = mergeWorkerViews(views);
  const uncovered = [
    ...new Set([...views.flatMap((view) => view.uncovered_sources), ...unsupportedSources, ...sourceErrors]),
  ];
  const coverageUnknown = views.some((view) => !view.connection_coverage) || unsupportedSources.length > 0;
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
      {(uncovered.length > 0 || coverageUnknown) && (
        <div class="worker-columns__coverage">
          <StateBanner
            tone="info"
            urgency="status"
            title="Some Workers May Be Missing"
            detail={
              uncovered.length ? `Coverage unavailable for ${uncovered.join(', ')}.` : 'Connection coverage is unknown.'
            }
          />
        </div>
      )}
      {workers.length > 0 ? (
        <ColumnTrack layout={layout === 'grid' ? 'scroll' : 'paged'} minColumnWidth={px(250)} peek={px(24)}>
          {workers.map((worker) => (
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
                {workerGroups(worker).length === 0 && (
                  <Text variant="p" tone="quiet" size="compact">
                    No assigned or planned tickets
                  </Text>
                )}
                {workerGroups(worker).map((group) => (
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
          ))}
        </ColumnTrack>
      ) : (
        <div class="worker-columns__zero" role="status">
          {loading
            ? 'Loading workers…'
            : sourceErrors.length || coverageUnknown
              ? 'Worker availability is unknown.'
              : 'No workers are registered for this project.'}
        </div>
      )}
    </section>
  );
}
