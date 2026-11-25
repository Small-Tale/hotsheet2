import { describe, expect, it } from 'vitest';

import { remotePermissionList, remotePermissionOwner } from './permissions';

const app = { id: 'remote:H:app', checkoutId: 'app', root: '/srv/app', stores: ['/srv/app.hs2'] };
const api = { id: 'remote:H:api', checkoutId: 'api', root: '/srv/api', stores: [] };

describe('HS2-HSFVHC remote permission ownership', () => {
  it('finds the open tab by checkout identity, root, or linked store', () => {
    expect(remotePermissionOwner('checkout:app', [app, api])).toBe(app);
    expect(remotePermissionOwner('checkout:api', [app, api])).toBe(api);
    expect(remotePermissionOwner('/srv/api', [app, api])).toBe(api);
    expect(remotePermissionOwner('/srv/app.hs2', [app, api])).toBe(app);
    for (const project of ['checkout:other', 'unregistered:/srv/app', '/srv/app/sub', '', undefined, 7])
      expect(remotePermissionOwner(project, [app, api]), String(project)).toBeUndefined();
    expect(remotePermissionOwner('checkout:app', [])).toBeUndefined();
  });

  it('relabels owned requests with their tab id and passes others through unchanged', () => {
    const unowned = { id: 3, project: 'checkout:closed', tool: 'Bash' };
    expect(
      remotePermissionList(
        [
          { id: 1, project: 'checkout:app', tool: 'Bash' },
          { id: 2, project: '/srv/api', tool: 'Edit' },
          unowned,
          { id: 4, tool: 'Read' },
          null,
          ['odd'],
        ],
        [app, api],
      ),
    ).toEqual([
      { id: 1, project: 'remote:H:app', tool: 'Bash' },
      { id: 2, project: 'remote:H:api', tool: 'Edit' },
      unowned,
      { id: 4, tool: 'Read' },
      null,
      ['odd'],
    ]);
  });

  it('leaves a non-list answer, such as an error body, alone', () => {
    const error = { error: 'unavailable' };
    expect(remotePermissionList(error, [app])).toBe(error);
    expect(remotePermissionList([], [app])).toEqual([]);
  });
});
