import './ticket-source-color-picker.css';

import { Grid } from '@kerfjs/ui/grid';
import { FieldLabel, Text } from '@kerfjs/ui/text';

import { resolveTicketSourceColor, TICKET_SOURCE_COLORS } from './customization-palette';
import { TicketSourceIcon, type TicketSourceIdentity } from './ticket-source-icon';

/** Visual, keyboard-accessible palette for one project's ticket source. */
export function TicketSourceColorPicker({
  source,
  sourceId,
}: {
  source: TicketSourceIdentity;
  /** Present for the Git appearance editor, which saves each selection immediately. */
  sourceId?: string;
}) {
  const selected = resolveTicketSourceColor(source.color),
    labelId = `ticket-source-color-picker-${sourceId ?? 'connection'}-label`;
  return (
    <div
      class="ticket-source-color-picker"
      data-component="ticket-source-color-picker"
      role="radiogroup"
      aria-labelledby={labelId}
    >
      <FieldLabel id={labelId}>Icon Color</FieldLabel>
      <Grid columns={4} gap="xs">
        {TICKET_SOURCE_COLORS.map((option) => (
          <label class="ticket-source-color-picker__choice" style={`--ticket-source-choice-color: ${option.value}`}>
            <input
              type="radio"
              name="project-source-color"
              value={option.value}
              data-source-id={sourceId}
              checked={selected === option.value}
            />
            <span class="ticket-source-color-picker__preview">
              <TicketSourceIcon source={{ ...source, color: option.value }} size="list" />
            </span>
            <Text variant="span" size="compact">
              {option.label}
            </Text>
          </label>
        ))}
      </Grid>
    </div>
  );
}
