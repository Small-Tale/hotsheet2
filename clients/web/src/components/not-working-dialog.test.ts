import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { NotWorkingDialog } from './not-working-dialog';

describe('NotWorkingDialog', () => {
  it('accepts either notes or pending attachments and exposes stable handlers', () => {
    const empty = String(NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [] }));
    expect(empty).toContain('data-action="submit-not-working"');
    expect(empty).toContain('disabled>Report Not Working');
    expect(empty).toContain('aria-label="Browse evidence attachments"');
    const note = String(NotWorkingDialog({ slug: 'HS2-DONE', note: 'Regression', attachments: [] }));
    expect(note).not.toContain('disabled>Report Not Working');
    const attachment = String(NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [{ id: 'pending-1', name: 'proof.png' }] }));
    expect(attachment).toContain('data-pending-attachment-id="pending-1"');
    expect(attachment).not.toContain('disabled>Report Not Working');
    expect(attachment).toContain('aria-label="Remove proof.png"');
  });

  it('capability-gates attachments and preserves explicit failure/submitting states', () => {
    const unsupported = String(NotWorkingDialog({ slug: 'HS2-DONE', note: 'Broken', attachments: [], attachmentsEnabled: false }));
    expect(unsupported).toContain('does not support attachments');
    expect(unsupported).not.toContain('pending-attachment-picker');
    const notesUnsupported = String(NotWorkingDialog({ slug: 'HS2-DONE', note: '', attachments: [], notesEnabled: false }));
    expect(notesUnsupported).toContain('does not support notes');
    expect(notesUnsupported).not.toContain('name="not-working-note"');
    const submitting = String(NotWorkingDialog({ slug: 'HS2-DONE', note: 'Broken', attachments: [], submitting: true, error: 'Upload failed' }));
    expect(submitting).toContain('Submitting…');
    expect(submitting).toContain('role="alert">Upload failed');
  });

  it('keeps long filenames shrinkable and uses pointer/not-allowed cursor semantics', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'pending-attachment-picker.css'), 'utf8');
    expect(css).toContain('min-width: 0; overflow: hidden; flex: 1; text-overflow: ellipsis');
    const dialogCss = readFileSync(resolve(import.meta.dirname, 'not-working-dialog.css'), 'utf8');
    expect(dialogCss).toContain('button:disabled { cursor: not-allowed;');
  });
});
