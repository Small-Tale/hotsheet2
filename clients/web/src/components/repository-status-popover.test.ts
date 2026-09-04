import { describe,expect,it } from 'vitest';

import type { RepositoryStatus } from '../api';
import { RepositoryStatusPopover,repositoryStatusState } from './repository-status-popover';

const status=(patch:Partial<RepositoryStatus>={}):RepositoryStatus=>({branch:'main',upstream:'origin/main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,...patch});

describe('RepositoryStatusPopover',()=>{
  it('classifies every repository state without hiding orthogonal counts',()=>{
    expect(repositoryStatusState(status())).toBe('clean');
    expect(repositoryStatusState(status({unstaged:1}))).toBe('dirty');
    expect(repositoryStatusState(status({ahead:1}))).toBe('ahead');
    expect(repositoryStatusState(status({behind:1}))).toBe('behind');
    expect(repositoryStatusState(status({ahead:1,behind:1}))).toBe('diverged');
    expect(repositoryStatusState(status({conflicted:1}))).toBe('conflicted');
    expect(repositoryStatusState(null,'git failed')).toBe('error');
  });

  it('renders branch, upstream, all counts, and refresh controls',()=>{
    const markup=String(RepositoryStatusPopover({status:status({ahead:2,behind:3,staged:4,unstaged:5,untracked:6,conflicted:7})}));
    for(const text of ['main','origin/main','2</strong> ahead','3</strong> behind','4</strong> staged','5</strong> unstaged','6</strong> untracked','7</strong> conflicted'])expect(markup).toContain(text);
    expect(markup).toContain('data-action="refresh-repository-status"');
    expect(markup).toContain('popoverTarget="repository-status-popover"');
  });

  it('renders an actionable error without inventing repository values',()=>{
    const markup=String(RepositoryStatusPopover({status:null,error:'git status failed'}));
    expect(markup).toContain('data-state="error"');
    expect(markup).toContain('git status failed');
    expect(markup).not.toContain('repository-status-popover__metrics');
  });
});
