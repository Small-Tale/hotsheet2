import './stats-dashboard.css';

import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';

import type { CheckoutAnalytics } from '../api';
import { formatCost, formatCycleTime, recentThroughput, summarizeAnalytics } from '../stats-summary';

/** Checkout analytics displayed by the Stats workspace (HS2-38RJMK). */
export type StatsDashboardState =
  { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; report: CheckoutAnalytics };

export interface StatsDashboardProps {
  projects: readonly { id: string; name: string; state: StatsDashboardState }[];
  crossProject?: boolean;
  now?: Date;
}

function Bars({ values, label }: { values: readonly { label: string; count: number }[]; label: string }) {
  const max = Math.max(1, ...values.map((item) => item.count));
  return (
    <div
      class="stats-dashboard__bars"
      role="img"
      aria-label={`${label}: ${values.map((item) => `${item.label} ${item.count}`).join(', ')}`}
    >
      {values.map((item) => (
        <div class="stats-dashboard__bar-row" data-key={`${label}:${item.label}`}>
          <span class="stats-dashboard__bar-label">{item.label}</span>
          <span class="stats-dashboard__bar-track">
            <span style={`width: ${Math.max(0, item.count / max) * 100}%`} />
          </span>
          <span class="stats-dashboard__bar-value">{item.count}</span>
        </div>
      ))}
    </div>
  );
}

/** Ticket flow and usage from linked Git stores; unsupported sources remain explicit. */
export function StatsDashboard({ projects, crossProject = false, now = new Date() }: StatsDashboardProps) {
  const ready = projects.filter((project) => project.state.status === 'ready');
  const reports = ready.map((project) => (project.state as Extract<StatsDashboardState, { status: 'ready' }>).report);
  const summary = summarizeAnalytics(reports);
  const loading = projects.some((project) => project.state.status === 'loading');
  const errors = projects.filter((project) => project.state.status === 'error');
  const throughput = recentThroughput(summary.throughputByDay, now);
  const status = Object.entries(summary.byStatus)
    .sort((a, b) => b[1] - a[1])
    .map(([label, count]) => ({ label, count }));
  const categories = Object.entries(summary.byCategory)
    .sort((a, b) => b[1] - a[1])
    .map(([label, count]) => ({ label, count }));
  const models = Object.entries(summary.costByModel).sort((a, b) => b[1] - a[1]);
  return (
    <section class="stats-dashboard" data-component="stats-dashboard" aria-label="Ticket Flow and Usage">
      <List gap="l">
        <div>
          <Text variant="h2" size="large" flush>
            Ticket flow and usage
          </Text>
          <Text variant="p" tone="quiet" flush>
            Current ticket counts and settled usage across {summary.stores} linked Git{' '}
            {summary.stores === 1 ? 'store' : 'stores'}.{crossProject ? ' Shared stores are counted once.' : ''}
          </Text>
        </div>
        {loading && <EmptyState busy title="Loading Analytics" />}
        {errors.map((project) => (
          <StateBanner
            data-key={`analytics-error:${project.id}`}
            title={`${project.name} Analytics Unavailable`}
            detail={project.state.status === 'error' ? project.state.message : ''}
            tone="danger"
          />
        ))}
        {summary.unavailableSources > 0 && (
          <StateBanner
            title="Some Ticket Sources Are Unavailable for Analytics"
            detail={`${summary.unavailableSources} linked provider ${summary.unavailableSources === 1 ? 'source is' : 'sources are'} excluded from these totals.`}
            tone="warning"
          />
        )}
        {summary.stores === 0 && !loading && errors.length === 0 ? (
          <EmptyState
            title="No Linked Git Ticket Stores"
            detail="Link a Git ticket source to see ticket flow and usage."
          />
        ) : (
          summary.stores > 0 && (
            <>
              <div class="stats-dashboard__metrics">
                <div>
                  <strong>{summary.total.toLocaleString()}</strong>
                  <span>Tickets</span>
                </div>
                <div>
                  <strong>{summary.open.toLocaleString()}</strong>
                  <span>Open</span>
                </div>
                <div>
                  <strong>{summary.completed.toLocaleString()}</strong>
                  <span>Completed</span>
                </div>
                <div>
                  <strong>{formatCycleTime(summary.averageCycleSeconds, summary.cycleSamples)}</strong>
                  <span>Average cycle time · {summary.cycleSamples} samples</span>
                </div>
                <div>
                  <strong>{formatCost(summary.costUsd)}</strong>
                  <span>Recorded cost · {summary.events} events</span>
                </div>
                <div>
                  <strong>{(summary.tokensIn + summary.tokensOut).toLocaleString()}</strong>
                  <span>Recorded tokens</span>
                </div>
              </div>
              <div class="stats-dashboard__charts">
                <section>
                  <Text variant="h3" flush>
                    Completed tickets · last 14 days
                  </Text>
                  <div
                    class="stats-dashboard__throughput"
                    role="img"
                    aria-label={`Completed tickets by day: ${throughput.map(({ day, count }) => `${day} ${count}`).join(', ')}`}
                  >
                    {throughput.map(({ day, count }) => (
                      <div
                        data-key={`throughput:${day}`}
                        title={`${day}: ${count}`}
                        style={`height: ${Math.max(3, (count / Math.max(1, ...throughput.map((point) => point.count))) * 100)}%`}
                      />
                    ))}
                  </div>
                  <Text variant="p" size="compact" tone="quiet" flush>
                    {throughput[0]?.day} – {throughput.at(-1)?.day}
                  </Text>
                </section>
                <section>
                  <Text variant="h3" flush>
                    Current status
                  </Text>
                  <Bars label="Current status" values={status} />
                </section>
                <section>
                  <Text variant="h3" flush>
                    Category
                  </Text>
                  <Bars label="Category" values={categories} />
                </section>
                <section>
                  <Text variant="h3" flush>
                    Recorded cost by model
                  </Text>
                  {models.length ? (
                    <Bars
                      label="Recorded cost by model (USD)"
                      values={models.map(([label, count]) => ({ label, count }))}
                    />
                  ) : (
                    <Text variant="p" tone="quiet" flush>
                      No model costs recorded.
                    </Text>
                  )}
                </section>
              </div>
            </>
          )
        )}
        <Text variant="p" tone="quiet" size="compact" flush>
          Historical cumulative flow is unavailable until ticket status transitions are recorded.
        </Text>
        {crossProject && projects.length > 0 && (
          <section>
            <Text variant="h3" flush>
              Projects
            </Text>
            <ul class="stats-dashboard__projects">
              {projects.map((project) => {
                const projectSummary =
                  project.state.status === 'ready' ? summarizeAnalytics([project.state.report]) : undefined;
                return (
                  <li data-key={`stats-project:${project.id}`}>
                    <strong>{project.name}</strong>
                    <span>
                      {projectSummary
                        ? `${projectSummary.total} tickets · ${formatCost(projectSummary.costUsd)}`
                        : project.state.status === 'loading'
                          ? 'Loading'
                          : 'Unavailable'}
                    </span>
                  </li>
                );
              })}
            </ul>
          </section>
        )}
      </List>
    </section>
  );
}
