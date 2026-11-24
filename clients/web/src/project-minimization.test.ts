import { describe, expect, it } from 'vitest';

import {
  loadMinimizedProjects,
  minimizedProjectDescription,
  minimizeProject,
  restoreProject,
  saveMinimizedProjects,
  visibleProjectIds,
} from './project-minimization';

describe('HS2-4Q9DVS project minimization', () => {
  const order = ['studio', 'docs', 'analytics', 'design'];

  it('keeps the selected workspace when a background tab is minimized', () => {
    const result = minimizeProject(order, [], 'studio', 'analytics');
    expect(result).toEqual({ minimized: ['analytics'] });
    expect(visibleProjectIds(order, result.minimized)).toEqual(['studio', 'docs', 'design']);
  });

  it('chooses the next visible tab, then the previous tab, then no tab', () => {
    expect(minimizeProject(order, ['analytics'], 'docs', 'docs')).toEqual({
      minimized: ['analytics', 'docs'],
      nextSelected: 'design',
    });
    expect(minimizeProject(order, ['analytics', 'design'], 'docs', 'docs')).toEqual({
      minimized: ['analytics', 'design', 'docs'],
      nextSelected: 'studio',
    });
    expect(minimizeProject(order, ['studio', 'analytics', 'design'], 'docs', 'docs')).toEqual({
      minimized: ['studio', 'analytics', 'design', 'docs'],
      nextSelected: undefined,
    });
  });

  it('restores a project in its original order and ignores stale or repeated actions', () => {
    const hidden = ['docs', 'design'];
    expect(minimizeProject(order, hidden, 'studio', 'missing')).toEqual({ minimized: hidden });
    expect(minimizeProject(order, hidden, 'studio', 'docs')).toEqual({ minimized: hidden });
    expect(visibleProjectIds(order, restoreProject(hidden, 'docs'))).toEqual(['studio', 'docs', 'analytics']);
  });

  it('loads only distinct nonblank identities and persists them across reload', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value);
      },
    };
    saveMinimizedProjects(storage, ['docs', 'analytics']);
    expect(loadMinimizedProjects(storage)).toEqual(['docs', 'analytics']);
    values.set('hotsheet.layout.minimized-projects.v1', '["docs","docs","",42,"analytics"]');
    expect(loadMinimizedProjects(storage)).toEqual(['docs', 'analytics']);
    values.set('hotsheet.layout.minimized-projects.v1', '{broken');
    expect(loadMinimizedProjects(storage)).toEqual([]);
  });

  it('disambiguates duplicate names with a unique host or path', () => {
    const projects = [
      { id: 'one', name: 'Shared', root: '/one', remoteHost: 'Host A' },
      { id: 'two', name: 'Shared', root: '/two', remoteHost: 'Host B' },
      { id: 'three', name: 'Unique', root: '/three' },
    ];
    expect(minimizedProjectDescription(projects[0], projects)).toBe('Host A');
    expect(minimizedProjectDescription(projects[2], projects)).toBeUndefined();
    expect(
      minimizedProjectDescription({ ...projects[1], remoteHost: 'Host A' }, [
        projects[0],
        { ...projects[1], remoteHost: 'Host A' },
      ]),
    ).toBe('/two');
  });
});
