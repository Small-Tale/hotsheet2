import { ArrowUp, GitBranch } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './repository-summary.css';

export interface RepositorySummaryProps { branch: string; unpushed: number; uncommitted: number }
export function RepositorySummary({ branch, unpushed, uncommitted }: RepositorySummaryProps) {
  return <button type="button" class="repository-summary" data-component="repository-summary" data-action="open-repository-status" aria-label={`Repository status for ${branch}`}>
    <span class="repository-summary__branch"><LucideIcon icon={GitBranch} name="git-branch" /><span class="repository-summary__branch-name">{branch}</span></span>
    <span class="repository-summary__metric" title={`${unpushed} unpushed commits`}><LucideIcon icon={ArrowUp} name="arrow-up" />{unpushed}</span>
    <span class="repository-summary__metric repository-summary__metric--changes" title={`${uncommitted} uncommitted changes`}>{uncommitted}</span>
  </button>;
}
