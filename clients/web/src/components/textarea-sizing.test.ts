import { readdirSync, readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const sourceRoot = new URL('../', import.meta.url);

function stylesheets(directory: URL): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (entry.isDirectory()) return stylesheets(new URL(`${entry.name}/`, directory));
    return entry.name.endsWith('.css') ? [new URL(entry.name, directory).pathname] : [];
  });
}

/** Every innermost rule in a stylesheet, with its declarations. Only textareas resize in this app. */
function rules(css: string): Array<{ selector: string; body: string }> {
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(([, selector, body]) => ({ selector: selector.trim(), body }));
}

describe('touch textarea sizing (HS2-6PC150)', () => {
  it('turns on content sizing without a resize grip only for coarse pointers that support it', () => {
    const css = readFileSync(new URL('./textarea-sizing.css', import.meta.url), 'utf8');
    expect(css).toMatch(
      /@supports \(field-sizing: content\) \{\s*@media \(pointer: coarse\) \{\s*:root \{\s*--hotsheet-textarea-field-sizing: content;\s*--hotsheet-textarea-resize: none;/,
    );
    expect(readFileSync(new URL('../style.css', import.meta.url), 'utf8')).toContain(
      "@import './components/textarea-sizing.css';",
    );
    // The UX demo loads the same shared tokens so touch auto-grow is representable in the catalog (HS2-YBBJEN).
    expect(readFileSync(new URL('../ux-demo/style.css', import.meta.url), 'utf8')).toContain(
      "@import '../components/textarea-sizing.css';",
    );
  });

  it('routes every resizable app textarea through the shared touch tokens', () => {
    const resizable = stylesheets(sourceRoot).flatMap((path) =>
      rules(readFileSync(path, 'utf8'))
        .filter(({ body }) => /\bresize:/.test(body) && !/\bresize: none;/.test(body))
        .map((rule) => ({ path, ...rule })),
    );
    // The details editor, notes, note replies, the composer, blocked reason, conflict merges,
    // command settings, the not-working note, and dev review.
    expect(resizable.length).toBeGreaterThanOrEqual(10);
    for (const { path, selector, body } of resizable) {
      expect({ path, selector, resize: body.match(/resize: [^;]+;/)?.[0] }).toEqual({
        path,
        selector,
        resize: 'resize: var(--hotsheet-textarea-resize, vertical);',
      });
      expect(body, `${selector} in ${path}`).toContain('field-sizing: var(--hotsheet-textarea-field-sizing, fixed);');
    }
  });

  it('drops stored desktop editor heights on touch so the editors can grow', () => {
    const inspector = readFileSync(new URL('./ticket-inspector.css', import.meta.url), 'utf8'),
      composer = readFileSync(new URL('./quick-ticket-composer.css', import.meta.url), 'utf8');
    expect(inspector).toMatch(
      /@media \(pointer: coarse\) \{\s*\.ticket-inspector__body\[data-presentation\] \{\s*--markdown-editor-source-height: auto;\s*--ticket-info-panel-blocked-reason-height: auto;\s*--note-card-editor-height: auto;/,
    );
    expect(composer).toMatch(
      /@media \(pointer: coarse\) \{\s*\.quick-ticket-composer__details textarea \{\s*height: auto;\s*min-height: remify\(40px\);/,
    );
  });
});
