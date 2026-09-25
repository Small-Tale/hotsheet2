import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './trash-settings.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

export function TrashSettings({ days, message = '' }: { days: number; message?: string }) {
  return (
    <form data-component="trash-settings" data-action="save-trash-settings">
      <List className="trash-settings" gap="l">
        <header>
          <List gap="xs">
            <Text variant="h2">Trash retention</Text>
            <Text tone="quiet">
              Deleted tickets remain recoverable until the automatic cleanup removes them. The default is 30 days. Git
              history keeps every purged ticket file.
            </Text>
          </List>
        </header>
        <wa-input
          name="trash-cleanup-days"
          type="number"
          label="Keep deleted tickets for (days)"
          value={String(days)}
          required
        ></wa-input>
        <footer>
          <Row vAlign="middle" gap="xs">
            <wa-button type="submit" variant="brand">
              Save retention
            </wa-button>
            <span role="status">{message}</span>
          </Row>
        </footer>
      </List>
    </form>
  );
}
