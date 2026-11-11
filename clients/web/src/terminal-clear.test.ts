import { describe, expect, it } from 'vitest';

import {
  clearTerminalViewports,
  TERMINAL_CLEAR_EVENT,
  terminalClearKeyTarget,
  terminalViewportIdentity,
} from './terminal-clear';

/** A viewport stand-in: an event target with the data attributes production viewports carry. */
function viewport(dataset: Record<string, string>) {
  const target = new EventTarget() as HTMLElement & { cleared: number };
  Object.assign(target, { dataset, cleared: 0 });
  target.addEventListener(TERMINAL_CLEAR_EVENT, () => {
    target.cleared += 1;
  });
  return target;
}

describe('terminal clear (HS2-N2AG8V)', () => {
  it('reads a viewport identity only when both project and terminal are known', () => {
    expect(terminalViewportIdentity(viewport({ projectId: 'p', terminalId: 't' }))).toEqual({
      projectId: 'p',
      terminalId: 't',
    });
    expect(terminalViewportIdentity(viewport({ terminalId: 't' }))).toBeUndefined();
    expect(terminalViewportIdentity(viewport({ projectId: 'p' }))).toBeUndefined();
  });

  it('targets the focused terminal viewport only for an uncomposed physical Command-K', () => {
    const inside = { tagName: 'VIEWPORT' },
      textarea = {
        closest: (selector: string) => (selector === '[data-component="terminal-viewport"]' ? inside : null),
      },
      outside = { closest: () => null },
      press = (over: Partial<KeyboardEvent>, target: unknown = textarea) => ({
        key: 'k',
        metaKey: true,
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
        isComposing: false,
        target: target as EventTarget,
        ...over,
      });
    expect(terminalClearKeyTarget(press({}))).toBe(inside);
    expect(terminalClearKeyTarget(press({}, outside))).toBeUndefined();
    expect(terminalClearKeyTarget(press({}, null))).toBeUndefined();
    expect(terminalClearKeyTarget(press({ isComposing: true }))).toBeUndefined();
    expect(terminalClearKeyTarget(press({ metaKey: false, ctrlKey: true }))).toBeUndefined();
    expect(terminalClearKeyTarget(press({ shiftKey: true }))).toBeUndefined();
    expect(terminalClearKeyTarget(press({ key: 'l' }))).toBeUndefined();
  });

  it('clears every mounted viewport of the terminal and none of another terminal or project', () => {
    const dedicated = viewport({ projectId: 'p', terminalId: 't' }),
      preview = viewport({ projectId: 'p', terminalId: 't', displayMode: 'scaled-preview' }),
      sibling = viewport({ projectId: 'p', terminalId: 'u' }),
      otherProject = viewport({ projectId: 'q', terminalId: 't' }),
      unknown = viewport({}),
      root = {
        querySelectorAll: (selector: string) => {
          expect(selector).toBe('[data-component="terminal-viewport"]');
          return [dedicated, preview, sibling, otherProject, unknown];
        },
      } as unknown as ParentNode;
    expect(clearTerminalViewports(root, { projectId: 'p', terminalId: 't' })).toBe(2);
    expect([dedicated.cleared, preview.cleared, sibling.cleared, otherProject.cleared, unknown.cleared]).toEqual([
      1, 1, 0, 0, 0,
    ]);
    // Repeated clears each reach the emulator again; an absent terminal asks nobody.
    expect(clearTerminalViewports(root, { projectId: 'p', terminalId: 't' })).toBe(2);
    expect(dedicated.cleared).toBe(2);
    expect(clearTerminalViewports(root, { projectId: 'p', terminalId: 'gone' })).toBe(0);
  });
});
