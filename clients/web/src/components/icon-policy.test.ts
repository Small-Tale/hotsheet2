import { readdirSync, readFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const decorativeGlyph = /[\u25A0-\u27BF\u{1F300}-\u{1FAFF}]/u;
const extensions = new Set(['.css', '.html', '.ts', '.tsx']);

function sourceFiles(root: string): string[] {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return extensions.has(extname(path)) && !path.endsWith('.test.ts') ? [path] : [];
  });
}

describe('client icon policy', () => {
  it('contains no emoji or decorative geometric font glyphs in client source', () => {
    const roots = [
      resolve(import.meta.dirname, '..'),
      resolve(import.meta.dirname, '../../../../spikes/kerf-webawesome/src'),
    ];
    const violations = roots.flatMap(sourceFiles).filter((path) => decorativeGlyph.test(readFileSync(path, 'utf8')));
    expect(violations).toEqual([]);
  });
});

/**
 * Classes of app-owned `span`/`div` wrappers whose only child is one `LucideIcon`. Since Kerf
 * 5.0.0-beta.75 such an icon takes its color through the `color` prop (HS2-GQ57YW), so a CSS
 * `color` on a selector ending in one of these classes is a tint that belongs on the icon instead.
 */
function singleIconWrapperClasses(sources: readonly string[]): Map<string, string> {
  const wrappers = new Map<string, string>();
  const wrapper = /<(span|div)\b([^>]*?)>\s*<LucideIcon\b(?:(?!\/>)[\s\S])*?\/>\s*<\/\1>/g;
  for (const path of sources.filter((source) => source.endsWith('.tsx'))) {
    for (const match of readFileSync(path, 'utf8').matchAll(wrapper)) {
      const attributes = match[2];
      // A wrapper with a role or tab stop is an interactive control; its color states are its own.
      if (/\b(?:role|tabIndex)=/.test(attributes)) continue;
      const classes = /\bclass="([^"]+)"/.exec(attributes)?.[1] ?? '';
      for (const name of classes.split(/\s+/)) if (name) wrappers.set(name, path);
    }
  }
  return wrappers;
}

describe('LucideIcon color ownership (HS2-GQ57YW)', () => {
  it('finds the single-icon wrapper shape it guards', () => {
    const fixture = resolve(import.meta.dirname, 'permission-request-card.tsx');
    expect(singleIconWrapperClasses([fixture]).has('permission-request-card__summary-icon')).toBe(true);
  });

  it('colors single-icon wrappers through the LucideIcon color prop, not wrapper CSS', () => {
    const sources = sourceFiles(resolve(import.meta.dirname, '..'));
    const wrappers = singleIconWrapperClasses(sources);
    const violations: string[] = [];
    for (const path of sources.filter((source) => source.endsWith('.css'))) {
      const css = readFileSync(path, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
      for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
        if (!/(?:^|[;\s])color\s*:/.test(body)) continue;
        for (const selector of selectors.split(',')) {
          const last = /\.([\w-]+)(?:\[[^\]]*\]|:[\w-]+(?:\([^)]*\))?)*\s*$/.exec(selector.trim())?.[1];
          if (last && wrappers.has(last)) violations.push(`${path.split('/src/')[1]}: ${selector.trim()}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
