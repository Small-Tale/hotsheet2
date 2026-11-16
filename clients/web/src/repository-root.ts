import { resolve } from 'node:path';

import { nonBlank } from './machine-home';

/**
 * The Hot Sheet checkout the local bridge builds and launches binaries from. Hosts that run from a
 * copied snapshot (the stable dev host under the user cache directory, the production host's
 * temporary snapshot) set `HOTSHEET_REPO_ROOT`, because a path relative to the running module or the
 * working directory would land beside the snapshot instead (HS2-KFKE8W). An unset, empty, or
 * whitespace-only value counts as unset and falls back to the checkout two levels above the working
 * directory (`clients/web` inside the checkout); a blank value never resolves to the working directory
 * itself (HS2-GVSJ5Y).
 */
export function developmentRepositoryRoot(
  cwd = process.cwd(),
  environment: Readonly<Record<string, string | undefined>> = process.env,
): string {
  const explicit = nonBlank(environment.HOTSHEET_REPO_ROOT);
  return explicit === undefined ? resolve(cwd, '../..') : resolve(explicit);
}
