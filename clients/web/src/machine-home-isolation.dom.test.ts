// HS2-34QFBQ: the jsdom project's workers inherit the same per-run machine home as the unit
// project (`vitest.config.ts`), so DOM tests can never reach the developer's real stores.
import { homedir, tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

describe('HS2-34QFBQ DOM project machine-home isolation', () => {
  it('runs under a temporary HOTSHEET_HOME, not the real one', () => {
    const home = resolve(process.env.HOTSHEET_HOME ?? '');
    expect(home).not.toBe(resolve(homedir(), '.hotsheet2'));
    expect(home.startsWith(resolve(tmpdir(), 'hotsheet-vitest-home-'))).toBe(true);
  });
});
