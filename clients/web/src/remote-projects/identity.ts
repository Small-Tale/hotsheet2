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

/** HS2-CW8EY3: the remote machine's name: the peer's live `host_name`, or the reviewed origin's host for an
 * older peer that omits it, never the label this device gave itself while pairing (HS2-HGG426). */
export function remoteHostLabel(hostName: string | null | undefined, origin: string): string {
  const reported = hostName?.trim();
  if (reported) return reported;
  try {
    return new URL(origin).hostname || origin;
  } catch {
    return origin;
  }
}

/** A remote project tab names the remote machine, then the project (HS2-HGG426). */
export function remoteProjectTabName(hostName: string | null | undefined, origin: string, projectName: string): string {
  return `${remoteHostLabel(hostName, origin)} · ${projectName}`;
}
