import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { Disclosure } from '@kerfjs/ui/disclosure';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { MoreHorizontal, Pencil, X } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import type {
  DesktopRemoteCheckout,
  DesktopRemoteDiscovery,
  DesktopRemotePairing,
  DesktopRemoteServer,
} from '../remote-projects/client';

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;

/** Quiet status for a remote project that cannot be opened (HS2-A5CKQS). */
function checkoutStatus(state: DesktopRemoteCheckout['state']): string | undefined {
  if (state === 'inaccessible') return 'Unavailable';
  if (state === 'identity_changed') return 'Identity Changed';
  return undefined;
}

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
        rootAttributes={{
          'data-desktop-remote-dialog': '',
          'data-controlled-open': String(open),
        }}
      >
        {/* Kerf's native dialog body scrolls the form and keeps every focus ring inside its inset; an
            app-owned scroller here cut the rings at both inline edges (HS2-MV0BA4). */}
        <List gap="xs">
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
                <Disclosure title="Advanced TLS Settings" rootAttributes={{ 'data-remote-tls-settings': '' }}>
                  <List gap="xs">
                    <TextField name="remote-ca-file" label="Host CA PEM Path (Optional)" />
                    <TextField name="remote-cert-file" label="Client Certificate PEM Path (Optional)" />
                    <TextField name="remote-key-file" label="Client Key PEM Path (Optional)" />
                  </List>
                </Disclosure>
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
          {/* Servers and their projects are list rows, not solid buttons, so the two levels read as
              different things: a server row opens discovery and keeps Edit and Remove behind its
              overflow menu (HS2-A5CKQS, maintainer choice "Row overflow menu"). The rows stay
              single-line, so a long origin ellipsizes, until KF-89C223 keeps a multiline row's
              description on its own line. */}
          {!formOpen && servers.length > 0 && (
            <List gap="2xs" rootAttributes={{ 'data-remote-servers': '' }}>
              <ListHeader label="Approved Servers" />
              {servers
                .filter((server) => !server.revoked)
                .map((server) => {
                  const attributes = { 'data-remote-id': server.id, 'data-revision': String(server.revision) };
                  return (
                    <ListActionRow
                      label={server.label}
                      description={server.origin}
                      status={server.last_failure ? 'Retry' : undefined}
                      accessibleLabel={server.last_failure ? `Retry ${server.label}` : server.label}
                      action={PROJECT_LIFECYCLE_ACTIONS.discoverDesktopRemoteServer.value}
                      selected={discovery?.remote.id === server.id}
                      disabled={busy}
                      rootAttributes={attributes}
                      trailingMenu={{
                        label: `Actions for ${server.label}`,
                        icon: menuIcon,
                        disabled: busy,
                        items: [
                          {
                            label: 'Edit',
                            action: PROJECT_LIFECYCLE_ACTIONS.editDesktopRemoteServer.value,
                            icon: <LucideIcon icon={Pencil} name="pencil" />,
                            attributes,
                          },
                          {
                            label: 'Remove',
                            action: PROJECT_LIFECYCLE_ACTIONS.removeDesktopRemoteServer.value,
                            tone: 'danger' as const,
                            icon: <LucideIcon icon={X} name="x" />,
                            attributes,
                          },
                        ],
                      }}
                    />
                  );
                })}
            </List>
          )}
          {!formOpen && discovery && (
            <List gap="2xs" rootAttributes={{ 'data-remote-checkouts': '' }}>
              <ListHeader label={`Projects on ${discovery.remote.label}`} wrap />
              {discovery.checkouts?.length ? (
                discovery.checkouts.map((checkout) => (
                  <ListItem
                    label={checkout.name}
                    status={checkoutStatus(checkout.state)}
                    action={PROJECT_LIFECYCLE_ACTIONS.openDesktopRemoteCheckout.value}
                    disabled={busy || checkout.state !== 'available'}
                    rootAttributes={{ 'data-checkout-id': checkout.id }}
                  />
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
      </DialogSurface>
    </div>
  );
}
