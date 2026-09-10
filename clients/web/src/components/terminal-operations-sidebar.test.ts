import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {aggregateTerminalProjectSummaries,TerminalOperationsSidebar} from './terminal-operations-sidebar';

describe('TerminalOperationsSidebar',()=>{
  const projects=[
    {id:'one',name:'One',completedToday:2,inProgress:3,trend:[1,0,2]},
    {id:'two',name:'Two',completedToday:4,inProgress:1,trend:[0,3,4]},
  ];

  it('sums project counts and aligned trend days',()=>{
    expect(aggregateTerminalProjectSummaries(projects)).toEqual({id:'all',name:'All projects',completedToday:6,inProgress:4,trend:[1,3,6]});
  });

  it('adds an aggregate group only when multiple projects are open',()=>{
    const multiple=String(TerminalOperationsSidebar({projects})),single=String(TerminalOperationsSidebar({projects:[projects[0]]}));
    expect(multiple).toContain('All projects');
    expect(multiple).toContain('data-chart-tone="success"');
    expect(multiple.match(/data-chart-tone="brand"/g)).toHaveLength(2);
    expect(multiple.match(/data-component="project-summary"/g)).toHaveLength(3);
    expect(single).not.toContain('All projects');
    expect(single.match(/data-component="project-summary"/g)).toHaveLength(1);
  });

  it('separates the multi-project aggregate from individual projects',()=>{
    const css=readFileSync(new URL('./terminal-operations-sidebar.css',import.meta.url),'utf8');
    expect(css).toMatch(/terminal-operations-sidebar__group\[data-project-id="all"\] \{[^}]*padding-bottom: var\(--wa-space-m\);[^}]*border-bottom: 1px solid var\(--wa-color-surface-border\)/);
  });
});
