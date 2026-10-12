import './ticket-tag-editor.css';

import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

import { TagChip } from './tag-chip';

export interface TicketTagEditorProps {
  tags: readonly string[];
  suggestions?: readonly string[];
  editable?: boolean;
  popoverId?: string;
}

export function normalizeTicketTag(value: string): string {
  return value.trim().replaceAll(/\s+/g, '-');
}

export function addTicketTag(tags: readonly string[], value: string): string[] {
  const tag = normalizeTicketTag(value);
  return !tag || tags.includes(tag) ? [...tags] : [...tags, tag];
}

export function removeTicketTag(tags: readonly string[], value: string): string[] {
  return tags.filter((tag) => tag !== value);
}

export function TicketTagEditor({
  tags,
  suggestions = [],
  editable = true,
  popoverId = 'ticket-tag-popover',
}: TicketTagEditorProps) {
  const available = [...new Set(suggestions)].filter((tag) => !tags.includes(tag)).sort();
  const titleId = `${popoverId}-title`;
  return (
    <div class="ticket-tag-editor" data-component="ticket-tag-editor" data-editable={String(editable)}>
      <Row wrap gap="xs">
        {tags.map((tag) => TagChip({ id: tag, label: tag, removable: editable }))}
      </Row>
      {editable && (
        <div
          id={popoverId}
          class="ticket-tag-editor__popover"
          data-component="ticket-tag-popover"
          popover="auto"
          role="dialog"
          aria-labelledby={titleId}
        >
          <Text id={titleId} flush lineHeight="tight">
            <strong>Add tag</strong>
          </Text>
          <label class="ticket-tag-editor__field">
            <Text variant="span" size="compact" tone="quiet">
              Tag name
            </Text>
            <input
              name="ticket-tag-input"
              list={`${popoverId}-suggestions`}
              autocomplete="off"
              placeholder="Search or create a tag"
              autofocus
            />
          </label>
          <Text size="compact" tone="quiet" flush>
            Press Enter to add
          </Text>
          <datalist id={`${popoverId}-suggestions`}>
            {available.map((tag) => (
              <option value={tag} />
            ))}
          </datalist>
        </div>
      )}
    </div>
  );
}
