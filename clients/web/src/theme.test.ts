import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const sourceRoot = import.meta.dirname;
const themePath = resolve(sourceRoot, 'theme.css');
const productionCss = [
  resolve(sourceRoot, 'style.css'),
  ...readdirSync(resolve(sourceRoot, 'components'))
    .filter(file => file.endsWith('.css'))
    .map(file => resolve(sourceRoot, 'components', file)),
];
const auxiliaryClientCss = [
  resolve(sourceRoot, 'dev-review/dev-review.css'),
  resolve(sourceRoot, 'ux-demo/style.css'),
];
const clientCss = [...productionCss, ...auxiliaryClientCss];

function css(path: string): string {
  return readFileSync(path, 'utf8');
}

describe('shared client theme', () => {
  it('is loaded after Web Awesome by both production entry points', () => {
    for (const entry of ['main.tsx', 'ux-demo/main.tsx']) {
      const source = css(resolve(sourceRoot, entry));
      const expectedImport = entry === 'main.tsx' ? "import './theme.css';" : "import '../theme.css';";
      expect(source).toContain(expectedImport);
      expect(source.indexOf("import '@awesome.me/webawesome/dist/styles/webawesome.css';"))
        .toBeLessThan(source.indexOf(expectedImport));
    }
  });

  it('defines every required and referenced Hot Sheet semantic alias exactly once', () => {
    const allCss = [themePath, ...clientCss].map(css).join('\n');
    const definitions = [...allCss.matchAll(/(--hs-[\w-]+)\s*:/g)].map(match => match[1]);
    const references = [...allCss.matchAll(/var\((--hs-[\w-]+)\)/g)].map(match => match[1]);
    const required = ['--hs-shell-divider', '--hs-ticket-state-needs-review', '--hs-ticket-state-up-next'];

    expect(new Set(definitions)).toEqual(new Set(required));
    expect(definitions).toHaveLength(required.length);
    expect(new Set(references)).toEqual(new Set(required));
  });

  it('keeps every client-owned stylesheet on the shared semantic color palette', () => {
    for (const path of productionCss) {
      const source = css(path);
      expect(source, `${path} contains a color literal outside theme.css`)
        .not.toMatch(/#[\da-f]{3,8}\b|(?:rgb|hsl)a?\([^)]*\)/i);
    }
    for (const path of auxiliaryClientCss) {
      expect(css(path), `${path} contains a color literal outside theme.css`)
        .not.toMatch(/#[\da-f]{3,8}\b|(?:rgb|hsl)a?\([^)]*\)/i);
    }
    for (const token of [
      '--wa-color-surface-default', '--wa-color-surface-lowered', '--wa-color-overlay-modal',
      '--wa-color-text-normal', '--wa-color-text-quiet', '--wa-color-brand-fill-quiet',
      '--wa-color-brand-border-quiet', '--wa-color-success-fill-quiet',
      '--wa-color-warning-fill-quiet', '--wa-color-danger-fill-quiet',
      '--wa-color-neutral-fill-quiet', '--wa-color-neutral-border-normal',
      '--wa-color-neutral-on-quiet', '--wa-color-focus', '--wa-shadow-l',
    ]) {
      expect(css(themePath), `theme.css does not define ${token}`).toContain(`${token}:`);
      expect(clientCss.map(css).join('\n'), `client CSS does not consume ${token}`).toContain(`var(${token})`);
    }
    const themeLiterals = [...css(themePath).matchAll(/#[\da-f]{3,8}\b|(?:rgb|hsl)a?\([^)]*\)/gi)]
      .map(match => match[0].toLocaleLowerCase());
    expect(new Set(themeLiterals).size, 'theme.css repeats a palette literal instead of aliasing its semantic source')
      .toBe(themeLiterals.length);
  });

  it('uses the Web Awesome typography scale instead of one-off font sizes', () => {
    const typeToken = 'var\\(--wa-font-size-(?:3xs|2xs|xs|s|m|l|xl|2xl|3xl|4xl|5xl|smaller|larger)\\)';
    const allowedSize = new RegExp(`^(?:${typeToken}|clamp\\(${typeToken}, \\d*\\.?\\d+vw, ${typeToken}\\))$`);
    for (const path of clientCss) {
      const source = css(path);
      for (const match of source.matchAll(/font-size:\s*([^;}]+)/g)) {
        const value = match[1].trim().replace(/\s*!important$/, '');
        expect(value, `${path} contains arbitrary font-size ${match[1].trim()}`)
          .toMatch(allowedSize);
      }
      expect(source, `${path} contains an arbitrary size in a font shorthand`)
        .not.toMatch(/font:\s*(?:\d+\s+)?\d*\.?\d+(?:px|rem|em)\b/);
    }
    expect(css(resolve(sourceRoot, 'components/workspace-header.css')))
      .toMatch(/view-mode-switcher__badge[^}]*font-size: var\(--wa-font-size-3xs\)/);
  });
});
