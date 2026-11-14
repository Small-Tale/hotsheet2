import { describe, expect, it } from 'vitest';

import { parseRegisteredProjects, registeredProjectName, sortRegisteredProjects } from './registered-projects-model';

describe('HS2-R806FZ registered project model', () => {
  it('parses a valid list and rejects malformed rows', () => {
    const list = [{ id: 'a', alias: 'a', root: '/a', available: true }];
    expect(parseRegisteredProjects(list)).toEqual(list);
    expect(() => parseRegisteredProjects([{ id: 'a', alias: 'a', root: '/a' }])).toThrow('invalid project list');
    expect(() => parseRegisteredProjects({})).toThrow('invalid project list');
  });

  it('names a project by alias, else its last folder segment, else its root', () => {
    expect(registeredProjectName({ alias: 'web', root: '/x/y' })).toBe('web');
    expect(registeredProjectName({ alias: '', root: '/x/old-clone/' })).toBe('old-clone');
    expect(registeredProjectName({ alias: '', root: 'C:\\Users\\x\\proj' })).toBe('proj');
    expect(registeredProjectName({ alias: '', root: '/' })).toBe('/');
  });

  it('sorts by name without regard to case, then by folder', () => {
    const sorted = sortRegisteredProjects([
      { id: '3', alias: 'web', root: '/b', available: true },
      { id: '1', alias: 'API', root: '/z', available: true },
      { id: '2', alias: 'web', root: '/a', available: false },
    ]);
    expect(sorted.map((p) => p.id)).toEqual(['1', '2', '3']);
  });
});
