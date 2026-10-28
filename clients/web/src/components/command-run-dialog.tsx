import './command-run-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { Square } from 'lucide';

import type { CommandDefinition, CommandRun } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { StatusBadge, type TicketStatus } from './status-badge';

export function CommandRunDialog({
  command,
  run,
  confirmStop = false,
  launches = [],
  launchTickets = {},
}: {
  command?: CommandDefinition;
  run?: CommandRun;
  confirmStop?: boolean;
  /** Shell and AI commands' recorded launches, newest first (HS2-PWNPM2). */
  launches?: readonly CommandRun[];
  /** Loaded tickets an AI launch queued, by slug, for their title and current status. */
  launchTickets?: Readonly<Record<string, { title: string; status: TicketStatus }>>;
}) {
  if (!command) return null;
  // Both presentations are Web Awesome dialogs inside Kerf DialogSurface; each keeps its own key so a
  // morph between them never hands one dialog's host to the other (HS2-Z7C27E, HS2-EKJRMJ). The
  // output dialog is dismissed through Web Awesome's own header close (or Escape), which fires the
  // wa-hide that clears the app's dialog state, so it carries no duplicate footer Close.
  if (confirmStop)
    return (
      <div class="command-run-dialog" data-key="command-cancellation-dialog">
        <DialogSurface
          label={`Stop ${command.title}?`}
          size="small"
          rootAttributes={{
            'data-key': 'command-cancellation-dialog',
            'data-dialog-component': 'command-cancellation-dialog',
            'data-controlled-open': 'true',
          }}
          footer={
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <Button emphasis="secondary" action={COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.value}>
                Keep Running
              </Button>
              <Button
                tone={'danger'}
                action={COMMANDS_AND_AI_ACTIONS.confirmStopCommand.value}
                rootAttributes={{ 'data-run-id': run?.id }}
                icon={<LucideIcon icon={Square} name="square" size={13.6} />}
              >
                Stop Command
              </Button>
            </Row>
          }
        >
          <Text flush>The command is still running. Stop it now?</Text>
        </DialogSurface>
      </div>
    );
  const attributes = {
    'data-key': 'command-run-dialog',
    'data-dialog-component': 'command-run-dialog',
    'data-controlled-open': 'true',
  };
  // A shell command runs in a terminal tab and an AI command queues a ticket, so their history lists
  // each recorded launch instead of captured output (HS2-PWNPM2).
  if (command.kind === 'shell' || command.kind === 'ai')
    return (
      <div class="command-run-dialog" data-key="command-run-dialog">
        <DialogSurface label={command.title} size="small" bodyInset="none" rootAttributes={attributes}>
          {/* Kerf ListInsetText gives the launch rows content-item geometry on the title's text edge. */}
          <ListInsetText>
            {launches.length ? (
              <List gap="xs" rootAttributes={{ role: 'list', 'aria-label': 'Recent launches' }}>
                {launches.map((item) => {
                  const slug = item.launch?.ticket_slug,
                    ticket = slug ? launchTickets[slug] : undefined;
                  return (
                    <div role="listitem" data-command-launch={item.id}>
                      <Row hAlign="full" vAlign="middle" gap="xs" wrap>
                        <Row gap="xs" vAlign="middle" wrap>
                          {slug ? (
                            <>
                              <Text variant="span" size="compact">
                                <a
                                  href={`#ticket-${slug}`}
                                  {...NOTIFICATIONS_AND_LINKS_ACTIONS.openLinkedTicket.attrs}
                                  data-ticket-slug={slug}
                                  title={`Open ${slug}`}
                                >
                                  {slug}
                                </a>
                              </Text>
                              {ticket ? (
                                <>
                                  <Text variant="span" size="compact">
                                    {ticket.title}
                                  </Text>
                                  <StatusBadge status={ticket.status} appearance="plain" compact />
                                </>
                              ) : undefined}
                            </>
                          ) : (
                            <Text variant="span" size="compact">
                              {item.launch?.kind === 'ai'
                                ? 'Queued a ticket'
                                : `Opened in terminal ${item.launch?.terminal_label ?? command.title}`}
                            </Text>
                          )}
                        </Row>
                        {item.started_at ? (
                          <Text variant="span" size="compact" tone="quiet">
                            <time dateTime={item.started_at}>{formatLaunchTime(item.started_at)}</time>
                          </Text>
                        ) : undefined}
                      </Row>
                    </div>
                  );
                })}
              </List>
            ) : (
              <Text flush tone="quiet">
                Never run
              </Text>
            )}
          </ListInsetText>
        </DialogSurface>
      </div>
    );
  return (
    <div class="command-run-dialog" data-key="command-run-dialog">
      <DialogSurface label={command.title} size="small" rootAttributes={attributes}>
        <List gap="m">
          <Row hAlign="full" vAlign="middle" gap="xs">
            <strong>{run ? run.state : 'Never run'}</strong>
            {run?.exit_code !== undefined && <span>Exit {run.exit_code}</span>}
          </Row>
          <pre class="command-run-dialog__output" aria-label="Command output">
            {run?.output.length
              ? run.output.map((line) => `${line.stream === 'stderr' ? 'error: ' : ''}${line.text}`).join('\n')
              : 'No output recorded.'}
          </pre>
        </List>
      </DialogSurface>
    </div>
  );
}

function formatLaunchTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(date);
}
