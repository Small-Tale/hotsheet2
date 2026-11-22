import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { WifiOff } from 'lucide';

import { ShellBanner } from './shell-banner';

/**
 * An open remote project's server stopped answering mid-session (HS2-WNK73S). The tab and its last
 * loaded tickets stay; the project's change stream reconnects on its own bounded backoff, and the
 * banner leaves once the stream is live again.
 */
export function RemoteReconnectingBanner({ host }: { host: string }) {
  return (
    <ShellBanner
      component="remote-reconnecting-banner"
      title={`Reconnecting to ${host}`}
      detail="The remote server is not answering. These are the tickets loaded before the connection dropped; Hot Sheet reconnects automatically."
      tone="warning"
      icon={<LucideIcon icon={WifiOff} name="wifi-off" />}
    />
  );
}
