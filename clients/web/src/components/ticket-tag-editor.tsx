import './ticket-tag-editor.css';

import { Plus } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { TagChip } from './tag-chip';

export interface TicketTagEditorProps {
  tags: readonly string[];
  suggestions?: readonly string[];
  editable?: boolean;
}

export function normalizeTicketTag(value: string): string {
  return value.trim().replaceAll(/\s+/g, '-');
}

export function addTicketTag(tags: readonly string[], value: string): string[] {
  const tag = normalizeTicketTag(value);
  return !tag || tags.includes(tag) ? [...tags] : [...tags, tag];
}

export function removeTicketTag(tags: readonly string[], value: string): string[] {
  return tags.filter(tag => tag !== value);
}

export function TicketTagEditor({ tags, suggestions = [], editable = true }: TicketTagEditorProps) {
  const available = [...new Set(suggestions)].filter(tag => !tags.includes(tag)).sort();
  return <div class="ticket-tag-editor" data-component="ticket-tag-editor" data-editable={String(editable)}>
    <div class="ticket-tag-editor__chips">{tags.map(tag => TagChip({ id: tag, label: tag, removable: editable }))}</div>
    {editable && <label class="ticket-tag-editor__add"><LucideIcon icon={Plus} name="plus" /><span>Add tag</span><input name="ticket-tag-input" list="ticket-tag-suggestions" autocomplete="off" aria-label="Add tag" /><datalist id="ticket-tag-suggestions">{available.map(tag => <option value={tag}></option>)}</datalist></label>}
  </div>;
}
