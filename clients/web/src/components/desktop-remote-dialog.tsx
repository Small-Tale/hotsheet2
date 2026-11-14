import './desktop-remote-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import type { DesktopRemoteDiscovery, DesktopRemotePairing, DesktopRemoteServer } from '../remote-projects/client';

export function DesktopRemoteDialog({
  open,
  servers,
  formOpen,
  editing,
  discovery,
  pairing,
  busy,
  error,
}: {
  open: boolean;
  servers: DesktopRemoteServer[];
  formOpen: boolean;
  editing?: DesktopRemoteServer;
  discovery?: DesktopRemoteDiscovery;
  pairing?: DesktopRemotePairing;
  busy: boolean;
  error: string;
}) {
  return (
    <div class="desktop-remote-dialog">
      <DialogSurface
        label="Remote project"
        height={remify(700)}
        maxHeight={remify(700)}
        viewportGutter={remify(16)}
        bodyLayout="fill"
        rootAttributes={{
          'data-desktop-remote-dialog': '',
          'data-controlled-open': String(open),
        }}
      >
        <div class="desktop-remote-dialog__content">
          <List gap="xs" flex>
            {formOpen ? (
              <Text tone="quiet">Connect to another computer. Its host must approve this device once.</Text>
            ) : undefined}
            {formOpen ? (
              <form {...PROJECT_LIFECYCLE_ACTIONS.beginDesktopRemotePair.attrs}>
                <List gap="xs">
                  <TextField
                    name="remote-address"
                    label="Server Address"
                    placeholder="https://host.example.com"
                    value={editing?.origin ?? ''}
                    required
                  />
                  <TextField name="remote-label" label="This Device's Name" value={editing?.label ?? ''} required />
                  <details>
                    <summary>Advanced TLS settings</summary>
                    <List gap="xs">
                      <TextField name="remote-ca-file" label="Host CA PEM Path (Optional)" />
                      <TextField name="remote-cert-file" label="Client Certificate PEM Path (Optional)" />
                      <TextField name="remote-key-file" label="Client Key PEM Path (Optional)" />
                    </List>
                  </details>
                  <Button type="submit" emphasis="primary" disabled={busy}>
                    {editing ? 'Review Connection' : 'Request Access'}
                  </Button>
                </List>
              </form>
            ) : undefined}
            {formOpen && pairing && (
              <div role="status" data-remote-pairing-id={pairing.approvalId}>
                <Text>
                  On the host computer, approve request {pairing.approvalId} for {pairing.label}. Server ID:{' '}
                  {pairing.serverId}.
                </Text>
                <Text tone="quiet">
                  Run `hotsheet-cli remote sessions approve {pairing.approvalId} --role read_write` there.
                </Text>
                <Button action={PROJECT_LIFECYCLE_ACTIONS.finishDesktopRemotePair.value} disabled={busy}>
                  I Approved It — Connect
                </Button>
              </div>
            )}
            {error ? (
              <Text tone="danger" role="alert">
                {error}
              </Text>
            ) : undefined}
            {formOpen ? (
              <Button emphasis="plain" action={PROJECT_LIFECYCLE_ACTIONS.cancelDesktopRemoteForm.value}>
                Back to Servers
              </Button>
            ) : undefined}
            {!formOpen ? (
              <Button emphasis="secondary" action={PROJECT_LIFECYCLE_ACTIONS.showDesktopRemoteForm.value}>
                Add Server
              </Button>
            ) : undefined}
            {!formOpen && servers.length > 0 && (
              <List gap="xs">
                <Text>Approved servers</Text>
                {servers
                  .filter((server) => !server.revoked)
                  .map((server) => (
                    <div
                      class="desktop-remote-dialog__server"
                      data-remote-id={server.id}
                      data-revision={server.revision}
                    >
                      <div class="desktop-remote-dialog__server-info">
                        <Button action={PROJECT_LIFECYCLE_ACTIONS.discoverDesktopRemoteServer.value} disabled={busy}>
                          {server.last_failure ? `Retry ${server.label}` : server.label}
                        </Button>
                        <Text tone="quiet" variant="span" wrap="anywhere">
                          {server.origin}
                        </Text>
                      </div>
                      <Button
                        emphasis="plain"
                        action={PROJECT_LIFECYCLE_ACTIONS.editDesktopRemoteServer.value}
                        disabled={busy}
                        ariaLabel={`Edit ${server.label}`}
                      >
                        Edit
                      </Button>
                      <Button
                        emphasis="plain"
                        action={PROJECT_LIFECYCLE_ACTIONS.removeDesktopRemoteServer.value}
                        disabled={busy}
                        ariaLabel={`Remove ${server.label}`}
                      >
                        Remove
                      </Button>
                    </div>
                  ))}
              </List>
            )}
            {!formOpen && discovery && (
              <List gap="xs">
                <Text>
                  {discovery.remote.label} · {discovery.remote.origin} ·{' '}
                  {discovery.remote.origin.startsWith('https:') ? 'HTTPS' : 'HTTP'}
                </Text>
                {discovery.checkouts?.length ? (
                  discovery.checkouts.map((checkout) => (
                    <Button
                      rootAttributes={{ 'data-checkout-id': checkout.id }}
                      action={PROJECT_LIFECYCLE_ACTIONS.openDesktopRemoteCheckout.value}
                      disabled={busy || checkout.state !== 'available'}
                    >
                      {checkout.name}
                    </Button>
                  ))
                ) : (
                  <Text tone="quiet">No available projects are open on this server.</Text>
                )}
              </List>
            )}
            <Button emphasis="plain" action={PROJECT_LIFECYCLE_ACTIONS.cancelDesktopRemoteDialog.value}>
              Close
            </Button>
          </List>
        </div>
      </DialogSurface>
    </div>
  );
}
