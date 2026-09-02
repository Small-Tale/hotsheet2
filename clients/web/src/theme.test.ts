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
      '#b42318', '#30323a', '#25262b', '#f8f9fb', '#f7f8fa', '#f1f3f6', '#f1f2f5',
      '#eef0f4', '#e8ebf0', '#e1e3e8', '#dfe1e6', '#e5e7eb', '#d9dce3', '#d7d9df',
      '#d9d9de', '#c7cbd3', '#d4d8e0', '#b9c0cc', '#aeb4c0', '#aeb3bd', '#252833',
      '#4b5563', '#555762', '#59606d', '#596170', '#62646d', '#646771', '#656873',
      '#667085', '#666a75', '#686a73', '#6b7280', '#717581', '#747985', '#777a84',
      '#777b86', '#858893', '#8a8d96',
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
    expect(css(themePath)).toContain('--wa-color-surface-lowered: #f8f9fb');
    expect(css(themePath)).toContain('--wa-color-neutral-fill-quiet: #f1f3f6');
    expect(css(themePath)).toContain('--wa-color-neutral-border-quiet: #e8ebf0');
    expect(css(themePath)).toContain('--wa-color-neutral-border-normal: #d7dae1');
    expect(css(themePath)).toContain('--wa-color-neutral-border-loud: #b9c0cc');
    expect(css(themePath)).toContain('--wa-color-neutral-on-quiet: #656873');
    expect(css(themePath)).toContain('--wa-color-neutral-on-normal: #252833');
    expect(css(themePath)).toContain('--wa-color-focus: #3b82f6');
    expect(existsSync(resolve(sourceRoot, 'components/ticket-state-colors.css'))).toBe(false);

    const allProductionCss = productionCss.map(css).join('\n');
    for (const token of [
      '--wa-color-surface-default', '--wa-color-text-normal', '--wa-color-brand-on-quiet',
      '--wa-color-warning-fill-quiet', '--wa-color-danger-on-quiet', '--wa-space-xs',
      '--wa-space-s', '--wa-border-radius-pill', '--wa-focus-ring', '--wa-shadow-l',
    ]) expect(allProductionCss).toContain(`var(${token})`);
    expect(allProductionCss).not.toMatch(/gap:\s*\.(?:5|75)rem;/);
    expect(css(resolve(sourceRoot, 'dev-review/dev-review.css'))).not.toContain('#f8f9fb');
  });

  it('leaves repeated literals only for intentional domain states and geometry', () => {
    const occurrences = new Map<string, number>();
    for (const source of productionCss.map(css)) {
      for (const match of source.matchAll(/#[\da-f]{6}\b/gi)) {
        const literal = match[0].toLocaleLowerCase();
        occurrences.set(literal, (occurrences.get(literal) ?? 0) + 1);
      }
    }
    const repeated = [...occurrences].filter(([, count]) => count > 1).map(([literal]) => literal).sort();
    expect(repeated).toEqual([
      '#396342', '#60a5fa', '#681d18', '#76231e', '#79251f', '#8b909b', '#8c2822',
      '#8e302a', '#b52c25', '#b94b44', '#c33149', '#d99994', '#dc2626', '#feecec',
      '#fff0ee', '#fff7f6',
    ]);
  });
});
