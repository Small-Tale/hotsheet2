import { beforeEach, describe, expect, it } from 'vitest';

import {
  markdownAppearance,
  MarkdownEditorDemo,
  MarkdownEditorSettings,
  markdownInset,
  resetMarkdownEditorDemo,
} from './content-components-demo';

function fakeRoot() {
  const controls: Record<string, { value: string; checked: boolean }> = {
    'markdown-appearance': { value: 'embedded', checked: false },
    'markdown-inset': { value: 'flush', checked: false },
  };
  const root = {
    querySelector: (selector: string) => {
      const match = /\[data-settings="markdown-editor"\] \[name="([^"]+)"\]/.exec(selector);
      return match ? (controls[match[1]] ?? null) : null;
    },
  } as unknown as ParentNode;
  return { root, controls };
}

describe('MarkdownEditor demo settings (HS2-QBR5HC)', () => {
  beforeEach(() => {
    markdownAppearance.value = 'standalone';
    markdownInset.value = 'padded';
  });

  it('offers both public variants and a reset action in its settings form', () => {
    const settings = String(MarkdownEditorSettings());
    expect(settings).toContain('data-settings="markdown-editor"');
    expect(settings).toContain('name="markdown-appearance"');
    expect(settings).toContain('name="markdown-inset"');
    expect(settings).toContain('data-action="reset-settings"');
  });

  it('projects the chosen appearance and inset into the editor, then resets state and live controls', () => {
    markdownAppearance.value = 'embedded';
    markdownInset.value = 'flush';
    const changed = String(MarkdownEditorDemo());
    expect(changed).toContain('data-appearance="embedded"');
    expect(changed).toContain('data-inset="flush"');
    expect(changed).toContain('markdown-editor--embedded');
    expect(changed).toContain('markdown-editor--flush');

    const { root, controls } = fakeRoot();
    resetMarkdownEditorDemo(root);
    expect(markdownAppearance.value).toBe('standalone');
    expect(markdownInset.value).toBe('padded');
    expect(controls['markdown-appearance'].value).toBe('standalone');
    expect(controls['markdown-inset'].value).toBe('padded');
    const reset = String(MarkdownEditorDemo());
    expect(reset).toContain('data-appearance="standalone"');
    expect(reset).not.toContain('markdown-editor--flush');

    markdownInset.value = 'flush';
    expect(String(MarkdownEditorDemo())).toContain('data-inset="flush"');
  });
});
