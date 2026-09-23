import { readFileSync } from 'node:fs';

import ts from 'typescript';
import { describe, expect, it } from 'vitest';

import { sourceTokens } from './source-format-matchers';

const groups = [
  ['project-lifecycle', 'wireProjectLifecycleInteractions'],
  ['repository', 'wireRepositoryInteractions'],
  ['navigation-and-tabs', 'wireNavigationAndTabInteractions'],
  ['terminals', 'wireTerminalInteractions'],
  ['ticket-selection', 'wireTicketSelectionInteractions'],
  ['views-and-saved-views', 'wireViewAndSavedViewInteractions'],
  ['commands-and-ai', 'wireCommandAndAiInteractions'],
  ['notifications-and-links', 'wireNotificationAndLinkInteractions'],
  ['search-and-composer', 'wireSearchAndComposerInteractions'],
  ['attachments-and-gallery', 'wireAttachmentAndGalleryInteractions'],
  ['inspector-and-editor', 'wireInspectorAndEditorInteractions'],
  ['shell-and-global', 'wireShellAndGlobalInteractions'],
];
const read = (file: string) => readFileSync(new URL(file, import.meta.url), 'utf8');
const main = read('./main.tsx');
const runtime = read('./app/runtime.tsx');
const interactionBindings = read('./app/interaction-bindings.ts');
const applicationWiring = read('./app/wire-interactions.ts');
const registrations = new Set([
  'delegate',
  'delegateCapture',
  'wireTabBars',
  'wireTokenSearchFields',
  'window.addEventListener',
  'document.addEventListener',
]);

describe('feature-owned interaction wiring (HS2-YWF98M)', () => {
  it('invokes each side-effect-free group once in the original registration order', () => {
    const orderedRegistrations = [
      'projectLifecycle',
      'repository',
      'navigationAndTabs',
      'terminals',
      'ticketSelection',
      'viewsAndSavedViews',
      'commandsAndAi',
      'notificationsAndLinks',
      'searchAndComposer',
      'attachmentsAndGallery',
      'inspectorAndEditor',
      'shellAndGlobal',
    ];
    expect([...applicationWiring.matchAll(/registrations\.(\w+)\(\);/g)].map((match) => match[1])).toEqual(
      orderedRegistrations,
    );
    expect(runtime.match(/wireHotSheetInteractions\(/g)).toHaveLength(1);
    expect(runtime).not.toMatch(/const register\w+Interactions/);
    expect(interactionBindings.match(/wire\w+Interactions\(dependencies\);/g)).toHaveLength(groups.length);
    for (const [, group] of groups) expect(interactionBindings).toContain(`${group}(dependencies);`);
    for (const [file, group] of groups) {
      const source = read(`./interactions/${file}.ts`);
      const module = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
      expect(module.statements.some(ts.isExpressionStatement), file).toBe(false);
      expect(source, file).toContain(`export function ${group}(`);
      expect(source, file).not.toMatch(/from ['"].*main/);
      expect(source, file).not.toMatch(/\b(?:any|signal)\s*[<(]/);
    }
  });

  it('preserves every ordered event/root/selector and capture registration from the pre-extraction baseline', () => {
    const actual: string[] = [];
    for (const [file, group] of groups) {
      const tree = ts.createSourceFile(file, read(`./interactions/${file}.ts`), ts.ScriptTarget.Latest, true);
      const declaration = tree.statements.find(
        (node): node is ts.FunctionDeclaration => ts.isFunctionDeclaration(node) && node.name?.text === group,
      )!;
      const visit = (node: ts.Node) => {
        if (ts.isCallExpression(node)) {
          const call = node.expression.getText(tree);
          if (registrations.has(call))
            actual.push(
              [
                file,
                call,
                ...node.arguments
                  .slice(0, call.startsWith('delegate') ? 3 : 1)
                  .map((argument) => sourceTokens(argument.getText(tree))),
              ].join('\t'),
            );
        }
        ts.forEachChild(node, visit);
      };
      visit(declaration.body!);
    }
    const baseline = read('./interactions/registration-inventory.txt')
      .trimEnd()
      .split('\n')
      .map((line) =>
        line
          .split('\t')
          .map((part, index) => (index > 1 ? sourceTokens(part) : part))
          .join('\t'),
      );
    expect(actual).toEqual(baseline);
    expect(actual.filter((line) => /\tdelegate(?:Capture)?\t/.test(line))).toHaveLength(404);
    expect(
      actual
        .filter((line) => /\tdelegate(?:Capture)?\t/.test(line))
        .every((line) => line.split('\t')[2] === 'document.body'),
    ).toBe(true);
    expect(main).not.toMatch(/delegate(?:Capture)?\(document\.body/);
    expect(runtime).not.toMatch(/delegate(?:Capture)?\(document\.body/);
  });

  it('retains shared Kerf tab and token-search adapters in their owning modules', () => {
    const search = read('./interactions/search-and-composer.ts'),
      navigation = read('./interactions/navigation-and-tabs.ts');
    expect(search).toContainSource("onEdit:({id,editor,event})=>{if(id!=='workspace-search')return;");
    expect(search).not.toContain("delegate(document.body,'input','[data-token-search-editor=\"workspace-search\"]'");
    expect(navigation).toContainSource('if(barId===PROJECT_TAB_BAR_ID)');
    expect(navigation).toContainSource('if(barId!==TERMINAL_DRAWER_TAB_BAR_ID)return;');
    expect(navigation).not.toContain('DRAWER_APP_TAB_SELECTOR');
  });

  it('keeps main.tsx as a bounded side-effect bootstrap', () => {
    expect(main.split('\n')).toHaveLength(13);
    expect(main).toContain('await startHotSheetWebClient();');
    expect(main).not.toMatch(/\b(?:signal|mount|effect|wire\w+Interactions)\s*\(/);
    expect(runtime).not.toMatch(/from ['"][^'"]*\/main['"]/);
  });

  it('preserves mutable interaction accessors when registrations share one live port', () => {
    const port = runtime.slice(
      runtime.indexOf('const interactionBindingsPort:'),
      runtime.indexOf('wireHotSheetInteractions(createHotSheetInteractionBindings'),
    );
    const getters = [...port.matchAll(/\bget (\w+)\(\)/g)].map((match) => match[1]);
    expect(getters.length).toBeGreaterThan(20);
    for (const name of getters) expect(port).toContain(`set ${name}(value)`);
  });
});
