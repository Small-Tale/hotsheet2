import { afterEach, describe, expect, it, vi } from 'vitest';

import { createDevApp } from './dev-server';
import { createLocalHostApp } from './local-host';

afterEach(() => vi.unstubAllEnvs());

describe('HS2-7DBJMS remote browser boundary', () => {
  it('denies unauthenticated project access and every local-only bridge route', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const app = createDevApp();
    expect((await app.request('/__hotsheet/checkouts')).status).toBe(401);
    expect((await app.request('/__hotsheet/project-api/synthetic/tickets')).status).toBe(401);
    expect((await app.request('/__hotsheet/folders/choose', { method: 'POST' })).status).toBe(404);
    expect((await app.request('/__hotsheet/remote-auth/owner')).status).toBe(403);
    expect(
      (
        await app.request('/__hotsheet/project-api/synthetic/tickets', {
          headers: { 'x-hotsheet-secret': 'spoofed' },
        })
      ).status,
    ).toBe(401);
  });

  it('keeps production desktop storage out of remote mode', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const app = createLocalHostApp('/nonexistent-synthetic-dist', createDevApp(), '/nonexistent-synthetic-storage');
    expect((await app.request('/__hotsheet/desktop-storage')).status).toBe(404);
  });
});
