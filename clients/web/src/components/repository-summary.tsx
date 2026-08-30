import './repository-summary.css';

import { ArrowUp, GitBranch } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuItem } from './menu-item';

export interface RepositorySummaryProps { branch: string; unpushed: number; uncommitted: number }
export function RepositorySummary({ branch, unpushed, uncommitted }: RepositorySummaryProps) {
  return <div class="repository-summary" data-component="repository-summary"><MenuItem action="open-repository-status" accessibleLabel={`Repository status for ${branch}`} icon={<LucideIcon icon={GitBranch} name="git-branch" />} label={<span class="repository-summary__branch-name">{branch}</span>} trailing={<><span class="repository-summary__metric" title={`${unpushed} unpushed commits`}><LucideIcon icon={ArrowUp} name="arrow-up" />{unpushed}</span><span class="repository-summary__metric repository-summary__metric--changes" title={`${uncommitted} uncommitted changes`}>{uncommitted}</span></>} /></div>;
}
