import './markdown-editor.css';

import { Maximize2, Minimize2 } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';

export type MarkdownEditorMode = 'write' | 'preview';
export interface MarkdownEditorProps { value: string; mode: MarkdownEditorMode; expanded?: boolean; dirty?: boolean; label?: string; appearance?: 'standalone' | 'embedded'; showExpand?: boolean; expandAction?: string }

export function MarkdownEditor({ value, mode, expanded = false, dirty = false, label = 'Markdown content', appearance = 'standalone', showExpand = true, expandAction = 'toggle-markdown-expanded' }: MarkdownEditorProps) {
  return <section class={`${expanded ? 'markdown-editor markdown-editor--expanded' : 'markdown-editor'}${appearance === 'embedded' ? ' markdown-editor--embedded' : ''}`} data-component="markdown-editor" data-mode={mode} data-expanded={String(expanded)} data-appearance={appearance}>
    {(appearance === 'standalone' || showExpand) && <header class="markdown-editor__toolbar">
      <span>{mode === 'write' ? 'Editing Markdown' : 'Markdown'}</span>
      {showExpand && <button type="button" class="markdown-editor__expand" data-action={expandAction} aria-label={expanded ? 'Use inline editor' : 'Expand editor'}><LucideIcon icon={expanded ? Minimize2 : Maximize2} name={expanded ? 'minimize-2' : 'maximize-2'} /></button>}
    </header>}
    <div class="markdown-editor__surface">
      {mode === 'write'
        ? <textarea name="markdown-source" aria-label={label} spellcheck="true">{value}</textarea>
        : <div class="markdown-editor__preview" role="button" tabIndex={0} data-action="edit-markdown" aria-label={`Edit ${label}`} title="Double-click to edit"><MarkdownPreview source={value} emptyLabel="Double-click to add Markdown." /></div>}
    </div>
    {mode === 'write' && <footer><span aria-live="polite">{dirty ? 'Unsaved changes' : 'No changes'}</span><div><wa-button appearance="plain" data-action="cancel-markdown-edit">Cancel</wa-button><wa-button variant="brand" data-action="save-markdown" disabled={!dirty}>Save</wa-button></div></footer>}
  </section>;
}
