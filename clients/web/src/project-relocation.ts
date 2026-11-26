/**
 * HS2-86ES0B: a remembered project root that `hotsheet-cli checkout relocate` moved resolves to
 * the checkout's current root instead of failing with ENOENT. The server's checkout registry owns
 * the evidence (`POST /projects/follow-relocation`); nothing here searches the disk, so a folder
 * moved without telling the registry still fails and its tab keeps the Retry and Close actions.
 */

export interface RelocationLookup {
  /** Canonicalize an existing path; rejects with an `ENOENT` error for a vanished one. */
  realpath: (path: string) => Promise<string>;
  /** The current root of the checkout an explicit relocate moved away from `root`, if any. */
  followRelocation: (root: string) => Promise<string | undefined>;
}

/** Errors that mean the remembered folder is gone, not that it is unreadable. */
export function isMissingPathError(error: unknown): boolean {
  const code = (error as { code?: unknown } | null)?.code;
  return code === 'ENOENT' || code === 'ENOTDIR';
}

/**
 * The canonical root to open for a remembered `root`: the folder itself when it exists, else the
 * current root of the checkout relocated away from it. Any other failure, or a vanished root the
 * registry cannot follow, rethrows the original error so the restore failure still names it.
 */
export async function resolveProjectRoot(root: string, lookup: RelocationLookup): Promise<string> {
  try {
    return await lookup.realpath(root);
  } catch (error) {
    if (!isMissingPathError(error)) throw error;
    const moved = await lookup.followRelocation(root).catch(() => undefined);
    if (!moved) throw error;
    return lookup.realpath(moved);
  }
}
