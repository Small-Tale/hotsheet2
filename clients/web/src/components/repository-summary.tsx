import './repository-summary.css';

import { ArrowDown, ArrowUp, CircleAlert, GitBranch } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuItem } from './menu-item';

export interface RepositorySummaryProps { branch: string; unpushed: number; behind?:number; uncommitted: number; conflicted?:number; error?:boolean }
export function RepositorySummary({ branch, unpushed, behind=0, uncommitted, conflicted=0, error=false }: RepositorySummaryProps) {
  const accessible=error?`Repository status unavailable for ${branch}`:`Repository status for ${branch}: ${unpushed} ahead, ${behind} behind, ${uncommitted} uncommitted, ${conflicted} conflicted`;
  return <div class="repository-summary" data-component="repository-summary" data-state={error?'error':conflicted?'conflicted':uncommitted?'dirty':behind?'behind':unpushed?'ahead':'clean'}><MenuItem action="open-repository-status" accessibleLabel={accessible} icon={<LucideIcon icon={error||conflicted?CircleAlert:GitBranch} name={error||conflicted?'circle-alert':'git-branch'} />} label={<span class="repository-summary__branch-name">{branch}</span>} trailing={<>{error?<span class="repository-summary__metric repository-summary__metric--error">Unavailable</span>:<><span class="repository-summary__metric" title={`${unpushed} unpushed commits`}><LucideIcon icon={ArrowUp} name="arrow-up" />{unpushed}</span>{behind>0&&<span class="repository-summary__metric" title={`${behind} commits behind`}><LucideIcon icon={ArrowDown} name="arrow-down" />{behind}</span>}<span class="repository-summary__metric repository-summary__metric--changes" title={`${uncommitted} uncommitted changes`}>{uncommitted}</span></>}</>} /></div>;
}
