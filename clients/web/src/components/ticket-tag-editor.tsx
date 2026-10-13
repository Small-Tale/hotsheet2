import './ticket-tag-editor.css';

import { openAnchoredSurface } from '@kerfjs/ui/anchored-surface';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { type OverlayHandle } from 'kerfjs/overlay';

import { TagChip } from './tag-chip';

export interface TicketTagEditorProps {
  tags: readonly string[];
  suggestions?: readonly string[];
  editable?: boolean;
  /** Id prefix for this editor's Add tag suggestions (unique per inspector presentation). */
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

/** Id of an editor's suggestion `<datalist>`, which the Add tag field lists from. */
export function ticketTagSuggestionsId(popoverId: string): string {
  return `${popoverId}-suggestions`;
}

export function TicketTagEditor({
  tags,
  suggestions = [],
  editable = true,
  popoverId = 'ticket-tag-popover',
}: TicketTagEditorProps) {
  const available = [...new Set(suggestions)].filter((tag) => !tags.includes(tag)).sort();
  return (
    <div class="ticket-tag-editor" data-component="ticket-tag-editor" data-editable={String(editable)}>
      <Row wrap gap="xs">
        {tags.map((tag) => TagChip({ id: tag, label: tag, removable: editable }))}
      </Row>
      {/* The suggestions stay in the page and re-render with the ticket; the Add tag surface's
          field references them by id while Kerf's AnchoredSurface is open (HS2-Q4JE3V). */}
      {editable && (
        <datalist id={ticketTagSuggestionsId(popoverId)}>
          {available.map((tag) => (
            <option value={tag} />
          ))}
        </datalist>
      )}
    </div>
  );
}

/**
 * Content of the Add tag surface. Kerf `openAnchoredSurface` owns its chrome, position, focus,
 * and dismissal; Enter and focusout on the field add the tag (HS2-Q4JE3V).
 */
export function TicketTagPopoverContent({ suggestionsId }: { suggestionsId?: string }) {
  return (
    <div class="ticket-tag-editor__surface" data-component="ticket-tag-popover">
      <Text flush lineHeight="tight">
        <strong>Add tag</strong>
      </Text>
      <label class="ticket-tag-editor__field">
        <Text variant="span" size="compact" tone="quiet">
          Tag name
        </Text>
        <input
          class="ticket-tag-editor__input"
          name="ticket-tag-input"
          list={suggestionsId}
          autocomplete="off"
          placeholder="Search or create a tag"
        />
      </label>
      <Text size="compact" tone="quiet" flush>
        Press Enter to add
      </Text>
    </div>
  );
}

/**
 * Open the Add tag surface beside its Tags header trigger. The production inspector and the UX demo
 * share this so the surface options cannot drift (HS2-Q4JE3V).
 */
export function openTicketTagSurface(trigger: Element): OverlayHandle {
  return openAnchoredSurface(
    trigger,
    TicketTagPopoverContent({ suggestionsId: (trigger as HTMLElement).dataset.tagSuggestions }),
    { label: 'Add tag', placement: 'bottom', align: 'end', dismiss: ['escape', 'outside'] },
  );
}
