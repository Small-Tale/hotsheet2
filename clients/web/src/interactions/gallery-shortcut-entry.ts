import type { GalleryAnnotationTool } from '../gallery-annotation-editor';

const toolKeys = {
  v: 'select',
  r: 'rect',
  f: 'freehand',
  a: 'arrow',
  i: 'insertion',
  s: 'strike',
} as const satisfies Record<string, GalleryAnnotationTool>;

type ShortcutEvent = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>;

/** Apply an annotation tool key through the same markup transition as the gallery control. */
export function applyGalleryToolShortcut(
  event: ShortcutEvent,
  target: HTMLElement,
  markupActive: () => boolean,
  canBeginMarkup: () => boolean,
  toggleMarkup: () => void,
  setTool: (tool: GalleryAnnotationTool) => void,
): boolean {
  const key = event.key.toLowerCase(),
    tool = key in toolKeys ? toolKeys[key as keyof typeof toolKeys] : undefined;
  if (
    !tool ||
    event.metaKey ||
    event.ctrlKey ||
    event.altKey ||
    target.closest('input, textarea, [contenteditable="true"], [data-gallery-range-handle]')
  )
    return false;
  if (!markupActive()) {
    if (event.shiftKey || !canBeginMarkup()) return false;
    toggleMarkup();
    if (!markupActive()) return false;
  }
  setTool(tool);
  return true;
}

/** Shared by the button and keyboard entry so Finish and re-entry reset the same session state. */
export function toggleGalleryMarkupSession(options: {
  readonly saving: boolean;
  readonly markup: { value: boolean };
  readonly begin: () => void;
  readonly finish: () => void;
  readonly clearHistory: () => void;
  readonly setTool: (tool: GalleryAnnotationTool) => void;
  readonly clearSelection: () => void;
}): void {
  if (options.saving) return;
  if (options.markup.value) {
    options.markup.value = false;
    options.finish();
  } else {
    options.begin();
    options.markup.value = true;
    options.clearHistory();
  }
  options.setTool('select');
  options.clearSelection();
}
