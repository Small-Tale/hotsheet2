import './markdown-editor.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Maximize2, Minimize2 } from 'lucide';

import type { AttachmentReferenceContext } from '../attachment-references';
import { MarkdownPreview } from './markdown-preview';

export type MarkdownEditorMode = 'write' | 'preview';
export interface MarkdownEditorProps {
  value: string;
  mode: MarkdownEditorMode;
  expanded?: boolean;
  dirty?: boolean;
  label?: string;
  appearance?: 'standalone' | 'embedded';
  /**
   * `flush` drops the preview and source padding so a host surface that owns its own inset (the
   * inspector Details surface, HS2-MGVE50) aligns the text to its column.
   */
  inset?: 'padded' | 'flush';
  showExpand?: boolean;
  expandAction?: string;
  editable?: boolean;
  attachmentContext?: AttachmentReferenceContext;
}

export function MarkdownEditor({
  value,
  mode,
  expanded = false,
  label = 'Markdown content',
  appearance = 'standalone',
  inset = 'padded',
  showExpand = true,
  expandAction = 'toggle-markdown-expanded',
  editable = true,
  attachmentContext,
}: MarkdownEditorProps) {
  const empty = !value.trim();
  return (
    <section
      class={`${expanded ? 'markdown-editor markdown-editor--expanded' : 'markdown-editor'}${appearance === 'embedded' ? ' markdown-editor--embedded' : ''}${inset === 'flush' ? ' markdown-editor--flush' : ''}`}
      data-component="markdown-editor"
      data-mode={mode}
      data-expanded={String(expanded)}
      data-appearance={appearance}
      data-inset={inset}
    >
      {(appearance === 'standalone' || showExpand) && (
        <header class="markdown-editor__toolbar">
          <span>{mode === 'write' ? 'Editing Markdown' : 'Markdown'}</span>
          {showExpand && (
            <button
              type="button"
              class="markdown-editor__expand"
              data-action={expandAction}
              aria-label={expanded ? 'Use inline editor' : 'Expand editor'}
            >
              <LucideIcon
                size={15.2}
                icon={expanded ? Minimize2 : Maximize2}
                name={expanded ? 'minimize-2' : 'maximize-2'}
              />
            </button>
          )}
        </header>
      )}
      <div class="markdown-editor__surface">
        {mode === 'write' ? (
          <textarea class="markdown-editor__source" name="markdown-source" aria-label={label} spellcheck="true">
            {value}
          </textarea>
        ) : (
          <div
            class="markdown-editor__preview"
            role={editable ? 'button' : undefined}
            tabIndex={editable ? 0 : undefined}
            data-action={editable ? 'edit-markdown' : undefined}
            data-empty={String(empty)}
            aria-label={editable ? `Edit ${label}` : label}
            title={editable ? (empty ? 'Click to add Markdown' : 'Click to edit') : undefined}
          >
            <MarkdownPreview
              source={value}
              emptyLabel={editable ? 'Click to add Markdown.' : 'No details.'}
              attachmentContext={attachmentContext}
            />
          </div>
        )}
      </div>
    </section>
  );
}
