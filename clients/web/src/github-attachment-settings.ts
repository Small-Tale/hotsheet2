/**
 * GitHub assets-repository settings (HS2-HSA64D). They keep the original Hot Sheet plugin's
 * keys and are configured headlessly with `hotsheet github-connect --attachment-repo`; the
 * connection dialog rebuilds `settings` on save, so an edit must carry them over.
 */
export const GITHUB_ATTACHMENT_SETTINGS = ['attachment_repo', 'attachment_folder', 'attachment_branch'] as const;

/** Copy an existing connection's string-valued attachment settings into rebuilt settings. */
export function carryGithubAttachmentSettings(
  existing: Readonly<Record<string, unknown>> | undefined,
  settings: Record<string, unknown>,
): Record<string, unknown> {
  if (!existing) return settings;
  for (const key of GITHUB_ATTACHMENT_SETTINGS) {
    const value = existing[key];
    if (typeof value === 'string') settings[key] = value;
  }
  return settings;
}
