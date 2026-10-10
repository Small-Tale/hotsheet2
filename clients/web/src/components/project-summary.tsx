import './project-summary.css';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';

export interface ProjectSummaryProps {
  completedToday: number;
  inProgress: number;
  trend: number[];
  partial?: boolean;
  projectId?: string;
  chartTone?: 'brand' | 'success';
  chartMaximum?: number;
  backgroundTrend?: number[];
  /** `compact` is the shorter, more padded summary stacked per project in the terminal operations sidebar. */
  size?: 'default' | 'compact';
}

export function chartDomainMaximum(values: readonly number[], sharedMaximum?: number): number {
  return Math.max(1, sharedMaximum ?? 0, ...values);
}

export function aggregateAlignedChartValues(series: readonly (readonly number[])[]): number[] {
  const length = Math.max(0, ...series.map((values) => values.length));
  return Array.from({ length }, (_, index) =>
    series.reduce((sum, values) => sum + (values.at(index - length) ?? 0), 0),
  );
}

export function ProjectSummary({
  completedToday,
  inProgress,
  trend,
  partial = false,
  projectId,
  chartTone = 'brand',
  chartMaximum,
  backgroundTrend,
  size = 'default',
}: ProjectSummaryProps) {
  const chartLength = Math.max(trend.length, backgroundTrend?.length ?? 0);
  const alignedTrend = Array.from({ length: chartLength }, (_, index) => trend.at(index - chartLength) ?? 0);
  const maximum = chartDomainMaximum([...alignedTrend, ...(backgroundTrend ?? [])], chartMaximum);
  const chartLabel = `${partial ? 'At least these tickets were' : 'Tickets'} completed over the last ${chartLength} days: ${alignedTrend.join(', ')}${backgroundTrend ? `. All projects: ${backgroundTrend.join(', ')}` : ''}`;
  return (
    <button
      type="button"
      class="project-summary"
      data-component="project-summary"
      data-size={size}
      data-chart-tone={chartTone}
      data-chart-maximum={maximum}
      data-chart-background={String(Boolean(backgroundTrend))}
      {...NAVIGATION_AND_TABS_ACTIONS.openProjectStats.attrs}
      data-project-id={projectId}
      aria-label={`Open project statistics: ${partial ? 'at least ' : ''}${completedToday} completed today, ${partial ? 'at least ' : ''}${inProgress} in progress`}
    >
      <span class="project-summary__content">
        <span class="project-summary__chart" role="img" aria-label={chartLabel}>
          {Array.from({ length: chartLength }, (_, index) => {
            const value = alignedTrend[index];
            const backgroundValue = backgroundTrend?.at(index - chartLength) ?? 0;
            // Bars scale proportionally to the shared maximum (the tallest aggregate day = 100%), so a
            // project's fill is exactly value/maximum and its aggregate background is aggregate/maximum —
            // the fill therefore never exceeds the gray bar and per-day fills sum to the aggregate. Only a
            // 1% floor keeps a non-zero bar renderable; the CSS min-height handles visibility without
            // clamping small bars to a fixed height that would make a fraction look like the whole (HS2-C9JM65).
            return (
              <span class="project-summary__bar-slot">
                {backgroundTrend && (
                  <span
                    class="project-summary__bar-background"
                    aria-hidden="true"
                    style={
                      backgroundValue === 0
                        ? undefined
                        : `--bar-height:${Math.max(1, Math.round((backgroundValue / maximum) * 100))}%`
                    }
                    data-background-bar={index}
                    data-background-zero={String(backgroundValue === 0)}
                  />
                )}
                <span
                  class="project-summary__bar-foreground"
                  aria-hidden="true"
                  style={value === 0 ? undefined : `--bar-height:${Math.max(1, Math.round((value / maximum) * 100))}%`}
                  data-bar={index}
                  data-zero={String(value === 0)}
                />
              </span>
            );
          })}
        </span>
        <span class="project-summary__counts">
          <strong>{completedToday} completed today</strong>
          <span>{inProgress} in progress</span>
        </span>
      </span>
    </button>
  );
}
