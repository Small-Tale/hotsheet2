import { describe, expect, it } from 'vitest';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

// HS2-T9VXEP guard: spawned HS2 binaries inherit this run's machine home, which must be the
// throwaway directory from test/isolated-home.mjs and never the real `~/.hotsheet2`.
describe('migrator test isolation', () => {
  it('runs every spawned binary under a temporary HOTSHEET_HOME', () => {
    const home = process.env.HOTSHEET_HOME;
    expect(home).toBeTruthy();
    expect(resolve(home)).not.toBe(resolve(join(homedir(), '.hotsheet2')));
    expect(home.startsWith(tmpdir())).toBe(true);
  });

  it('runs every spawned binary with installed AI-tool probes off (HS2-F6R2HQ)', () => {
    expect(process.env.HOTSHEET_AI_TOOL_PROBES).toBe('off');
  });
});
