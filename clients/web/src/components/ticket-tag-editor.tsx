import './ticket-tag-editor.css';

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
  return tags.filter(tag => tag !== value);
}

export function TicketTagEditor({ tags, suggestions = [], editable = true, popoverId = 'ticket-tag-popover' }: TicketTagEditorProps) {
  const available = [...new Set(suggestions)].filter(tag => !tags.includes(tag)).sort();
  const titleId = `${popoverId}-title`;
  return <div class="ticket-tag-editor" data-component="ticket-tag-editor" data-editable={String(editable)}>
    <div class="ticket-tag-editor__chips">{tags.map(tag => TagChip({ id: tag, label: tag, removable: editable }))}</div>
    {editable && <div id={popoverId} class="ticket-tag-editor__popover" data-component="ticket-tag-popover" popover="auto" role="dialog" aria-labelledby={titleId}><strong id={titleId}>Add tag</strong><label><span>Tag name</span><input name="ticket-tag-input" list={`${popoverId}-suggestions`} autocomplete="off" placeholder="Search or create a tag" autofocus /></label><small>Press Enter to add</small><datalist id={`${popoverId}-suggestions`}>{available.map(tag => <option value={tag}></option>)}</datalist></div>}
  </div>;
}
