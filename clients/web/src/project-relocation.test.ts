import { describe, expect, it, vi } from 'vitest';

import { isMissingPathError, resolveProjectRoot } from './project-relocation';

const missing = (path: string) =>
  Object.assign(new Error(`ENOENT: no such file or directory, realpath '${path}'`), {
    code: 'ENOENT',
  });

function lookup(existing: Record<string, string>, moved: Record<string, string> = {}) {
  return {
    realpath: vi.fn(async (path: string) => {
      if (path in existing) return existing[path];
      throw missing(path);
    }),
    followRelocation: vi.fn(async (root: string) => moved[root]),
  };
}

describe('resolveProjectRoot (HS2-86ES0B)', () => {
  it('opens an existing folder as itself without asking the registry', async () => {
    const fixture = lookup({ '/work/app': '/private/work/app' }, { '/work/app': '/elsewhere/app' });
    await expect(resolveProjectRoot('/work/app', fixture)).resolves.toBe('/private/work/app');
    expect(fixture.followRelocation).not.toHaveBeenCalled();
  });

  it('follows a vanished root to the relocated checkout and canonicalizes its current root', async () => {
    const fixture = lookup({ '/new/app': '/private/new/app' }, { '/old/app': '/new/app' });
    await expect(resolveProjectRoot('/old/app', fixture)).resolves.toBe('/private/new/app');
    expect(fixture.followRelocation).toHaveBeenCalledWith('/old/app');
  });

  it('keeps the original ENOENT when the registry cannot follow the root or its lookup fails', async () => {
    await expect(resolveProjectRoot('/old/app', lookup({}))).rejects.toThrow("realpath '/old/app'");
    const failing = { ...lookup({}), followRelocation: vi.fn(async () => Promise.reject(new Error('server down'))) };
    await expect(resolveProjectRoot('/old/app', failing)).rejects.toThrow("realpath '/old/app'");
  });

  it('never follows an error other than a missing path', async () => {
    const denied = Object.assign(new Error('EACCES'), { code: 'EACCES' });
    const fixture = { realpath: vi.fn(async () => Promise.reject(denied)), followRelocation: vi.fn() };
    await expect(resolveProjectRoot('/locked/app', fixture)).rejects.toBe(denied);
    expect(fixture.followRelocation).not.toHaveBeenCalled();
  });

  it('classifies only ENOENT and ENOTDIR as a missing path', () => {
    expect(isMissingPathError(missing('/x'))).toBe(true);
    expect(isMissingPathError(Object.assign(new Error(), { code: 'ENOTDIR' }))).toBe(true);
    expect(isMissingPathError(Object.assign(new Error(), { code: 'EACCES' }))).toBe(false);
    expect(isMissingPathError(undefined)).toBe(false);
    expect(isMissingPathError('ENOENT')).toBe(false);
  });
});
