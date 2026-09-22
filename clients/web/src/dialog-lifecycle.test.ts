import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('dialog lifecycle event contracts', () => {
  const source = [
    './main.tsx',
    './interactions/terminals.ts',
    './interactions/ticket-selection.ts',
    './interactions/commands-and-ai.ts',
    './interactions/inspector-and-editor.ts',
    './interactions/search-and-composer.ts',
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
      /'wa-hide','\[data-component="not-working-dialog"\]'.*activeLabel.*!notWorkingSubmitting\.value.*dialog\.getAttribute\('aria-label'\)===activeLabel.*closeNotWorking\(\)/,
    );
  });

  it('uses captured native close for the remaining HTML command dialogs', () => {
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'close','\[data-component="command-run-dialog"\], \[data-component="command-cancellation-dialog"\]'/,
    );
    expect(source).not.toMatch(/wa-request-close[^\n]*command-run-dialog/);
    expect(source).not.toContain("'wa-request-close'");
  });

  it('lets Web Awesome own TicketReader dismissal and completes state changes after hide', () => {
    expect(source).toMatchSource(/delegateCapture\(document\.body,'wa-hide','\[data-component="ticket-reader"\]'/);
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'wa-after-hide','\[data-component="ticket-reader"\]'/,
    );
    expect(source).not.toMatch(/delegate(?:Capture)?\(document\.body,'keydown','\[data-component="ticket-reader"\]'/);
  });

  it('lets the persistent quick-ticket dialog own modality and Escape ordering', () => {
    expect(source).toMatchSource(/expand-ticket-composer[^\n]*openTicketComposer/);
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'wa-hide','\[data-component="quick-ticket-composer"\]'[^\n]*composerSubmitting\.value[^\n]*event\.preventDefault\(\)/,
    );
    expect(source).toMatchSource(
      /delegateCapture\(document\.body,'wa-after-hide','\[data-component="quick-ticket-composer"\]'[^\n]*resetTicketComposer\(\)/,
    );
    expect(source).not.toMatch(/if\(composerExpanded\.value\)resetTicketComposer\(\).*ticketContextMenu/);
    expect(source).toMatch(/function closeProjectIds\([^\n]*resetTicketComposer\(\)/);
    expect(source).toMatch(/function activateOpenProject\([^\n]*resetTicketComposer\(false\)/);
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
    expect(component).toContain('open={open || undefined}');
  });

  it('does not mount a dormant Not Working dialog host without a ticket target', () => {
    expect(source).toContainSource('{target.slug&&<NotWorkingSurface');
  });
});
