import './repository-status-popover.css';

import { ArrowDown, ArrowUp, CircleCheck, FilePenLine, GitBranch, RefreshCw, TriangleAlert } from 'lucide';

import type { RepositoryStatus } from '../api';
import { LucideIcon } from './lucide-icon';

export type RepositoryStatusState='clean'|'dirty'|'ahead'|'behind'|'diverged'|'conflicted'|'error';

export function repositoryStatusState(status:RepositoryStatus|null,error=''):RepositoryStatusState {
  if(error||!status)return 'error';
  if(status.conflicted>0)return 'conflicted';
  if(status.ahead>0&&status.behind>0)return 'diverged';
  if(status.behind>0)return 'behind';
  if(status.ahead>0)return 'ahead';
  if(status.staged+status.unstaged+status.untracked>0)return 'dirty';
  return 'clean';
}

const stateCopy:Record<RepositoryStatusState,string>={
  clean:'Working tree is clean',dirty:'Local changes have not been committed',ahead:'Local commits have not been pushed',behind:'Remote commits have not been integrated',diverged:'Local and remote histories have diverged',conflicted:'Repository has unresolved conflicts',error:'Repository status is unavailable',
};

export function RepositoryStatusPopover({status,error='',refreshing=false}:{status:RepositoryStatus|null;error?:string;refreshing?:boolean}){
  const state=repositoryStatusState(status,error),branch=status?.branch||'No branch',upstream=status?.upstream||'No upstream';
  return <section popover="auto" id="repository-status-popover" class="repository-status-popover" data-component="repository-status-popover" data-state={state} role="dialog" aria-labelledby="repository-status-title">
    <header><span class="repository-status-popover__icon"><LucideIcon icon={state==='clean'?CircleCheck:state==='error'||state==='conflicted'?TriangleAlert:GitBranch} name={state==='clean'?'circle-check':state==='error'||state==='conflicted'?'triangle-alert':'git-branch'}/></span><div><h2 id="repository-status-title">Repository status</h2><p>{stateCopy[state]}</p></div></header>
    {status&&<><dl class="repository-status-popover__identity"><dt>Branch</dt><dd>{branch}</dd><dt>Upstream</dt><dd>{upstream}</dd></dl><div class="repository-status-popover__metrics" aria-label="Repository status counts"><span><LucideIcon icon={ArrowUp} name="arrow-up"/><strong>{status.ahead}</strong> ahead</span><span><LucideIcon icon={ArrowDown} name="arrow-down"/><strong>{status.behind}</strong> behind</span><span><LucideIcon icon={FilePenLine} name="file-pen-line"/><strong>{status.staged}</strong> staged</span><span><strong>{status.unstaged}</strong> unstaged</span><span><strong>{status.untracked}</strong> untracked</span><span data-conflicted={status.conflicted>0?'true':undefined}><strong>{status.conflicted}</strong> conflicted</span></div></>}
    {error&&<p class="repository-status-popover__error" role="alert">{error}</p>}
    <footer><button type="button" data-action="refresh-repository-status" disabled={refreshing}><LucideIcon icon={RefreshCw} name="refresh-cw"/>{refreshing?'Refreshing…':'Refresh'}</button><button type="button" popoverTarget="repository-status-popover" popoverTargetAction="hide">Close</button></footer>
  </section>;
}
