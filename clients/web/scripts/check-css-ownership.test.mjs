import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  applyAllowlist,
  blockOf,
  buildOwnership,
  checkWorkspace,
  classesIn,
  findViolations,
  formatReport,
  moduleFacts,
  ownClassesIn,
  parseSelectorList,
  SHELL_SCOPES,
  typeOf,
} from './check-css-ownership.mjs';

const workspace = resolve(import.meta.dirname, '..');

// A miniature workspace: `files` maps workspace-relative paths to sources.
function scan(files, shellScopes = {}) {
  const entries = Object.entries(files).map(([path, source]) => ({ path, source }));
  return findViolations(
    buildOwnership({
      stylesheets: entries.filter(({ path }) => path.endsWith('.css')),
      modules: entries.filter(({ path }) => !path.endsWith('.css')),
      shellScopes,
    }),
  );
}
const selectors = (violations, kind) =>
  violations.filter((violation) => !kind || violation.kind === kind).map((violation) => violation.selector);

// Two components with their own stylesheets; `Card` renders `Badge` (with a LucideIcon) below its body.
const badge = {
  'src/components/badge.tsx': `import './badge.css';
export function Badge() { return <span class="badge"><b class="badge__dot" /><em>new</em></span>; }`,
  'src/components/badge.css': '.badge { display: inline-flex; } .badge__dot { width: 8px; }',
};
const card = (css, tsx = '') => ({
  ...badge,
  'src/components/card.tsx': `import './card.css';
import { Badge } from './badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
export function Card() {
  return (
    <article class="card">
      <header class="card__header"><LucideIcon name="star" /><h2>Title</h2></header>
      <div class="card__body"><p>Text</p><Badge /></div>
    </article>
  );
}
${tsx}`,
  'src/components/card.css': css,
});

describe('selector parsing', () => {
  it('splits lists and compounds, keeping functional arguments and strings intact', () => {
    const [first, second] = parseSelectorList(`.a > .b:has(.c, .d) svg,
      .e[data-x='a, b'] + .f`);
    expect(first.text).toBe('.a > .b:has(.c, .d) svg');
    expect(first.compounds).toEqual([
      { combinator: '', compound: '.a' },
      { combinator: '>', compound: '.b:has(.c, .d)' },
      { combinator: ' ', compound: 'svg' },
    ]);
    expect(second.compounds.map(({ combinator }) => combinator)).toEqual(['', '+']);
    expect(second.text).toBe(".e[data-x='a, b'] + .f");
  });

  it('normalizes multi-line selectors so allowlist keys are stable', () => {
    expect(parseSelectorList('.a:has(\n  .b\n)\n  .c')[0].text).toBe('.a:has(.b) .c');
  });

  it('reads classes, own classes, types, and BEM blocks', () => {
    expect(classesIn('.a.b--x:not(.c__d)')).toEqual(['a', 'b--x', 'c__d']);
    expect(ownClassesIn('.a:not(.c__d)')).toEqual(['a']);
    expect(typeOf('svg:first-child')).toBe('svg');
    expect(typeOf('wa-select::part(combobox)')).toBe('wa-select');
    expect(typeOf('[name="x"]')).toBe('');
    expect(blockOf('ticket-row__title--muted')).toBe('ticket-row');
  });
});

describe('module facts', () => {
  it('collects literal classes, intrinsic tags, imports, declared components, and projected JSX', () => {
    const facts = moduleFacts(
      'src/components/x.tsx',
      `import './x.css';
const Local = () => <i />;
export function X(p: { on: boolean }) {
  const html = '<table class="x__raw"></table>';
  document.createElement('canvas');
  return <Row className={\`x__row \${p.on ? 'x__row--on' : ''}\`} trailing={<small />}><span class="x__copy" /></Row>;
}`,
    );
    expect(facts.cssImports).toEqual(['./x.css']);
    expect([...facts.classTokens]).toEqual(expect.arrayContaining(['x__row', 'x__row--on', 'x__copy', 'x__raw']));
    expect([...facts.tags]).toEqual(expect.arrayContaining(['i', 'span', 'table', 'canvas', 'small']));
    expect(facts.declares).toEqual(new Set(['Local', 'X']));
    const row = facts.elements.find((element) => element.name === 'Row');
    expect(row.component).toBe(true);
    expect(row.children.map((child) => child.name).sort()).toEqual(['small', 'span']);
  });
});

describe('ownership detection', () => {
  it('accepts a component styling its own blocks, its own raw elements, and its own icons', () => {
    expect(
      scan(
        card(`.card { display: grid; }
.card__header > h2, .card__header svg { margin: 0; }
.card__body > p { margin: 0; }
.card:hover .card__header { color: red; }`),
      ),
    ).toEqual([]);
  });

  it('flags Kerf classes and [data-component] selectors', () => {
    const found = scan(card('.card .kui-button { color: red; } .card [data-component="toolbar"] { gap: 0; }'));
    expect(selectors(found, 'kerf')).toEqual(['.card .kui-button', '.card [data-component="toolbar"]']);
  });

  it("flags another component's class block anywhere in the selector, including :has()", () => {
    const found = scan(
      card('.card .badge { color: red; } .card__body:has(.badge__dot) { gap: 0; } .card.badge--wide { x: 1; }'),
    );
    expect(selectors(found, 'foreign-class')).toEqual([
      '.card .badge',
      '.card__body:has(.badge__dot)',
      '.card.badge--wide',
    ]);
    expect(found[0].detail).toBe('.badge belongs to src/components/badge.tsx');
  });

  it('attributes blocks by rendering module, not file name (block differs from the file name)', () => {
    const files = {
      'src/components/row.tsx': `import './row.css';\nexport const Row = () => <li class="list-row"><b class="list-row__title" /></li>;`,
      'src/components/row.css': '.list-row { display: flex; }',
      'src/components/list.tsx': `import './list.css';\nexport const List = () => <ul class="list"><Row /></ul>;`,
      'src/components/list.css': '.list { margin: 0; } .list .list-row__title { color: red; }',
    };
    expect(selectors(scan(files), 'foreign-class')).toEqual(['.list .list-row__title']);
  });

  it('flags borrowed classes of a component without its own stylesheet', () => {
    const files = {
      'src/components/viewport.tsx': `export const Viewport = () => <div class="term-viewport" data-connection="ok" />;`,
      'src/components/dialog.tsx': `import './dialog.css';\nexport const Dialog = () => <section class="dialog"><Viewport /></section>;`,
      'src/components/dialog.css': '.dialog .term-viewport { inset: 0; } .dialog { padding: 8px; }',
    };
    const found = scan(files);
    expect(selectors(found, 'foreign-class')).toEqual(['.dialog .term-viewport']);
    expect(found[0].detail).toContain('src/components/viewport.tsx');
  });

  it('resolves a block several modules render to the stylesheet that roots it', () => {
    const files = {
      'src/components/tile.tsx': `import './tile.css';\nexport const Tile = () => <div class="tile"><div class="tile__frame" /></div>;`,
      'src/components/tile.css': '.tile__frame { inset: 0; }',
      'src/components/preview.tsx': `import './preview.css';\nexport const Preview = () => <div class="preview"><div class="tile__frame" /></div>;`,
      'src/components/preview.css': '.preview .tile__frame { inset: 4px; }',
    };
    expect(selectors(scan(files))).toEqual(['.preview .tile__frame']);
  });

  it('flags element descendants of a hook class on another component root, even projected content', () => {
    const files = card(
      '.card__tab i { width: 4px; } .card__tab { flex: none; }',
      `export const Tabs = () => <AppTab className="card__tab" trailing={<i />} />;`,
    );
    const found = scan(files);
    expect(selectors(found, 'hook-descendant')).toEqual(['.card__tab i']);
    expect(found[0].detail).toContain("hook class on <AppTab>'s root");
  });

  it('flags descendant element selectors that also reach inside a child component (svg sizing)', () => {
    const found = scan(
      card('.card__body em { color: red; } .card svg { width: 16px; } .card__body svg { width: 8px; }'),
    );
    expect(selectors(found, 'foreign-element')).toEqual(['.card svg', '.card__body em', '.card__body svg']);
    expect(found.find(({ selector }) => selector === '.card__body em').detail).toContain('also reaches inside <Badge>');
  });

  it('flags a child-combinator element the component does not author (a child component root)', () => {
    const files = card(
      '.card__spinner > svg { width: 8px; } .card__own > svg { width: 8px; }',
      `export const S = () => <span class="card__spinner"><LoadingSpinner /></span>;
export const O = () => <span class="card__own"><LucideIcon name="x" /></span>;`,
    );
    expect(selectors(scan(files), 'foreign-element')).toEqual(['.card__spinner > svg']);
  });

  it("treats the component's own local components as its markup, not foreign", () => {
    const files = {
      'src/components/table.tsx': `import './table.css';
function BandRow() { return <tr><td /></tr>; }
export const Table = () => <table class="table"><tbody><BandRow /></tbody></table>;`,
      'src/components/table.css': '.table td { padding: 0; }',
    };
    expect(scan(files)).toEqual([]);
  });

  it('judges classed elements built outside JSX by what the module renders', () => {
    const files = {
      'src/components/raw.tsx': `import './raw.css';
export function mount(host: HTMLElement) { host.innerHTML = '<div class="raw"><button>x</button></div>'; }`,
      'src/components/raw.css': '.raw button { cursor: pointer; } .raw video { width: 100%; }',
    };
    expect(selectors(scan(files), 'foreign-element')).toEqual(['.raw video']);
  });

  it('ignores the universal selector, document elements, and sibling chains', () => {
    const files = card(
      'html.card--busy body { cursor: wait; } .card__body * { min-width: 0; } .card__body > p + p { margin: 0; }',
    );
    expect(scan(files)).toEqual([]);
  });

  it('gives shell stylesheets the style-less modules in their scope and passes owners through @import', () => {
    const files = {
      'src/ux/main.tsx': `import './style.css';\nexport const Main = () => <main class="stage"><Demo /></main>;`,
      'src/ux/demo.tsx': `export const Demo = () => <section class="demo-stage"><h2>x</h2></section>;`,
      'src/ux/own.tsx': `import './own.css';\nexport const Own = () => <section class="own-demo" />;`,
      'src/ux/own.css': '.own-demo { gap: 0; }',
      'src/ux/style.css': `@import './shared.css';\n.stage { gap: 0; } .demo-stage > h2 { margin: 0; } .own-demo { color: red; }`,
      'src/ux/shared.css': '.demo-stage { padding: 0; }',
    };
    expect(selectors(scan(files))).toEqual(['.demo-stage', '.demo-stage > h2', '.own-demo']);
    expect(selectors(scan(files, { 'src/ux/style.css': ['src/ux/'] }))).toEqual(['.own-demo']);
    expect(() => scan(files, { 'src/ux/missing.css': ['src/ux/'] })).toThrow('does not exist');
  });

  it('ignores @keyframes steps', () => {
    expect(scan(card('@keyframes pulse { from { opacity: 0; } to { opacity: 1; } }'))).toEqual([]);
  });
});

describe('allowlist', () => {
  const violation = (selector, file = 'src/components/card.css') => ({
    file,
    line: 1,
    selector,
    kind: 'foreign-class',
    detail: 'x',
  });
  const entry = (selector, extra = {}) => ({
    file: 'src/components/card.css',
    selector,
    ticket: 'HS2-ABC123',
    reason: 'tracked',
    ...extra,
  });

  it('covers exactly the listed findings and reports uncovered ones', () => {
    const result = applyAllowlist([violation('.a'), violation('.b')], { entries: [entry('.a')] });
    expect(result.unexpected.map(({ selector }) => selector)).toEqual(['.b']);
    expect(result.stale).toEqual([]);
    expect(result.allowed).toBe(1);
  });

  it('fails an entry that no longer matches, or matches fewer findings than its count', () => {
    const result = applyAllowlist([violation('.a')], {
      entries: [entry('.a', { count: 2 }), entry('.gone')],
    });
    expect(result.stale.map(({ selector, matched }) => [selector, matched])).toEqual([
      ['.a', 1],
      ['.gone', 0],
    ]);
    expect(formatReport({ violations: [violation('.a')], ...result }).ok).toBe(false);
    expect(formatReport({ violations: [violation('.a')], ...result }).text).toContain('lower its count or delete it');
  });

  it('reports a new occurrence of an already allowlisted selector', () => {
    const result = applyAllowlist([violation('.a'), violation('.a')], { entries: [entry('.a')] });
    expect(result.unexpected).toHaveLength(1);
  });

  it('requires a ticket, a reason, a valid count, and unique entries', () => {
    const { problems } = applyAllowlist([], {
      entries: [
        { file: 'f.css', selector: '.a', reason: 'r' },
        { file: 'f.css', selector: '.b', ticket: 'TODO', reason: 'r' },
        { file: 'f.css', selector: '.c', ticket: 'KF-5X1TWD' },
        { file: 'f.css', selector: '.d', ticket: 'HS2-ABC123', reason: 'r', count: 1 },
        { file: 'f.css', selector: '.e', ticket: 'HS2-ABC123', reason: 'r' },
        { file: 'f.css', selector: '.e', ticket: 'HS2-ABC123', reason: 'r' },
        { selector: '.f', ticket: 'HS2-ABC123', reason: 'r' },
      ],
    });
    expect(problems).toEqual([
      'allowlist entry f.css .a must name its tracking ticket (HS2-XXXXXX or KF-XXXXXX)',
      'allowlist entry f.css .b must name its tracking ticket (HS2-XXXXXX or KF-XXXXXX)',
      'allowlist entry f.css .c must give a reason',
      'allowlist entry f.css .d count must be an integer above 1 (omit it for 1)',
      'duplicate allowlist entry f.css .e',
      expect.stringContaining('missing file or selector'),
    ]);
  });

  it('passes a fully covered report', () => {
    const result = applyAllowlist([violation('.a')], { entries: [entry('.a')] });
    expect(formatReport({ violations: [violation('.a')], ...result })).toEqual({
      ok: true,
      text: 'CSS ownership clean: 1 known cross-component selector(s), all covered by 1 ticketed allowlist entry.',
    });
  });
});

describe('the clients/web workspace', () => {
  let temporary;
  afterEach(() => temporary && rmSync(temporary, { recursive: true, force: true }));

  it('is clean against its checked-in allowlist, and every entry names its ticket', () => {
    const report = checkWorkspace(workspace);
    expect(formatReport(report).text).toMatch(/^CSS ownership clean/);
    const allowlist = JSON.parse(readFileSync(join(workspace, 'css-ownership-allowlist.json'), 'utf8'));
    for (const item of allowlist.entries) expect(item.ticket).toMatch(/^(?:HS2|KF)-[0-9A-Z]{6}$/);
  });

  it('fails when a new cross-component selector is added', () => {
    temporary = mkdtempSync(join(tmpdir(), 'hotsheet-css-ownership-'));
    const allowlist = join(temporary, 'allowlist.json');
    const entries = JSON.parse(readFileSync(join(workspace, 'css-ownership-allowlist.json'), 'utf8')).entries;
    writeFileSync(allowlist, JSON.stringify({ entries: entries.slice(1) }));
    const report = checkWorkspace(workspace, allowlist);
    expect(report.unexpected.map(({ file, selector }) => ({ file, selector }))).toEqual([
      { file: entries[0].file, selector: entries[0].selector },
    ]);
    expect(formatReport(report).ok).toBe(false);
  });

  // HS2-148B5C's borrowed terminal-preview classes and HS2-M2W2DP's `.ai-conversation__activity svg` are
  // fixed; the fixture tests above keep both shapes covered.
  it('covers the reported missed cases (HS2-7ZGYJY)', () => {
    const found = checkWorkspace(workspace).violations.map(({ file, selector }) => `${file} ${selector}`);
    expect(found).toEqual(
      expect.arrayContaining([
        'src/components/terminal-drawer.css .terminal-drawer__rail svg',
        'src/components/terminal-drawer.css .terminal-drawer__rail .terminal-tab i',
      ]),
    );
  });

  it('keeps UX demo stage styles out of the demoed components (HS2-TV78E1)', () => {
    const demo = checkWorkspace(workspace).violations.filter(({ file }) => file === 'src/ux-demo/style.css');
    expect(demo.filter(({ kind }) => kind === 'foreign-element')).toEqual([]);
    const allowlist = JSON.parse(readFileSync(join(workspace, 'css-ownership-allowlist.json'), 'utf8'));
    expect(allowlist.entries.filter(({ ticket }) => ticket === 'HS2-TV78E1')).toEqual([]);
  });

  it('keeps every shell scope pointing at a real stylesheet', () => {
    for (const path of Object.keys(SHELL_SCOPES)) expect(() => readFileSync(join(workspace, path))).not.toThrow();
  });

  it('runs as part of npm run lint', () => {
    const { scripts } = JSON.parse(readFileSync(join(workspace, 'package.json'), 'utf8'));
    expect(scripts['css:ownership']).toBe('node scripts/check-css-ownership.mjs');
    expect(scripts.lint.split('&&').map((step) => step.trim())).toContain('npm run css:ownership');
  });

  it('scans a fixture workspace end to end', () => {
    temporary = mkdtempSync(join(tmpdir(), 'hotsheet-css-ownership-'));
    const write = (path, source) => {
      mkdirSync(dirname(join(temporary, path)), { recursive: true });
      writeFileSync(join(temporary, path), source);
    };
    for (const path of Object.keys(SHELL_SCOPES)) write(path, '');
    write('src/components/a.tsx', 'import \'./a.css\';\nexport const A = () => <div class="a"><B /></div>;');
    write('src/components/a.css', '.a .b { color: red; }');
    write('src/components/b.tsx', 'import \'./b.css\';\nexport const B = () => <div class="b" />;');
    write('src/components/b.css', '.b { color: blue; }');
    write('css-ownership-allowlist.json', JSON.stringify({ entries: [] }));
    const report = checkWorkspace(temporary);
    expect(report.unexpected.map(({ file, selector }) => `${file} ${selector}`)).toEqual([
      'src/components/a.css .a .b',
    ]);
  });
});
