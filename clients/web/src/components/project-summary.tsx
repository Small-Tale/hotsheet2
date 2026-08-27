import './project-summary.css';

export interface ProjectSummaryProps {
  completedToday: number;
  inProgress: number;
  trend: number[];
}

export function ProjectSummary({ completedToday, inProgress, trend }: ProjectSummaryProps) {
  const maximum = Math.max(...trend, 1);
  return <section class="project-summary" data-component="project-summary" aria-label="Project summary">
    <div class="project-summary__chart" role="img" aria-label={`Tickets completed over the last ${trend.length} days: ${trend.join(', ')}`}>
      {trend.map((value, index) => <span style={`--bar-height:${Math.max(12, Math.round(value / maximum * 100))}%`} data-bar={index}></span>)}
    </div>
    <div class="project-summary__counts"><strong>{completedToday} completed today</strong><span>{inProgress} currently in progress</span></div>
  </section>;
}
