import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { NotWorkingDialog } from './not-working-dialog';
import { PendingAttachmentPicker } from './pending-attachment-picker';

describe('NotWorkingDialog', () => {
  it('accepts either notes or pending attachments and exposes stable handlers', () => {
    const empty = String(NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [] }));
    expect(empty).toContain('data-action="submit-not-working"');
    expect(empty).toMatch(/<button(?=[^>]*disabled)[^>]*>[^]*Report Not Working<\/span><\/button>/);
    expect(empty).toContain('aria-label="Browse evidence attachments"');
    const note = String(NotWorkingDialog({ slug: 'HS2-DONE', note: 'Regression', attachments: [] }));
    expect(note).not.toMatch(/<button(?=[^>]*disabled)[^>]*>[^]*Report Not Working<\/span><\/button>/);
    const attachment = String(
      NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [{ id: 'pending-1', name: 'proof.png' }] }),
    );
    expect(attachment).toContain('data-pending-attachment-id="pending-1"');
    expect(attachment).not.toMatch(/<button(?=[^>]*disabled)[^>]*>[^]*Report Not Working<\/span><\/button>/);
    expect(attachment).toContain('aria-label="Remove proof.png"');
  });

  it('capability-gates attachments and preserves explicit failure/submitting states', () => {
    const unsupported = String(
      NotWorkingDialog({ slug: 'HS2-DONE', note: 'Broken', attachments: [], attachmentsEnabled: false }),
    );
    expect(unsupported).toContain('does not support attachments');
    expect(unsupported).not.toContain('pending-attachment-picker');
    const notesUnsupported = String(
      NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [], notesEnabled: false }),
    );
    expect(notesUnsupported).toContain('does not support notes');
    expect(notesUnsupported).not.toContain('name="not-working-note"');
    const submitting = String(
      NotWorkingDialog({ slug: 'HS2-DONE', note: 'Broken', attachments: [], submitting: true, error: 'Upload failed' }),
    );
    expect(submitting).toContain('Submitting…');
    expect(submitting).toMatch(/role="alert"><p class="kui-text"[^>]*data-tone="danger"[^>]*>Upload failed/);
    expect(String(NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [] }))).toContain(
      '<div class="not-working-dialog__error" role="alert"></div>',
    );
  });

  it('relabels the shared evidence flow when reopening a verified or archived ticket', () => {
    const markup = String(
      NotWorkingDialog({ slug: 'HS2-REOPEN', mode: 'reopen', note: 'Regression', attachments: [] }),
    );
    expect(markup).toContain('Reopen Ticket — HS2-REOPEN');
    expect(markup).toContain('What needs another attempt?');
    expect(markup).toContain('>Reopen Ticket</span></button>');
  });

  it('keeps long filenames shrinkable and uses pointer/not-allowed cursor semantics', () => {
    // Long names truncate through Kerf Text; the picker CSS no longer lays out rows (HS2-0NGK56).
    const picker = String(PendingAttachmentPicker({ attachments: [{ id: 'long', name: 'a'.repeat(120) }] }));
    expect(picker).toContain('data-wrap="truncate"');
    const css = readFileSync(resolve(import.meta.dirname, 'pending-attachment-picker.css'), 'utf8');
    expect(css).not.toContain('__item');
    // Kerf buttons own their pointer/not-allowed cursors; the dialog does not style buttons (HS2-P6ZJDS).
    const dialogCss = readFileSync(resolve(import.meta.dirname, 'not-working-dialog.css'), 'utf8');
    expect(dialogCss).not.toMatch(/button|::part|__footer|__form|__hint/);
    const markup = String(NotWorkingDialog({ slug: 'HS2-DONE', note: 'Broken', attachments: [] }));
    expect(markup).toMatch(
      /<button(?=[^>]*type="submit")(?=[^>]*data-tone="danger")[^>]*>[^]*Report Not Working<\/span><\/button>/,
    );
    expect(markup).toContain('aria-labelledby="not-working-note-label"');
  });
});
