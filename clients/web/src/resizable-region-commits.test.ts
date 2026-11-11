import { describe, expect, it } from 'vitest';

import { createRegionCommitRoutes } from './resizable-region-commits';

describe('region commit routes (HS2-BS0XFK)', () => {
  it('delivers a commit only to the route registered for its region', () => {
    const routes = createRegionCommitRoutes();
    const sizes: number[] = [];
    const dispose = routes.route('announcer-dock', (size) => sizes.push(size));
    routes.commit({ id: 'other-region', size: 10, source: 'pointer' });
    routes.commit({ id: 'announcer-dock', size: 200, source: 'keyboard' });
    expect(sizes).toEqual([200]);
    dispose();
    routes.commit({ id: 'announcer-dock', size: 220, source: 'pointer' });
    expect(sizes).toEqual([200]);
  });

  it('keeps a newer route when an older disposer runs late', () => {
    const routes = createRegionCommitRoutes();
    const seen: string[] = [];
    const disposeOld = routes.route('announcer-dock', () => seen.push('old'));
    routes.route('announcer-dock', () => seen.push('new'));
    disposeOld();
    routes.commit({ id: 'announcer-dock', size: 200, source: 'pointer' });
    expect(seen).toEqual(['new']);
  });
});
