import { describe, expect, it } from 'vitest';

import {
  clickBeginsMarkdownEdit,
  hasTextSelectionWithin,
  keyBeginsMarkdownEdit,
  MARKDOWN_INTERACTIVE_SELECTOR,
  repeatPressWouldLeaveNewEditor,
  startsInInteractiveDescendant,
} from './markdown-click-edit';

/** A minimal element tree: `interactive` stands in for a selector match, as the browser decides it. */
interface FakeElement {
  localName: string;
  interactive: boolean;
  parentElement: FakeElement | null;
  matches(selector: string): boolean;
  contains(other: unknown): boolean;
}
function element(localName: string, parent: FakeElement | null = null, interactive = false): FakeElement {
  const node: FakeElement = {
    localName,
    interactive,
    parentElement: parent,
    matches: (selector) => selector === MARKDOWN_INTERACTIVE_SELECTOR && node.interactive,
    contains(other) {
      for (let current = other as FakeElement | null; current; current = current.parentElement)
        if (current === node) return true;
      return false;
    },
  };
  return node;
}
function tree() {
  const surface = element('div', null, true),
    paragraph = element('p', surface),
    text = element('strong', paragraph),
    link = element('a', paragraph, true),
    linkLabel = element('code', link),
    awButton = element('wa-button', surface),
    outside = element('span');
  return { surface, paragraph, text, link, linkLabel, awButton, outside };
}
const click = (target: unknown, overrides: Partial<MouseEvent> = {}) =>
  ({
    defaultPrevented: false,
    button: 0,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    target,
    ...overrides,
  }) as MouseEvent;
const noSelection = { isCollapsed: true, rangeCount: 0, getRangeAt: () => ({}) as Range };

describe('single-click Markdown edit entry (HS2-H1K9YY)', () => {
  it('lists links, buttons, enabled form controls, and role/action controls as interactive', () => {
    for (const fragment of [
      'a[href]',
      'button',
      'input:not(:disabled)',
      '[data-action]',
      '[role="button"]',
      '[role="link"]',
      'summary',
    ])
      expect(MARKDOWN_INTERACTIVE_SELECTOR.split(',')).toContain(fragment);
    // A rendered GFM task checkbox is disabled, so clicking it is ordinary content, not a control.
    expect(MARKDOWN_INTERACTIVE_SELECTOR).not.toMatch(/(^|,)input(,|$)/);
  });

  it('treats plain content as editable and links, their children, and Web Awesome hosts as their own action', () => {
    const { surface, paragraph, text, link, linkLabel, awButton, outside } = tree();
    expect(startsInInteractiveDescendant(text, surface)).toBe(false);
    expect(startsInInteractiveDescendant(paragraph, surface)).toBe(false);
    // The surface itself is a role=button; only descendants count, so a click on its padding edits.
    expect(startsInInteractiveDescendant(surface, surface)).toBe(false);
    expect(startsInInteractiveDescendant(link, surface)).toBe(true);
    expect(startsInInteractiveDescendant(linkLabel, surface)).toBe(true);
    expect(startsInInteractiveDescendant(awButton, surface)).toBe(true);
    expect(startsInInteractiveDescendant(outside, surface)).toBe(false);
    expect(startsInInteractiveDescendant(null, surface)).toBe(false);
    expect(startsInInteractiveDescendant({}, surface)).toBe(false);
  });

  it('begins editing only for a plain primary click on content', () => {
    const { surface, text, link } = tree();
    const begins = (event: MouseEvent) => {
      const original = globalThis.getSelection;
      globalThis.getSelection = () => noSelection as unknown as Selection;
      try {
        return clickBeginsMarkdownEdit(event, surface);
      } finally {
        globalThis.getSelection = original;
      }
    };
    expect(begins(click(text))).toBe(true);
    expect(begins(click(link))).toBe(false);
    expect(begins(click(text, { button: 1 }))).toBe(false);
    expect(begins(click(text, { defaultPrevented: true }))).toBe(false);
    for (const modifier of ['metaKey', 'ctrlKey', 'shiftKey', 'altKey'] as const)
      expect(begins(click(text, { [modifier]: true }))).toBe(false);
  });

  it('leaves a drag-selection inside the surface alone but ignores selections elsewhere', () => {
    const { surface, text, outside } = tree();
    const selectionIn = (node: unknown) => ({
      isCollapsed: false,
      rangeCount: 1,
      getRangeAt: () => ({ commonAncestorContainer: node }) as unknown as Range,
    });
    expect(hasTextSelectionWithin(surface, selectionIn(text))).toBe(true);
    expect(hasTextSelectionWithin(surface, selectionIn(outside))).toBe(false);
    expect(hasTextSelectionWithin(surface, noSelection)).toBe(false);
    expect(hasTextSelectionWithin(surface, { ...selectionIn(text), rangeCount: 0 })).toBe(false);
    expect(hasTextSelectionWithin(surface, null)).toBe(false);
  });

  it('enters editing from Enter or Space on the surface but leaves keys on nested links to them', () => {
    const { surface, link } = tree();
    const key = (keyName: string, target: unknown, defaultPrevented = false) =>
      ({ key: keyName, target, defaultPrevented }) as KeyboardEvent;
    expect(keyBeginsMarkdownEdit(key('Enter', surface), surface)).toBe(true);
    expect(keyBeginsMarkdownEdit(key(' ', surface), surface)).toBe(true);
    expect(keyBeginsMarkdownEdit(key('a', surface), surface)).toBe(false);
    expect(keyBeginsMarkdownEdit(key('Enter', surface, true), surface)).toBe(false);
    expect(keyBeginsMarkdownEdit(key('Enter', link), surface)).toBe(false);
  });

  it('keeps focus in a just-opened editor on the repeat press of a double-click in the same card', () => {
    const { surface: card, paragraph, link, outside } = tree();
    const textarea = element('textarea', card, true);
    const withClosest = <T extends FakeElement>(node: T) =>
      Object.assign(node, {
        closest: (selector: string) => (selector.includes('note-card') ? card : null),
      });
    const press = (target: unknown, detail = 2, button = 0) => ({ detail, button, target }) as MouseEvent;
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(paragraph)), textarea as unknown as Element)).toBe(true);
    // A first press, a non-primary press, a press on the editor itself, or on a nested control is left alone.
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(paragraph), 1), textarea as unknown as Element)).toBe(
      false,
    );
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(paragraph), 2, 2), textarea as unknown as Element)).toBe(
      false,
    );
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(textarea)), textarea as unknown as Element)).toBe(false);
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(link)), textarea as unknown as Element)).toBe(false);
    // Focus elsewhere, or a press outside any click-edit container, is ordinary.
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(paragraph)), { localName: 'input' })).toBe(false);
    expect(repeatPressWouldLeaveNewEditor(press(withClosest(paragraph)), null)).toBe(false);
    const stray = Object.assign(outside, { closest: () => null });
    expect(repeatPressWouldLeaveNewEditor(press(stray), textarea as unknown as Element)).toBe(false);
    expect(repeatPressWouldLeaveNewEditor(press(null), textarea as unknown as Element)).toBe(false);
  });
});
