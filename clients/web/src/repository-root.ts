import { resolve } from 'node:path';

/**
 * The Hot Sheet checkout the local bridge builds and launches binaries from. Hosts that run from a
 * copied snapshot (the stable dev host under the user cache directory, the production host's
 * temporary snapshot) set `HOTSHEET_REPO_ROOT`, because a path relative to the running module or the
 * working directory would land beside the snapshot instead (HS2-KFKE8W).
 */
export function developmentRepositoryRoot(
  cwd = process.cwd(),
  environment: Readonly<Record<string, string | undefined>> = process.env,
): string {
  return resolve(environment.HOTSHEET_REPO_ROOT ?? resolve(cwd, '../..'));
}
