import { describe, expect, it } from 'vitest';

import {
  explainProjectUnavailable,
  type ProjectUnavailableCause,
  projectUnavailableCause,
  projectUnavailableTechnicalDetails,
} from './project-unavailable-cause';

const root = '/var/folders/xy/T/hs2/deleted-project';

describe('projectUnavailableCause (HS2-ZKAVZZ)', () => {
  const cases: [string, { error: string; root?: string; recoveryPid?: number }, ProjectUnavailableCause][] = [
    // The real bridge strings recorded against a deleted folder, a file, a locked folder, and a broken link.
    ['deleted folder', { error: `ENOENT: no such file or directory, realpath '${root}'` }, 'folder-missing'],
    [
      'moved parent folder',
      { error: "ENOENT: no such file or directory, realpath '/var/folders/xy/T/hs2'" },
      'folder-missing',
    ],
    ['ENOENT without a path', { error: 'ENOENT: no such file or directory' }, 'folder-missing'],
    [
      'file path under /private',
      { error: `ENOTDIR: not a directory, open '/private${root}/.hotsheet2/store'` },
      'not-a-folder',
    ],
    ['permission denied', { error: `EACCES: permission denied, realpath '${root}'` }, 'permission-denied'],
    ['operation not permitted', { error: `EPERM: operation not permitted, scandir '${root}'` }, 'permission-denied'],
    [
      'linked store gone',
      { error: "ENOENT: no such file or directory, realpath '/private/var/folders/xy/T/hs2/gone.hs2'" },
      'store-link-broken',
    ],
    [
      'linked store not a store',
      { error: 'The linked ticket repository is unavailable or is not a Hot Sheet store.' },
      'store-link-broken',
    ],
    [
      'linked store path is a file',
      { error: "ENOTDIR: not a directory, realpath '/work/other.hs2/hotsheet-store.json'" },
      'store-link-broken',
    ],
    ['missing repository', { error: 'No Hot Sheet 2 ticket repository was found at /x.' }, 'store-link-broken'],
    ['fixture server down', { error: 'beta server unavailable' }, 'server-unreachable'],
    ['bridge unreachable (Chromium)', { error: 'Failed to fetch' }, 'server-unreachable'],
    ['bridge unreachable (WebKit)', { error: 'Load failed' }, 'server-unreachable'],
    ['connection refused', { error: 'connect ECONNREFUSED 127.0.0.1:4100' }, 'server-unreachable'],
    [
      'server health timeout',
      { error: 'Timed out waiting for a healthy Hot Sheet server after 30s (starting).' },
      'server-unreachable',
    ],
    ['unresponsive registered process', { error: 'Could not open project.', recoveryPid: 4242 }, 'server-unreachable'],
    [
      'missing file inside the project',
      { error: `ENOENT: no such file or directory, open '${root}/x.json'` },
      'unknown',
    ],
    ['unknown error', { error: 'The older server only supports schema 2.' }, 'unknown'],
    ['empty error', { error: '' }, 'unknown'],
  ];

  it.each(cases)('%s', (_label, failure, cause) => {
    expect(projectUnavailableCause({ root, ...failure })).toBe(cause);
  });

  it('compares Windows paths and trailing separators', () => {
    expect(
      projectUnavailableCause({
        root: 'C:\\work\\project\\',
        error: "ENOENT: no such file or directory, realpath 'C:\\work\\project'",
      }),
    ).toBe('folder-missing');
  });
});

describe('explainProjectUnavailable (HS2-ZKAVZZ)', () => {
  const explain = (error: string, recoveryPid?: number) =>
    explainProjectUnavailable({ root, name: 'deleted-project', error, ...(recoveryPid ? { recoveryPid } : {}) });

  it('gives every cause a title-style title, a plain detail without codes or paths, and a fitting action', () => {
    const explanations = [
      explain(`ENOENT: no such file or directory, realpath '${root}'`),
      explain(`ENOTDIR: not a directory, open '${root}/.hotsheet2/store'`),
      explain(`EACCES: permission denied, realpath '${root}'`),
      explain('Failed to fetch'),
      explain('The linked ticket repository is unavailable or is not a Hot Sheet store.'),
      explain('Something else.'),
    ];
    expect(explanations.map(({ cause, title, primaryAction }) => [cause, title, primaryAction])).toEqual([
      ['folder-missing', 'Project Folder Not Found', 'close'],
      ['not-a-folder', 'Project Path Is Not a Folder', 'close'],
      ['permission-denied', 'No Permission to Open Project', 'retry'],
      ['server-unreachable', 'Server Unavailable', 'retry'],
      ['store-link-broken', 'Ticket Repository Not Found', 'retry'],
      ['unknown', 'Project Unavailable', 'retry'],
    ]);
    for (const { title, detail } of explanations) {
      expect(title).not.toMatch(/[.!?]$/);
      expect(detail).toMatch(/^[A-Z].*\.$/);
      expect(detail).toContain('deleted-project');
      expect(detail).not.toMatch(/E[A-Z]{3,}|realpath|\//);
    }
  });

  it('never advises restarting the server when only the folder is gone', () => {
    expect(explain(`ENOENT: no such file or directory, realpath '${root}'`).detail).not.toMatch(/server/i);
  });

  it('keeps the raw error, the unresponsive process, and the folder as technical details', () => {
    expect(
      projectUnavailableTechnicalDetails({ root, name: 'x', error: ' Could not open project. ', recoveryPid: 4242 }),
    ).toEqual([
      'Could not open project.',
      'Registered Hot Sheet server process 4242 is not responding.',
      `Project folder: ${root}`,
    ]);
    expect(projectUnavailableTechnicalDetails({ root, name: 'x', error: '' })).toEqual([
      'No error was reported.',
      `Project folder: ${root}`,
    ]);
  });
});
