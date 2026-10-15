import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('transient operation feedback', () => {
  it('uses toasts for completed actions instead of persistent surface messages', () => {
    const source = [
      './app/runtime.tsx',
      './interactions/repository.ts',
      './interactions/attachments-and-gallery.ts',
      './interactions/inspector-and-editor.ts',
      './features/corrupt-ticket-recovery.ts',
    ]
      .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
      .join('\n');
    for (const pattern of [
      /showToast\(`Opened in \$\{codeReview\.value\?\.difftool/,
      /showToast\('Attachment removed\.'\)/,
      /showToast\('Opened the file location\.'\)/,
      /showToast\(`Queued \$\{created\.slug\} for AI repair\.`\)/,
    ])
      expect(source).toMatch(pattern);

    expect(source).not.toMatch(/codeReviewMessage\.value=`Opened in/);
    expect(source).not.toContain("attachmentMessage.value='Attachment removed.'");
    expect(source).not.toMatch(/setCorruptRecovery\([^)]*,\{message:.*(?:Opened|Queued)/);
    // The command settings editor's old "Saved locally." Save-button toast was removed with the
    // Save button itself: the HS2-656XJ2/D9JBXT redesign autosaves on a debounce; success is silent
    // like ticket editing (HS2-MTBMH8) and the status line only carries validation or save errors.
    expect(source).not.toContain("commandSettingsMessage.value='Saved locally.'");
  });

  it('keeps autosave progress and success out of inline status lines (HS2-GHA1D3)', () => {
    const source = [
      './features/ai-configuration.tsx',
      './interactions/commands-and-ai.ts',
      './interactions/attachments-and-gallery.ts',
    ]
      .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
      .join('\n');
    // Progress shows in the global server-busy indicator; success is silent or a toast; the status
    // line keeps only failures.
    expect(source).not.toMatch(/Message\.value = '(?:Saving|Updating|Removing)[^']*…'/);
    expect(source).not.toMatch(/Message\.value = 'Saved/);
  });
});
