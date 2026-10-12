import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';

import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

/** Lifecycle settings: the shared Trash retention window, composed entirely from Kerf layout (HS2-6QE8FG). */
export function TrashSettings({ days, message = '' }: { days: number; message?: string }) {
  return (
    <form data-component="trash-settings" {...COMMANDS_AND_AI_ACTIONS.saveTrashSettings.attrs}>
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Trash retention</Text>
          <Text tone="quiet">
            Deleted tickets remain recoverable until the automatic cleanup removes them. The default is 30 days. Git
            history keeps every purged ticket file.
          </Text>
        </List>
        {/* A responsive track keeps the number field short on wide screens and full width on a phone. */}
        <Grid minColumnWidth={rem(16)} autoFill>
          <wa-input
            name="trash-cleanup-days"
            type="number"
            label="Keep deleted tickets for (days)"
            value={String(days)}
            required
          />
        </Grid>
        <Row vAlign="middle" gap="xs">
          <wa-button type="submit" variant="brand">
            Save retention
          </wa-button>
          <Text variant="span" tone="quiet" size="compact" role="status">
            {message}
          </Text>
        </Row>
      </List>
    </form>
  );
}
