import { describe, expect, it } from 'vitest';

import { remoteProjectId } from './identity';
import {
  remoteTerminalItem,
  remoteTerminalLaunchCwd,
  remoteTerminalList,
  remoteTerminalOwner,
  remoteTerminalView,
} from './terminals';

const host = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const other = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
const app = remoteProjectId(host, 'app-1a2b3c');
const api = remoteProjectId(host, 'api-4d5e6f');
const nested = remoteProjectId(host, 'nested-7a8b9c');

describe('HS2-PPHJMZ remote terminal launch directory', () => {
  it('launches a tab terminal in its remote checkout, never at the composite id', () => {
    expect(remoteTerminalLaunchCwd(app, app, '/srv/app')).toBe('/srv/app');
    expect(remoteTerminalLaunchCwd(undefined, app, '/srv/app')).toBe('/srv/app');
    expect(remoteTerminalLaunchCwd('', app, '/srv/app')).toBe('/srv/app');
    expect(remoteTerminalLaunchCwd(null, app, '/srv/app/')).toBe('/srv/app/');
  });

  it('maps a composite-relative directory under the remote root, with the root separator', () => {
    expect(remoteTerminalLaunchCwd(`${app}/packages/ui`, app, '/srv/app')).toBe('/srv/app/packages/ui');
    expect(remoteTerminalLaunchCwd(`${app}/packages/ui`, app, 'C:\\work\\app')).toBe('C:\\work\\app\\packages\\ui');
    expect(remoteTerminalLaunchCwd(`${app}/`, app, '/srv/app/')).toBe('/srv/app');
  });

  it('keeps a remote command directory and refuses another tab or a non-text value', () => {
    expect(remoteTerminalLaunchCwd('/srv/app/tools', app, '/srv/app')).toBe('/srv/app/tools');
    expect(() => remoteTerminalLaunchCwd(api, app, '/srv/app')).toThrow(/different project/);
    expect(() => remoteTerminalLaunchCwd(remoteProjectId(other, 'app-1a2b3c'), app, '/srv/app')).toThrow(
      /different project/,
    );
    expect(() => remoteTerminalLaunchCwd(42, app, '/srv/app')).toThrow(/text/);
  });
});

describe('HS2-JJKVJJ remote terminal ownership', () => {
  const scopes = [
    { id: app, root: '/srv/app' },
    { id: api, root: '/srv/api' },
    { id: nested, root: '/srv/app/nested' },
  ];

  it('gives each terminal only to the most specific open checkout on its server', () => {
    expect(remoteTerminalOwner(scopes, { launch_cwd: '/srv/app' })).toBe(app);
    expect(remoteTerminalOwner(scopes, { launch_cwd: '/srv/app/src', cwd: '/srv/api' })).toBe(app);
    expect(remoteTerminalOwner(scopes, { launch_cwd: '/srv/app/nested/lib' })).toBe(nested);
    expect(remoteTerminalOwner(scopes, { launch_cwd: '/srv/api' })).toBe(api);
    expect(remoteTerminalOwner(scopes, { launch_cwd: '/home/someone' })).toBeUndefined();
  });

  it('falls back to the reported directory, and to a lone tab only when unambiguous', () => {
    expect(remoteTerminalOwner(scopes, { cwd: '/srv/api/bin' })).toBe(api);
    expect(remoteTerminalOwner(scopes, { launch_cwd: '/home/someone', cwd: '/srv/api/bin' })).toBe(api);
    expect(remoteTerminalOwner(scopes, {})).toBeUndefined();
    expect(remoteTerminalOwner([scopes[0]], {})).toBe(app);
    expect(remoteTerminalOwner([scopes[0]], { launch_cwd: '', cwd: '' })).toBe(app);
  });

  it('never lets the only open tab on a server claim a terminal launched outside it', () => {
    const only = [scopes[0]];
    expect(remoteTerminalOwner(only, { launch_cwd: '/home/someone' })).toBeUndefined();
    expect(remoteTerminalOwner(only, { cwd: '/home/someone' })).toBeUndefined();
    expect(remoteTerminalList([{ id: 'stray', launch_cwd: '/home/someone' }], app, only)).toEqual([]);
  });

  it('keeps a terminal an older client launched at the composite id with that tab only', () => {
    expect(remoteTerminalOwner(scopes, { launch_cwd: app, cwd: '/home/someone' })).toBe(app);
    expect(remoteTerminalOwner(scopes, { launch_cwd: api })).toBe(api);
    expect(remoteTerminalOwner(scopes, { launch_cwd: remoteProjectId(host, 'closed-000000') })).toBeUndefined();
  });

  it('lists a server terminal in exactly one tab and presents the tab id as its launch directory', () => {
    const list = [
      { id: 'one', alive: true, busy: false, launch_cwd: '/srv/app', cwd: '/srv/app/src' },
      { id: 'two', alive: true, busy: false, launch_cwd: '/srv/api' },
      { id: 'three', alive: true, busy: false, launch_cwd: '/srv/app/nested' },
      { id: 'stray', alive: true, busy: false, launch_cwd: '/home/someone' },
    ];
    expect(remoteTerminalList(list, app, scopes)).toEqual([
      { id: 'one', alive: true, busy: false, launch_cwd: app, cwd: '/srv/app/src' },
    ]);
    expect(remoteTerminalList(list, api, scopes)).toEqual([{ id: 'two', alive: true, busy: false, launch_cwd: api }]);
    expect(remoteTerminalList(list, nested, scopes)).toEqual([
      { id: 'three', alive: true, busy: false, launch_cwd: nested },
    ]);
    // Adding a terminal to one tab changes no other tab's list.
    const added = [...list, { id: 'four', alive: true, busy: false, launch_cwd: '/srv/api' }];
    expect(remoteTerminalList(added, app, scopes)).toEqual(remoteTerminalList(list, app, scopes));
    expect(remoteTerminalList(added, api, scopes)).toHaveLength(2);
  });

  it('passes a non-list answer and an unowned or non-object item through unchanged', () => {
    expect(remoteTerminalList({ error: 'x' }, app, scopes)).toEqual({ error: 'x' });
    expect(remoteTerminalList([null, 'x', { id: 'one', launch_cwd: '/srv/app' }], app, scopes)).toEqual([
      { id: 'one', launch_cwd: app },
    ]);
    expect(remoteTerminalItem({ id: 'one', launch_cwd: '/srv/api' }, app, scopes)).toEqual({
      id: 'one',
      launch_cwd: '/srv/api',
    });
    expect(remoteTerminalItem({ id: 'one', launch_cwd: '/srv/app' }, app, scopes)).toEqual({
      id: 'one',
      launch_cwd: app,
    });
    expect(remoteTerminalItem([1], app, scopes)).toEqual([1]);
    expect(remoteTerminalItem(null, app, scopes)).toBeNull();
    expect(remoteTerminalView({ id: 'x', cwd: '/srv/app' }, app)).toEqual({
      id: 'x',
      cwd: '/srv/app',
      launch_cwd: app,
    });
  });
});
