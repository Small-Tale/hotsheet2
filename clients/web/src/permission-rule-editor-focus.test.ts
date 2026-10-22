import { afterEach, expect, it, vi } from 'vitest';

import { createPermissionRuleEditorFocus } from './permission-rule-editor-focus';

afterEach(() => vi.unstubAllGlobals());

function fixture() {
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  const opener = (key: string, surface: string | undefined = 'preview') => {
    const node = {
      isConnected: true,
      disabled: false,
      focus: vi.fn(),
      getAttribute: (name: string) => (name === 'data-rule-key' ? key : null),
      hasAttribute: (name: string) => name === 'disabled' && node.disabled,
      closest: () => (surface ? { getAttribute: () => surface } : null),
    };
    return node;
  };
  let nodes: ReturnType<typeof opener>[] = [],
    editorOpen = false,
    context = 'a';
  const root = {
    querySelectorAll: () => nodes,
    querySelector: () => (editorOpen ? {} : null),
  } as unknown as ParentNode;
  const focus = createPermissionRuleEditorFocus(root, () => context);
  return {
    focus,
    opener,
    remember: (node: ReturnType<typeof opener>) => {
      focus.remember(node as unknown as HTMLElement);
    },
    nodes: (value: ReturnType<typeof opener>[]) => {
      nodes = value;
    },
    editor: (value: boolean) => {
      editorOpen = value;
    },
    context: (value: string) => {
      context = value;
    },
    flush: () => {
      for (const callback of frames.splice(0)) callback(0);
    },
  };
}

it('restores the logical opener after rendering replaces its node, in the correct surface', () => {
  const f = fixture(),
    old = f.opener('same'),
    dialog = f.opener('same', 'dialog');
  f.nodes([old, dialog]);
  f.remember(old);
  const replacement = f.opener('same');
  old.isConnected = false;
  f.nodes([replacement, dialog]);
  f.focus.restore();
  expect(replacement.focus).not.toHaveBeenCalled();
  f.flush();
  expect(replacement.focus).toHaveBeenCalledWith({ preventScroll: true });
  expect(dialog.focus).not.toHaveBeenCalled();
});

it('restores focus to the row after editing from its transient context-menu item', () => {
  const f = fixture(),
    row = f.opener('same'),
    menuItem = f.opener('same');
  f.nodes([row]);
  f.remember(menuItem);
  menuItem.isConnected = false;
  f.focus.restore();
  f.flush();
  expect(row.focus).toHaveBeenCalledWith({ preventScroll: true });
});

it('returns to the current row position after a saved matcher changes identity and skips disabled targets', () => {
  const f = fixture(),
    first = f.opener('first'),
    old = f.opener('old');
  f.nodes([first, old]);
  f.remember(old);
  old.isConnected = false;
  const updated = f.opener('new');
  f.nodes([first, updated]);
  f.focus.restore();
  f.flush();
  expect(updated.focus).toHaveBeenCalledOnce();
  updated.disabled = true;
  f.focus.restore();
  f.flush();
  expect(updated.focus).toHaveBeenCalledOnce();
  f.nodes([]);
  f.focus.restore();
  f.flush();
  expect(first.focus).not.toHaveBeenCalled();
});

it('does not steal focus from a live editor, another context, a newer opener, or a disposed owner', () => {
  const f = fixture(),
    first = f.opener('one', undefined),
    second = f.opener('two');
  f.nodes([first, second]);
  f.focus.restore();
  f.flush();
  f.remember(first);
  const old = f.focus.snapshot();
  f.editor(true);
  f.focus.restore();
  f.flush();
  f.editor(false);
  f.context('b');
  f.focus.restore();
  f.flush();
  f.context('a');
  f.remember(second);
  f.focus.restore(old);
  f.flush();
  expect(first.focus).not.toHaveBeenCalled();
  f.focus.restore();
  f.focus.dispose();
  f.flush();
  expect(second.focus).not.toHaveBeenCalled();
});

it('can retain a connected opener when the rule collection temporarily has no candidates', () => {
  const f = fixture(),
    node = f.opener('one');
  f.nodes([node]);
  f.remember(node);
  f.nodes([]);
  f.focus.restore();
  f.flush();
  expect(node.focus).toHaveBeenCalledOnce();
});
