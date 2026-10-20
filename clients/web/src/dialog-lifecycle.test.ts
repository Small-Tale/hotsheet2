import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('dialog lifecycle event contracts', () => {
  const source = [
    './app/runtime.tsx',
    './features/hot-sheet-app.tsx',
    './features/hot-sheet-shell.tsx',
    './interactions/terminals.ts',
    './interactions/ticket-selection.ts',
    './interactions/commands-and-ai.ts',
    './interactions/inspector-and-editor.ts',
    './interactions/search-and-composer.ts',
    './features/saved-views.ts',
    './features/project-teardown.ts',
  ]
    .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
    .join('\n');

  it('uses cancelable Web Awesome wa-hide for every component dialog', () => {
    for (const selector of [
      'data-terminal-visibility-name-dialog',
      'data-terminal-rename-dialog',
      'bulk-tag-dialog',
      'bulk-delete-dialog',
      'not-working-dialog',
    ]) {
      expect(source).toContain(`'wa-hide'`);
      expect(source).not.toMatch(new RegExp(`wa-request-close[^\\n]*${selector}`));
    }
    expect(source).toMatchSource(
      /'wa-hide',TICKET_SELECTION_TARGETS\.notWorkingDialog\.selector.*activeLabel.*!notWorkingSubmitting\.value.*dialog\.getAttribute\('aria-label'\)===activeLabel.*closeNotWorking\(\)/,
    );
  });

  it('clears command dialog state on a captured Kerf close request (HS2-Z7C27E, HS2-BH7NZ4)', () => {
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'kui-request-close','\[data-dialog-component="command-run-dialog"\], \[data-dialog-component="command-cancellation-dialog"\]'/,
    );
    expect(source).not.toMatch(/wa-request-close[^\n]*command-run-dialog/);
    expect(source).not.toContain("'wa-request-close'");
  });

  it('lets Kerf request TicketReader dismissal and completes state changes after native close', () => {
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'kui-request-close',INSPECTOR_AND_EDITOR_TARGETS\.ticketReaderDialog\.selector/,
    );
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'close',INSPECTOR_AND_EDITOR_TARGETS\.ticketReaderDialog\.selector/,
    );
    expect(source).toContainSource('wireDialogStack(appRoot)');
    expect(source).not.toMatch(
      /delegate(?:Capture)?\(document\.body,'keydown',INSPECTOR_AND_EDITOR_TARGETS\.ticketReader\.selector/,
    );
  });

  it('lets the persistent quick-ticket dialog own modality and Escape ordering', () => {
    // HS2-NRW5ZR moved the composer gestures into the shared wireQuickTicketComposer helper.
    const helper = readFileSync(new URL('./components/quick-ticket-composer-interactions.ts', import.meta.url), 'utf8');
    expect(source).toMatchSource(
      /wireQuickTicketComposer\(document\.body,\{expand:openTicketComposer,expanded:\(\)=>composerExpanded\.value,canHide:\(\)=>!composerSubmitting\.value,dismissed:resetTicketComposer/,
    );
    expect(helper).toMatchSource(
      /SEARCH_AND_COMPOSER_ACTIONS\.expandTicketComposer\.selector,\(_event,target\)=>\{bindings\.expand\(/,
    );
    expect(helper).toMatchSource(
      /delegateCapture\(root,'wa-hide',COMPOSER,[^\n]*!bindings\.canHide\(\)\)event\.preventDefault\(\)/,
    );
    expect(helper).toMatchSource(
      /delegateCapture\(root,'wa-after-hide',COMPOSER,[^\n]*bindings\.expanded\(\)\)bindings\.dismissed\(\)/,
    );
    expect(source).not.toMatch(/if\(composerExpanded\.value\)resetTicketComposer\(\).*ticketContextMenu/);
    expect(source).toMatchSource(/function closeProjectIds\([^\n]*resetTicketComposer\(\)/);
    expect(source).toMatchSource(/function activateOpenProject\([^\n]*resetTicketComposer\(false\)/);
  });

  it('synchronously resets the saved-view live name before leaving opening focus to native autofocus', () => {
    const opening = source.slice(
      source.indexOf('function showSavedViewDialog()'),
      source.indexOf('function setSavedViewQuery('),
    );
    expect(opening).toContainSource('savedViewDialogOpen.value=true');
    expect(opening).toContainSource('document.querySelector<Control>(\'[name="saved-view-name"]\')');
    expect(opening).toContainSource('if(name&&name.value!==savedViewName.value)name.value=savedViewName.value');
    expect(opening).not.toMatch(/requestAnimationFrame|setTimeout|\.show\(|\.focus\(/);
    const component = readFileSync(new URL('./components/saved-view-dialog.tsx', import.meta.url), 'utf8');
    expect(component).toMatch(/name="saved-view-name"[\s\S]*?autofocus/);
    // The Kerf DialogSurface opens through the controlled-open pass (`openDialog`), not an `open` attribute.
    expect(component).toContain("'data-controlled-open': String(open)");
    expect(component).not.toContain('open={open');
  });

  it('does not mount a dormant Not Working dialog host without a ticket target', () => {
    expect(source).toContainSource('{target.slug&&<NotWorkingSurface');
  });
});
