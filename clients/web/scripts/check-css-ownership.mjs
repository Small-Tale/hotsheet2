// Component CSS ownership guard (HS2-EWYDH7, parent HS2-0873J5).
//
// A component stylesheet may style only the class blocks its owning component module renders,
// plus native HTML and raw Web Awesome elements that component renders itself. It never reaches
// into another component: not a Kerf component (`.kui-*`, `[data-component]`), and not another
// application component's classes or markup.
//
// Kerf's doctor enforces this only across packages (its ownership rules compare
// `entry.package`), so app-to-app ownership is checked here until KF-5X1TWD ships.
//
// Ownership comes from the TSX sources, not from file names: each stylesheet is owned by the
// modules that import it, and each class block is owned by the module(s) that render it.
// Known residue lives in css-ownership-allowlist.json; every entry names its ticket and the
// exact number of findings it covers, and an entry that no longer matches fails the check, so
// the list can only shrink.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, posix, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

import postcss from 'postcss';
import ts from 'typescript';

const IDENTIFIER = /^-?[a-z_][a-z0-9_-]*$/i;
const TICKET = /^(?:HS2|KF)-[0-9A-Z]{6}$/;

/**
 * Kerf leaf primitives whose whole markup is one raw element. A component that renders the
 * primitive itself may size that element like its own markup; the same element inside another
 * component is still that component's.
 */
export const ICON_PRIMITIVES = new Map([['LucideIcon', 'svg']]);

/**
 * Shell stylesheets: one app surface split across many style-less modules. Each also owns the
 * modules under its prefixes that import no stylesheet of their own inside the scope.
 */
export const SHELL_SCOPES = {
  'src/style.css': ['src/main.tsx', 'src/app/'],
  'src/ux-demo/style.css': ['src/ux-demo/'],
  'src/dev-review/dev-review.css': ['src/dev-review/'],
};

/** The BEM block of a class: `ticket-row__title--muted` → `ticket-row`. */
export function blockOf(className) {
  return className.split('__')[0].split('--')[0];
}

// Split `text` at top-level separators, skipping strings, escapes, and bracketed or
// parenthesized groups.
function splitTopLevel(text, isSeparator) {
  const parts = [];
  let depth = 0,
    quote = '',
    current = '';
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (char === '\\') {
      current += char + (text[index + 1] ?? '');
      index += 1;
      continue;
    }
    if (quote) {
      if (char === quote) quote = '';
      current += char;
      continue;
    }
    if (char === '"' || char === "'") quote = char;
    else if (char === '(' || char === '[') depth += 1;
    else if (char === ')' || char === ']') depth -= 1;
    else if (depth === 0 && isSeparator(char)) {
      parts.push({ text: current, separator: char });
      current = '';
      continue;
    }
    current += char;
  }
  parts.push({ text: current, separator: '' });
  return parts;
}

/**
 * Parse a selector list into complex selectors: `{ text, compounds }`, where `text` has its
 * whitespace normalized and `compounds` lists `{ combinator, compound }` left to right (the last
 * compound is the subject; the first has an empty combinator).
 */
export function parseSelectorList(selectorList) {
  return splitTopLevel(selectorList, (char) => char === ',')
    .map(({ text }) => text.replace(/\s+/g, ' ').replace(/\( /g, '(').replace(/ \)/g, ')').trim())
    .filter(Boolean)
    .map((complex) => {
      const compounds = [];
      let combinator = '';
      for (const { text, separator } of splitTopLevel(complex, (char) => /[\s>+~]/.test(char))) {
        const compound = text.trim();
        if (compound) {
          compounds.push({ combinator: compounds.length ? combinator || ' ' : '', compound });
          combinator = '';
        }
        if (separator.trim()) combinator = separator;
      }
      return { text: complex, compounds };
    });
}

// The compound without the arguments of functional pseudo-classes and pseudo-elements.
function withoutPseudoArguments(compound) {
  let result = '',
    depth = 0;
  for (const char of compound) {
    if (char === '(') depth += 1;
    else if (char === ')') depth -= 1;
    else if (depth === 0) result += char;
  }
  return result;
}

/** Every class a compound mentions, including inside `:has()`, `:not()`, `:is()`, and `:where()`. */
export function classesIn(compound) {
  return [...compound.matchAll(/\.(-?[a-z_][a-z0-9_-]*)/gi)].map((match) => match[1]);
}

/** The classes on the compound's own element (functional pseudo-class arguments excluded). */
export function ownClassesIn(compound) {
  return classesIn(withoutPseudoArguments(compound));
}

/** The compound's type selector (`svg`, `wa-button`, `*`), or `''` when it has none. */
export function typeOf(compound) {
  const match = /^(\*|[a-z][a-z0-9-]*)/i.exec(withoutPseudoArguments(compound).trim());
  return match ? match[1].toLowerCase() : '';
}

/**
 * What one TypeScript/TSX module renders.
 *
 * - `classTokens`: every class-like token in its string and template literals and in raw HTML
 *   `class="…"` attributes.
 * - `tags`: intrinsic JSX tags, `createElement('tag')` calls, and raw HTML tags.
 * - `elements`: one JSX node tree per element that carries literal classes. A node is
 *   `{ name, component, classes, children }`; a component node's children are the JSX this module
 *   projects into it (its children and JSX-valued props), which render inside that component.
 * - `declares`: top-level names the module declares (its own local components).
 * - `cssImports`: relative stylesheet imports.
 */
export function moduleFacts(path, source) {
  const file = ts.createSourceFile(
    path,
    source,
    ts.ScriptTarget.Latest,
    true,
    path.endsWith('x') ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const classTokens = new Set(),
    tags = new Set(),
    elements = [],
    cssImports = [],
    declares = new Set();
  const tokensOf = (text) =>
    text
      .split(/\s+/)
      .map((token) => token.replace(/[-_]+$/, ''))
      .filter((token) => token && IDENTIFIER.test(token));
  const literalTokens = (node, into) => {
    const visit = (child) => {
      if (ts.isStringLiteralLike(child)) for (const token of tokensOf(child.text)) into.add(token);
      else if (ts.isTemplateExpression(child)) {
        for (const token of tokensOf(child.head.text)) into.add(token);
        for (const span of child.templateSpans) for (const token of tokensOf(span.literal.text)) into.add(token);
      }
      ts.forEachChild(child, visit);
    };
    visit(node);
    return into;
  };
  const opening = (node) => (ts.isJsxElement(node) ? node.openingElement : node);
  const nameOf = (node) => opening(node).tagName.getText(file);
  const isIntrinsic = (name) => /^[a-z]/.test(name);
  const nodes = new Map();
  // The JSX node tree rooted at `jsx` (memoized: every element is visited once as a root too).
  const nodeOf = (jsx) => {
    if (nodes.has(jsx)) return nodes.get(jsx);
    const name = nameOf(jsx);
    const node = { name, component: !isIntrinsic(name), classes: new Set(), children: [] };
    nodes.set(jsx, node);
    for (const attribute of opening(jsx).attributes.properties)
      if (ts.isJsxAttribute(attribute) && attribute.initializer) {
        if (['class', 'className'].includes(attribute.name.getText(file)))
          literalTokens(attribute.initializer, node.classes);
        else if (node.component) collectChildren(attribute.initializer, node.children);
      }
    if (ts.isJsxElement(jsx)) for (const child of jsx.children) collectChildren(child, node.children);
    return node;
  };
  // JSX elements reachable from `start` without passing through another element.
  function collectChildren(start, into) {
    const visit = (child) => {
      if (ts.isJsxElement(child) || ts.isJsxSelfClosingElement(child)) into.push(nodeOf(child));
      else ts.forEachChild(child, visit);
    };
    visit(start);
  }
  const rawHtml = (text) => {
    for (const match of text.matchAll(/<([a-z][a-z0-9-]*)[\s>/]/gi)) tags.add(match[1].toLowerCase());
    for (const match of text.matchAll(/\bclass=(["'])([^"']*)\1/g))
      for (const token of tokensOf(match[2])) classTokens.add(token);
  };
  const visit = (node) => {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      const specifier = node.moduleSpecifier.text;
      if (specifier.startsWith('.') && specifier.endsWith('.css')) cssImports.push(specifier);
      return;
    }
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const element = nodeOf(node);
      if (!element.component) tags.add(element.name.toLowerCase());
      if (element.classes.size) elements.push(element);
    }
    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      ['createElement', 'createElementNS'].includes(node.expression.name.text)
    ) {
      const tag = node.arguments.at(-1);
      if (tag && ts.isStringLiteralLike(tag)) tags.add(tag.text.toLowerCase());
    }
    if (ts.isStringLiteralLike(node)) {
      for (const token of tokensOf(node.text)) classTokens.add(token);
      rawHtml(node.text);
    } else if (ts.isTemplateExpression(node)) {
      literalTokens(node, classTokens);
      rawHtml([node.head.text, ...node.templateSpans.map((span) => span.literal.text)].join(' '));
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  for (const statement of file.statements) {
    if (ts.isFunctionDeclaration(statement) && statement.name) declares.add(statement.name.text);
    if (ts.isVariableStatement(statement))
      for (const declaration of statement.declarationList.declarations)
        if (ts.isIdentifier(declaration.name)) declares.add(declaration.name.text);
  }
  const components = new Set([...nodes.values()].filter((node) => node.component).map((node) => node.name));
  return { path, classTokens, tags, components, elements, cssImports, declares };
}

/**
 * Build the ownership model.
 *
 * - A stylesheet's owners are the modules that import it, directly or through CSS `@import`,
 *   plus, for a shell stylesheet (`shellScopes`: sheet path → path prefixes), every module under
 *   its prefixes that imports no stylesheet of its own inside that scope.
 * - A class block's renderers are the modules whose literals contain one of its classes. When
 *   several modules render a block, its owners narrow to the owners of a stylesheet named after
 *   the block, else to renderers whose own stylesheet styles the block as a selector root.
 *
 * Paths are workspace-relative with `/` separators.
 */
export function buildOwnership({ stylesheets, modules, shellScopes = {} }) {
  const sheets = new Map(stylesheets.map((sheet) => [sheet.path, { ...sheet, owners: new Set() }]));
  const facts = modules.map(({ path, source }) => moduleFacts(path, source));
  const factsByPath = new Map(facts.map((fact) => [fact.path, fact]));
  const importsOf = (fact) => fact.cssImports.map((specifier) => posix.join(posix.dirname(fact.path), specifier));
  for (const fact of facts) for (const imported of importsOf(fact)) sheets.get(imported)?.owners.add(fact.path);
  for (const [path, prefixes] of Object.entries(shellScopes)) {
    const sheet = sheets.get(path);
    if (!sheet) throw new Error(`Shell stylesheet ${path} does not exist.`);
    const inScope = (candidate) => prefixes.some((prefix) => candidate.startsWith(prefix));
    for (const fact of facts)
      if (inScope(fact.path) && !importsOf(fact).some((imported) => imported !== path && inScope(imported)))
        sheet.owners.add(fact.path);
  }
  for (const sheet of sheets.values()) {
    sheet.rules = [];
    sheet.imports = [];
    const root = postcss.parse(sheet.source, { from: sheet.path });
    root.walkAtRules('import', (rule) => {
      const match = /^(?:url\()?\s*['"]?(\.[^'")\s]+\.css)/.exec(rule.params);
      if (match) sheet.imports.push(posix.join(posix.dirname(sheet.path), match[1]));
    });
    root.walkRules((rule) => {
      if (rule.parent?.type === 'atrule' && /keyframes$/i.test(rule.parent.name)) return;
      for (const complex of parseSelectorList(rule.selector))
        sheet.rules.push({ selector: complex.text, compounds: complex.compounds, line: rule.source?.start?.line ?? 0 });
    });
  }
  // CSS @import passes a sheet's owners on to the sheets it imports.
  for (let changed = true; changed;) {
    changed = false;
    for (const sheet of sheets.values())
      for (const imported of sheet.imports) {
        const target = sheets.get(imported);
        for (const owner of target ? sheet.owners : [])
          if (!target.owners.has(owner)) {
            target.owners.add(owner);
            changed = true;
          }
      }
  }
  const styledBlocks = new Set();
  const rootBlocks = new Map();
  for (const sheet of sheets.values())
    for (const { compounds } of sheet.rules) {
      for (const { compound } of compounds) for (const name of classesIn(compound)) styledBlocks.add(blockOf(name));
      const first = ownClassesIn(compounds[0].compound)[0];
      if (first) {
        const block = blockOf(first);
        if (!rootBlocks.has(block)) rootBlocks.set(block, new Set());
        rootBlocks.get(block).add(sheet.path);
      }
    }
  const renderers = new Map();
  for (const fact of facts)
    for (const token of fact.classTokens) {
      const block = blockOf(token);
      if (!styledBlocks.has(block)) continue;
      if (!renderers.has(block)) renderers.set(block, new Set());
      renderers.get(block).add(fact.path);
    }
  const sheetNamed = new Map();
  for (const sheet of sheets.values()) {
    const name = posix.basename(sheet.path, '.css');
    if (!sheetNamed.has(name)) sheetNamed.set(name, []);
    sheetNamed.get(name).push(sheet);
  }
  const blockOwners = new Map();
  for (const [block, rendered] of renderers) {
    let owners = rendered;
    if (owners.size > 1) {
      const named = (sheetNamed.get(block) ?? []).flatMap((sheet) => [...sheet.owners]);
      const rooted = [...(rootBlocks.get(block) ?? [])].flatMap((path) => [...sheets.get(path).owners]);
      const narrowed = named.length ? named : rooted.filter((path) => rendered.has(path));
      if (narrowed.length) owners = new Set(narrowed);
    }
    blockOwners.set(block, owners);
  }
  return { sheets, factsByPath, blockOwners, sheetNamed };
}

const list = (items) => [...new Set(items)].sort().join(', ');

/**
 * Every ownership violation, `{ file, line, selector, kind, detail }`. `kind` is one of:
 *
 * - `kerf`: a Kerf class (`.kui-*`) or `[data-component]`;
 * - `foreign-class`: a class block another component renders, anywhere in the selector;
 * - `hook-descendant`: an element below a class this component places on another component's
 *   root, which is inside that component;
 * - `foreign-element`: an element subject that is (or, through a descendant combinator, also
 *   matches) markup another component renders.
 */
export function findViolations(model) {
  const violations = [];
  for (const sheet of model.sheets.values()) {
    const owners = sheet.owners;
    const ownerFacts = [...owners].map((path) => model.factsByPath.get(path)).filter(Boolean);
    const ownerNames = list(ownerFacts.map((fact) => fact.path));
    const isOwnBlock = (block) => {
      const blockOwners = model.blockOwners.get(block);
      if (blockOwners) return [...blockOwners].some((path) => owners.has(path));
      // Rendered by no literal anywhere: fall back to the stylesheet named after the block.
      const named = model.sheetNamed.get(block);
      return !named || named.includes(sheet);
    };
    const ownerOfBlock = (block) =>
      list(model.blockOwners.get(block) ?? (model.sheetNamed.get(block) ?? []).map((other) => other.path));
    const isForeignComponent = (node) =>
      node.component && !ICON_PRIMITIVES.has(node.name) && !ownerFacts.some((fact) => fact.declares.has(node.name));
    const nodeTag = (node) => (node.component ? ICON_PRIMITIVES.get(node.name) : node.name.toLowerCase());
    const descendants = (node, into = []) => {
      for (const child of node.children) {
        into.push(child);
        // An own local component's markup lives in another function: opaque, but not foreign.
        if (!child.component || isForeignComponent(child) || ICON_PRIMITIVES.has(child.name)) descendants(child, into);
      }
      return into;
    };
    const names = (nodes) => list(nodes.filter(isForeignComponent).map((node) => `<${node.name}>`));
    // Walk the classless steps below an own element carrying `classes` through the JSX trees.
    const elementVerdict = (classes, steps) => {
      const tag = typeOf(steps.at(-1).compound);
      const matches = ownerFacts.flatMap((fact) =>
        fact.elements.filter((element) => classes.some((name) => element.classes.has(name))),
      );
      if (!matches.length)
        // The classed element is built outside JSX literals: judge by the modules as a whole.
        return ownerFacts.some(
          (fact) => fact.tags.has(tag) || [...fact.components].some((name) => ICON_PRIMITIVES.get(name) === tag),
        )
          ? null
          : {
              kind: 'foreign-element',
              detail: `<${tag}> is never rendered by ${ownerNames}; it comes from a child component`,
            };
      for (const element of matches) {
        const via = `.${classes.find((name) => element.classes.has(name))}`;
        if (element.component)
          return {
            kind: 'hook-descendant',
            detail: `${via} is a hook class on <${element.name}>'s root; <${tag}> sits inside that component`,
          };
        let current = [element];
        for (const [index, { combinator, compound }] of steps.entries()) {
          if (combinator === '+' || combinator === '~') break;
          const wanted = typeOf(compound);
          const matchesTag = (node) => !wanted || wanted === '*' || nodeTag(node) === wanted;
          const range = current.flatMap((node) => (combinator === '>' ? node.children : descendants(node)));
          if (combinator === ' ' && range.some(isForeignComponent))
            return {
              kind: 'foreign-element',
              detail: `the descendant selector below ${via} also reaches inside ${names(range)}; use a child combinator, an own class, or the child's props`,
            };
          const next = range.filter(matchesTag);
          if (!next.length) {
            if (index === steps.length - 1 && range.some(isForeignComponent))
              return {
                kind: 'foreign-element',
                detail: `<${tag}> below ${via} is not authored by ${ownerNames}; it can only be the root of ${names(range)}`,
              };
            break;
          }
          current = next;
        }
      }
      return null;
    };
    for (const { selector, compounds, line } of sheet.rules) {
      const report = (kind, detail) => violations.push({ file: sheet.path, line, selector, kind, detail });
      if (/\.kui-|\[\s*data-component\b/.test(selector)) {
        report('kerf', 'targets a Kerf component class or [data-component]; use its props, variants, or tokens');
        continue;
      }
      const foreign = [
        ...new Set(compounds.flatMap(({ compound }) => classesIn(compound).map(blockOf)).filter((b) => !isOwnBlock(b))),
      ];
      if (foreign.length) {
        report(
          'foreign-class',
          foreign
            .map((block) => `.${block} belongs to ${ownerFacts.length ? ownerOfBlock(block) : 'another component'}`)
            .join('; '),
        );
        continue;
      }
      // A classless subject element below one of this component's own elements.
      const subject = compounds.at(-1);
      if (compounds.length < 2 || ownClassesIn(subject.compound).length || !ownerFacts.length) continue;
      // The universal selector and the document's own elements belong to no component.
      if (['', '*', 'html', 'body'].includes(typeOf(subject.compound))) continue;
      const index = compounds.findLastIndex(
        ({ compound }, position) => position < compounds.length - 1 && ownClassesIn(compound).length,
      );
      if (index < 0) continue;
      const verdict = elementVerdict(ownClassesIn(compounds[index].compound), compounds.slice(index + 1));
      if (verdict) report(verdict.kind, verdict.detail);
    }
  }
  return violations.sort(
    (a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.selector.localeCompare(b.selector),
  );
}

/**
 * Validate the allowlist and compare it with the findings. Each entry is
 * `{ file, selector, count?, ticket, reason }` and covers exactly `count` (default 1) findings of
 * that selector in that file. Returns the findings no entry covers (`unexpected`), the entries
 * that now cover fewer findings than they claim (`stale`), and malformed-entry `problems`.
 */
export function applyAllowlist(violations, allowlist) {
  const problems = [];
  const entries = new Map();
  for (const entry of allowlist.entries ?? []) {
    const label = `${entry.file ?? '?'} ${entry.selector ?? '?'}`;
    if (!entry.file || !entry.selector)
      problems.push(`allowlist entry is missing file or selector: ${JSON.stringify(entry)}`);
    if (!TICKET.test(entry.ticket ?? ''))
      problems.push(`allowlist entry ${label} must name its tracking ticket (HS2-XXXXXX or KF-XXXXXX)`);
    if (!entry.reason) problems.push(`allowlist entry ${label} must give a reason`);
    if (entry.count !== undefined && !(Number.isInteger(entry.count) && entry.count > 1))
      problems.push(`allowlist entry ${label} count must be an integer above 1 (omit it for 1)`);
    const key = `${entry.file}\u0000${entry.selector}`;
    if (entries.has(key)) problems.push(`duplicate allowlist entry ${label}`);
    entries.set(key, { entry, allowed: entry.count ?? 1, seen: 0 });
  }
  const unexpected = [];
  for (const violation of violations) {
    const record = entries.get(`${violation.file}\u0000${violation.selector}`);
    if (record && record.seen < record.allowed) record.seen += 1;
    else unexpected.push(violation);
  }
  const stale = [...entries.values()]
    .filter((record) => record.seen < record.allowed)
    .map((record) => ({ ...record.entry, matched: record.seen }));
  const allowed = [...entries.values()].reduce((sum, record) => sum + record.seen, 0);
  return { unexpected, stale, problems, allowed, entries: entries.size };
}

function filesUnder(directory, predicate) {
  const result = [];
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) result.push(...filesUnder(path, predicate));
    else if (predicate(path)) result.push(path);
  }
  return result;
}

/** Scan a web workspace and compare the findings with its allowlist. */
export function checkWorkspace(workspace, allowlistPath = join(workspace, 'css-ownership-allowlist.json')) {
  const read = (path) => ({ path: relative(workspace, path).split(sep).join('/'), source: readFileSync(path, 'utf8') });
  const src = join(workspace, 'src');
  const stylesheets = filesUnder(src, (path) => path.endsWith('.css')).map(read);
  const modules = filesUnder(
    src,
    (path) => /\.(?:tsx?|mts)$/.test(path) && !/\.(?:test|spec)\.|\.d\.ts$/.test(path),
  ).map(read);
  const violations = findViolations(buildOwnership({ stylesheets, modules, shellScopes: SHELL_SCOPES }));
  const allowlist = JSON.parse(readFileSync(allowlistPath, 'utf8'));
  return { violations, ...applyAllowlist(violations, allowlist) };
}

/** Format a report; returns `{ ok, text }`. */
export function formatReport({ violations, unexpected, stale, problems, allowed, entries }) {
  const errors = [
    ...problems,
    ...unexpected.map((item) => `${item.file}:${item.line} ${item.kind}: ${item.selector}\n    ${item.detail}`),
    ...stale.map(
      (entry) =>
        `stale allowlist entry: ${entry.file} ${entry.selector} (${entry.ticket}) now matches ${entry.matched} of ${entry.count ?? 1}; lower its count or delete it`,
    ),
  ];
  if (!errors.length)
    return {
      ok: true,
      text: `CSS ownership clean: ${violations.length} known cross-component selector(s), all covered by ${entries} ticketed allowlist entr${entries === 1 ? 'y' : 'ies'}.`,
    };
  return {
    ok: false,
    text:
      `CSS ownership check failed: ${unexpected.length} uncovered cross-component selector(s), ${stale.length} stale allowlist entr${stale.length === 1 ? 'y' : 'ies'}, ${problems.length} malformed entr${problems.length === 1 ? 'y' : 'ies'} (${allowed} findings allowlisted).\n` +
      'A component stylesheet may style only its own class blocks and the native/Web Awesome elements it renders itself; ' +
      'configure a child component through its props, variants, or tokens instead (docs/ux-components.md).\n\n' +
      errors.join('\n'),
  };
}

function run() {
  const workspace = resolve(import.meta.dirname, '..');
  const { ok, text } = formatReport(checkWorkspace(workspace));
  if (ok) console.log(text);
  else {
    console.error(text);
    process.exitCode = 1;
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) run();
