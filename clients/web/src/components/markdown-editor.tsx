import './markdown-editor.css';

import { Maximize2, Minimize2 } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';

export type MarkdownEditorMode = 'write' | 'preview';
export interface MarkdownEditorProps { value: string; mode: MarkdownEditorMode; expanded?: boolean; dirty?: boolean; label?: string; appearance?: 'standalone' | 'embedded'; showExpand?: boolean; expandAction?: string; editable?: boolean }

export function MarkdownEditor({ value, mode, expanded = false, dirty = false, label = 'Markdown content', appearance = 'standalone', showExpand = true, expandAction = 'toggle-markdown-expanded', editable = true }: MarkdownEditorProps) {
  const empty = !value.trim();
  return <section class={`${expanded ? 'markdown-editor markdown-editor--expanded' : 'markdown-editor'}${appearance === 'embedded' ? ' markdown-editor--embedded' : ''}`} data-component="markdown-editor" data-mode={mode} data-expanded={String(expanded)} data-appearance={appearance}>
    {(appearance === 'standalone' || showExpand) && <header class="markdown-editor__toolbar">
      <span>{mode === 'write' ? 'Editing Markdown' : 'Markdown'}</span>
      {showExpand && <button type="button" class="markdown-editor__expand" data-action={expandAction} aria-label={expanded ? 'Use inline editor' : 'Expand editor'}><LucideIcon icon={expanded ? Minimize2 : Maximize2} name={expanded ? 'minimize-2' : 'maximize-2'} /></button>}
    </header>}
    <div class="markdown-editor__surface">
      {mode === 'write'
        ? <textarea name="markdown-source" aria-label={label} spellcheck="true">{value}</textarea>
        : <div class="markdown-editor__preview" role={editable ? 'button' : undefined} tabIndex={editable ? 0 : undefined} data-action={editable ? 'edit-markdown' : undefined} data-empty={String(empty)} aria-label={editable ? `Edit ${label}` : label} title={editable ? empty ? 'Click to add Markdown' : 'Double-click to edit' : undefined}><MarkdownPreview source={value} emptyLabel={editable ? 'Click to add Markdown.' : 'No details.'} /></div>}
    </div>
    {mode === 'write' && appearance === 'standalone' && <footer><span aria-live="polite">{dirty ? 'Saving changes…' : 'Changes saved'}</span></footer>}
  </section>;
}
