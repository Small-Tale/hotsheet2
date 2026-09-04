import { signal } from 'kerfjs';

import type { RepositoryStatus } from '../api';
import { RepositoryStatusPopover, type RepositoryStatusView } from '../components/repository-status-popover';
import type { CodeReviewComparison } from '../components/ticket-code-review';

export const repositoryDemoView=signal<RepositoryStatusView>('unstaged');
export const repositoryDemoEvent=signal('Choose a view or open a file or commit.');
export const repositoryDemoComparison=signal<CodeReviewComparison>({active:false,side:'a'});
export const repositoryDemoExpandedCommits=signal<string[]>([]);

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

export function RepositoryStatusPopoverDemo(){return <section aria-label="RepositoryStatusPopover demo"><RepositoryStatusPopover embedded status={status} view={repositoryDemoView.value} comparison={repositoryDemoComparison.value} expandedCommits={repositoryDemoExpandedCommits.value}/><p class="component-stage__event" aria-live="polite">{repositoryDemoEvent.value}</p></section>}
