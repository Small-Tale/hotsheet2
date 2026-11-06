// HS2-N7EY18, HS2-R1D0S5: which browser specs exercise a changed client module.
//
// Path keywords alone missed two escapes. A terminal-viewport-runtime change ran only
// terminal-feedback, not the specs that route the attach socket. A ticket-search-field change ran
// no providers.spec case that renders the field in its shipped parent. This index reads the source
// instead of a hand-kept map, so a new spec or module is covered without editing the gate:
//
// 1. Hooks: what a browser spec can observe of a module.
//    - The class tokens the module owns: literals prefixed by its file stem (the root-class
//      convention), and classes defined in its sibling stylesheet.
//    - The route shapes it builds or matches, such as `/terminals/${id}/attach` (terminals/*/attach).
// 2. Delegated helpers: a module also carries the hooks of the plain `.ts` client modules it
//    imports directly; terminal-viewport-runtime opens the attach socket that terminal-viewport.ts
//    names. Child `.tsx` components keep their own surfaces, so a parent does not inherit them.
// 3. Shipped parents: a module with no hooks walks up its importers to the nearest ancestors that
//    have hooks, and the specs reach it through them.
// 4. Specs: each spec is split into top-level test, describe and helper blocks. A hook in a test
//    selects that test; a hook in a helper selects the tests that call it; a hook in shared setup,
//    an imported test fixture, or a direct import of the module selects the whole file.
//
// Everything except exerciseIndexForRepo is pure.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';

const WEB = 'clients/web';
const SOURCE = /^clients\/web\/src\/.+\.(?:tsx?|css)$/;
const MODULE = /\.(?:tsx?)$/;
const isTest = (path) => /\.test\.tsx?$/.test(path);
/**
 * Composition roots import nearly everything, so walking through them would select every spec. A
 * module reached only through them keeps the gate's shared-infrastructure rules instead; demos are
 * not shipped parents (the demo spec still matches a component's own hooks).
 */
const COMPOSITION_ROOT = /^clients\/web\/src\/(?:app\/|ux-demo\/|main\.tsx$)/;
const escape = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const stemOf = (path) =>
  posix
    .basename(path)
    .replace(/\.(?:tsx?|css)$/, '')
    .toLowerCase();

/** A class token is multi-word kebab or BEM, so a one-word stem never matches prose. */
const CLASS_TOKEN = /[a-z][a-z0-9]*(?:(?:-{1,2}|__)[a-z0-9]+)+/g;
const isOwnedToken = (token, stem) =>
  !token.startsWith('kui-') && (token === stem || token.startsWith(`${stem}__`) || token.startsWith(`${stem}--`));

const DOM_QUERY =
  /(?:querySelector(?:All)?|closest|matches|classList\.(?:add|remove|toggle|contains|replace))\(\s*(['"`])((?:(?!\1).)*)\1/g;

/** `/a/<param>/b` where the param is a template hole, a capture group or a non-slash class. */
// The trailing segment is a lookahead so `/a/(x)/b/(y)/c` yields both a/*/b and b/*/c.
const ROUTE = /\/([a-z][a-z0-9-]*)\/(?:\$\{[^}\n]{1,80}\}|\(\[\^\/\??\]\+\)|\[\^\/\??\]\+)(?=\/([a-z][a-z0-9-]*))/g;

export function routeHooks(source) {
  const text = source.replaceAll('\\/', '/');
  return [...new Set([...text.matchAll(ROUTE)].map((match) => `${match[1]}/*/${match[2]}`))].sort();
}

export function stylesheetClasses(css) {
  return [...new Set([...css.matchAll(/\.([a-z][a-z0-9_-]*)/g)].map((match) => match[1]))]
    .filter((token) => !token.startsWith('kui-') && /[-_]/.test(token))
    .sort();
}

/**
 * The class and route hooks a module owns, from its source and its sibling stylesheet. With
 * `queried`, the classes it queries or toggles count too; only the changed module itself uses that,
 * because a large parent queries far more than the part a helper serves.
 */
export function ownHooks(path, source, siblingCss = '', { queried = false } = {}) {
  const stem = stemOf(path);
  const classes = new Set(stylesheetClasses(siblingCss));
  if (path.endsWith('.css')) for (const token of stylesheetClasses(source)) classes.add(token);
  else {
    for (const literal of source.matchAll(/'([^'\n]*)'|"([^"\n]*)"|`([^`]*)`/g))
      for (const token of (literal[1] ?? literal[2] ?? literal[3]).match(CLASS_TOKEN) ?? [])
        if (isOwnedToken(token, stem)) classes.add(token);
    // The elements a module queries or restyles are its surface too: terminal-viewport-runtime owns
    // no markup but drives every `.terminal-viewport`, which it recognizes by a BEM modifier.
    // Attribute names (`[data-layout-mode=…]`) are not surfaces; their quoted values are.
    if (queried)
      for (const query of source.matchAll(DOM_QUERY))
        for (const token of query[2].replace(/\[\s*[\w-]+\s*(?:[~|^$*]?=)?/g, '[').match(CLASS_TOKEN) ?? [])
          if (!token.startsWith('kui-') && !token.startsWith('data-')) {
            classes.add(token);
            classes.add(token.split(/__|--/)[0]);
          }
  }
  return { classes: [...classes].sort(), routes: path.endsWith('.css') ? [] : routeHooks(source) };
}

const hookCount = (hooks) => hooks.classes.length + hooks.routes.length;
const mergeHooks = (...all) => ({
  classes: [...new Set(all.flatMap((hooks) => hooks.classes))].sort(),
  routes: [...new Set(all.flatMap((hooks) => hooks.routes))].sort(),
});

// Static imports and re-exports plus dynamic import(); type-only imports never reach the browser.
// The same pattern as clients/web/src/browser-dependency-scan.ts (HS2-N9RD7X).
const IMPORT =
  /(?:^|[;\s])(?:import|export)\s+(?!type\s)(?:[\w*{}\s,$]+?\s+from\s+)?['"](\.{1,2}\/[^'"]+)['"]|\bimport\(\s*['"](\.{1,2}\/[^'"]+)['"]\s*\)/g;

/** Relative runtime imports of a module, resolved against the known file set. */
export function relativeImports(path, source, files) {
  const found = new Set();
  for (const match of source.matchAll(IMPORT)) {
    const base = posix.normalize(posix.join(posix.dirname(path), match[1] ?? match[2]));
    for (const candidate of [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`])
      if (files.has(candidate)) {
        found.add(candidate);
        break;
      }
  }
  return [...found].sort();
}

/** Matchers for one hook set, applied to spec text with escaped slashes normalized. */
function hookMatchers(hooks) {
  return [
    ...hooks.classes.map((token) => ({ hook: token, pattern: new RegExp(`(?<![\\w-])${escape(token)}(?![\\w-])`) })),
    ...hooks.routes.map((route) => {
      const [a, b] = route.split('/*/');
      return { hook: route, pattern: new RegExp(`/${escape(a)}/\\S{1,60}?/${escape(b)}(?![\\w-])`) };
    }),
  ];
}

/**
 * Top-level blocks of a spec: `test`/`test.describe` units (with their grep titles), named helpers,
 * and shared statements. A block runs from one column-zero statement to the next.
 */
export function specBlocks(source) {
  const lines = source.split('\n');
  const blocks = [];
  let current = null;
  let pendingTitle = false;
  for (const line of lines) {
    const starts = /^[A-Za-z]/.test(line) && !/^(?:else|catch|finally)\b/.test(line);
    if (starts || !current) {
      current = { lines: [], titles: [], unstable: false, helper: null, kind: 'shared' };
      blocks.push(current);
      const helper =
        /^(?:export )?(?:async )?function\s+([A-Za-z_$][\w$]*)|^(?:export )?(?:const|let)\s+([A-Za-z_$][\w$]*)\s*=/.exec(
          line,
        );
      if (helper) {
        current.kind = 'helper';
        current.helper = helper[1] ?? helper[2];
      }
    }
    current.lines.push(line);
    if (current.kind === 'helper') continue;
    // Prettier may wrap a long title onto the line after `test(`.
    if (pendingTitle) {
      pendingTitle = false;
      const wrapped = /^\s*'((?:\\.|[^'\\])*)'/.exec(line);
      if (wrapped) {
        current.titles.push(wrapped[1].replace(/\\(.)/g, '$1'));
        continue;
      }
      current.unstable = true;
    }
    const quoted = /^\s*test(?:\.describe)?(?:\.(?:only|skip|fixme|serial))?\(\s*'((?:\\.|[^'\\])*)'/.exec(line);
    const template = /^\s*test(?:\.describe)?(?:\.(?:only|skip|fixme|serial))?\(\s*`((?:\\.|[^`\\$]|\$(?!\{))*)/.exec(
      line,
    );
    if (quoted) current.titles.push(quoted[1].replace(/\\(.)/g, '$1'));
    else if (template && template[1].length >= 12) current.titles.push(template[1].replace(/\\(.)/g, '$1'));
    else if (template) current.unstable = true;
    if (/^\s*test(?:\.describe)?(?:\.(?:only|skip|fixme|serial))?\(\s*$/.test(line)) pendingTitle = true;
    if (quoted || template || pendingTitle) current.kind = 'test';
  }
  return blocks
    .map((block) => ({ ...block, text: block.lines.join('\n').replaceAll('\\/', '/') }))
    .map(({ lines: _lines, ...block }) => block);
}

/**
 * Which tests of one spec exercise the hooks. Returns `{ whole, titles, hits }`: `whole` when shared
 * setup or an untitled test reaches a hook, otherwise the grep titles of the reached tests.
 */
export function exercisedTests(source, matchers) {
  if (!matchers.length) return { whole: false, titles: [], hits: [] };
  const blocks = specBlocks(source);
  const hits = new Set();
  const reached = new Set();
  for (const [index, block] of blocks.entries())
    for (const { hook, pattern } of matchers)
      if (pattern.test(block.text)) {
        hits.add(hook);
        reached.add(index);
      }
  // A reached helper reaches every block that calls it, transitively.
  const helpers = new Set();
  for (let grew = true; grew;) {
    grew = false;
    for (const index of reached) {
      const name = blocks[index].helper;
      if (name && !helpers.has(name)) {
        helpers.add(name);
        const call = new RegExp(`(?<![\\w$.])${escape(name)}\\b`);
        for (const [other, block] of blocks.entries())
          if (!reached.has(other) && block.helper !== name && call.test(block.text)) {
            reached.add(other);
            grew = true;
          }
      }
    }
  }
  const titles = new Set();
  let whole = false;
  for (const index of reached) {
    const block = blocks[index];
    if (block.kind === 'helper') continue;
    if (block.kind === 'shared' || block.unstable || !block.titles.length) whole = true;
    for (const title of block.titles) titles.add(title);
  }
  return { whole, titles: whole ? [] : [...titles].sort(), hits: [...hits].sort() };
}

const UX_DEMO = /^clients\/web\/src\/ux-demo\//;
const DEMO_SPEC = 'tests/ux-demo.spec.ts';
/** The catalog aggregators import every demo; walking through them would reach every entry. */
const DEMO_AGGREGATOR = /^clients\/web\/src\/ux-demo\/(?:registry|catalog|main)\.tsx?$/;

/** The catalog entry ids a demo file declares in `demoModule.entries` (one indentation level). */
export function demoEntryIds(source) {
  const start = source.search(/export const demoModule\b[^=]*=\s*\{/);
  if (start < 0) return [];
  const body = source.slice(start);
  const open = /\n(\s*)entries:\s*\{\n(\s+)/.exec(body);
  if (!open) return [];
  const indent = open[2];
  const ids = [];
  for (const line of body.slice(open.index + open[0].length - indent.length).split('\n')) {
    if (line.trim() && !line.startsWith(indent)) break;
    const key = new RegExp(`^${indent}(?:'([a-z][a-z0-9-]*)'|([a-z][a-zA-Z0-9]*)):`).exec(line);
    if (key) ids.push(key[1] ?? key[2]);
  }
  return ids;
}

/**
 * Entries that render a changed demo module: its own, plus those of every demo file that imports it,
 * transitively. A fixture shared across demos (READER_NOTES, HS2-EWF0FT) thereby reaches each demo
 * that renders it, not the whole demo spec.
 */
export function demoEntriesReaching(module, sources, importers) {
  const seen = new Set([module]);
  const queue = [module];
  const ids = new Set();
  while (queue.length) {
    const path = queue.shift();
    for (const id of demoEntryIds(sources.get(path) ?? '')) ids.add(id);
    for (const parent of importers.get(path) ?? [])
      if (!seen.has(parent) && UX_DEMO.test(parent) && !DEMO_AGGREGATOR.test(parent) && !isTest(parent)) {
        seen.add(parent);
        queue.push(parent);
      }
  }
  return [...ids].sort();
}

/** A demo entry is opened as `component=<id>`; a multi-word id may also be listed as a literal. */
function demoMatchers(ids) {
  return ids.map((id) => ({
    hook: `component=${id}`,
    pattern: id.includes('-')
      ? new RegExp(`component=${escape(id)}(?![\\w-])|['"\`]${escape(id)}['"\`]`)
      : new RegExp(`component=${escape(id)}(?![\\w-])`),
  }));
}

/**
 * The exercise index for changed paths. `sources` maps every client source path (repo-relative,
 * `.ts`/`.tsx`/`.css`, tests excluded) to its text; `specs` maps every `clients/web/tests/*.ts` file.
 */
export function exerciseIndex(changes, { sources, specs }) {
  const files = new Set(sources.keys());
  const imports = new Map();
  const importers = new Map();
  for (const [path, text] of sources)
    if (MODULE.test(path)) {
      const targets = relativeImports(path, text, files);
      imports.set(path, targets);
      for (const target of targets) importers.set(target, [...(importers.get(target) ?? []), path]);
    }
  const siblingCss = (path) => sources.get(path.replace(/\.tsx?$/, '.css')) ?? '';
  const own = (path, options) =>
    ownHooks(path, sources.get(path) ?? '', path.endsWith('.css') ? '' : siblingCss(path), options);
  // The changed module's surface: its own and queried hooks plus its direct plain-module helpers.
  const effective = (path) =>
    mergeHooks(
      own(path, { queried: true }),
      ...(imports.get(path) ?? []).filter((target) => target.endsWith('.ts') && !isTest(target)).map((p) => own(p)),
    );
  const result = {
    files: new Set(),
    providerTitles: new Set(),
    providerWhole: false,
    demoTitles: new Set(),
    reasons: [],
  };
  for (const changed of changes) {
    if (!SOURCE.test(changed) || isTest(changed) || !sources.has(changed)) continue;
    // A stylesheet speaks for its component module when one sits beside it.
    const module = changed.endsWith('.css')
      ? ([changed.replace(/\.css$/, '.tsx'), changed.replace(/\.css$/, '.ts')].find((path) => sources.has(path)) ??
        changed)
      : changed;
    let hooks = effective(module);
    let via = [module];
    if (!hookCount(hooks)) {
      // Walk up to the nearest shipped parents that carry hooks.
      const seen = new Set([module]);
      let frontier = [module];
      via = [];
      const found = [];
      while (frontier.length && !found.length) {
        const next = [];
        for (const path of frontier)
          for (const parent of importers.get(path) ?? [])
            if (!seen.has(parent) && !isTest(parent) && !COMPOSITION_ROOT.test(parent)) {
              seen.add(parent);
              // A parent speaks only for its own surface, never for its other helpers.
              if (hookCount(own(parent))) found.push(parent);
              else next.push(parent);
            }
        frontier = next;
      }
      via = found.sort();
      hooks = mergeHooks(...found.map(own));
    }
    // HS2-EWF0FT escape: a demo file, or a fixture it exports, reaches the specs only through the
    // catalog entries that render it, which tests open as `/ux-demo?component=<id>`. Demo code is
    // not shipped, so its queried classes never select shipped specs, and entry ids match only in
    // specs that open the demo route (a shipped `"note-card"` locator does not render the demo).
    // A catalog aggregator change reaches every entry, so it selects the whole demo spec.
    const demoModule = UX_DEMO.test(module);
    if (demoModule && DEMO_AGGREGATOR.test(module)) {
      result.files.add(DEMO_SPEC);
      result.reasons.push(`HS2-N7EY18/HS2-R1D0S5: ${changed} composes every ux-demo entry`);
      continue;
    }
    const matchers = demoModule ? [] : hookMatchers(hooks);
    const demos = demoModule ? demoEntriesReaching(module, sources, importers) : [];
    const entryMatchers = demoMatchers(demos);
    if (demoModule) {
      hooks = { classes: [], routes: [] };
      via = [`ux-demo entries ${demos.slice(0, 4).join(', ')}${demos.length > 4 ? `, +${demos.length - 4}` : ''}`];
    }
    const reachedSpecs = [];
    for (const [spec, text] of [...specs].sort(([a], [b]) => a.localeCompare(b))) {
      if (!/\.spec\.ts$/.test(spec)) continue;
      const file = spec.replace(`${WEB}/`, '');
      const fixtures = relativeImports(spec, text, new Set(specs.keys())).filter((path) => !/\.spec\.ts$/.test(path));
      const importsChanged = relativeImports(spec, text, files).includes(module);
      const fixtureHit = fixtures.some((fixture) =>
        matchers.some(({ pattern }) => pattern.test((specs.get(fixture) ?? '').replaceAll('\\/', '/'))),
      );
      const tests = exercisedTests(text, text.includes('/ux-demo') ? [...matchers, ...entryMatchers] : matchers);
      const whole = importsChanged || fixtureHit || tests.whole;
      if (!whole && !tests.titles.length) continue;
      reachedSpecs.push(file);
      if (file === 'tests/providers.spec.ts') {
        if (whole) result.providerWhole = true;
        else for (const title of tests.titles) result.providerTitles.add(title);
      } else if (file === DEMO_SPEC && !whole) for (const title of tests.titles) result.demoTitles.add(title);
      else result.files.add(file);
    }
    if (reachedSpecs.length)
      result.reasons.push(
        `HS2-N7EY18/HS2-R1D0S5: ${changed} is exercised through ${via.join(', ')} (${[...hooks.classes, ...hooks.routes].slice(0, 4).join(', ')}${hookCount(hooks) > 4 ? `, +${hookCount(hooks) - 4}` : ''})`,
      );
  }
  return {
    files: [...result.files].sort(),
    providerTitles: [...result.providerTitles].sort(),
    providerWhole: result.providerWhole,
    // A title already covered by the whole file is not run twice.
    demoTitles: result.files.has(DEMO_SPEC) ? [] : [...result.demoTitles].sort(),
    reasons: result.reasons,
  };
}

function walk(directory, prefix, keep, out = new Map()) {
  if (!existsSync(directory)) return out;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    const relative = `${prefix}/${entry.name}`;
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && entry.name !== 'fixtures') walk(path, relative, keep, out);
    } else if (keep(relative)) out.set(relative, readFileSync(path, 'utf8'));
  }
  return out;
}

/** Read the working tree once and build the index for the changed client paths. */
export function exerciseIndexForRepo(repo, changes) {
  if (!changes.some((path) => SOURCE.test(path) && !isTest(path)))
    return { files: [], providerTitles: [], providerWhole: false, demoTitles: [], reasons: [] };
  const sources = walk(join(repo, WEB, 'src'), `${WEB}/src`, (path) => SOURCE.test(path) && !isTest(path));
  const specs = walk(
    join(repo, WEB, 'tests'),
    `${WEB}/tests`,
    (path) => /\.ts$/.test(path) && dirname(path) === `${WEB}/tests`,
  );
  return exerciseIndex(changes, { sources, specs });
}

/** HS2-B42XMS: the Kerf `CatalogExample` specimens a demo source renders. */
export function catalogSpecimenCount(source) {
  return (source.match(/<CatalogExample\b/g) ?? []).length;
}

/**
 * The demo-spec tests that pin each entry's specimen count: those titled after `CatalogExample`
 * (HS2-Z3FMEZ). Found by title, so a renamed or added inventory test needs no edit here.
 */
export function specimenInventoryTitles(specText) {
  const titles = [];
  for (const match of specText.matchAll(/^test\((['"`])((?:\\.|(?!\1)[^\\])*?)\1/gm))
    if (match[2].includes('CatalogExample')) titles.push(match[2].replace(/\\(.)/g, '$1'));
  return titles;
}

/**
 * HS2-B42XMS escape: adding or removing a demo specimen changes the counts the inventory test
 * pins, yet a single-word entry id (`list`, `toolbar`) never matches that test's literals, and
 * CI 40807614449 caught such a change only after push. When a changed demo file's specimen count
 * differs from the base, add the inventory titles to the demo selection. `readBase`/`readWorking`
 * return a path's text or null; a whole-file demo selection already covers the inventory.
 */
export function withSpecimenInventory(index, { changes, readBase, readWorking, demoSpec }) {
  if (index.files.includes(DEMO_SPEC)) return index;
  const changed = changes.filter(
    (path) => UX_DEMO.test(path) && /\.tsx?$/.test(path) && !isTest(path) && !DEMO_AGGREGATOR.test(path),
  );
  const moved = changed.filter(
    (path) => catalogSpecimenCount(readBase(path) ?? '') !== catalogSpecimenCount(readWorking(path) ?? ''),
  );
  const titles = moved.length ? specimenInventoryTitles(demoSpec ?? '') : [];
  if (!titles.length) return index;
  return {
    ...index,
    demoTitles: [...new Set([...index.demoTitles, ...titles])].sort(),
    reasons: [
      ...index.reasons,
      `HS2-B42XMS: ${moved.join(', ')} changes its CatalogExample specimen count, so the catalog specimen inventory runs`,
    ],
  };
}
