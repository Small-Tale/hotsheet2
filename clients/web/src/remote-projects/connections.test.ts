import { describe, expect, it } from 'vitest';

import { remoteConnectionCreate, remoteConnectionItem, remoteConnectionList } from './connections';
import { remoteProjectId } from './identity';

const host = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const otherHost = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
const app = remoteProjectId(host, 'same-checkout');
const api = remoteProjectId(host, 'api-checkout');
const equalOtherHost = remoteProjectId(otherHost, 'same-checkout');

describe('HS2-TCJQHX remote AI connection identity', () => {
  it('sends the native checkout id to the remote host and rejects another tab or host', () => {
    expect(
      remoteConnectionCreate({ tool: 'claude', checkout: app, connection_id: 'chat-1' }, app, 'same-checkout'),
    ).toEqual({
      tool: 'claude',
      checkout: 'same-checkout',
      connection_id: 'chat-1',
    });
    expect(() => remoteConnectionCreate({ checkout: api }, app, 'same-checkout')).toThrow(/different remote project/);
    expect(() => remoteConnectionCreate({ checkout: equalOtherHost }, app, 'same-checkout')).toThrow(
      /different remote project/,
    );
    expect(() => remoteConnectionCreate([], app, 'same-checkout')).toThrow(/JSON object/);
  });

  it('projects only the owning checkout connections and saved sessions into a tab', () => {
    const scopes = [
      { id: app, root: '/srv/app', stores: ['/srv/app.hs2'] },
      { id: api, root: '/srv/api', stores: ['/srv/api.hs2'] },
    ];
    const connections = [
      { id: 'app-chat', project: '/srv/app' },
      { id: 'api-chat', project: '/srv/api' },
      { id: 'app-hook', project: '/srv/app.hs2' },
      { id: 'stray', project: '/srv/stray' },
    ];
    expect(remoteConnectionList(connections, app, scopes)).toEqual([
      { id: 'app-chat', project: app },
      { id: 'app-hook', project: app },
    ]);
    expect(remoteConnectionList(connections, api, scopes)).toEqual([{ id: 'api-chat', project: api }]);
    expect(remoteConnectionItem({ id: 'new', project: '/srv/app' }, app, scopes)).toEqual({ id: 'new', project: app });
    expect(remoteConnectionItem({ id: 'foreign', project: '/srv/api' }, app, scopes)).toBeUndefined();
  });

  it('keeps equal native checkout ids on different hosts isolated by reviewed binding', () => {
    const appScope = [{ id: app, root: '/srv/shared', stores: [] }];
    const otherScope = [{ id: equalOtherHost, root: '/srv/shared', stores: [] }];
    const list = [{ id: 'chat', project: '/srv/shared' }];
    expect(remoteConnectionList(list, app, appScope)).toEqual([{ id: 'chat', project: app }]);
    expect(remoteConnectionList(list, equalOtherHost, otherScope)).toEqual([{ id: 'chat', project: equalOtherHost }]);
    expect(remoteConnectionList(list, app, otherScope)).toEqual([]);
  });

  it('does not expose malformed or unowned host records', () => {
    const scopes = [{ id: app, root: '/srv/app', stores: [] }];
    expect(remoteConnectionList([null, 'x', { project: '' }, { project: '/srv/else' }], app, scopes)).toEqual([]);
    expect(remoteConnectionList({ error: 'unavailable' }, app, scopes)).toEqual({ error: 'unavailable' });
    expect(remoteConnectionItem(null, app, scopes)).toBeUndefined();
  });
});
