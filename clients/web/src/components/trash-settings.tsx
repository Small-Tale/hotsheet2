import { Button } from '@kerfjs/ui/button';
import { flex, px } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';

/** An edited, not yet applied retention value for one project (HS2-KPD942, HS2-F55956). */
export interface TrashRetentionDraft {
  /** The raw field value the user typed. */
  value: string;
  /** Trash tickets applying `value` would purge right now, once the server previewed it. */
  purgeCount?: number;
  /** Navigation away was held because this draft is unapplied; ask to apply or revert. */
  leaving?: boolean;
}

/** "3 tickets will be purged" for a positive preview count; nothing otherwise. */
export function trashPurgeHint(count: number | undefined): string {
  if (!count) return '';
  return `${count} ${count === 1 ? 'ticket' : 'tickets'} in Trash will be purged`;
}

/** True when `draft` differs from the applied retention `days`. */
export function trashDraftIsDirty(days: number, draft?: TrashRetentionDraft): boolean {
  return draft !== undefined && draft.value.trim() !== String(days);
}

/**
 * Lifecycle settings: the shared Trash retention window, composed from Kerf layout (HS2-6QE8FG).
 * The field and its "Apply now" action share one line; a shorter window previews how many Trash
 * tickets it would purge immediately (HS2-KPD942). Leaving with an unapplied value asks whether to
 * apply or revert it (HS2-F55956).
 */
export function TrashSettings({
  days,
  draft,
  message = '',
}: {
  days: number;
  draft?: TrashRetentionDraft;
  message?: string;
}) {
  const dirty = trashDraftIsDirty(days, draft),
    hint = dirty ? trashPurgeHint(draft?.purgeCount) : '';
  return (
    <form
      data-component="trash-settings"
      data-dirty={dirty ? 'true' : undefined}
      {...COMMANDS_AND_AI_ACTIONS.saveTrashSettings.attrs}
    >
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Trash retention</Text>
          <Text tone="quiet">
            Deleted tickets remain recoverable until the automatic cleanup removes them. The default is 30 days. Git
            history keeps every purged ticket file.
          </Text>
        </List>
        <List gap="xs">
          {/* The button sits on the field's line, bottom-aligned with the control under its label. */}
          <Row vAlign="bottom" gap="xs" wrap>
            {/* A Kerf List flex item keeps the field short (320px basis) and lets a narrow screen wrap the
                action below; the List stretches the input across it (HS2-J7BTAG). */}
            <List flex={flex(0, 1, px(320))}>
              <TextField
                name="trash-cleanup-days"
                type="number"
                label="Keep Deleted Tickets for (Days)"
                value={draft?.value ?? String(days)}
                required
              />
            </List>
            <Button type="submit" tone={'brand'} disabled={!dirty}>
              Apply Now
            </Button>
          </Row>
          {hint ? (
            <Text size="compact" data-role="trash-purge-hint">
              {hint}
            </Text>
          ) : null}
          <Text variant="span" tone="danger" size="compact" role="status">
            {message}
          </Text>
        </List>
      </List>
      {draft?.leaving && dirty && (
        <div data-key="trash-retention-leave-dialog">
          <DialogSurface
            label="Apply the new Trash retention?"
            size="small"
            rootAttributes={{
              'data-key': 'trash-retention-leave-dialog',
              ...COMMANDS_AND_AI_TARGETS.trashRetentionLeaveDialog.attrs,
              'data-controlled-open': 'true',
            }}
            footer={
              <Row hAlign="right" vAlign="middle" gap="xs" wrap>
                <Button emphasis="secondary" action={COMMANDS_AND_AI_ACTIONS.revertTrashSettings.value}>
                  {`Revert to ${days} ${days === 1 ? 'day' : 'days'}`}
                </Button>
                <Button tone={'brand'} action={COMMANDS_AND_AI_ACTIONS.applyLeavingTrashSettings.value}>
                  Apply
                </Button>
              </Row>
            }
          >
            <List gap="xs">
              <Text flush>
                You changed Trash retention to {draft.value.trim()} {draft.value.trim() === '1' ? 'day' : 'days'} but
                did not apply it.
              </Text>
              {hint ? <Text flush>{hint}</Text> : null}
            </List>
          </DialogSurface>
        </div>
      )}
    </form>
  );
}
