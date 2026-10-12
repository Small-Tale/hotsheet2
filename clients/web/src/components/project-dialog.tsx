import './project-dialog.css';

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
}: {
  open: boolean;
  root: string;
  error: string;
  recovery?: UnhealthyServerRecoveryView;
  recoveryBusy?: boolean;
}) {
  return (
    <DialogSurface preferredWidth={remify(768)} viewportGutter={remify(16)}>
      <wa-dialog
        data-key="project-dialog"
        data-project-dialog
        label="Open project"
        open={open}
        data-controlled-open={String(open)}
      >
        <form {...REPOSITORY_ACTIONS.openProjectForm.attrs}>
          <List gap="m">
            <Text tone="quiet">
              Select a code checkout. Hot Sheet automatically uses a valid sibling <code>&lt;project&gt;.hs2</code>{' '}
              ticket store. Override it when the project uses another store.
            </Text>
            <div class="project-dialog__path">
              <wa-input name="project-root" label="Project folder" value={root} required />
              <wa-button
                appearance="outlined"
                type="button"
                {...PROJECT_LIFECYCLE_ACTIONS.browseProjectPath.attrs}
                aria-label="Browse for project folder"
                title="Browse for project folder"
              >
                <LucideIcon size="s" icon={Ellipsis} name="ellipsis" />
              </wa-button>
            </div>
            <div class="project-dialog__path">
              <wa-input
                name="ticket-store"
                label="Ticket store (optional)"
                placeholder="Automatically discover &lt;project&gt;.hs2"
              />
              <wa-button
                appearance="outlined"
                type="button"
                {...PROJECT_LIFECYCLE_ACTIONS.browseProjectPath.attrs}
                aria-label="Browse for ticket store"
                title="Browse for ticket store"
              >
                <LucideIcon size="s" icon={Ellipsis} name="ellipsis" />
              </wa-button>
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
                title="Unresponsive local server"
                detail={`Hot Sheet cannot verify active work. Recovery first asks process ${recovery.expected.pid} to stop, then force-stops only that exact registered instance if necessary.`}
                action={
                  <wa-button
                    appearance="outlined"
                    type="button"
                    {...PROJECT_LIFECYCLE_ACTIONS.recoverUnhealthyServer.attrs}
                    disabled={recoveryBusy}
                  >
                    {recoveryBusy ? 'Recovering…' : 'Stop server and retry'}
                  </wa-button>
                }
              />
            )}
            <footer>
              <Row hAlign="right" vAlign="middle" gap="xs">
                <wa-button appearance="plain" type="button" {...PROJECT_LIFECYCLE_ACTIONS.cancelOpenProject.attrs}>
                  Cancel
                </wa-button>
                <wa-button appearance="accent" type="submit">
                  Open project
                </wa-button>
              </Row>
            </footer>
          </List>
        </form>
      </wa-dialog>
    </DialogSurface>
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
    <wa-dialog data-remote-project-dialog label="Open a project" open={open} data-controlled-open={String(open)}>
      <div class="remote-project-dialog">
        <List gap="m">
          <Text tone="quiet">
            Pick a project that is open on the Hot Sheet server. Browsing the server’s files isn’t available from
            another device.
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
            <nav class="remote-project-dialog__list" aria-label="Open projects">
              <List gap="2xs">
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
            </nav>
          )}
          <footer>
            <Row hAlign="right" vAlign="middle" gap="xs">
              <wa-button appearance="plain" type="button" {...PROJECT_LIFECYCLE_ACTIONS.cancelRemoteProject.attrs}>
                Cancel
              </wa-button>
            </Row>
          </footer>
        </List>
      </div>
    </wa-dialog>
  );
}
