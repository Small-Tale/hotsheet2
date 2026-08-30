import { readdirSync, readFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const decorativeGlyph = /[\u25A0-\u27BF\u{1F300}-\u{1FAFF}]/u;
const extensions = new Set(['.css', '.html', '.ts', '.tsx']);

function sourceFiles(root: string): string[] {
  return readdirSync(root, { withFileTypes: true }).flatMap(entry => {
    const path = join(root, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return extensions.has(extname(path)) && !path.endsWith('.test.ts') ? [path] : [];
  });
}

describe('client icon policy', () => {
  it('contains no emoji or decorative geometric font glyphs in client source', () => {
    const roots = [resolve(import.meta.dirname, '..'), resolve(import.meta.dirname, '../../../../spikes/kerf-webawesome/src')];
    const violations = roots.flatMap(sourceFiles).filter(path => decorativeGlyph.test(readFileSync(path, 'utf8')));
    expect(violations).toEqual([]);
  });
});
