import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { sourceTokens } from '../source-format-matchers';

// Kerf's TabBar strip sets only overflow-x: auto, which computes overflow-y to auto; every consumer strip
// must clamp the vertical axis so tabs never scroll vertically (HS2-QG4K9W).
const strips = [
  ['terminal-drawer.css', '.terminal-drawer__views .kui-tab-bar__tabs'],
  ['project-tab-bar.css', '.project-tab-bar .kui-tab-bar__tabs'],
  ['ticket-inspector.css', '.ticket-inspector__tabs .kui-tab-bar__tabs'],
] as const;

describe('tab strip overflow', () => {
  it.each(strips)('%s clamps vertical scrolling on its TabBar strip', (file, selector) => {
    const css = sourceTokens(readFileSync(resolve(import.meta.dirname, file), 'utf8')),
      rules = [...css.matchAll(/([^{}]+)\{([^}]*)\}/g)].filter(([, head]) => head.endsWith(sourceTokens(selector)));
    expect(rules.some(([, , body]) => body.includes('overflow-y:hidden'))).toBe(true);
  });

  it('lets the drawer strip grow with its tabs instead of clipping them', () => {
    const css = sourceTokens(readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8')),
      body = css.match(/\.terminal-drawer__views\.kui-tab-bar__tabs\{([^}]*)\}/)?.[1] ?? '';
    expect(body).toContain('height:auto');
    expect(body).toContain('min-height:var(--kui-toolbar-group-size)');
  });
});
