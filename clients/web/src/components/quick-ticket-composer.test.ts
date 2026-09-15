import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { QuickTicketComposer,QuickTicketLauncher,showQuickTicketComposer } from './quick-ticket-composer';

describe('QuickTicketComposer', () => {
  it('gives the title the available width while keeping category compact', () => {
    const css=readFileSync(new URL('./quick-ticket-composer.css',import.meta.url),'utf8');
    expect(css).toMatch(/\.quick-ticket-dialog \{[^}]*--width:min\(58rem, calc\(100vw - 2rem\)\)/);
    expect(css).toContain('grid-template-columns: minmax(0, 1fr) minmax(12rem, 15rem)');
    expect(css).toMatch(/@media \(max-width: 38rem\)[^{]*\{[^}]*\.quick-ticket-composer \{ grid-template-columns: 1fr/);
    // HS2-Q7WJ6T: the form sits inside the dialog panel, so its base rule must not draw its own
    // border or box-shadow (that redundant brand-colored rounded border read as a stray outline
    // below the dialog title). The drag drop-target highlight keeps its own box-shadow ring.
    const baseRule=css.match(/\n\.quick-ticket-composer \{([^}]*)\}/)![1];
    expect(baseRule).not.toMatch(/(^|;|\s)border:/);
    expect(baseRule).not.toContain('box-shadow:');
    expect(css).toMatch(/\.quick-ticket-composer:is\(\[data-dragging="true"\], \[data-dragging-ticket="true"\]\) \{[^}]*box-shadow:/);
  });
  it('has distinct collapsed, editable, and provider-disabled presentations', () => {
    const collapsed = String(QuickTicketLauncher());
    expect(collapsed).toContain('data-ticket-drop-action="duplicate"');
    expect(collapsed).toContain('data-action="expand-ticket-composer"');
    expect(collapsed).toContain('data-component="quick-ticket-composer-launcher"');
    expect(collapsed).toContain('data-new-ticket-drop-target="true"');
    const collapsedComposer = String(QuickTicketComposer({ expanded: false }));
    expect(collapsedComposer).toContain('data-component="quick-ticket-composer"');
    expect(collapsedComposer).toContain('aria-hidden="true"');
    expect(collapsedComposer).toContain(' inert');
    expect(collapsedComposer).not.toContain(' open');
    expect(collapsedComposer).not.toContain('data-action="create-ticket-form"');
    const expanded = String(QuickTicketComposer({ expanded: true, title: 'New work', details: 'Why this matters', category: 'bug', upNext: true, attachments: [{ id: 'proof', name: 'proof.png' }] }));
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
    const disabled = String(QuickTicketComposer({ expanded: true, canCreate: false, attachmentsEnabled: false, providerName: 'Read-only Jira' }));
    expect(disabled).toContain('does not support creating tickets');
    expect(disabled).toContain('does not support attachments');
    expect(disabled).not.toContain('name="new-ticket-attachments"');
    expect(disabled).toContain('disabled');
  });

  it('opens the persistent live Web Awesome dialog so it can remember its trigger', () => {
    const calls: string[] = [];
    const nativeDialog = { open: false, setAttribute: (name: string, value: string) => calls.push(`${name}:${value}`) };
    const dialog = { open: false, shadowRoot: { querySelector: () => nativeDialog }, show: () => { calls.push('show'); return Promise.resolve(); } };
    const root = { querySelector: () => dialog } as unknown as ParentNode;
    expect(showQuickTicketComposer(root)).toBe(true);
    expect(calls).toEqual(['role:presentation', 'show']);
    dialog.open = true;
    nativeDialog.open = true;
    expect(showQuickTicketComposer(root)).toBe(false);
    expect(calls).toEqual(['role:presentation', 'show', 'role:presentation']);
  });

  it('keeps one-line details vertically resizable and places Up Next after category',()=>{
    const css=readFileSync(new URL('./quick-ticket-composer.css',import.meta.url),'utf8'),markup=String(QuickTicketComposer({expanded:true}));
    expect(markup).toMatch(/new-ticket-category[\s\S]*toggle-new-ticket-up-next[\s\S]*new-ticket-details/);
    expect(markup).toMatch(/name="new-ticket-details"[^>]*data-morph-skip/);
    expect(css).toMatch(/__details textarea \{[^}]*min-height: var\(--hs-new-ticket-details-sidebar-height, 2\.5rem\);[^}]*resize: vertical/);
  });

  it('shows creation progress and attachment errors accessibly', () => {
    const markup = String(QuickTicketComposer({ expanded: true, submitting: true, attachmentMessage: 'proof.png could not be read', attachmentError: true }));
    expect(markup).toContain('data-submitting="true"');
    expect(markup).toContain('Creating…');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('proof.png could not be read');
  });

  it('blocks creation while attachment screening is still busy', () => {
    const markup = String(QuickTicketComposer({ expanded: true, busy: true }));
    expect(markup).toContain('Create ticket');
    expect(markup).not.toContain('Creating…');
    expect(markup.match(/disabled/g)).toHaveLength(1);
  });
});
