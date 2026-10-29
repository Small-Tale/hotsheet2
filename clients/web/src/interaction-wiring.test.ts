import { readFileSync } from 'node:fs';

import type { AttrSpec } from 'kerfjs';
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

it('HS2-E0H7S0 native capture uses explicit local recovery rather than discarded server flush promises', () => {
  const capture = runtime.slice(
    runtime.indexOf('listenForDesktopRecovery(window,'),
    runtime.indexOf(
      "document.addEventListener('visibilitychange'",
      runtime.indexOf('listenForDesktopRecovery(window,'),
    ),
  );
  expect(capture).toContain('captureTicketDraftRecovery()');
  expect(capture).not.toContain('flushTicketDrafts()');
  const attachments = read('./interactions/attachments-and-gallery.ts');
  const attachmentCapture = attachments.slice(
    attachments.indexOf('listenForDesktopRecovery('),
    attachments.indexOf('document.addEventListener(', attachments.indexOf('listenForDesktopRecovery(')),
  );
  expect(attachmentCapture).toContain('labelEditor.captureRecovery()');
  expect(attachmentCapture).toContain('filenameEditor.captureRecovery()');
  expect(attachments).toContain('void filenameEditor.finish()'); // ordinary pagehide remains intact
  expect(read('./features/ticket-workflows.ts')).toContain('previous?.destination === key');
  expect(read('./attachment-label-editing.ts')).toContain('previous?.destination === key');
});
/**
 * Every shared attr-spec table, so a `TABLE.key.selector` argument resolves to the literal
 * selector string it registers and the inventory keeps proving byte-identical selectors.
 */
const attrTables = Object.assign(
  {},
  ...Object.values(
    import.meta.glob<Record<string, Record<string, AttrSpec>>>('./interaction-attrs/*.ts', { eager: true }),
  ),
) as Partial<Record<string, Partial<Record<string, AttrSpec>>>>;
const registrationArgument = (text: string) => {
  const spec = /^(\w+)\.(\w+)\.selector$/.exec(text);
  if (!spec) return text;
  const selector = attrTables[spec[1]]?.[spec[2]]?.selector;
  expect(selector, text).toBeDefined();
  return `'${selector!}'`;
};
const registrations = new Set([
  'delegate',
  'delegateCapture',
  'wireTabBars',
  'wireSearchFields',
  'wireTicketSearchFields',
  'wireRepositoryBrowser',
  'wireTicketCollection',
  'wireCommandSettingsEditor',
  'wireNotificationSettings',
  'wireBulkTicketDialog',
  'wireAiConversation',
  'wireTicketTitleEditor',
  'wireTicketTagEditor',
  'wireAttachmentContextMenuOpeners',
  'wireFeedbackChoices',
  'wireBlockedReasonEditor',
  'wireTerminalKeyBar',
  'wireTerminalClipboardActions',
  'wireTerminalClipboardSheets',
  'wireTerminalRenameDialog',
  'wireWorkspaceHeaderActions',
  'wireSearchScopeToolbar',
  'wireAttachmentGalleryControls',
  'wireTicketAttachmentActions',
  'wireQuickTicketComposer',
  'wireTrashSettings',
  'wireContextMenuDismissal',
  'wireTicketRowUpNextToggle',
  'wireTerminalDashboardMenu',
  'wireTerminalTextSizeControl',
  'wireMarkdownDetailsEditor',
  'wireNoteComposer',
  'wireNoteEditor',
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
    expect([...applicationWiring.matchAll(/registrations\.(\w+)\(\)/g)].map((match) => match[1])).toEqual(
      orderedRegistrations,
    );
    expect(runtime.match(/wireHotSheetInteractions\(/g)).toHaveLength(1);
    expect(runtime).not.toMatch(/const register\w+Interactions/);
    expect(interactionBindings.match(/wire\w+Interactions\(dependencies\)/g)).toHaveLength(groups.length);
    for (const [, group] of groups) expect(interactionBindings).toContain(`${group}(dependencies)`);
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
                  .map((argument) => sourceTokens(registrationArgument(argument.getText(tree)))),
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
    expect(actual.filter((line) => /\tdelegate(?:Capture)?\t/.test(line))).toHaveLength(382); // HS2-B8M0QB, HS2-63D6F9, HS2-1YXCCW, HS2-RFJ1GN, HS2-2Z11A5, HS2-PWNPM2, HS2-9YVX6Q
    expect(
      actual
        .filter((line) => /\tdelegate(?:Capture)?\t/.test(line))
        .every((line) => line.split('\t')[2] === 'document.body'),
    ).toBe(true);
    expect(main).not.toMatch(/delegate(?:Capture)?\(document\.body/);
    expect(runtime).not.toMatch(/delegate(?:Capture)?\(document\.body/);
  });

  it('resolves every interaction-attrs table key through its spec rather than a literal selector', () => {
    expect(Object.keys(attrTables).length).toBeGreaterThanOrEqual(12);
    for (const [file] of groups)
      expect(read(`./interactions/${file}.ts`), file).not.toMatch(
        /delegate(?:Capture)?\(\s*document\.body,\s*'[^']+',\s*'\[[\w-]+="[^"]*"\]'/,
      );
  });

  it('retains shared Kerf tab and search-field adapters in their owning modules', () => {
    const search = read('./interactions/search-and-composer.ts'),
      navigation = read('./interactions/navigation-and-tabs.ts');
    // Workspace, saved-view, and guidance fields are Kerf model-managed (HS2-5JXBQY, HS2-6ZA2RN).
    expect(search).toContainSource(
      "models:{'workspace-search':workspaceSearchModel,'saved-view-query':savedViewSearchModel,'guidance-search':guidanceSearchModel}",
    );
    expect(search).not.toContain('onEdit:');
    expect(search).not.toContain("delegate(document.body,'input','[data-search-field-editor=\"workspace-search\"]'");
    expect(navigation).toContainSource('if(barId===PROJECT_TAB_BAR_ID)');
    expect(navigation).toContainSource('if(barId!==TERMINAL_DRAWER_TAB_BAR_ID)return;');
    expect(navigation).not.toContain('DRAWER_APP_TAB_SELECTOR');
  });

  it('keeps main.tsx as a bounded side-effect bootstrap', () => {
    expect(main.split('\n')).toHaveLength(46); // HS2-E0H7S0: install native close after parent readiness.
    // Kerf DialogSurface's register touches the document, so only the browser entry loads it (HS2-BH7NZ4).
    expect(main).toContain("import '@kerfjs/ui/surface-scaffold/register';");
    // Page-lifetime iOS keyboard root-scroll restore (HS2-BCA512).
    expect(main).toContain('installDocumentScrollRestore(window);');
    expect(main).toContain('const { appRoot } = await startHotSheetWebClient();');
    // HS2-RKZKX7: hydrate desktop state before application defaults inspect localStorage.
    expect(main).toContain("'__HOTSHEET_DESKTOP_STORAGE__' in window ? await initializeDesktopStorage() : undefined");
    // HS2-E0H7S0: native close callback becomes ready only after the real parent's recovery listeners.
    expect(main.indexOf('installDesktopClose(desktopStorage)')).toBeGreaterThan(
      main.indexOf('await startHotSheetWebClient()'),
    );
    expect(main.indexOf('await initializeDesktopStorage()')).toBeLessThan(
      main.indexOf('await startHotSheetWebClient()'),
    );
    // The single page-lifetime scroll-divider instance lives at the entry (HS2-TF76Z2).
    expect(main).toContain('void wireScrollDividers(appRoot, { targets: [TERMINAL_VISIBILITY_SCROLL_TARGET] });');
    expect(runtime).not.toContain('wireScrollDividers(');
    // SplitView's page-lifetime resize wiring contract lives beside it (HS2-3B8345).
    expect(main).toContain('void wireResizableRegions(appRoot, { onCommit: () => undefined });');
    expect(runtime).not.toContain('wireResizableRegions(');
    // ChoiceChips' documented wiring contract (Kerf beta.106, HS2-BH7NZ4).
    expect(main).toContain('void wireChoiceChips(appRoot);');
    expect(main).toContain('void wireTextFields(appRoot);');
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
