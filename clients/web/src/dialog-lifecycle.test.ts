import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('dialog lifecycle event contracts', () => {
  const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');

  it('uses cancelable Web Awesome wa-hide for every component dialog', () => {
    for (const selector of ['data-terminal-visibility-name-dialog', 'data-terminal-rename-dialog', 'bulk-tag-dialog', 'bulk-delete-dialog', 'not-working-dialog']) {
      expect(source).toContain(`'wa-hide'`);
      expect(source).not.toMatch(new RegExp(`wa-request-close[^\\n]*${selector}`));
    }
    expect(source).toMatch(/'wa-hide','\[data-component="not-working-dialog"\]'.*notWorkingSubmitting\.value.*event\.preventDefault\(\).*closeNotWorking\(\)/);
  });

  it('uses captured native close for the remaining HTML command dialogs', () => {
    expect(source).toMatch(/delegateCapture\(document\.body,'close','\[data-component="command-run-dialog"\], \[data-component="command-cancellation-dialog"\]'/);
    expect(source).not.toMatch(/wa-request-close[^\n]*command-run-dialog/);
    expect(source).not.toContain("'wa-request-close'");
  });

  it('lets Web Awesome own TicketReader dismissal and completes state changes after hide', () => {
    expect(source).toMatch(/delegateCapture\(document\.body,'wa-hide','\[data-component="ticket-reader"\]'/);
    expect(source).toMatch(/delegateCapture\(document\.body,'wa-after-hide','\[data-component="ticket-reader"\]'/);
    expect(source).not.toMatch(/delegate(?:Capture)?\(document\.body,'keydown','\[data-component="ticket-reader"\]'/);
  });

  it('lets the persistent quick-ticket dialog own modality and Escape ordering', () => {
    expect(source).toMatch(/expand-ticket-composer[^\n]*openTicketComposer/);
    expect(source).toMatch(/delegateCapture\(document\.body,'wa-hide','\[data-component="quick-ticket-composer"\]'[^\n]*composerSubmitting\.value[^\n]*event\.preventDefault\(\)/);
    expect(source).toMatch(/delegateCapture\(document\.body,'wa-after-hide','\[data-component="quick-ticket-composer"\]'[^\n]*resetTicketComposer\(\)/);
    expect(source).not.toMatch(/if\(composerExpanded\.value\)resetTicketComposer\(\).*ticketContextMenu/);
    expect(source).toMatch(/function closeProjectIds\([^\n]*resetTicketComposer\(\)/);
    expect(source).toMatch(/function activateOpenProject\([^\n]*resetTicketComposer\(false\)/);
  });

  it('does not mount a dormant Not Working dialog host without a ticket target', () => {
    expect(source).toContain('{target.slug&&<NotWorkingSurface');
  });
});
