import { Button } from '@kerfjs/ui/button';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Ban, Clock, WifiOff } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';

export interface RemoteProjectPlaceholderProps {
  /** The remote project id the placeholder stands for. */
  projectId: string;
  name: string;
  /** `loading`: the first open is still running (HS2-EJ5K09); `reconnecting`: the host did not answer
   * (HS2-WNK73S); `expired` or `revoked`: the host refused this computer's approval (HS2-4B0KSD). */
  phase: 'loading' | 'reconnecting' | 'expired' | 'revoked';
  /** A retry of the host is in flight. */
  connecting?: boolean;
  /** The remote server binding the tab belongs to; the renewal and review actions name it. */
  bindingId?: string;
  /** Why the last attempt failed, when it was not simply unanswered. */
  error?: string;
  /** This computer already asked the host to renew its approval. */
  renewalRequested?: boolean;
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
  bindingId = '',
  error,
  renewalRequested = false,
}: RemoteProjectPlaceholderProps) {
  const attributes = { 'data-remote-project-placeholder': phase, 'data-project-id': projectId };
  if (phase === 'loading')
    return (
      <List fill hAlign="center" vAlign="middle" rootAttributes={attributes}>
        <EmptyState title={`Opening ${name}…`} detail="Connecting to the remote server…" busy />
      </List>
    );
  const checkAgain = (
    <Button
      emphasis={renewalRequested ? 'primary' : 'secondary'}
      action={NAVIGATION_AND_TABS_ACTIONS.retryRemoteProject.value}
      disabled={connecting}
      rootAttributes={{ 'data-project-id': projectId }}
    >
      {connecting ? 'Checking…' : 'Check Again'}
    </Button>
  );
  // HS2-4B0KSD: an expired approval is renewed, never retried in a loop; the binding and its projects stay.
  if (phase === 'expired')
    return (
      <List fill hAlign="center" vAlign="middle" rootAttributes={attributes}>
        <EmptyState
          title={`Renew Access to ${name}`}
          detail={
            renewalRequested
              ? 'Renewal requested. Approve it on the remote host in Settings → Devices, or run hotsheet-cli remote sessions renew there, then choose Check Again.'
              : 'This computer’s approval on the remote server expired. Renew asks its host to approve this computer again; the connection and its projects stay as they are.'
          }
          busy={connecting}
          icon={<LucideIcon icon={Clock} name="clock" />}
          action={
            <>
              {renewalRequested ? undefined : (
                <Button
                  emphasis="primary"
                  action={NAVIGATION_AND_TABS_ACTIONS.renewRemoteApproval.value}
                  disabled={connecting}
                  rootAttributes={{ 'data-remote-scope': bindingId }}
                >
                  Renew
                </Button>
              )}
              {checkAgain}
            </>
          }
        />
      </List>
    );
  if (phase === 'revoked')
    return (
      <List fill hAlign="center" vAlign="middle" rootAttributes={attributes}>
        <EmptyState
          title={`Access Revoked by ${name}`}
          detail="The remote host removed this computer’s approval. Pair it again to restore access, or close the tab."
          icon={<LucideIcon icon={Ban} name="ban" />}
          action={
            <>
              <Button
                emphasis="primary"
                action={NAVIGATION_AND_TABS_ACTIONS.reviewRemoteServer.value}
                rootAttributes={{ 'data-remote-scope': bindingId }}
              >
                Pair Again
              </Button>
              {checkAgain}
            </>
          }
        />
      </List>
    );
  return (
    <List fill hAlign="center" vAlign="middle" rootAttributes={attributes}>
      <EmptyState
        title={`Reconnecting to ${name}`}
        detail={`${error && error !== 'The server did not answer.' ? error : 'The remote server is not answering.'} Hot Sheet keeps trying and opens the project as soon as the server is reachable. Close the tab to stop reopening it.`}
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
