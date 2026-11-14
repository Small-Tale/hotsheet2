/** Browser-facing project ids include the reviewed machine binding, so two servers may expose the
 * same checkout id without sharing a tab, API path, or change-stream cursor (HS2-BT4KN6). */
export function remoteProjectId(bindingId: string, checkoutId: string): string {
  if (!/^[0-9A-HJKMNP-TV-Z]{26}$/.test(bindingId) || !checkoutId || checkoutId.length > 160)
    throw new Error('Invalid remote project identity.');
  return `remote:${bindingId}:${encodeURIComponent(checkoutId)}`;
}

export function parseRemoteProjectId(value: string): { bindingId: string; checkoutId: string } | undefined {
  const match = /^remote:([0-9A-HJKMNP-TV-Z]{26}):(.+)$/.exec(value);
  if (!match) return undefined;
  try {
    const checkoutId = decodeURIComponent(match[2]);
    if (!checkoutId || checkoutId.length > 160 || encodeURIComponent(checkoutId) !== match[2]) return undefined;
    return { bindingId: match[1], checkoutId };
  } catch {
    return undefined;
  }
}

/** Whether a (possibly URL-encoded) project id names a cross-server tab (HS2-F04J42). */
export function isRemoteProjectId(project: string): boolean {
  try {
    return decodeURIComponent(project).startsWith('remote:');
  } catch {
    return project.startsWith('remote:');
  }
}
