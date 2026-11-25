import { describe, expect, it } from 'vitest';

import { unavailableProjectKind, unavailableProjectShell } from './project-unavailable-shell';

describe('HS2-H0QGK7 unavailable project shell', () => {
  it('classifies each non-ready selection, with a restore failure first', () => {
    expect(unavailableProjectKind({ restoreFailure: false, opening: false })).toBeUndefined();
    expect(unavailableProjectKind({ restoreFailure: true, opening: true, remotePhase: 'loading' })).toBe(
      'restore-failure',
    );
    expect(unavailableProjectKind({ restoreFailure: false, opening: true, remotePhase: 'loading' })).toBe(
      'remote-loading',
    );
    for (const remotePhase of ['reconnecting', 'expired', 'revoked'] as const)
      expect(unavailableProjectKind({ restoreFailure: false, opening: false, remotePhase })).toBe('remote-unavailable');
    expect(unavailableProjectKind({ restoreFailure: false, opening: true })).toBe('opening');
  });

  it('names one reason and an empty state for every region', () => {
    expect(unavailableProjectShell('restore-failure', 'deleted-project')).toEqual({
      kind: 'restore-failure',
      reason: 'Unavailable because deleted-project could not be opened',
      sidebar: {
        title: 'Project Unavailable',
        detail: 'Ticket views and commands appear here once deleted-project opens again.',
      },
      inspector: {
        title: 'Project Unavailable',
        detail: 'Ticket details appear here once deleted-project opens again.',
      },
      drawer: { title: 'Project Unavailable', detail: 'Terminals appear here once deleted-project opens again.' },
    });
    const remote = unavailableProjectShell('remote-unavailable', 'Build host');
    expect(remote.reason).toBe('Unavailable while Build host cannot reach its server');
    expect(remote.sidebar.title).toBe('Server Unavailable');
    for (const kind of ['opening', 'remote-loading'] as const) {
      const shell = unavailableProjectShell(kind, 'alpha');
      expect(shell.reason).toBe('Unavailable while alpha opens');
      expect([shell.sidebar.title, shell.inspector.title, shell.drawer.title]).toEqual([
        'Opening Project',
        'Opening Project',
        'Opening Project',
      ]);
      expect(shell.drawer.detail).toBe('Terminals appear here once alpha opens.');
    }
  });
});
