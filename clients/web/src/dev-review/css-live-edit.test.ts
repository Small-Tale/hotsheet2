import { describe, expect, it } from 'vitest';

import { cssSnapshotAttachment, formatCssSnapshot } from './css-live-edit';

describe('CSS Live Edit snapshots', () => {
  it('formats complete stylesheet rules and inline declarations in stable source order', () => {
    const snapshot = formatCssSnapshot(
      [
        { label: 'inline <style#theme>', rules: ['.card { padding: 8px; }', '.label { color: red; }'] },
        { label: 'https://example.test/private.css (rules unavailable to the browser)' },
      ],
      [{ selector: 'html > body > section', cssText: 'margin-top: 4px;' }],
    );
    expect(snapshot).toBe(`/* stylesheet 1: inline <style#theme> */
.card { padding: 8px; }
.label { color: red; }

/* stylesheet 2: https://example.test/private.css (rules unavailable to the browser) */

/* inline style declarations */
/* inline style: html > body > section */
html > body > section { margin-top: 4px; }
`);
  });

  it('encodes a UTF-8 CSS attachment without changing its contents', () => {
    const snapshot = '.label::after { content: "✓"; }\n';
    const attachment = cssSnapshotAttachment({ btoa } as Window, 'before.css', snapshot);
    expect(attachment).toMatchObject({ filename: 'before.css', mimeType: 'text/css' });
    expect(
      new TextDecoder().decode(
        Uint8Array.from(atob(attachment.dataUrl.split(',')[1]), (character) => character.charCodeAt(0)),
      ),
    ).toBe(snapshot);
  });
});
