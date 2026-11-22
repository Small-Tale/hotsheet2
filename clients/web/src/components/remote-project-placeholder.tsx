import { Button } from '@kerfjs/ui/button';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { WifiOff } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';

export interface RemoteProjectPlaceholderProps {
  /** The remote project id the placeholder stands for. */
  projectId: string;
  name: string;
  /** `loading`: the first open is still running (HS2-EJ5K09); `reconnecting`: the host did not answer (HS2-WNK73S). */
  phase: 'loading' | 'reconnecting';
  /** A retry of the host is in flight. */
  connecting?: boolean;
}

/**
 * The workspace of a selected remembered remote project that is not open yet. Loading reuses the
 * opening presentation; reconnecting says the server is not answering, that Hot Sheet keeps trying,
 * and offers an immediate retry. The tab stays until the user closes it.
 */
export function RemoteProjectPlaceholder({
  projectId,
  name,
  phase,
  connecting = false,
}: RemoteProjectPlaceholderProps) {
  const attributes = { 'data-remote-project-placeholder': phase, 'data-project-id': projectId };
  if (phase === 'loading')
    return (
      <List fill hAlign="center" vAlign="middle" rootAttributes={attributes}>
        <EmptyState title={`Opening ${name}…`} detail="Connecting to the remote server…" busy />
      </List>
    );
  return (
    <List fill hAlign="center" vAlign="middle" rootAttributes={attributes}>
      <EmptyState
        title={`Reconnecting to ${name}`}
        detail="The remote server is not answering. Hot Sheet keeps trying and opens the project as soon as the server is reachable. Close the tab to stop reopening it."
        busy={connecting}
        icon={<LucideIcon icon={WifiOff} name="wifi-off" />}
        action={
          <Button
            emphasis="primary"
            action={NAVIGATION_AND_TABS_ACTIONS.retryRemoteProject.value}
            disabled={connecting}
            rootAttributes={{ 'data-project-id': projectId }}
          >
            {connecting ? 'Connecting…' : 'Retry now'}
          </Button>
        }
      />
    </List>
  );
}
