import { expect, it } from 'vitest';

import type { TerminalDashboardGroup } from './components/terminal-dashboard';
import { TerminalSnapshotRefresh } from './terminal-snapshot-refresh';

const group = (projectId: string, id: string): TerminalDashboardGroup => ({
  projectId,
  projectName: projectId,
  sessions: [{ id, projectId, projectName: projectId, alive: true, busy: false, scrollback: '' }],
});

it('merges independent project resyncs and retains failed snapshots without resolving halts', () => {
  const refresh = new TerminalSnapshotRefresh(),
    a = refresh.begin('a'),
    b = refresh.begin('b'),
    before = [group('a', 'old'), group('b', 'old')],
    updated = refresh.merge(before, [{ projectId: 'b', version: b, group: group('b', 'new') }], ['a', 'b']);
  expect(updated.map((g) => g.sessions[0].id)).toEqual(['old', 'new']);
  expect(refresh.merge(updated, [{ projectId: 'a', version: a }], ['a', 'b'])).toEqual(updated);
  expect(
    refresh
      .merge(updated, [{ projectId: 'a', version: a, group: group('a', 'new') }], ['a', 'b'])
      .map((g) => g.sessions[0].id),
  ).toEqual(['new', 'new']);
});

it('rejects stale same-project snapshots, accepts authoritative empty state and discards closed projects', () => {
  const refresh = new TerminalSnapshotRefresh(),
    stale = refresh.begin('a'),
    latest = refresh.begin('a'),
    updated = refresh.merge(
      [group('a', 'old')],
      [{ projectId: 'a', version: latest, group: { ...group('a', 'new'), sessions: [] } }],
      ['a'],
    );
  expect(updated[0].sessions).toEqual([]);
  expect(refresh.merge(updated, [{ projectId: 'a', version: stale, group: group('a', 'stale') }], ['a'])).toEqual(
    updated,
  );
  expect(refresh.merge(updated, [{ projectId: 'a', version: latest, group: group('a', 'new') }], [])).toEqual([]);
});
