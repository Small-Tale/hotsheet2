import './ticket-source-color-picker.css';

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
  const selected = resolveTicketSourceColor(source.color);
  return (
    <fieldset class="ticket-source-color-picker" data-component="ticket-source-color-picker">
      <legend>Icon color</legend>
      <div class="ticket-source-color-picker__grid">
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
            <span class="ticket-source-color-picker__label">{option.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}
