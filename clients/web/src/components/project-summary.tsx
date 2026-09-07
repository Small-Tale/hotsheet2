import './project-summary.css';

export interface ProjectSummaryProps {
  completedToday: number;
  inProgress: number;
  trend: number[];
}

export function ProjectSummary({ completedToday, inProgress, trend }: ProjectSummaryProps) {
  const maximum = Math.max(...trend, 1);
  return <button type="button" class="project-summary" data-component="project-summary" data-action="open-project-stats" aria-label={`Open project statistics: ${completedToday} completed today, ${inProgress} in progress`}>
    <span class="project-summary__content">
      <span class="project-summary__chart" role="img" aria-label={`Tickets completed over the last ${trend.length} days: ${trend.join(', ')}`}>
        {trend.map((value, index) => <span style={value === 0 ? undefined : `--bar-height:${Math.max(12, Math.round(value / maximum * 100))}%`} data-bar={index} data-zero={String(value === 0)}></span>)}
      </span>
      <span class="project-summary__counts"><strong>{completedToday} completed today</strong><span>{inProgress} in progress</span></span>
    </span>
  </button>;
}
