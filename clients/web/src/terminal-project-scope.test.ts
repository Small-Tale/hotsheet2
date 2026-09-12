import {describe,expect,it} from 'vitest';

import {terminalProjectOwner} from './terminal-project-scope';

describe('terminalProjectOwner',()=>{
  const projects=[{id:'hotsheet',root:'/work/hotsheet2'},{id:'kerf',root:'/work/kerf'},{id:'procurement',root:'/work/procurement'}];

  it('assigns each host-wide terminal only to the project containing its cwd',()=>{
    expect(terminalProjectOwner(projects,'/work/hotsheet2')).toBe('hotsheet');
    expect(terminalProjectOwner(projects,'/work/kerf/packages/ui')).toBe('kerf');
    expect(terminalProjectOwner(projects,'/work/procurement')).toBe('procurement');
    expect(terminalProjectOwner(projects,'/work/unrelated')).toBeUndefined();
  });

  it('uses the most specific root for nested open projects',()=>{
    expect(terminalProjectOwner([{id:'parent',root:'/work'},{id:'child',root:'/work/app'}],'/work/app/src')).toBe('child');
  });

  it('keeps cwd-less legacy terminals only when ownership is unambiguous',()=>{
    expect(terminalProjectOwner([projects[0]],undefined)).toBe('hotsheet');
    expect(terminalProjectOwner(projects,undefined)).toBeUndefined();
  });
});

