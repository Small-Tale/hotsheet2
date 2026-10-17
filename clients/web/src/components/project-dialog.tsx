import './project-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { Ellipsis } from 'lucide';

import type { Checkout } from '../api';
import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';

export interface UnhealthyServerRecoveryView {
  expected: { pid: number };
}

export function projectDialogRoot(project?: { root: string }) {
  return project?.root ?? '.';
}

export function ProjectDialog({
  open,
  root,
  error,
  recovery,
  recoveryBusy = false,
  hostActionsAvailable = hostActionsAvailableHere(),
}: {
  open: boolean;
  root: string;
  error: string;
  recovery?: UnhealthyServerRecoveryView;
  recoveryBusy?: boolean;
  /** False on a remote client: host-only controls render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}) {
  // Keyed so the render morph never repurposes this dialog for a sibling DialogSurface: opening a project
  // waits for this exact element's wa-after-hide before presenting ticket setup or HS1 import.
  return (
    <div class="project-dialog" data-key="project-dialog">
      <DialogSurface
        label="Open Project"
        preferredWidth={remify(768)}
        viewportGutter={remify(16)}
        rootAttributes={{ 'data-project-dialog': '', 'data-controlled-open': String(open) }}
      >
        <form {...REPOSITORY_ACTIONS.openProjectForm.attrs}>
          <List gap="m">
            <Text tone="quiet">
              Select a code checkout. Hot Sheet automatically uses a valid sibling <code>&lt;project&gt;.hs2</code>{' '}
              ticket store. Override it when the project uses another store.
            </Text>
            <div class="project-dialog__path">
              <wa-input name="project-root" label="Project Folder" value={root} required />
              <Button
                emphasis="secondary"
                type="button"
                action={PROJECT_LIFECYCLE_ACTIONS.browseProjectPath.value}
                ariaLabel={'Browse for Project Folder'}
                title={hostOnlyDisabledReason('choose-folder', hostActionsAvailable) ?? 'Browse for project folder'}
                disabled={!hostActionsAvailable}
                icon={<LucideIcon size="s" icon={Ellipsis} name="ellipsis" />}
              />
            </div>
            <div class="project-dialog__path">
              <wa-input
                name="ticket-store"
                label="Ticket Store (Optional)"
                placeholder="Automatically discover &lt;project&gt;.hs2"
              />
              <Button
                emphasis="secondary"
                type="button"
                action={PROJECT_LIFECYCLE_ACTIONS.browseProjectPath.value}
                ariaLabel={'Browse for Ticket Store'}
                title={hostOnlyDisabledReason('choose-folder', hostActionsAvailable) ?? 'Browse for ticket store'}
                disabled={!hostActionsAvailable}
                icon={<LucideIcon size="s" icon={Ellipsis} name="ellipsis" />}
              />
            </div>
            <div class="project-dialog__error" role="alert">
              {error && (
                <Text flush tone="danger">
                  {error}
                </Text>
              )}
            </div>
            {recovery && (
              <StateBanner
                className="project-dialog__server-recovery"
                tone="warning"
                urgency="alert"
                copyLayout="stacked"
                actionPlacement="below"
                title="Unresponsive Local Server"
                detail={`Hot Sheet cannot verify active work. Recovery first asks process ${recovery.expected.pid} to stop, then force-stops only that exact registered instance if necessary.`}
                action={
                  <Button
                    emphasis="secondary"
                    type="button"
                    action={PROJECT_LIFECYCLE_ACTIONS.recoverUnhealthyServer.value}
                    disabled={recoveryBusy}
                  >
                    {recoveryBusy ? 'Recovering…' : 'Stop server and retry'}
                  </Button>
                }
              />
            )}
            <footer>
              <Row hAlign="right" vAlign="middle" gap="xs">
                <Button emphasis="plain" type="button" action={PROJECT_LIFECYCLE_ACTIONS.cancelOpenProject.value}>
                  Cancel
                </Button>
                <Button emphasis="primary" type="submit">
                  Open Project
                </Button>
              </Row>
            </footer>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}

export function RemoteProjectDialog({
  open,
  checkouts,
  loading = false,
  error = '',
}: {
  open: boolean;
  checkouts: Checkout[];
  loading?: boolean;
  error?: string;
}) {
  return (
    // Kerf bounds the dialog height; the checkout List flexes inside that bound and owns its scrolling
    // (KF-93GZC1), so the app keeps no viewport-size CSS (HS2-W4RNKJ).
    <DialogSurface
      label="Open a project"
      maxHeight={remify(560)}
      viewportGutter={remify(16)}
      bodyLayout="fill"
      rootAttributes={{
        'data-key': 'remote-project-dialog',
        'data-remote-project-dialog': '',
        'data-controlled-open': String(open),
      }}
    >
      <List gap="m" flex className="remote-project-dialog">
        <Text tone="quiet">
          Pick a project that is open on the Hot Sheet server. Browsing the server’s files isn’t available from another
          device.
        </Text>
        {loading ? (
          <Text flush role="status">
            Loading projects…
          </Text>
        ) : error ? (
          <div class="project-dialog__error" role="alert">
            <Text flush tone="danger">
              {error}
            </Text>
          </div>
        ) : checkouts.length === 0 ? (
          <Text flush tone="quiet">
            No projects are open on the server yet. Open one from the device running Hot Sheet first.
          </Text>
        ) : (
          <List
            gap="2xs"
            flex
            scrollable
            controlInsets="tb"
            className="remote-project-dialog__list"
            rootAttributes={{ role: 'navigation', 'aria-label': 'Open Projects' }}
          >
            {checkouts.map((checkout) => (
              <ListItem
                action="open-remote-checkout"
                itemId={checkout.id}
                multiline
                rootAttributes={{ 'data-checkout-root': checkout.root }}
                label={
                  <Text variant="span" wrap="anywhere">
                    {checkout.alias || checkout.root.split('/').filter(Boolean).pop() || checkout.root}
                  </Text>
                }
                description={
                  <Text variant="span" wrap="anywhere" className="remote-project-dialog__path">
                    {checkout.root}
                  </Text>
                }
              />
            ))}
          </List>
        )}
        <footer>
          <Row hAlign="right" vAlign="middle" gap="xs">
            <Button emphasis="plain" type="button" action={PROJECT_LIFECYCLE_ACTIONS.cancelRemoteProject.value}>
              Cancel
            </Button>
          </Row>
        </footer>
      </List>
    </DialogSurface>
  );
}
