import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

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
  'wireRemoteDevicesSettings',
  'wireRegisteredProjectsSettings',
  'wireBulkTicketDialog',
  'wireTicketGroups',
  'wireOrchestrationTimeline',
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
  'wireMinimizedTicketsControl',
  'wireAttachmentGalleryControls',
  'wireTicketAttachmentActions',
  'wireQuickTicketComposer',
  'wireOrchestrationSettings',
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
    expect(actual.filter((line) => /\tdelegate(?:Capture)?\t/.test(line))).toHaveLength(501); // HS2-SSD28G accepts ticket drops on dragenter (2); HS2-PQWZRG added plain-text search paste; HS2-DR0ZSQ added the Systems machine rows; HS2-K7CZR2 moved the advisory and note-delivery actions out of a raw body listener (7); HS2-W1XEH7 added Show minimized; HS2-HC67SP added the project heading toggle; HS2-EEBM4W added the HS1 job details close; HS2-S2T4VJ added drawer Retry and Close; HS2-4Q9DVS added minimize and restore; HS2-RWA8V6 added backup controls; HS2-7VB81E added the existing-repository choice, HS2-RJRRSF added selected terminal right-click, HS2-4B0KSD added Renew, Dismiss and Pair Again (3), HS2-WNK73S added Retry now, HS2-3GJZGG feature checkboxes and Restore Defaults, HS2-Q8WF64 added Minimize (2), HS2-7HPXCY added phone search (3), HS2-FN018Q auto-group toggle, HS2-MN0MXT removed the preview action; HS2-CHH28E, HS2-6T8C1D, HS2-J4G9GB, HS2-MCGCPQ, HS2-BT4KN6, HS2-KBVZV1, HS2-NHK8V8, HS2-EFTHS6, HS2-9AN4DN, HS2-JJ9RYN, HS2-TWZSZF, HS2-N2AG8V, HS2-PRXD39, HS2-EQVZTZ, HS2-F788GW, HS2-B8M0QB, HS2-63D6F9, HS2-1YXCCW, HS2-RFJ1GN, HS2-2Z11A5, HS2-PWNPM2, HS2-9YVX6Q, HS2-ZP869N, HS2-KPJWBK, HS2-C7GMQN, HS2-8QJ6W3, HS2-5E8Y9R, HS2-Q0GWXT, HS2-NBDXPW, HS2-M5QPED
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
    // Workspace, saved-view, guidance, and terminal fields are Kerf model-managed (HS2-5JXBQY, HS2-6ZA2RN, HS2-TWZSZF).
    expect(search).toContainSource(
      "models:{'workspace-search':workspaceSearchModel,'saved-view-query':savedViewSearchModel,'guidance-search':guidanceSearchModel,[TERMINAL_SEARCH_FIELD_ID]:terminalSearchModel}",
    );
    expect(search).not.toContain('onEdit:');
    expect(search).not.toContain("delegate(document.body,'input','[data-search-field-editor=\"workspace-search\"]'");
    expect(navigation).toContainSource('if(barId===PROJECT_TAB_BAR_ID)');
    expect(navigation).toContainSource('if(barId!==TERMINAL_DRAWER_TAB_BAR_ID)return;');
    expect(navigation).not.toContain('DRAWER_APP_TAB_SELECTOR');
  });

  it('keeps main.tsx as a bounded side-effect bootstrap', () => {
    // HS2-2055AY/HS2-7DBJMS: Kerf overlay wiring and remote approval remain in the bounded browser entry.
    // HS2-ZF70Y9 added the host-gated `/ux-demo` branch ahead of the app boot.
    expect(main.split('\n')).toHaveLength(67);
    // The catalog boot is not awaited: its chunk imports this entry chunk, and awaiting it from the
    // entry's top-level await deadlocks both modules (HS2-ZF70Y9).
    expect(main).toContain(
      'if (shouldBootUxDemo(window.location.pathname, documentHostFeatures(document))) void bootUxDemo(document);',
    );
    expect(main).not.toContain('await bootUxDemo(');
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
    // SplitView's page-lifetime resize wiring contract lives beside it (HS2-3B8345) and routes standalone
    // region commits to their owners (HS2-BS0XFK).
    expect(main).toContain('void wireResizableRegions(appRoot, { onCommit: regionCommitRoutes.commit });');
    expect(runtime).not.toContain('wireResizableRegions(');
    // ChoiceChips' documented wiring contract (Kerf beta.106, HS2-BH7NZ4).
    expect(main).toContain('void wireChoiceChips(appRoot);');
    expect(main).toContain('void wireTextFields(appRoot);');
    // Kerf Form's required page-lifetime wiring (beta.124, KF-JPE38T, HS2-GAJSX9).
    expect(main).toContain('void wireForms(appRoot);');
    expect(main).toContain('void wireOverlaySheets(appRoot);');
    expect(main).toContain('void wirePinnablePopover(surface);');
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

describe('hs2_k7czr2 page listeners are owned by a lifetime', () => {
  const srcRoot = fileURLToPath(new URL('.', import.meta.url));
  const sources = (directory: string): string[] =>
    readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) return sources(path);
      return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
    });
  const files = sources(srcRoot).map((path) => [relative(srcRoot, path), readFileSync(path, 'utf8')] as const);

  it('hs2_k7czr2_no_raw_body_listeners keeps document.body listeners in sanctioned modules', () => {
    // Each sanctioned module removes its listener through its own lifetime; everything else
    // registers through Kerf `delegate()` in an interaction group's scope.
    const sanctioned: Record<string, string> = {
      'features/halted-sessions.tsx': 'document.body.removeEventListener(TOP_LAYER_PRESENTED_EVENT, acknowledge)',
    };
    const offenders = files
      .filter(([, source]) => /document\.body\.addEventListener\(/.test(source))
      .map(([path]) => path.split('\\').join('/'));
    expect(offenders.sort()).toEqual(Object.keys(sanctioned).sort());
    for (const [path, removal] of Object.entries(sanctioned))
      expect(files.find(([file]) => file.split('\\').join('/') === path)![1]).toContain(removal);
  });

  it('hs2_k7czr2_overlay_disposer_retained keeps every wireTopLayerOverlays disposer', () => {
    // Call sites, not comments that name the helper.
    const calls = files.flatMap(([path, source]) =>
      path.startsWith('top-layer-overlay')
        ? []
        : source
            .split('\n')
            .map((line) => line.trim())
            .filter((line) => line.includes('wireTopLayerOverlays(') && !/^(?:\/\/|\*|\/\*)/.test(line))
            .map((line) => [path.split('\\').join('/'), line] as const),
    );
    expect(calls.map(([path]) => path).sort()).toEqual(['interactions/terminals.ts', 'ux-demo/main.tsx']);
    // A bare statement discards the disposer; the group or demo scope must own it.
    for (const [, line] of calls) expect(line).toMatch(/^(?:lifetime|demoListeners)\.add\(wireTopLayerOverlays\(/);
  });
});
