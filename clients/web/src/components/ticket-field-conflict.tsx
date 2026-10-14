import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/textarea/textarea.js';
import './ticket-field-conflict.css';

import { ContentItem } from '@kerfjs/ui/content-item';
import { rem, uiColor } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { GitMerge, RotateCcw } from 'lucide';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import type { TicketFieldConflict as TicketFieldConflictState } from '../ticket-field-reconciliation';

export interface TicketFieldConflictProps {
  conflict: TicketFieldConflictState;
  resolution: string;
}

function Version({ label, value }: { label: string; value: string }) {
  return (
    <label class="ticket-field-conflict__version">
      <Text variant="span" size="compact" tone="quiet">
        <strong>{label}</strong>
      </Text>
      <output>{value || 'Empty'}</output>
    </label>
  );
}

/** Concurrent-edit resolution card: both versions, an editable merge, and the two resolutions. */
export function TicketFieldConflict({ conflict, resolution }: TicketFieldConflictProps) {
  const field = conflict.label.toLocaleLowerCase();
  return (
    <section
      class="ticket-field-conflict"
      data-component="ticket-field-conflict"
      data-conflict-field={conflict.key}
      aria-label={`Resolve ${field} Conflict`}
    >
      <ContentItem appearance="warning" frame="framed">
        <List gap="xs">
          <Row gap="xs" vAlign="top">
            <LucideIcon icon={GitMerge} name="git-merge" size="s" color={uiColor('warning-on-quiet')} />
            <List gap="none">
              <Text variant="span" lineHeight="tight">
                <strong>Resolve {field} conflict</strong>
              </Text>
              <Text variant="span" size="compact" tone="quiet">
                This field changed elsewhere while you were editing it.
              </Text>
            </List>
          </Row>
          <Grid minColumnWidth={rem(10)} gap="xs" rootAttributes={{ 'data-versions': 'true' }}>
            <Version label="Their latest version" value={conflict.theirs} />
            <Version label="Your version" value={conflict.mine} />
          </Grid>
          <wa-textarea
            class="ticket-field-conflict__merge"
            name="ticket-conflict-resolution"
            label={`Merged ${field}`}
            size="small"
            resize="auto"
            value={resolution}
          />
          <Row hAlign="right" gap="xs" wrap>
            <wa-button
              size="small"
              appearance="outlined"
              type="button"
              {...INSPECTOR_AND_EDITOR_ACTIONS.acceptRemoteTicketField.attrs}
            >
              <LucideIcon slot="start" size="s" icon={RotateCcw} name="rotate-ccw" />
              Use Theirs
            </wa-button>
            <wa-button
              size="small"
              variant="brand"
              type="button"
              {...INSPECTOR_AND_EDITOR_ACTIONS.applyTicketFieldMerge.attrs}
            >
              <LucideIcon slot="start" size="s" icon={GitMerge} name="git-merge" />
              Apply Merged Value
            </wa-button>
          </Row>
        </List>
      </ContentItem>
    </section>
  );
}
