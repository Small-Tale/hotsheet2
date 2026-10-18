import { access, mkdir, readFile, realpath, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, resolve } from 'node:path';

/**
 * Pre-fix production hosts wrote a disposable snapshot CLI into Codex's hook config. After the
 * first upgrade restart that path is gone, while an open Codex TUI may still retain it. Restore
 * only a Hot Sheet snapshot path from this machine's temporary directory as an alias to the
 * durable CLI before setup rewrites the config.
 */
export async function restoreLegacyCodexHookCli(
  projectRoot: string,
  stableCli: string | undefined,
  temporaryRoot = tmpdir(),
): Promise<void> {
  if (!stableCli || !isAbsolute(stableCli)) return;
  let hooks: unknown;
  try {
    hooks = JSON.parse(await readFile(resolve(projectRoot, '.codex/hooks.json'), 'utf8'));
    await access(stableCli);
  } catch {
    return;
  }
  if (!hooks || typeof hooks !== 'object' || !('hooks' in hooks)) return;
  const events = hooks.hooks;
  if (!events || typeof events !== 'object' || !('PermissionRequest' in events)) return;
  const entries = events.PermissionRequest;
  if (!Array.isArray(entries)) return;
  for (const entry of entries as unknown[]) {
    if (!entry || typeof entry !== 'object' || !('hooks' in entry) || !Array.isArray(entry.hooks)) continue;
    for (const handler of entry.hooks as unknown[]) {
      if (!handler || typeof handler !== 'object' || !('command' in handler) || typeof handler.command !== 'string')
        continue;
      const match = /^(\S+) permission-hook --agent codex$/.exec(handler.command);
      if (!match) continue;
      const oldCli = match[1];
      if (!oldCli) continue;
      const snapshotRoot = dirname(dirname(oldCli));
      if (
        !isAbsolute(oldCli) ||
        oldCli !== resolve(snapshotRoot, 'bin/hotsheet-cli') ||
        dirname(snapshotRoot) !== resolve(temporaryRoot) ||
        !/^hotsheet-local-host-[A-Za-z0-9]+$/.test(basename(snapshotRoot))
      )
        continue;
      try {
        await access(oldCli);
      } catch {
        await mkdir(dirname(oldCli), { recursive: true });
        const canonicalTemp = await realpath(temporaryRoot),
          canonicalSnapshot = await realpath(snapshotRoot),
          canonicalBin = await realpath(dirname(oldCli));
        if (
          canonicalSnapshot !== resolve(canonicalTemp, basename(snapshotRoot)) ||
          canonicalBin !== resolve(canonicalSnapshot, 'bin')
        )
          continue;
        try {
          await symlink(stableCli, oldCli);
        } catch (error) {
          if (!(error instanceof Error && 'code' in error && error.code === 'EEXIST')) throw error;
          await access(oldCli);
        }
      }
      return;
    }
  }
}
