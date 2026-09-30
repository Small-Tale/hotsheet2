import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { sourceTokens } from '../source-format-matchers';

// Kerf beta.60's TabBar clamps its strip's block axis itself (`--kui-tab-bar-strip-overflow-block`
// defaults to hidden), so no consumer strip needs an app rule on `.kui-tab-bar__tabs` (HS2-QG4K9W,
// HS2-PKPGGZ).
const strips = ['terminal-drawer.css', 'project-tab-bar.css', 'ticket-inspector.css'] as const;

describe('tab strip overflow', () => {
  it.each(strips)('%s leaves the TabBar strip axis clamp to Kerf', (file) => {
    const css = readFileSync(resolve(import.meta.dirname, file), 'utf8');
    expect(css).not.toContain('.kui-tab-bar__tabs');
  });

  it('lets the drawer strip grow with its tabs through the public strip tokens', () => {
    const css = sourceTokens(readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8')),
      body = css.match(/\.terminal-drawer__views\{([^}]*)\}/)?.[1] ?? '';
    expect(body).toContain('--kui-tab-bar-strip-min-height:var(--kui-toolbar-group-size)');
    expect(body).toContain('--kui-tab-bar-strip-flex:');
  });
});
