import { describe, expect, it, vi } from 'vitest';

import type { GalleryAnnotationTool } from '../gallery-annotation-editor';
import { applyGalleryToolShortcut, toggleGalleryMarkupSession } from './gallery-shortcut-entry';

const keyEvent = (
  key: string,
  modifiers: Partial<Pick<KeyboardEvent, 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>> = {},
) => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...modifiers,
});
const target = (focusedSelector?: string) =>
  ({
    closest: (selector: string) => (focusedSelector && selector.includes(focusedSelector) ? {} : null),
  }) as unknown as HTMLElement;

function gallerySession() {
  const markup = { value: false };
  const state = { tool: 'select' as GalleryAnnotationTool, selected: 'old' as string | undefined };
  const history = { undo: ['old'], redo: ['old'] };
  const calls: string[] = [];
  let saving = false;
  const setTool = (tool: GalleryAnnotationTool) => {
    state.tool = tool;
    calls.push(`tool:${tool}`);
  };
  const toggle = () => {
    toggleGalleryMarkupSession({
      saving,
      markup,
      begin: () => calls.push('begin'),
      finish: () => calls.push('finish'),
      clearHistory: () => {
        history.undo.length = 0;
        history.redo.length = 0;
      },
      setTool,
      clearSelection: () => {
        state.selected = undefined;
      },
    });
  };
  const canBegin = vi.fn(() => true);
  const press = (event: ReturnType<typeof keyEvent>, focusedSelector?: string) =>
    applyGalleryToolShortcut(event, target(focusedSelector), () => markup.value, canBegin, toggle, setTool);
  return { markup, state, history, calls, canBegin, press, toggle, setSaving: (value: boolean) => (saving = value) };
}

describe('gallery annotation shortcut entry (HS2-TP8RP5)', () => {
  it.each([
    ['v', 'select'],
    ['R', 'rect'],
    ['f', 'freehand'],
    ['a', 'arrow'],
    ['I', 'insertion'],
    ['s', 'strike'],
  ] as const)(
    '%s starts the real markup session before selecting %s; Finish resets and re-entry starts clean',
    (key, tool) => {
      const session = gallerySession();
      expect(session.press(keyEvent(key))).toBe(true);
      expect(session.markup.value).toBe(true);
      expect(session.state.tool).toBe(tool);
      expect(session.state.selected).toBeUndefined();
      expect(session.history).toEqual({ undo: [], redo: [] });
      expect(session.calls).toEqual(['begin', 'tool:select', `tool:${tool}`]);

      session.toggle(); // Finish markup uses the same transition as the real button.
      expect(session.markup.value).toBe(false);
      expect(session.state.tool).toBe('select');
      expect(session.calls.at(-2)).toBe('finish');
      session.history.undo.push('stale');
      expect(session.press(keyEvent('r'))).toBe(true);
      expect(session.markup.value).toBe(true);
      expect(session.state.tool).toBe('rect');
      expect(session.history.undo).toEqual([]);
      expect(session.calls.filter((call) => call === 'begin')).toHaveLength(2);
    },
  );

  it('ignores modified, focused, non-tool and disabled entry without starting a session', () => {
    const session = gallerySession();
    for (const event of [
      keyEvent('r', { metaKey: true }),
      keyEvent('r', { ctrlKey: true }),
      keyEvent('r', { altKey: true }),
      keyEvent('r', { shiftKey: true }),
      keyEvent('z'),
    ])
      expect(session.press(event)).toBe(false);
    for (const selector of ['input', 'textarea', '[contenteditable="true"]', '[data-gallery-range-handle]'])
      expect(session.press(keyEvent('r'), selector)).toBe(false);
    session.canBegin.mockReturnValue(false);
    expect(session.press(keyEvent('r'))).toBe(false);
    expect(session.calls).toEqual([]);
    expect(session.markup.value).toBe(false);
    expect(session.state.selected).toBe('old');
  });

  it('uses an active session for a shifted tool key and refuses entry while saving', () => {
    const session = gallerySession();
    session.setSaving(true);
    expect(session.press(keyEvent('r'))).toBe(false);
    expect(session.markup.value).toBe(false);
    expect(session.state.tool).toBe('select');
    session.setSaving(false);
    expect(session.press(keyEvent('r'))).toBe(true);
    expect(session.press(keyEvent('F', { shiftKey: true }))).toBe(true);
    expect(session.state.tool).toBe('freehand');
    expect(session.calls.filter((call) => call === 'begin')).toHaveLength(1);
  });
});
