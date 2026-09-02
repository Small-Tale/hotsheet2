import { existsSync, readdirSync,readFileSync } from 'node:fs';
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
    const allCss = [themePath, ...productionCss].map(css).join('\n');
    const definitions = [...allCss.matchAll(/(--hs-[\w-]+)\s*:/g)].map(match => match[1]);
    const references = [...allCss.matchAll(/var\((--hs-[\w-]+)\)/g)].map(match => match[1]);
    const required = ['--hs-ticket-state-needs-review', '--hs-ticket-state-up-next'];

    expect(new Set(definitions)).toEqual(new Set(required));
    expect(definitions).toHaveLength(required.length);
    expect(new Set(references)).toEqual(new Set(required));
  });

  it('keeps generic semantics on Web Awesome tokens and retired literals out of production CSS', () => {
    const migratedLiterals = [
      '#fff', '#3b82f6', '#1d4ed8', '#2563eb', '#dbeafe', '#eceef2', '#d7dae1',
      '#e3e4e8', '#eff6ff', '#eef4ff', '#eaf2ff', '#f0c86a', '#fff9e8', '#8a5a08',
      '#b42318', '#30323a', '#25262b',
    ];

    for (const path of productionCss) {
      const source = css(path);
      for (const literal of migratedLiterals) {
        expect(source, `${path} still contains migrated literal ${literal}`)
          .not.toMatch(new RegExp(`${literal}(?![0-9a-f])`, 'i'));
      }
    }
    expect(css(themePath)).toContain('--hs-ticket-state-needs-review: #8b5cf6');
    expect(css(themePath)).toContain('--hs-ticket-state-up-next: #eab308');
    expect(css(themePath)).toContain('--wa-color-surface-default: #fff');
    expect(css(themePath)).toContain('--wa-color-focus: #3b82f6');
    expect(existsSync(resolve(sourceRoot, 'components/ticket-state-colors.css'))).toBe(false);

    const allProductionCss = productionCss.map(css).join('\n');
    for (const token of [
      '--wa-color-surface-default', '--wa-color-text-normal', '--wa-color-brand-on-quiet',
      '--wa-color-warning-fill-quiet', '--wa-color-danger-on-quiet', '--wa-space-xs',
      '--wa-space-s', '--wa-border-radius-pill', '--wa-focus-ring', '--wa-shadow-l',
    ]) expect(allProductionCss).toContain(`var(${token})`);
    expect(allProductionCss).not.toMatch(/gap:\s*\.(?:5|75)rem;/);
  });
});
