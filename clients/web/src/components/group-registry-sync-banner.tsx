import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CloudAlert } from 'lucide';

import type { GroupRegistrySync } from '../server-response-parsers';
import { ShellBanner } from './shell-banner';

/**
 * Whether the group registry's publication needs a person (HS2-VFYSQK): the server's `blocked` (the
 * branch is behind, diverged, or carries other unpublished commits) or `failed` (a hook or the remote
 * refused). `waiting`, `local_only`, `synced`, and `untracked` are not problems a person must act on.
 */
export function groupRegistryNeedsAttention(status: GroupRegistrySync | undefined): status is GroupRegistrySync {
  return status?.state === 'blocked' || status?.state === 'failed';
}

/** The server's one-line detail as a sentence: capitalized, ending in punctuation. */
export function groupRegistrySyncSentence(detail: string): string {
  const text = detail.trim();
  if (!text) return '';
  const capitalized = text.charAt(0).toUpperCase() + text.slice(1);
  return /[.!?…]$/.test(capitalized) ? capitalized : `${capitalized}.`;
}

/**
 * A persistent project shell strip while the project's group registry is not reaching its remote
 * (HS2-VFYSQK), in the ticket-store sync banner's grammar (HS2-ADHFKZ): the title says what is not
 * publishing, the detail repeats the server's line (which names the next step) and says that groups
 * still work here but other clones do not see new or renamed ones. It has no dismiss: it clears when
 * the next group-list read reports another state.
 */
export function GroupRegistrySyncBanner({ status }: { status: GroupRegistrySync | undefined }) {
  if (!groupRegistryNeedsAttention(status)) return;
  const detail = [
    groupRegistrySyncSentence(status.detail),
    'Groups are saved on this computer, but other clones do not see new or renamed groups until they are published.',
    status.state === 'failed' ? 'Hot Sheet tries again on its next sync.' : undefined,
  ]
    .filter((part): part is string => Boolean(part))
    .join(' ');
  return (
    <ShellBanner
      component="group-registry-sync-banner"
      tone="warning"
      title="Ticket groups are not publishing changes"
      detail={detail}
      icon={<LucideIcon icon={CloudAlert} name="cloud-alert" />}
    />
  );
}
