import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './trash-settings.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

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
      class="trash-settings"
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
            <wa-input
              class="trash-settings__days"
              name="trash-cleanup-days"
              type="number"
              label="Keep deleted tickets for (days)"
              value={draft?.value ?? String(days)}
              required
            />
            <wa-button type="submit" variant="brand" disabled={!dirty}>
              Apply now
            </wa-button>
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
          <DialogSurface size="small">
            <wa-dialog
              data-key="trash-retention-leave-dialog"
              open
              label="Apply the new Trash retention?"
              {...COMMANDS_AND_AI_TARGETS.trashRetentionLeaveDialog.attrs}
            >
              <List gap="xs">
                <Text flush>
                  You changed Trash retention to {draft.value.trim()} {draft.value.trim() === '1' ? 'day' : 'days'} but
                  did not apply it.
                </Text>
                {hint ? <Text flush>{hint}</Text> : null}
              </List>
              <Row slot="footer" hAlign="right" vAlign="middle" gap="xs" wrap>
                <wa-button appearance="outlined" {...COMMANDS_AND_AI_ACTIONS.revertTrashSettings.attrs}>
                  Revert to {days} {days === 1 ? 'day' : 'days'}
                </wa-button>
                <wa-button variant="brand" {...COMMANDS_AND_AI_ACTIONS.applyLeavingTrashSettings.attrs}>
                  Apply
                </wa-button>
              </Row>
            </wa-dialog>
          </DialogSurface>
        </div>
      )}
    </form>
  );
}
