import './project-summary.css';

export interface ProjectSummaryProps {
  completed: number;
  inProgress: number;
  progress: number;
  trend: number[];
}

export function ProjectSummary({ completed, inProgress, progress, trend }: ProjectSummaryProps) {
  const maximum = Math.max(...trend, 1);
  return <section class="project-summary" data-component="project-summary" aria-label="Project progress">
    <div class="project-summary__chart" role="img" aria-label={`${progress}% complete`}>
      {trend.map((value, index) => <span style={`--bar-height:${Math.max(12, Math.round(value / maximum * 100))}%`} data-bar={index}></span>)}
    </div>
    <div class="project-summary__counts"><strong>{completed} completed</strong><span>{inProgress} in progress</span></div>
    <strong class="project-summary__progress">{progress}%</strong>
  </section>;
}
