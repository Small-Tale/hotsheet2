import './project-summary.css';

export interface ProjectSummaryProps {
  completedToday: number;
  inProgress: number;
  trend: number[];
  projectId?: string;
  chartTone?: 'brand' | 'success';
  chartMaximum?: number;
  backgroundTrend?: number[];
}

export function chartDomainMaximum(values: readonly number[], sharedMaximum?: number): number {
  return Math.max(1, sharedMaximum ?? 0, ...values);
}

export function aggregateAlignedChartValues(series: readonly (readonly number[])[]): number[] {
  const length = Math.max(0, ...series.map(values => values.length));
  return Array.from({ length }, (_, index) => series.reduce((sum, values) => sum + (values.at(index - length) ?? 0), 0));
}

export function ProjectSummary({ completedToday, inProgress, trend, projectId, chartTone = 'brand', chartMaximum, backgroundTrend }: ProjectSummaryProps) {
  const chartLength = Math.max(trend.length, backgroundTrend?.length ?? 0);
  const alignedTrend = Array.from({ length: chartLength }, (_, index) => trend.at(index - chartLength) ?? 0);
  const maximum = chartDomainMaximum([...alignedTrend, ...(backgroundTrend ?? [])], chartMaximum);
  const chartLabel = `Tickets completed over the last ${chartLength} days: ${alignedTrend.join(', ')}${backgroundTrend ? `. All projects: ${backgroundTrend.join(', ')}` : ''}`;
  return <button type="button" class="project-summary" data-component="project-summary" data-chart-tone={chartTone} data-chart-maximum={maximum} data-chart-background={String(Boolean(backgroundTrend))} data-action="open-project-stats" data-project-id={projectId} aria-label={`Open project statistics: ${completedToday} completed today, ${inProgress} in progress`}>
    <span class="project-summary__content">
      <span class="project-summary__chart" role="img" aria-label={chartLabel}>
        {Array.from({ length: chartLength }, (_, index) => {
          const value = alignedTrend[index];
          const backgroundValue = backgroundTrend?.at(index - chartLength) ?? 0;
          return <span class="project-summary__bar-slot">
            {backgroundTrend && <span class="project-summary__bar-background" aria-hidden="true" style={backgroundValue === 0 ? undefined : `--bar-height:${Math.max(12, Math.round(backgroundValue / maximum * 100))}%`} data-background-bar={index} data-background-zero={String(backgroundValue === 0)}></span>}
            <span class="project-summary__bar-foreground" aria-hidden="true" style={value === 0 ? undefined : `--bar-height:${Math.max(12, Math.round(value / maximum * 100))}%`} data-bar={index} data-zero={String(value === 0)}></span>
          </span>;
        })}
      </span>
      <span class="project-summary__counts"><strong>{completedToday} completed today</strong><span>{inProgress} in progress</span></span>
    </span>
  </button>;
}
