import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { createAttachmentFilenameEditor } from './attachment-filename-editing';
import { INPUT_DIALOG_TARGETS } from './interaction-attrs/input-dialogs';

/** Ordinary text editing: no network write until blur or Enter; Escape restores (HS2-WMZ6Z5). */
export function wireAttachmentFilenameEditor(
  root: HTMLElement,
  editor: ReturnType<typeof createAttachmentFilenameEditor>,
) {
  const scope = createScope();
  const restoreFocus = (field: Element) => {
    const active = root.ownerDocument.activeElement;
    if (active !== root.ownerDocument.body && active !== field) return;
    field
      .closest('[data-component="ticket-attachment-item"]')
      ?.querySelector<HTMLElement>('[data-action="open-attachment-menu"]')
      ?.focus();
  };
  scope.add(
    delegate(root, 'input', INPUT_DIALOG_TARGETS.filename.selector, (_event, field) => {
      editor.input((field as HTMLInputElement).value);
    }),
  );
  scope.add(
    delegate(root, 'focusout', INPUT_DIALOG_TARGETS.filename.selector, () => {
      void editor.finish();
    }),
  );
  scope.add(
    delegate(root, 'keydown', INPUT_DIALOG_TARGETS.filename.selector, (event, field) => {
      const key = (event as KeyboardEvent).key;
      if (key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        editor.cancel();
        queueMicrotask(() => {
          restoreFocus(field);
        });
      }
      if (key === 'Enter') {
        event.preventDefault();
        (field as HTMLElement).blur();
        void editor.finish().then((saved) => {
          if (saved)
            queueMicrotask(() => {
              restoreFocus(field);
            });
        });
      }
    }),
  );
  return () => {
    editor.dispose();
    scope.dispose();
  };
}
