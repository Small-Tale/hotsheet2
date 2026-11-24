import { existsSync, readdirSync, readFileSync } from 'node:fs';

import ts from 'typescript';
import { describe, expect, it } from 'vitest';

interface KerfProfile {
  scope: string;
  exceptions: Array<{ id: string; rules: string[]; target: string; rationale: string }>;
}

interface KerfDoctorConfig {
  mode: string;
  stages: Record<string, boolean>;
  cache: boolean;
  failOn?: string;
  suppressions: { id: string; rules: string[]; target: string; rationale: string }[];
}

function sourceFiles(directory: URL): URL[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = new URL(`${entry.name}${entry.isDirectory() ? '/' : ''}`, directory);
    return entry.isDirectory()
      ? sourceFiles(path)
      : entry.name.endsWith('.tsx') && !entry.name.includes('.test.')
        ? [path]
        : [];
  });
}

type JsxElementLike = ts.JsxSelfClosingElement | ts.JsxOpeningElement;

function stateBannerElements(includeShell = false): Array<{ file: string; line: number; element: JsxElementLike }> {
  return sourceFiles(new URL('./', import.meta.url)).flatMap((url) => {
    const source = readFileSync(url, 'utf8');
    if (!source.includes('<StateBanner') && !(includeShell && source.includes('<ShellBanner'))) return [];
    const file = url.pathname.split('/src/')[1] ?? url.pathname;
    const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const found: Array<{ file: string; line: number; element: JsxElementLike }> = [];
    const visit = (node: ts.Node): void => {
      if (
        (ts.isJsxSelfClosingElement(node) || ts.isJsxOpeningElement(node)) &&
        (node.tagName.getText(tree) === 'StateBanner' || (includeShell && node.tagName.getText(tree) === 'ShellBanner'))
      ) {
        found.push({ file, line: tree.getLineAndCharacterOfPosition(node.getStart(tree)).line + 1, element: node });
      }
      ts.forEachChild(node, visit);
    };
    visit(tree);
    return found;
  });
}

function jsxAttribute(element: JsxElementLike, name: string): ts.JsxAttribute | undefined {
  return element.attributes.properties.find(
    (property): property is ts.JsxAttribute => ts.isJsxAttribute(property) && property.name.getText() === name,
  );
}

/**
 * Whether a JSX element is what a `.map()` or `.flatMap()` callback returns: the arrow body or a
 * `return` operand, through parentheses, `cond ? a : b`, and `cond && a` (HS2-3TNEJF).
 */
function isMapCallbackResult(element: ts.Node): boolean {
  let node = element;
  for (;;) {
    const parent = node.parent;
    if (
      ts.isParenthesizedExpression(parent) ||
      (ts.isConditionalExpression(parent) && parent.condition !== node) ||
      (ts.isBinaryExpression(parent) &&
        parent.right === node &&
        [
          ts.SyntaxKind.AmpersandAmpersandToken,
          ts.SyntaxKind.BarBarToken,
          ts.SyntaxKind.QuestionQuestionToken,
        ].includes(parent.operatorToken.kind))
    ) {
      node = parent;
      continue;
    }
    let callback: ts.Node | undefined;
    if (ts.isArrowFunction(parent) && parent.body === node) callback = parent;
    else if (ts.isReturnStatement(parent)) callback = ts.findAncestor(parent, ts.isFunctionLike);
    if (callback === undefined || !ts.isCallExpression(callback.parent)) return false;
    const call = callback.parent;
    return (
      call.arguments[0] === callback &&
      ts.isPropertyAccessExpression(call.expression) &&
      ['map', 'flatMap'].includes(call.expression.name.text)
    );
  }
}

/** The text of a `"literal"`, `{'literal'}`, or substitution-free template initializer; otherwise undefined. */
function constantString(initializer: ts.JsxAttributeValue): string | undefined {
  const value = ts.isJsxExpression(initializer) ? initializer.expression : initializer;
  return value !== undefined && (ts.isStringLiteral(value) || ts.isNoSubstitutionTemplateLiteral(value))
    ? value.text
    : undefined;
}

describe('Kerf application UI profile', () => {
  it('limits exceptions to exact reviewed integration boundaries', () => {
    const profile = JSON.parse(
      readFileSync(new URL('../.kerf-ui-profile.json', import.meta.url), 'utf8'),
    ) as KerfProfile;
    expect(profile.scope).toBe('workspace');
    expect(profile.exceptions).toHaveLength(48);
    for (const exception of profile.exceptions.slice(0, 16)) {
      expect(exception.id).toMatch(/^web-awesome-/);
      expect(exception.rules).toEqual(['KUI-L011']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.css$/);
      expect(exception.target).not.toMatch(/[?*]|\.\./);
      if (exception.id === 'web-awesome-project-close-parts') {
        expect(exception.rationale).toContain('raw Web Awesome modal');
        expect(exception.rationale).toContain('KF-E2J9ND; adoption HS2-FXAAA6');
      } else expect(exception.rationale).toContain('Web Awesome shadow parts');
    }
    expect(profile.exceptions.slice(16, 17)).toEqual([
      {
        id: 'mobile-side-panel-safe-area-composition',
        rules: ['KUI-L004'],
        target: 'src/components/app-shell.tsx',
        rationale:
          'The mobile shell deliberately composes safe-area padding on its app-owned sidebar and inspector content inside Kerf overlay regions.',
      },
    ]);
    // HS2-M6B8AD reviewed every remaining nested-inset and dynamic-class review finding site by
    // site; each exception is exact (one file, one rule) and its rationale names the classes or
    // expressions it covers, so a new finding elsewhere still fails the gate.
    const reviewed = profile.exceptions.slice(17);
    expect(reviewed.length).toBeGreaterThan(0);
    const ids = new Set<string>(),
      targets = new Set<string>(),
      stale: string[] = [];
    for (const exception of reviewed) {
      expect(exception.id).toMatch(/^(?:intentional-nested-inset|state-modifier-classes)-[a-z0-9-]+$/);
      expect(exception.rules).toEqual([exception.id.startsWith('intentional-') ? 'KUI-L004' : 'KUI-L008']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.tsx$/);
      expect(existsSync(new URL(`../${exception.target}`, import.meta.url)), exception.target).toBe(true);
      expect(exception.rationale).toMatch(/^Reviewed HS2-[0-9A-Z]{6}: /);
      const source = readFileSync(new URL(`../${exception.target}`, import.meta.url), 'utf8');
      if (exception.rules[0] === 'KUI-L004') {
        expect(exception.rationale).toMatch(/design: [a-z][a-z0-9-]*__[a-z0-9-]+/);
        const classes = [...exception.rationale.matchAll(/\b[a-z][a-z0-9-]*__[a-z0-9-]+(?:--[a-z0-9-]+)?\b/g)].map(
          ([name]) => name,
        );
        for (const name of classes) if (!source.includes(name)) stale.push(`${exception.id}: ${name}`);
      } else {
        expect(exception.rationale).toMatch(/Expressions: class(?:Name)?=\{/);
        const sourceWithoutWhitespace = source.replace(/\s+/g, '');
        for (const expression of exception.rationale.split('Expressions: ')[1].split(' | '))
          if (!sourceWithoutWhitespace.includes(expression.replace(/\s+/g, '')))
            stale.push(`${exception.id}: ${expression}`);
      }
      expect(ids.has(exception.id), exception.id).toBe(false);
      ids.add(exception.id);
      targets.add(`${exception.rules[0]} ${exception.target}`);
    }
    expect(targets.size).toBe(reviewed.length);
    expect(stale).toEqual([]);
    // Kerf's corrected MessageComposer catalog (KF-Z00DKT) retired the false KUI-L203 exception (HS2-T8SPNP).
    expect(profile.exceptions.some((exception) => exception.rules.includes('KUI-L203'))).toBe(false);
  });

  it('avoids the discouraged raw fields after TextField, Checkbox, and ProgressRing adoption', () => {
    const profile = JSON.parse(readFileSync(new URL('../.kerf-ui-profile.json', import.meta.url), 'utf8')) as {
      preferences: Record<string, { preferred: string; avoid: string[]; rationale: string }>;
    };
    expect(Object.keys(profile.preferences).sort()).toEqual(['boolean-option', 'compact-progress', 'text-entry']);
    expect(profile.preferences['text-entry'].rationale).toContain('HS2-CQ1TZN');
    expect(profile.preferences['text-entry'].avoid).toEqual(['@kerfjs/ui:wa-input', '@kerfjs/ui:wa-textarea']);
    expect(profile.preferences['boolean-option'].avoid).toEqual(['@kerfjs/ui:wa-checkbox']);
    expect(profile.preferences['compact-progress'].avoid).toEqual(['@kerfjs/ui:wa-progress-ring']);
  });

  it('uses Kerf Button for standalone actions rather than raw Web Awesome buttons', () => {
    const root = new URL('./', import.meta.url);
    const files = (directory: URL): URL[] =>
      readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
        const path = new URL(`${entry.name}${entry.isDirectory() ? '/' : ''}`, directory);
        return entry.isDirectory()
          ? files(path)
          : entry.name.endsWith('.tsx') && !entry.name.includes('.test.')
            ? [path]
            : [];
      });
    const usages = files(root)
      .map((file) => ({
        file: file.pathname.split('/src/')[1],
        count: [...readFileSync(file, 'utf8').matchAll(/<wa-button\b/g)].length,
      }))
      .filter(({ count }) => count > 0);
    expect(usages).toEqual([]);
  });

  it('HS2-W77711 passes StateBanner action controls directly, never wrapped in a Row (KF-8TF7Y8)', () => {
    // The banner's action zone lays several controls out itself; a Row there duplicates that layout.
    // HS2-S2T4VJ: inspect only StateBanner action trees; another component in the same file may
    // legitimately use a Row in its action, as the drawer's EmptyState does.
    const wrapped = stateBannerElements(true).flatMap(({ file, line, element }) => {
      const action = jsxAttribute(element, 'action')?.initializer;
      if (!action || !ts.isJsxExpression(action) || !action.expression) return [];
      const rows: string[] = [];
      const visit = (node: ts.Node): void => {
        if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && node.tagName.getText() === 'Row')
          rows.push(`${file}:${line}`);
        ts.forEachChild(node, visit);
      };
      visit(action.expression);
      return rows;
    });
    expect(wrapped).toEqual([]);
  });

  it('HS2-NRDDPY stacks a StateBanner whose detail is not a constant string', () => {
    // Kerf's default inline copy layout keeps the detail on one ellipsized line, so a server or validation
    // message is cut off at phone width. A computed detail must use copyLayout="stacked", which wraps it.
    const inlineComputed = stateBannerElements().flatMap(({ file, line, element }) => {
      const detail = jsxAttribute(element, 'detail');
      if (detail?.initializer === undefined || constantString(detail.initializer) !== undefined) return [];
      const layout = jsxAttribute(element, 'copyLayout')?.initializer;
      return layout !== undefined && constantString(layout) === 'stacked' ? [] : [`${file}:${line}`];
    });
    expect(inlineComputed).toEqual([]);
  });

  it('HS2-QCVKZ5 passes StateBanner data attributes through rootAttributes, never as bare props', () => {
    // TypeScript does not check hyphenated JSX attributes, and StateBanner renders only its named props,
    // so a bare data-key silently disappears and conditional sibling banners lose their diff identity.
    const bare = stateBannerElements().flatMap(({ file, line, element }) =>
      element.attributes.properties
        .filter(ts.isJsxAttribute)
        .map((property) => property.name.getText())
        .filter((name) => name.startsWith('data-'))
        .map((name) => `${file}:${line} ${name}`),
    );
    expect(bare).toEqual([]);
  });

  it('HS2-3TNEJF keys every StateBanner that a .map() callback returns through rootAttributes data-key', () => {
    // Kerf diffs list items by id, then data-key; an unkeyed mapped banner is matched by position when an
    // item is added or removed. Only the callback's returned root needs the key: a banner nested inside
    // another element of the item is keyed by that element.
    const unkeyed = stateBannerElements().flatMap(({ file, line, element }) => {
      if (!isMapCallbackResult(ts.isJsxOpeningElement(element) ? element.parent : element)) return [];
      const attributes = jsxAttribute(element, 'rootAttributes')?.initializer;
      const object = attributes !== undefined && ts.isJsxExpression(attributes) ? attributes.expression : undefined;
      const keyed =
        object !== undefined &&
        ts.isObjectLiteralExpression(object) &&
        object.properties.some(
          (property) =>
            ts.isPropertyAssignment(property) && ts.isStringLiteral(property.name) && property.name.text === 'data-key',
        );
      return keyed ? [] : [`${file}:${line}`];
    });
    expect(unkeyed).toEqual([]);
  });

  it('HS2-3TNEJF recognizes only the returned root of a .map() or .flatMap() callback', () => {
    const banners = (source: string) => {
      const tree = ts.createSourceFile('probe.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
      const results: boolean[] = [];
      const visit = (node: ts.Node): void => {
        if (ts.isJsxSelfClosingElement(node)) results.push(isMapCallbackResult(node));
        ts.forEachChild(node, visit);
      };
      visit(tree);
      return results;
    };
    expect(banners('xs.map((x) => <StateBanner />)')).toEqual([true]);
    expect(banners('xs.flatMap((x) => (x ? <StateBanner /> : []))')).toEqual([true]);
    expect(banners('xs.map(function (x) { return <StateBanner />; })')).toEqual([true]);
    expect(banners('xs.map((x) => <div data-key={x}><StateBanner /></div>)')).toEqual([false]);
    expect(banners('const f = () => <StateBanner />; xs.filter((x) => <StateBanner />)')).toEqual([false, false]);
  });

  it('runs every static doctor stage while keeping browser execution opt-in', () => {
    const config = JSON.parse(
      readFileSync(new URL('../.kerf-ui-doctor.json', import.meta.url), 'utf8'),
    ) as KerfDoctorConfig;
    expect(config).toEqual(
      expect.objectContaining({
        mode: 'full',
        cache: true,
        stages: {
          catalog: true,
          typescript: true,
          eslint: true,
          analyzer: true,
          browser: false,
        },
      }),
    );
    // The doctor itself is the CI gate: any active error, review finding, or warning fails it
    // (beta.69 `failOn`, KF-6S5EKX; HS2-Z44YPD retired the wrapper script).
    expect(config.failOn).toBe('warning');
    // Strict component ownership replaces the app CSS ownership check (HS2-R9GQJE): Kerf judges
    // uncataloged CSS-importing modules, the three shell stylesheets as ownership groups, and other
    // packages' classes used as selector context.
    expect(config).toEqual(
      expect.objectContaining({
        ownership: 'component',
        ownershipContext: 'any-package',
        implicitComponentOwnership: true,
        ownershipGroups: [
          { styleSources: ['src/style.css'], sources: ['src/main.tsx', 'src/app/'] },
          { styleSources: ['src/ux-demo/style.css'], sources: ['src/ux-demo/'] },
        ],
      }),
    );
    // The Catalog headerActions gap closed once the demo renders its tool group only with settings (HS2-HMVBZW).
    expect(config.suppressions).toEqual([]);
    for (const suppression of config.suppressions)
      expect(suppression.rationale).toContain(suppression.id.slice(0, 9).replace('kf-', 'KF-').toUpperCase());
    // The doctor replaced the app CSS ownership check, so `npm run lint` ends with it (HS2-R9GQJE).
    const scripts = (
      JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as {
        scripts: Record<string, string>;
      }
    ).scripts;
    expect(scripts.lint.split('&&').map((step) => step.trim())).toEqual(['npm run format:check', 'npm run lint:web']);
    // HS2-9N8ZMD observes this ordered lint route; its focused helper tests verify
    // ESLint, catalog and doctor commands plus first-failure exit behavior.
    expect(scripts['lint:web']).toBe('node scripts/lint-web-observations.mjs');
    expect(scripts['format:sources:check']).toBe('node scripts/format-sources.mjs --check');
    // HS2-TY5JHN: the doctor runs through node with an explicit heap bound.
    expect(scripts['ui:doctor']).toBe('node --max-old-space-size=4096 node_modules/@kerfjs/ui/doctor/cli.mjs --full');
    expect(scripts).not.toHaveProperty('css:ownership');
  });
});
