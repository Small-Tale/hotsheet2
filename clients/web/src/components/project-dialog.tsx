import './project-dialog.css';

import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Ellipsis } from 'lucide';

import type { Checkout } from '../api';

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
    <wa-dialog data-project-dialog label="Open project" open={open} data-controlled-open={String(open)}>
      <form data-action="open-project-form">
        <List className="project-dialog" gap="m">
          <Text tone="quiet">
            Select a code checkout. Hot Sheet automatically uses a valid sibling <code>&lt;project&gt;.hs2</code> ticket
            store. Override it when the project uses another store.
          </Text>
          <div class="project-dialog__path">
            <wa-input name="project-root" label="Project folder" value={root} required></wa-input>
            <wa-button
              appearance="outlined"
              type="button"
              data-action="browse-project-path"
              aria-label="Browse for project folder"
              title="Browse for project folder"
            >
              <LucideIcon icon={Ellipsis} name="ellipsis" />
            </wa-button>
          </div>
          <div class="project-dialog__path">
            <wa-input
              name="ticket-store"
              label="Ticket store (optional)"
              placeholder="Automatically discover &lt;project&gt;.hs2"
            ></wa-input>
            <wa-button
              appearance="outlined"
              type="button"
              data-action="browse-project-path"
              aria-label="Browse for ticket store"
              title="Browse for ticket store"
            >
              <LucideIcon icon={Ellipsis} name="ellipsis" />
            </wa-button>
          </div>
          <p class="project-dialog__error" role="alert">
            {error}
          </p>
          {recovery && (
            <section class="project-dialog__server-recovery" role="alert">
              <strong>Unresponsive local server</strong>
              <p>
                Hot Sheet cannot verify active work. Recovery first asks process {recovery.expected.pid} to stop, then
                force-stops only that exact registered instance if necessary.
              </p>
              <wa-button
                appearance="outlined"
                type="button"
                data-action="recover-unhealthy-server"
                disabled={recoveryBusy}
              >
                {recoveryBusy ? 'Recovering…' : 'Stop server and retry'}
              </wa-button>
            </section>
          )}
          <footer>
            <Row hAlign="right" vAlign="middle" gap="xs">
              <wa-button appearance="plain" type="button" data-action="cancel-open-project">
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
      <List className="project-dialog remote-project-dialog" gap="m">
        <Text tone="quiet">
          Pick a project that is open on the Hot Sheet server. Browsing the server’s files isn’t available from another
          device.
        </Text>
        {loading ? (
          <p class="project-dialog__loading" role="status">
            Loading projects…
          </p>
        ) : error ? (
          <p class="project-dialog__error" role="alert">
            {error}
          </p>
        ) : checkouts.length === 0 ? (
          <p class="remote-project-dialog__empty">
            No projects are open on the server yet. Open one from the device running Hot Sheet first.
          </p>
        ) : (
          <ul class="remote-project-dialog__list" aria-label="Open projects" role="list">
            {checkouts.map((checkout) => (
              <li>
                <ListItem
                  action="open-remote-checkout"
                  itemId={checkout.id}
                  multiline
                  rootAttributes={{ 'data-checkout-root': checkout.root }}
                  label={
                    <span class="remote-project-dialog__copy">
                      <span class="remote-project-dialog__name">
                        {checkout.alias || checkout.root.split('/').filter(Boolean).pop() || checkout.root}
                      </span>
                      <small class="remote-project-dialog__path">{checkout.root}</small>
                    </span>
                  }
                />
              </li>
            ))}
          </ul>
        )}
        <footer>
          <Row hAlign="right" vAlign="middle" gap="xs">
            <wa-button appearance="plain" type="button" data-action="cancel-remote-project">
              Cancel
            </wa-button>
          </Row>
        </footer>
      </List>
    </wa-dialog>
  );
}
