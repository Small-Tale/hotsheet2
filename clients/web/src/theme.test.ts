import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const sourceRoot = import.meta.dirname;
const tokenPath = resolve(sourceRoot, 'hot-sheet-tokens.css');
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
const kerfSpacingMigratedComponents = [
  'ai-tool-settings.css', 'app-error.css', 'attachment-context-menu.css', 'bulk-ticket-dialog.css',
  'command-navigation.css', 'command-run-dialog.css', 'command-settings-editor.css', 'connection-details-dialog.css', 'conversation-export-dialog.css', 'corrupt-ticket-row.css', 'drive-options-menu.css', 'flow-back-button.css',
  'keyboard-settings.css', 'manual-model-dialog.css', 'markdown-preview.css', 'not-working-dialog.css', 'pending-attachment-picker.css',
  'project-tab-bar.css', 'repository-setup.css', 'saved-view-dialog.css', 'terminal-drawer.css', 'terminal-rename-dialog.css', 'ticket-duplicate-backlinks.css',
  'ticket-board-column.css', 'ticket-close-dialog.css', 'ticket-field-conflict.css', 'ticket-inspector-skeleton.css', 'ticket-link-choice-dialog.css', 'ticket-list.css',
  'ticket-inspector-panel.css', 'ticket-row.css', 'trash-settings.css', 'view-navigation.css',
].map(file => resolve(sourceRoot, 'components', file));

function css(path: string): string {
  return readFileSync(path, 'utf8');
}

describe('shared client theme', () => {
  it('loads Kerf Web Awesome theming before product-only tokens in both entry points', () => {
    for (const entry of ['main.tsx', 'ux-demo/main.tsx']) {
      const source = css(resolve(sourceRoot, entry));
      const expectedImport = entry === 'main.tsx' ? "import './hot-sheet-tokens.css';" : "import '../hot-sheet-tokens.css';";
      expect(source).toContain(expectedImport);
      expect(source.indexOf("import '@kerfjs/ui/webawesome.css';"))
        .toBeLessThan(source.indexOf(expectedImport));
      expect(source).not.toContain("import '@awesome.me/webawesome/dist/styles/webawesome.css';");
    }
  });

  it('defines every required and referenced Hot Sheet semantic alias exactly once', () => {
    const allCss = [tokenPath, ...clientCss].map(css).join('\n');
    const definitions = [...allCss.matchAll(/(--hs-[\w-]+)\s*:/g)].map(match => match[1]);
    const references = [...allCss.matchAll(/var\((--hs-[\w-]+)\)/g)].map(match => match[1]);
    const required = ['--hs-shell-divider', '--hs-terminal-background', '--hs-ticket-state-needs-review', '--hs-ticket-state-up-next', '--hs-ticket-state-up-next-on', '--hs-priority-high', '--hs-priority-default', '--hs-reader-font-size-s', '--hs-reader-font-size-m', '--hs-reader-font-size-l'];
    const cssReferences = required.filter(token => !token.startsWith('--hs-priority-'));

    expect(new Set(definitions)).toEqual(new Set(required));
    expect(definitions).toHaveLength(required.length);
    expect(new Set(references)).toEqual(new Set(cssReferences));
  });

  it('keeps every client-owned stylesheet on the shared semantic color palette', () => {
    for (const path of productionCss) {
      const source = css(path);
      expect(source, `${path} contains a color literal outside hot-sheet-tokens.css`)
        .not.toMatch(/#[\da-f]{3,8}\b|(?:rgb|hsl)a?\([^)]*\)/i);
    }
    for (const path of auxiliaryClientCss) {
      expect(css(path), `${path} contains a color literal outside hot-sheet-tokens.css`)
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
      expect(clientCss.map(css).join('\n'), `client CSS does not consume ${token}`).toContain(`var(${token})`);
    }
    const themeLiterals = [...css(tokenPath).matchAll(/#[\da-f]{3,8}\b|(?:rgb|hsl)a?\([^)]*\)/gi)]
      .map(match => match[0].toLocaleLowerCase());
    expect(new Set(themeLiterals).size, 'hot-sheet-tokens.css repeats a palette literal instead of aliasing its semantic source')
      .toBe(themeLiterals.length);
  });

  it('leaves generic palette ownership to Kerf and defines only product semantics locally', () => {
    const source = css(tokenPath);
    for (const declaration of [
      '--hs-priority-high: #ff8d28',
      '--hs-priority-default: #8e8e93',
      '--hs-ticket-state-needs-review: #cb30e0',
    ]) expect(source).toContain(declaration);
    expect(source).toContain('--hs-shell-divider: var(--wa-color-neutral-border-normal)');
    expect(source).not.toMatch(/--wa-[\w-]+\s*:/);
  });

  it('mirrors the kerf --kui-space-* spacing scale so components can author against it (HS2-4Y6SM9)', () => {
    const source = css(tokenPath);
    // Identical to kerf's foundation.css so the canonical scale resolves app-wide without importing
    // foundation wholesale (which also carries color-scheme/color/font foundations).
    for (const declaration of [
      '--kui-space-none: 0',
      '--kui-space-2xs: var(--wa-space-2xs, 0.25rem)',
      '--kui-space-xs: var(--wa-space-xs, 0.5rem)',
      '--kui-space-s: var(--wa-space-s, 0.75rem)',
      '--kui-space-m: var(--wa-space-m, 1rem)',
      '--kui-space-l: var(--wa-space-l, 1.5rem)',
      '--kui-space-xl: var(--wa-space-xl, 2rem)',
    ]) expect(source).toContain(declaration);
    // Components consume the scale for real semantic separation.
    const consumers = clientCss.map(css).join('\n');
    expect(consumers).toContain('var(--kui-space-xs)');
    expect(consumers).toContain('var(--kui-space-m)');
    for (const path of kerfSpacingMigratedComponents) {
      expect(css(path), `${path} regressed to Web Awesome spacing instead of the Kerf semantic scale`)
        .not.toContain('--wa-space-');
    }
  });

  it('uses the Web Awesome typography scale instead of one-off font sizes', () => {
    const typeToken = 'var\\(--wa-font-size-(?:3xs|2xs|xs|s|m|l|xl|2xl|3xl|4xl|5xl|smaller|larger)\\)';
    const semanticTypeToken = 'var\\(--hs-reader-font-size-(?:xs|s|m|l)\\)';
    const allowedSize = new RegExp(`^(?:${typeToken}|${semanticTypeToken}|clamp\\(${typeToken}, \\d*\\.?\\d+vw, ${typeToken}\\))$`);
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
    expect(css(tokenPath)).toContain('--hs-reader-font-size-s: calc(1.5 * var(--wa-font-size-s))');
  });
});
