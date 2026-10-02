import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import {
  QuickTicketComposer,
  QuickTicketLauncher,
  showQuickTicketComposer,
  strandedNewTicketAttachments,
} from './quick-ticket-composer';

describe('QuickTicketComposer', () => {
  it('gives the title the available width while keeping category compact', () => {
    const css = readFileSync(new URL('./quick-ticket-composer.css', import.meta.url), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.quick-ticket-composer \{[^}]*padding: var\(--kui-space-m\)[^}]*gap: var\(--kui-space-m\)/);
    expect(css).toMatch(/__metadata \{[^}]*gap: var\(--kui-space-xs\)/);
    expect(css).toMatch(/__details \{[^}]*gap: var\(--kui-space-2xs\)/);
    expect(css).toMatch(
      /__attachment \{[^}]*padding: var\(--kui-space-2xs\) var\(--kui-space-xs\)[^}]*gap: var\(--kui-space-xs\)/,
    );
    expect(css).toMatchSource(
      /\.quick-ticket-dialog \{[^}]*--width:min\(remify\(928px\), calc\(100vw - remify\(32px\)\)\)/,
    );
    expect(css).toContain('grid-template-columns: minmax(0, 1fr) minmax(remify(192px), remify(240px))');
    expect(css).toMatchSource(
      /@media \(max-width: remify\(608px\)\)[^{]*\{[^}]*\.quick-ticket-composer \{ grid-template-columns: 1fr/,
    );
    // HS2-Q7WJ6T: the form sits inside the dialog panel, so its base rule must not draw its own
    // border or box-shadow (that redundant brand-colored rounded border read as a stray outline
    // below the dialog title). The drag drop-target highlight keeps its own box-shadow ring.
    const baseRule = css.match(/\n\.quick-ticket-composer \{([^}]*)\}/)![1];
    expect(baseRule).not.toMatch(/(^|;|\s)border:/);
    expect(baseRule).not.toContain('box-shadow:');
    expect(css).toMatchSource(
      /\.quick-ticket-composer:is\(\[data-dragging="true"\], \[data-dragging-ticket="true"\]\) \{[^}]*box-shadow:/,
    );
  });
  it('has distinct collapsed, editable, and provider-disabled presentations', () => {
    const collapsed = String(QuickTicketLauncher());
    expect(collapsed).toContain('data-ticket-drop-action="duplicate"');
    expect(collapsed).toContain('data-action="expand-ticket-composer"');
    expect(collapsed).toContain('data-component="quick-ticket-composer-launcher"');
    expect(collapsed).toContain('data-new-ticket-drop-target="true"');
    expect(collapsed).toContain('New ticket…');
    expect(String(QuickTicketLauncher({ label: 'Ticket…' }))).toContain('Ticket…');
    expect(String(QuickTicketLauncher({ label: 'Ticket…' }))).not.toContain('New ticket…');
    const collapsedComposer = String(QuickTicketComposer({ expanded: false }));
    expect(collapsedComposer).toContain('data-component="quick-ticket-composer"');
    expect(collapsedComposer).toContain('aria-hidden="true"');
    expect(collapsedComposer).toContain(' inert');
    expect(collapsedComposer).not.toContain(' open');
    expect(collapsedComposer).not.toContain('data-action="create-ticket-form"');
    const expanded = String(
      QuickTicketComposer({
        expanded: true,
        title: 'New work',
        details: 'Why this matters',
        category: 'bug',
        upNext: true,
        attachments: [{ id: 'proof', name: 'proof.png' }],
      }),
    );
    expect(expanded).toContain('data-ticket-drop-action="duplicate"');
    expect(expanded).toContain('<wa-dialog');
    expect(expanded).toContain('label="Create ticket"');
    expect(expanded).toContain('role="dialog"');
    expect(expanded).toContain('aria-modal="true"');
    expect(expanded).toContain('aria-label="Create ticket"');
    expect(expanded).toContain(' open');
    expect(expanded).not.toContain('light-dismiss');
    expect(expanded).toContain('data-action="create-ticket-form"');
    expect(expanded).toContain('value="New work"');
    expect(expanded).toContain('data-lucide="bug"');
    expect(expanded).toContain('name="new-ticket-details" rows="1" data-morph-skip>Why this matters</textarea>');
    expect(expanded).toContain('data-action="toggle-new-ticket-up-next"');
    expect(expanded).toContain('aria-pressed="true"');
    expect(expanded).toContain('data-lucide="star"');
    expect(expanded).toContain('Browse attachments for new ticket');
    expect(expanded).toContain('Drop attachment files anywhere in this area or browse');
    expect(expanded).toContain('data-pending-attachment-id="proof"');
    expect(expanded).toContain('aria-label="Remove proof.png" title="Remove proof.png"');
    expect(expanded).not.toContain('data-lucide="x"');
    expect(expanded).toContain('data-dialog="close"');
    const disabled = String(
      QuickTicketComposer({
        expanded: true,
        canCreate: false,
        attachmentsEnabled: false,
        providerName: 'Read-only Jira',
      }),
    );
    expect(disabled).toContain('does not support creating tickets');
    expect(disabled).toContain('does not support attachments');
    expect(disabled).not.toContain('name="new-ticket-attachments"');
    expect(disabled).toContain('disabled');
  });

  it('opens the persistent live Web Awesome dialog so it can remember its trigger', () => {
    const calls: string[] = [];
    const nativeDialog = { open: false, setAttribute: (name: string, value: string) => calls.push(`${name}:${value}`) };
    const dialog = {
      open: false,
      shadowRoot: { querySelector: () => nativeDialog },
      show: () => {
        calls.push('show');
        return Promise.resolve();
      },
    };
    const root = { querySelector: () => dialog } as unknown as ParentNode;
    expect(showQuickTicketComposer(root)).toBe(true);
    expect(calls).toEqual(['role:presentation', 'show']);
    dialog.open = true;
    nativeDialog.open = true;
    expect(showQuickTicketComposer(root)).toBe(false);
    expect(calls).toEqual(['role:presentation', 'show', 'role:presentation']);
  });

  it('keeps one-line details vertically resizable (auto-growing on touch) and places Up Next after category', () => {
    const css = readFileSync(new URL('./quick-ticket-composer.css', import.meta.url), 'utf8'),
      markup = String(QuickTicketComposer({ expanded: true }));
    expect(markup).toMatch(/new-ticket-category[\s\S]*toggle-new-ticket-up-next[\s\S]*new-ticket-details/);
    expect(markup).toMatch(/name="new-ticket-details"[^>]*data-morph-skip/);
    expect(css).toMatch(
      /__details textarea \{[^}]*min-height: var\(--hs-new-ticket-details-sidebar-height, remify\(40px\)\);[^}]*resize: var\(--hotsheet-textarea-resize, vertical\)/,
    );
  });

  it('shows creation progress and attachment errors accessibly', () => {
    const markup = String(
      QuickTicketComposer({
        expanded: true,
        submitting: true,
        attachmentMessage: 'proof.png could not be read',
        attachmentError: true,
      }),
    );
    expect(markup).toContain('data-submitting="true"');
    expect(markup).toContain('Creating…');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('proof.png could not be read');
  });

  it('offers a ticket source Select only when several sources can create tickets (HS2-NZMJBJ)', () => {
    const single = String(
      QuickTicketComposer({
        expanded: true,
        providerName: 'HS2 git tickets',
        sources: [{ value: 'git-a', label: 'HS2 git tickets' }],
        source: 'git-a',
      }),
    );
    expect(single).toContain('<span>Creating in HS2 git tickets</span>');
    expect(single).not.toContain('name="new-ticket-source"');
    const multiple = String(
      QuickTicketComposer({
        expanded: true,
        providerName: 'GitHub issues',
        sources: [
          { value: 'git-a', label: 'HS2 git tickets' },
          { value: 'github-b', label: 'GitHub issues' },
        ],
        source: 'github-b',
      }),
    );
    expect(multiple).toMatch(/class="quick-ticket-composer__source"[\s\S]*Creating in[\s\S]*name="new-ticket-source"/);
    expect(multiple).toContain('aria-label="Ticket source"');
    expect(multiple).toMatch(/<wa-select[^>]*name="new-ticket-source"[^>]*value="github-b"/);
    expect(multiple).toContain('HS2 git tickets');
    expect(multiple).not.toContain('<span>Creating in GitHub issues</span>');
    const submitting = String(
      QuickTicketComposer({
        expanded: true,
        submitting: true,
        sources: [
          { value: 'git-a', label: 'HS2 git tickets' },
          { value: 'github-b', label: 'GitHub issues' },
        ],
      }),
    );
    expect(submitting).toMatch(/<wa-select[^>]*name="new-ticket-source"[^>]*value="git-a"/);
    expect(submitting).toMatch(/<wa-select[^>]*name="new-ticket-source"[^>]*disabled/);
  });

  it('blocks creation while attachment screening is still busy', () => {
    const markup = String(QuickTicketComposer({ expanded: true, busy: true }));
    expect(markup).toContain('Create ticket');
    expect(markup).not.toContain('Creating…');
    expect(markup.match(/disabled/g)).toHaveLength(1);
  });
  it('keeps staged files but blocks Create across an attachment-less source switch and back (HS2-8HHHK3)', () => {
    expect(strandedNewTicketAttachments(0, true)).toBe(false);
    expect(strandedNewTicketAttachments(2, true)).toBe(false);
    expect(strandedNewTicketAttachments(0, false)).toBe(false);
    expect(strandedNewTicketAttachments(1, false)).toBe(true);
    const sources = [
        { value: 'git-a', label: 'HS2 git tickets' },
        { value: 'github-b', label: 'GitHub issues' },
      ],
      attachments = [
        { id: 'proof', name: 'proof.png' },
        { id: 'trace', name: 'trace.log' },
      ],
      render = (source: string, providerName: string, attachmentsEnabled: boolean, staged = attachments) =>
        String(
          QuickTicketComposer({
            expanded: true,
            title: 'Routed work',
            sources,
            source,
            providerName,
            attachments: staged,
            attachmentsEnabled,
          }),
        ),
      createDisabled = (markup: string) => /<wa-button[^>]*type="submit"[^>]*disabled/.test(markup);
    // Source A takes attachments: files are staged, the drop zone shows, and Create is enabled.
    const sourceA = render('git-a', 'HS2 git tickets', true);
    expect(sourceA).toContain('data-pending-attachment-id="proof"');
    expect(sourceA).toContain('data-pending-attachment-id="trace"');
    expect(sourceA).toContain('Drop attachment files anywhere in this area or browse');
    expect(sourceA).not.toContain('data-action="clear-new-ticket-attachments"');
    expect(sourceA).not.toContain('data-new-ticket-attachments-stranded');
    expect(createDisabled(sourceA)).toBe(false);
    // Source B cannot: the files stay listed, Create is blocked with an explanation, Remove all appears.
    const sourceB = render('github-b', 'GitHub issues', false);
    expect(sourceB).toContain('data-pending-attachment-id="proof"');
    expect(sourceB).toContain('data-pending-attachment-id="trace"');
    expect(sourceB).not.toContain('name="new-ticket-attachments"');
    expect(sourceB).toMatch(
      /role="status" data-new-ticket-attachments-stranded="true">GitHub issues does not support attachments\. Remove the 2 staged files or choose a source that supports attachments to create this ticket\./,
    );
    expect(sourceB).not.toContain('This ticket provider does not support attachments.');
    expect(sourceB).toMatch(
      /<button type="button" data-action="clear-new-ticket-attachments" aria-label="Remove all staged attachments">[\s\S]*data-lucide="trash-2"[\s\S]*Remove all<\/button>/,
    );
    expect(createDisabled(sourceB)).toBe(true);
    expect(render('github-b', 'GitHub issues', false, attachments.slice(0, 1))).toContain('Remove the staged file or');
    // Back to source A: the same files are ready to create again and every B-only presentation is gone.
    const backToA = render('git-a', 'HS2 git tickets', true);
    expect(backToA).toBe(sourceA);
    // Removing the files on source B unblocks Create there and returns to the plain notice.
    const cleared = render('github-b', 'GitHub issues', false, []);
    expect(cleared).toContain('This ticket provider does not support attachments.');
    expect(cleared).not.toContain('data-action="clear-new-ticket-attachments"');
    expect(cleared).not.toContain('data-new-ticket-attachments-stranded');
    expect(createDisabled(cleared)).toBe(false);
    // While a create is in flight the Remove all control is disabled with the rest of the form.
    expect(
      String(QuickTicketComposer({ expanded: true, attachments, attachmentsEnabled: false, submitting: true })),
    ).toMatch(/data-action="clear-new-ticket-attachments"[^>]*disabled/);
  });
});
