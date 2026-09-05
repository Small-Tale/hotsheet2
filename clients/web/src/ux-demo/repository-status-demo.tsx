import { signal } from 'kerfjs';

import type { RepositoryStatus } from '../api';
import { RepositoryStatusPopover, type RepositoryStatusView } from '../components/repository-status-popover';
import type { CodeReviewComparison } from '../components/ticket-code-review';
import { syncSettingsControls } from './settings-controls';

export type RepositoryDemoScenario='clean'|'dirty'|'ahead'|'behind'|'diverged'|'conflicted'|'error';

export const repositoryDemoView=signal<RepositoryStatusView>('unstaged');
export const repositoryDemoEvent=signal('Choose a view or open a file or commit.');
export const repositoryDemoComparison=signal<CodeReviewComparison>({active:false,side:'a'});
export const repositoryDemoExpandedCommits=signal<string[]>([]);
export const repositoryDemoScenario=signal<RepositoryDemoScenario>('conflicted');

const status:RepositoryStatus={
  branch:'feature/repository-dialog',upstream:'origin/main',ahead:2,behind:1,staged:2,unstaged:2,untracked:1,conflicted:1,clean:false,root:'/work/hotsheet2',platform:'macos',commit_count:24,difftool:'Glassbox',truncated:false,
  files:[
    {path:'clients/web/src/components/repository-status-popover.tsx',staged:'modified',untracked:false,conflicted:false},
    {path:'clients/web/src/ux-demo/repository-status-demo.tsx',staged:'added',untracked:false,conflicted:false},
    {path:'clients/web/src/main.tsx',unstaged:'modified',untracked:false,conflicted:false},
    {path:'docs/06-clients.md',unstaged:'modified',untracked:false,conflicted:false},
    {path:'notes/repository-review.md',untracked:true,conflicted:false},
    {path:'clients/web/src/theme.css',staged:'unmerged',unstaged:'unmerged',untracked:false,conflicted:true},
  ],
  ranges:[{from:'aaa1111',to:'bbb2222',count:2}],
  commits:[
    {sha:'bbb2222',short_sha:'bbb2222',subject:'Refine repository status composition',body:'Use the shared menu primitives.\n\n- Align status icons\n- Preserve native file actions',committed_at:'2026-09-04T10:00:00Z'},
    {sha:'aaa1111',short_sha:'aaa1111',subject:'Build repository status browser',body:'Add the initial master-detail repository browser.\nIncludes **Markdown** commit details.',committed_at:'2026-09-04T09:00:00Z'},
  ],
};

export function repositoryStatusForScenario(scenario:RepositoryDemoScenario):RepositoryStatus|null {
  if(scenario==='error')return null;
  if(scenario==='conflicted')return status;
  const withoutChanges={...status,staged:0,unstaged:0,untracked:0,conflicted:0,files:[],clean:true};
  if(scenario==='dirty')return {...status,ahead:0,behind:0,conflicted:0,clean:false,files:status.files?.filter(file=>!file.conflicted)};
  if(scenario==='ahead')return {...withoutChanges,ahead:2,behind:0};
  if(scenario==='behind')return {...withoutChanges,ahead:0,behind:2};
  if(scenario==='diverged')return {...withoutChanges,ahead:2,behind:1};
  return {...withoutChanges,ahead:0,behind:0};
}

export function resetRepositoryStatusDemo(root?:ParentNode):void {
  repositoryDemoScenario.value='conflicted';
  if(root)syncSettingsControls(root,'repository-status-popover',{values:{scenario:'conflicted'}});
}

export function RepositoryStatusPopoverDemo(){const scenario=repositoryDemoScenario.value;return <section aria-label="RepositoryStatusPopover demo"><RepositoryStatusPopover embedded status={repositoryStatusForScenario(scenario)} error={scenario==='error'?'git status failed: repository is unavailable':''} view={repositoryDemoView.value} comparison={repositoryDemoComparison.value} expandedCommits={repositoryDemoExpandedCommits.value}/><p class="component-stage__event" aria-live="polite">{repositoryDemoEvent.value}</p></section>}

export function RepositoryStatusPopoverSettings(){return <form class="settings-form" data-settings="repository-status-popover">
  <wa-select name="scenario" label="Repository scenario" value={repositoryDemoScenario.value}>{(['clean','dirty','ahead','behind','diverged','conflicted','error'] as const).map(value=><wa-option value={value}>{value[0].toUpperCase()+value.slice(1)}</wa-option>)}</wa-select>
  <wa-button type="button" data-action="reset-settings">Reset</wa-button>
</form>}
