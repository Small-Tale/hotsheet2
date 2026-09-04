import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe,expect,it } from 'vitest';

import type { RepositoryFile,RepositoryFileChange,RepositoryStatus } from '../api';
import { repositoryAbsolutePath,repositoryFilesForView,repositoryFileStatusLetter,RepositoryStatusPopover,repositoryStatusState } from './repository-status-popover';

const files:RepositoryFile[]=[
  {path:'src/staged.ts',staged:'added',untracked:false,conflicted:false},
  {path:'src/changed.ts',unstaged:'modified',untracked:false,conflicted:false},
  {path:'src/new name.ts',original_path:'src/old name.ts',staged:'renamed',untracked:false,conflicted:false},
  {path:'notes/new.md',untracked:true,conflicted:false},
  {path:'src/conflict.ts',staged:'unmerged',unstaged:'unmerged',untracked:false,conflicted:true},
];
const status=(patch:Partial<RepositoryStatus>={}):RepositoryStatus=>({branch:'main',upstream:'origin/main',ahead:0,behind:0,staged:2,unstaged:1,untracked:1,conflicted:1,files,root:'/work/demo',platform:'macos',commit_count:12,commits:[{sha:'abc123456',short_sha:'abc1234',subject:'Ship repository browser',committed_at:'2026-09-04T00:00:00Z'}],ranges:[],difftool:'Glassbox',truncated:false,...patch});

describe('RepositoryStatusPopover',()=>{
  it('classifies every repository state without hiding orthogonal counts',()=>{
    expect(repositoryStatusState(status({staged:0,unstaged:0,untracked:0,conflicted:0}))).toBe('clean');
    expect(repositoryStatusState(status({conflicted:0}))).toBe('dirty');
    expect(repositoryStatusState(status({conflicted:0,ahead:1}))).toBe('ahead');
    expect(repositoryStatusState(status({conflicted:0,behind:1}))).toBe('behind');
    expect(repositoryStatusState(status({conflicted:0,ahead:1,behind:1}))).toBe('diverged');
    expect(repositoryStatusState(status())).toBe('conflicted');
    expect(repositoryStatusState(null,'git failed')).toBe('error');
  });

  it('renders value cells, selectable views, and iconic file status in a master-detail layout',()=>{
    const markup=String(RepositoryStatusPopover({status:status(),view:'staged'}));
    for(const text of ['<dt>Branch</dt><dd>main','<dt>Upstream</dt><dd>origin/main','<dt>Ahead</dt>','<dt>Behind</dt>','Staged','Unstaged','Untracked','Conflicted','Commits'])expect(markup).toContain(text);
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('data-component="menu-header"');
    expect(markup.match(/data-component="menu-item"/g)).toHaveLength(7);
    expect(markup).toContain('data-action="open-repository-file" data-item-id="src/staged.ts"');
    expect(markup).toContain('data-state="renamed"');
    expect(markup).toContain('aria-label="src/old name.ts"');
    expect(markup).toContain('data-lucide="square-pen"');
    expect(markup).toContain('class="repository-status-popover__file-status" aria-hidden="true">A</span>');
    expect(markup).toContain('class="repository-status-popover__file-status" aria-hidden="true">R</span>');
    expect(markup).not.toContain('repository-status-popover__file-state');
    expect(markup).toContain('repository-status-popover__path');
    expect(markup).toContain('data-action="refresh-repository-status"');
    const css=readFileSync(resolve(import.meta.dirname,'repository-status-popover.css'),'utf8');
    expect(css).toMatch(/__layout \{[^}]*grid-template-columns:/);
    expect(css).toMatch(/__detail \{[^}]*overflow: auto;/);
    expect(css).toMatch(/__values \{[^}]*background:/);
    expect(css).not.toMatch(/__values \{[^}]*border:/);
    expect(css).toMatch(/__values > div \+ div::before \{[^}]*left: var\(--wa-space-m\);/);
  });

  it('uses the canonical Git status letter for every file change kind',()=>{
    expect(['added','copied','deleted','modified','renamed','type_changed','unmerged','untracked'].map(change=>repositoryFileStatusLetter(change as RepositoryFileChange))).toEqual(['A','C','D','M','R','T','U','?']);
  });

  it('filters files into non-overlapping working-tree views',()=>{
    expect(repositoryFilesForView(files,'staged').map(file=>file.path)).toEqual(['src/staged.ts','src/new name.ts']);
    expect(repositoryFilesForView(files,'unstaged').map(file=>file.path)).toEqual(['src/changed.ts']);
    expect(repositoryFilesForView(files,'untracked').map(file=>file.path)).toEqual(['notes/new.md']);
    expect(repositoryFilesForView(files,'conflicted').map(file=>file.path)).toEqual(['src/conflict.ts']);
  });

  it('reuses code-review commit and range actions in the commits view',()=>{
    const markup=String(RepositoryStatusPopover({status:status({ranges:[{from:'aaa1111',to:'bbb2222',count:2}]}),view:'commits',comparison:{active:false,side:'a'}}));
    expect(markup).toContain('data-component="ticket-code-review"');
    expect(markup).toContain('Ship repository browser');
    expect(markup).toContain('data-action="open-repository-review"');
    expect(markup).toContain('data-review-mode="range"');
    expect(markup).toContain('aria-label="Compare two commits"');
  });

  it('renders bounded detail pages with an infinite-scroll sentinel',()=>{
    const markup=String(RepositoryStatusPopover({status:status(),view:'staged',detailFiles:[files[0]],detailHasMore:true}));
    expect(markup).toContain('data-item-id="src/staged.ts"');
    expect(markup).not.toContain('data-item-id="src/new name.ts"');
    expect(markup).toContain('data-repository-pagination-sentinel="true"');
    expect(String(RepositoryStatusPopover({status:status(),view:'commits',detailCommits:[],detailLoading:true}))).toContain('Finding commits');
  });

  it('normalizes absolute paths and host-specific reveal copy',()=>{
    expect(repositoryAbsolutePath('/work/demo/','src/file.ts','macos')).toBe('/work/demo/src/file.ts');
    expect(repositoryAbsolutePath('C:\\work\\demo','src/file.ts','windows')).toBe('C:\\work\\demo\\src\\file.ts');
    expect(String(RepositoryStatusPopover({status:status(),fileMenu:{path:'src/changed.ts',absolutePath:'/work/demo/src/changed.ts',x:10,y:20}}))).toContain('Show in Finder');
    expect(String(RepositoryStatusPopover({status:status({platform:'windows'}),fileMenu:{path:'src/changed.ts',x:10,y:20}}))).toContain('Show in File Explorer');
  });

  it('renders an actionable error without inventing repository values',()=>{
    const markup=String(RepositoryStatusPopover({status:null,error:'git status failed'}));
    expect(markup).toContain('data-state="error"');
    expect(markup).toContain('git status failed');
    expect(markup).not.toContain('<dt>Branch</dt>');
  });
});
