import { Eye, Maximize2, Minimize2, Pencil, Save, X } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';
import './markdown-editor.css';

export type MarkdownEditorMode = 'write' | 'preview';
export interface MarkdownEditorProps { value: string; mode: MarkdownEditorMode; expanded?: boolean; dirty?: boolean; label?: string }

export function MarkdownEditor({ value, mode, expanded = false, dirty = false, label = 'Markdown content' }: MarkdownEditorProps) {
  return <section class={expanded ? 'markdown-editor markdown-editor--expanded' : 'markdown-editor'} data-component="markdown-editor" data-mode={mode} data-expanded={String(expanded)}>
    <header class="markdown-editor__toolbar">
      <div class="markdown-editor__modes" role="group" aria-label="Editor mode">
        <button type="button" data-action="set-markdown-mode" data-markdown-mode="write" aria-pressed={String(mode === 'write')}><LucideIcon icon={Pencil} name="pencil" />Write</button>
        <button type="button" data-action="set-markdown-mode" data-markdown-mode="preview" aria-pressed={String(mode === 'preview')}><LucideIcon icon={Eye} name="eye" />Preview</button>
      </div>
      <button type="button" class="markdown-editor__expand" data-action="toggle-markdown-expanded" aria-label={expanded ? 'Use inline editor' : 'Expand editor'}><LucideIcon icon={expanded ? Minimize2 : Maximize2} name={expanded ? 'minimize-2' : 'maximize-2'} /></button>
    </header>
    <div class="markdown-editor__surface">
      {mode === 'write' ? <textarea name="markdown-source" aria-label={label} spellcheck="true">{value}</textarea> : <MarkdownPreview source={value} />}
    </div>
    <footer><span aria-live="polite">{dirty ? 'Unsaved changes' : 'Saved'}</span><div><wa-button appearance="plain" data-action="cancel-markdown-edit"><span slot="start"><LucideIcon icon={X} name="x" /></span>Cancel</wa-button><wa-button variant="brand" data-action="save-markdown" disabled={!dirty}><span slot="start"><LucideIcon icon={Save} name="save" /></span>Save</wa-button></div></footer>
  </section>;
}
