import { readFileSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { expect, it } from 'vitest';

import { restoreLegacyCodexHookCli } from './codex-hook-compat';

it('restores a pre-upgrade Codex hook command after its host snapshot is removed', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'hs-hook-upgrade-')),
    snapshot = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-')),
    oldCli = resolve(snapshot, 'bin/hotsheet-cli'),
    stable = resolve(root, 'home/bin/hotsheet-cli');
  try {
    await mkdir(resolve(root, '.codex'));
    await mkdir(resolve(root, 'home/bin'), { recursive: true });
    await writeFile(stable, 'durable cli');
    await writeFile(
      resolve(root, '.codex/hooks.json'),
      JSON.stringify({
        hooks: { PermissionRequest: [{ hooks: [{ command: `${oldCli} permission-hook --agent codex` }] }] },
      }),
    );
    await rm(snapshot, { recursive: true });
    await restoreLegacyCodexHookCli(root, stable);
    expect(readFileSync(oldCli, 'utf8')).toBe('durable cli');
    await restoreLegacyCodexHookCli(root, stable);
    expect(JSON.parse(await readFile(resolve(root, '.codex/hooks.json'), 'utf8')).hooks.PermissionRequest).toHaveLength(
      1,
    );
  } finally {
    await rm(snapshot, { recursive: true, force: true });
    await rm(root, { recursive: true, force: true });
  }
});

it('ignores hook commands outside Hot Sheet snapshot directories', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'hs-hook-foreign-')),
    foreign = resolve(root, 'foreign/bin/hotsheet-cli'),
    stable = resolve(root, 'home/bin/hotsheet-cli');
  try {
    await mkdir(resolve(root, '.codex'));
    await mkdir(resolve(root, 'home/bin'), { recursive: true });
    await writeFile(stable, 'durable cli');
    await writeFile(
      resolve(root, '.codex/hooks.json'),
      JSON.stringify({
        hooks: { PermissionRequest: [{ hooks: [{ command: `${foreign} permission-hook --agent codex` }] }] },
      }),
    );
    await restoreLegacyCodexHookCli(root, stable);
    await expect(readFile(foreign)).rejects.toMatchObject({ code: 'ENOENT' });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
