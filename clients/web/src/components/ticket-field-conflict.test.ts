import { describe, expect, it } from 'vitest';

import { TicketFieldConflict } from './ticket-field-conflict';

describe('TicketFieldConflict', () => {
  it('presents both versions and an editable merge choice', () => {
    const html = String(
      TicketFieldConflict({
        conflict: { key: 'details', field: 'details', label: 'Details', base: 'Base', mine: 'Mine', theirs: 'Theirs' },
        resolution: 'Mine plus theirs',
      }),
    );
    expect(html).toContain('data-component="ticket-field-conflict"');
    expect(html).toContain('Their latest version');
    expect(html).toContain('Your version');
    expect(html.match(/data-component="sunken-panel"/g)).toHaveLength(2);
    expect(html).toContain('<output>Theirs</output>');
    expect(html).toContain('<output>Mine</output>');
    expect(html).toContain('name="ticket-conflict-resolution"');
    expect(html).toContain('Mine plus theirs');
    expect(html).toContain('data-action="accept-remote-ticket-field"');
    expect(html).toContain('data-action="apply-ticket-field-merge"');
    expect(html).toContain('data-lucide="git-merge"');
  });

  it('routes its actions and merged field to another owner, such as an attachment label (HS2-9DQXGY)', () => {
    const html = String(
      TicketFieldConflict({
        conflict: {
          key: 'attachment_label:k1',
          field: 'attachment_label',
          label: 'Attachment Label',
          base: 'Repro',
          mine: 'Repro on Safari',
          theirs: 'Repro on WebKit',
        },
        resolution: 'Repro on Safari',
        acceptAction: 'accept-remote-attachment-label',
        mergeAction: 'apply-attachment-label-merge',
        resolutionName: 'attachment-label-conflict-resolution',
      }),
    );
    expect(html).toContain('Resolve attachment label conflict');
    expect(html).toContain('data-conflict-field="attachment_label:k1"');
    expect(html).toContain('name="attachment-label-conflict-resolution"');
    expect(html).toContain('data-action="accept-remote-attachment-label"');
    expect(html).toContain('data-action="apply-attachment-label-merge"');
    expect(html).not.toContain('ticket-conflict-resolution');
    expect(html).not.toContain('accept-remote-ticket-field');
  });
});
