import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const css = (path: string) => readFileSync(resolve(import.meta.dirname, path), 'utf8');

describe('touch text fields stay at 16px or larger (HS2-8JPRRC)', () => {
  it('defines the iOS zoom threshold as a literal px token', () => {
    expect(css('hot-sheet-tokens.css')).toMatch(/--hs-touch-field-min-font-size:\s*16px;/);
  });

  it.each([
    ['components/quick-ticket-composer.css', '.quick-ticket-composer__details textarea'],
    ['components/markdown-editor.css', '.markdown-editor__source'],
    ['components/ticket-tag-editor.css', '.ticket-tag-editor__popover input'],
    ['components/ticket-attachments.css', '.ticket-attachments__batch > header select'],
    ['components/provider-setup-form.css', '.provider-setup-form__field-control'],
  ])('%s raises %s to the token on coarse pointers only', (file, selector) => {
    const source = css(file),
      coarse = source.slice(source.lastIndexOf('@media (pointer: coarse)'));
    expect(coarse).toContain(selector);
    expect(coarse).toMatch(/font-size:\s*max\(\s*var\(--hs-touch-field-min-font-size\)/);
  });
});
