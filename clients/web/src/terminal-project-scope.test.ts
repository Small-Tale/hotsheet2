import { describe, expect, it } from 'vitest';

import { terminalDrawerActivation, terminalProjectOwner } from './terminal-project-scope';

describe('terminalProjectOwner', () => {
  const projects = [
    { id: 'hotsheet', root: '/work/hotsheet2' },
    { id: 'kerf', root: '/work/kerf' },
    { id: 'procurement', root: '/work/procurement' },
  ];

  it('assigns each host-wide terminal only to the project containing its cwd', () => {
    expect(terminalProjectOwner(projects, '/work/hotsheet2')).toBe('hotsheet');
    expect(terminalProjectOwner(projects, '/work/kerf/packages/ui')).toBe('kerf');
    expect(terminalProjectOwner(projects, '/work/procurement')).toBe('procurement');
    expect(terminalProjectOwner(projects, '/work/unrelated')).toBeUndefined();
  });

  it('uses the most specific root for nested open projects', () => {
    expect(
      terminalProjectOwner(
        [
          { id: 'parent', root: '/work' },
          { id: 'child', root: '/work/app' },
        ],
        '/work/app/src',
      ),
    ).toBe('child');
  });

  it('keeps cwd-less legacy terminals only when ownership is unambiguous', () => {
    expect(terminalProjectOwner([projects[0]], undefined)).toBe('hotsheet');
    expect(terminalProjectOwner(projects, undefined)).toBeUndefined();
  });
});

describe('terminalDrawerActivation', () => {
  const values = new Map([
      ['hotsheet.project.alpha.terminal-drawer-selection', 'alpha-shell'],
      ['hotsheet.project.beta.terminal-drawer-selection', 'beta-shell'],
    ]),
    storage = { getItem: (key: string) => values.get(key) ?? null };

  it('keeps the project and remembered terminal paired through repeated switches', () => {
    expect(['alpha', 'beta', 'alpha'].map((projectId) => terminalDrawerActivation(storage, projectId))).toEqual([
      { projectId: 'alpha', selectedId: 'alpha-shell' },
      { projectId: 'beta', selectedId: 'beta-shell' },
      { projectId: 'alpha', selectedId: 'alpha-shell' },
    ]);
  });

  it('falls back to the project grid only when that project has no remembered surface', () => {
    expect(terminalDrawerActivation(storage, 'new-project')).toEqual({ projectId: 'new-project', selectedId: 'grid' });
  });
});
