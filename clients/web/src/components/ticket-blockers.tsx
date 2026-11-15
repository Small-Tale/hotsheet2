import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Ban, CircleCheck, CircleHelp, Link, X } from 'lucide';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import type { BlockerAvailability, BlockerRow } from '../ticket-blockers';

export interface TicketBlockersProps {
  /** The inspected ticket's number, for the rows' accessible names. */
  slug: string;
  rows: readonly BlockerRow[];
  availability: BlockerAvailability;
  /** Native ids whose removal is in flight; their rows show busy and cannot be removed again. */
  removing?: readonly string[];
}

/**
 * The Info tab's Blocked By section (HS2-CHH28E): the tickets that must finish before the
 * inspected one can. The header's logical-end Add Blocker action opens the blocker dialog; a
 * source without the `dependencies` capability keeps it mounted but disabled with its reason.
 * Each row opens its blocker, and its trailing button removes that dependency.
 */
export function TicketBlockers({ slug, rows, availability, removing = [] }: TicketBlockersProps) {
  return (
    <section class="ticket-blockers" data-component="ticket-blockers" aria-label="Blocked By">
      <List gap="2xs" controlInsets="b">
        <ListHeader
          label="Blocked By"
          {...blockerCount(rows.length)}
          action={INSPECTOR_AND_EDITOR_ACTIONS.openBlockerDialog.value}
          actionLabel="Add Blocker…"
          actionIcon={<LucideIcon icon={Link} name="link" />}
          actionDisabled={!availability.enabled}
          disabledReason={availability.disabledReason}
        />
        {rows.map((row) => {
          const name = row.slug ?? row.nativeId,
            busy = removing.includes(row.nativeId);
          return (
            <ListActionRow
              action={TICKET_SELECTION_ACTIONS.openDuplicateTarget.value}
              itemId={row.reference}
              icon={
                row.state === 'unavailable' ? (
                  <LucideIcon icon={CircleHelp} name="circle-help" />
                ) : row.state === 'done' ? (
                  <LucideIcon icon={CircleCheck} name="circle-check" />
                ) : (
                  <LucideIcon icon={Ban} name="ban" />
                )
              }
              label={name}
              description={row.state === 'unavailable' ? 'Ticket unavailable' : (row.title ?? '')}
              status={row.state === 'done' ? 'Done' : undefined}
              accessibleLabel={
                row.state === 'unavailable'
                  ? `${name}: ticket unavailable`
                  : `Open blocker ${name}: ${row.title ?? ''}${row.state === 'done' ? ' (done)' : ''}`
              }
              disabled={row.state === 'unavailable'}
              busy={busy}
              title={row.title}
              rootAttributes={{ 'data-blocker-id': row.nativeId, 'data-blocker-state': row.state }}
              trailingAction={INSPECTOR_AND_EDITOR_ACTIONS.removeBlocker.value}
              trailingActionLabel={`Remove ${name} as a blocker of ${slug}`}
              trailingActionIcon={<LucideIcon icon={X} name="x" />}
              trailingActionDisabled={!availability.enabled || busy}
              trailingActionTitle={availability.enabled ? 'Remove blocker' : availability.disabledReason}
              trailingActionAttributes={{ 'data-blocker-id': row.nativeId }}
            />
          );
        })}
      </List>
    </section>
  );
}

/** The header's count; an empty section shows only its label and action. */
function blockerCount(count: number): { count: number; countLabel: string } | Record<string, never> {
  return count > 0 ? { count, countLabel: `${count} ${count === 1 ? 'blocker' : 'blockers'}` } : {};
}
