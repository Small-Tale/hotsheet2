import { readFile, readdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import ts from 'typescript';

const root = resolve(import.meta.dirname, '..'),
  catalogPath = resolve(root, 'src/ux-demo/catalog.ts'),
  componentsPath = resolve(root, 'src/components'),
  outputPath = resolve(root, 'ai/component-catalog-extension.json');
const compositionIds = new Set([
  'app-shell',
  'project-sidebar',
  'repository-status-popover',
  'command-settings-editor',
  'workspace-header',
  'ticket-list',
  'ticket-board',
  'ticket-inspector',
  'ticket-info-panel',
  'ticket-reader',
  'terminal-drawer',
  'terminal-dashboard',
  'terminal-operations-sidebar',
  'terminal-ticket-rail',
]);
const selfGeometryIds = new Set([
  'app-empty-state',
  'connection-state-banner',
  'fixed-aspect-terminal-card',
  'hs1-migration-banner',
  'markdown-editor',
  'pending-attachment-picker',
  'permission-request',
  'project-summary',
  'project-tab',
  'quick-ticket-composer',
  'repository-summary',
  'status-badge',
  'tag-chip',
  'ticket-row',
]);
const publicClassOverrides = {
  'app-empty-state': ['app-empty'],
  'fixed-aspect-terminal-card': ['terminal-tile'],
  'notification-navigation': ['settings-navigation'],
  'permission-request': ['permission-request-card'],
  'project-tabs': ['project-tab-bar'],
  'tag-chip': [],
  'terminal-rename-dialog': ['terminal-rename'],
  'ticket-info-panel': ['ticket-inspector__content'],
  'ticket-inspector-skeleton': ['ticket-inspector'],
  'ticket-row': ['ticket-list-row'],
  'ticket-timeline': ['ticket-inspector__timeline'],
};

function demos(source) {
  const file = ts.createSourceFile(catalogPath, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS),
    result = [];
  const visit = (node) => {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === 'demo' &&
      node.arguments.length >= 5 &&
      node.arguments.slice(0, 3).every(ts.isStringLiteralLike) &&
      node.arguments[4].kind === ts.SyntaxKind.TrueKeyword
    ) {
      result.push({
        id: node.arguments[0].text,
        name: node.arguments[1].text,
        purpose: node.arguments[2].text,
      });
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return result;
}

async function componentExports() {
  const result = [];
  for (const file of (await readdir(componentsPath)).filter((name) => name.endsWith('.tsx')).sort()) {
    const source = await readFile(resolve(componentsPath, file), 'utf8');
    result.push({
      base: file.slice(0, -4),
      names: [...source.matchAll(/export (?:function|const) ([A-Z]\w*)/g)].map((match) => match[1]),
    });
  }
  return result;
}

function geometry(id) {
  if (compositionIds.has(id))
    return {
      margin: 'parent',
      border: 'child',
      padding: 'child',
      notes: [
        'The embedding layout positions this composition; its contained Kerf and Hot Sheet surfaces retain their own border and padding.',
      ],
    };
  if (selfGeometryIds.has(id)) return { margin: 'none', border: 'self', padding: 'self' };
  return { margin: 'none', border: 'child', padding: 'child' };
}

export async function expectedCatalogExtension() {
  const [source, components] = await Promise.all([readFile(catalogPath, 'utf8'), componentExports()]);
  const entries = demos(source)
    .filter((demo) => components.some((component) => component.base === demo.id || component.names.includes(demo.name)))
    .map((demo) => ({
      id: demo.id,
      name: demo.name,
      kind: compositionIds.has(demo.id) ? 'composition' : 'component',
      purpose: demo.purpose,
      useWhen: [`Hot Sheet needs ${demo.purpose.charAt(0).toLowerCase()}${demo.purpose.slice(1)}`],
      avoidWhen: [`The interaction does not need Hot Sheet's domain-specific ${demo.name} behavior.`],
      publicClasses: publicClassOverrides[demo.id] ?? [demo.id],
      publicTokens: [],
      geometry: geometry(demo.id),
      documentation: 'docs/ux-components.md',
    }));
  return {
    $schema: '../node_modules/@kerfjs/ui/ai/component-catalog-extension.schema.json',
    schemaVersion: 1,
    package: 'hotsheet-web',
    description: 'Hot Sheet application-owned visual components and compositions used with Kerf UI.',
    entries,
  };
}

const expected = `${JSON.stringify(await expectedCatalogExtension(), null, 2)}\n`;
if (process.argv.includes('--check')) {
  const actual = await readFile(outputPath, 'utf8').catch(() => '');
  if (actual !== expected) {
    console.error('ai/component-catalog-extension.json is stale; run npm run catalog:sync');
    process.exitCode = 1;
  }
} else await writeFile(outputPath, expected);
