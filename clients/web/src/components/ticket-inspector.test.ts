import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketInspector } from './ticket-inspector';

const base = {
  slug: 'HS2-TEST',
  title: 'Inspect this ticket',
  status: 'started' as const,
  priority: 'high' as const,
  category: 'feature',
  tags: ['client'],
  details: 'Readable details.',
};

describe('TicketInspector', () => {
  it('allows the sidebar title to wrap without a line cap', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.ticket-inspector__header \{[^}]*padding: 0 0 var\(--kui-space-m\)/);
    expect(css).toMatch(
      /\.ticket-inspector__feedback \{[^}]*gap: var\(--kui-space-xs\)[^}]*margin: 0 var\(--kui-space-xs\) var\(--kui-space-m\)[^}]*padding: var\(--kui-space-xs\)/,
    );
    expect(css).toMatch(
      /\.ticket-inspector__close-outcome \{[^}]*gap: var\(--kui-space-2xs\)[^}]*margin: 0 var\(--kui-space-xs\) var\(--kui-space-m\)[^}]*padding: var\(--kui-space-xs\)/,
    );
    const titleRule = css.match(/\.ticket-inspector__header h1 \{([^}]*)\}/)?.[1] ?? '';
    expect(titleRule).toContain('overflow-wrap: anywhere');
    expect(titleRule).not.toContain('line-clamp');
  });

  it('renders each public tab without changing ticket identity', () => {
    for (const tab of ['info', 'timeline', 'code-review', 'attachments'] as const) {
      const markup = String(TicketInspector({ ...base, activeTab: tab }));
      expect(markup).toContain('HS2-TEST');
      expect(markup).toContain('data-component="tab-bar"');
      expect(markup).toContain('data-tab-bar-id="ticket-inspector-sidebar-HS2-TEST"');
      expect(markup).toContain('data-tab-activation="automatic"');
      expect(markup).toContain('aria-label="Ticket inspector sections"');
      expect(markup).toMatch(
        new RegExp(
          `data-inspector-tab="${tab}"[^>]*data-component="app-tab"[^>]*data-tab-id="${tab}"[^>]*data-selected="true"`,
        ),
      );
      expect(markup).toContain(
        `role="tab" aria-selected="true" data-action="set-inspector-tab" data-tab-id="${tab}" tabindex="0"`,
      );
      expect(markup).toContain('aria-label="Hide inspector"');
      expect(markup).toContain('data-lucide="panel-right-close"');
      expect(markup).toContain(
        'data-component="toolbar-text" data-size="small"><span class="kui-toolbar-text__text">HS2-TEST',
      );
      expect(markup).toContain('data-action="copy-ticket-slug" aria-label="Copy ticket number HS2-TEST"');
      expect(markup).toContain('data-appearance="borderless"');
      if (tab === 'info') {
        expect(markup.match(/<wa-option value="feature"/g)).toHaveLength(1);
        expect(markup).toContain('data-component="ticket-notes"');
        expect(markup).toContain('data-component="markdown-preview"');
      }
      if (tab === 'code-review') expect(markup).toContain('data-lucide="message-square-code"');
    }
  });

  it('changes only the Code Review tab icon', () => {
    const markup = String(
      TicketInspector({
        ...base,
        activeTab: 'code-review',
        codeReview: {
          difftool: 'Glassbox',
          truncated: false,
          ranges: [],
          commits: [
            { sha: 'abcdef', short_sha: 'abcdef', subject: 'Review action', committed_at: '2026-09-02T08:00:00Z' },
          ],
        },
      }),
    );
    expect(markup).toContain('data-inspector-tab="code-review"');
    expect(markup).toContain('data-lucide="message-square-code"');
    expect(markup).toContain('ticket-code-review__graph');
    expect(markup).toContain('data-lucide="git-commit-horizontal"');
    expect(markup).toContain('data-lucide="external-link"');
  });

  it('places the ticket number in the leading toolbar slot for the sidebar and reader (HS2-9MCJ2B, HS2-FZ5HB2)', () => {
    const slugChild = 'class="ticket-inspector__slug"';
    const sidebar = String(TicketInspector({ ...base }));
    expect(sidebar).toContain(`kui-toolbar__leading"><button type="button" ${slugChild}`);
    expect(sidebar).not.toContain(`kui-toolbar__center"><button type="button" ${slugChild}`);
    const reader = String(TicketInspector({ ...base, presentation: 'reader' }));
    expect(reader).toContain(`kui-toolbar__leading"><button type="button" ${slugChild}`);
    expect(reader).not.toContain(`kui-toolbar__center"><button type="button" ${slugChild}`);
    // The terminal rail forces center (its overlaid back button sits at the leading edge).
    const railScoped = String(TicketInspector({ ...base, slugPlacement: 'center' }));
    expect(railScoped).toContain(`kui-toolbar__center"><button type="button" ${slugChild}`);
    expect(railScoped).not.toContain(`kui-toolbar__leading"><button type="button" ${slugChild}`);
  });

  it('uses the same capability surface at reader scale with dialog close semantics', () => {
    const markup = String(
      TicketInspector({
        ...base,
        presentation: 'reader',
        notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }],
      }),
    );
    expect(markup).toContain('data-presentation="reader"');
    expect(markup).toContain('data-action="close-ticket-reader"');
    expect(markup).toContain('data-lucide="x"');
    expect(markup).toContain('data-lucide="a-large-small"');
    expect(markup).toContain('aria-label="Use large reader text size"');
    expect(markup).toMatch(
      /data-button-appearance="push"[^>]*data-single="true"[^>]*><button[^>]*data-action="toggle-reader-text-size"/,
    );
    const largeMarkup = String(TicketInspector({ ...base, presentation: 'reader', largeText: true }));
    expect(largeMarkup).toContain('aria-label="Use standard reader text size" aria-pressed="true"');
    expect(markup).toContain('data-component="note-card"');
    expect(markup).not.toContain('data-action="edit-ticket-reader"');
    expect(markup).toContain('data-action="edit-markdown"');
    expect(markup).toContain('data-edit-on-double-click="true"');
    expect(markup).not.toContain('data-action="edit-note"');
    expect(markup).toContain('popoverTarget="ticket-tag-reader-hs2-test"');
    expect(String(TicketInspector({ ...base }))).toContain('popoverTarget="ticket-tag-sidebar-hs2-test"');
    const editing = String(
      TicketInspector({
        ...base,
        presentation: 'reader',
        detailsMode: 'write',
        notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }],
      }),
    );
    expect(editing).toContain('name="markdown-source"');
    expect(editing).toContain('data-edit-on-double-click="true"');
  });

  it('shows a feedback-needed banner only when the ticket is waiting on the user', () => {
    expect(String(TicketInspector({ ...base }))).not.toContain('ticket-inspector__feedback');
    const waiting = String(TicketInspector({ ...base, feedbackNeeded: true }));
    expect(waiting).toContain('ticket-inspector__feedback');
    expect(waiting).toContain('data-needs-review="true"');
    expect(waiting).toContain('Needs review');
    expect(waiting).toContain('circle-alert');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    // The reader no longer paints a purple needs-review side rail; the "Needs review" pill is the only
    // feedback-needed indicator in the reader (HS2-N6WA7Y).
    expect(css).not.toMatch(/data-presentation="reader"\]\[data-needs-review="true"\]::before/);
  });

  it('renders the structured duplicate outcome and canonical ticket action', () => {
    const markup = String(
      TicketInspector({
        ...base,
        status: 'completed',
        closeReason: 'duplicate',
        duplicateTarget: { id: 'target-id', projectName: 'Hot Sheet 2', slug: 'HS2-TARGET', title: 'Canonical ticket' },
      }),
    );
    expect(markup).toContain('data-component="ticket-duplicate-target"');
    expect(markup).toContain('Duplicate of');
    expect(markup).toContain('Hot Sheet 2 · HS2-TARGET');
    expect(markup).toContain('Canonical ticket');
    expect(markup).toContain('data-action="open-duplicate-target" data-item-id="target-id"');
    expect(markup).toContain('data-lucide="copy-x"');
  });

  it('resolves same-ticket and cross-ticket attachment references in details', () => {
    const markup = String(
      TicketInspector({
        ...base,
        details: 'Local `attachment:proof.png` and cross `attachment:[HS2-OTHER]report.pdf`.',
        attachments: [{ id: 'A1', name: 'proof.png' }],
        attachmentContext: {
          baseUrl: '/project-api/demo',
          checkout: 'checkout one',
          ticket: 'HS2-TEST',
          attachments: [{ id: 'A1', filename: 'proof.png' }],
        },
      }),
    );
    expect(markup).toContain('/project-api/demo/checkouts/checkout%20one/tickets/HS2-TEST/attachments/A1');
    expect(markup).toContain(
      '/project-api/demo/checkouts/checkout%20one/tickets/HS2-OTHER/attachments/by-name/report.pdf',
    );
    expect(markup).toContain('data-action="open-referenced-attachment"');
  });

  it('renders project-qualified reverse duplicate backlinks and partial lookup status', () => {
    const markup = String(
      TicketInspector({
        ...base,
        duplicateBacklinks: [
          {
            reference: '@other/git-other:source',
            project_id: 'other',
            project_name: 'Other project',
            connection_id: 'git-other',
            native_id: 'source',
            qualified_id: 'git-other:source',
            slug: 'HS2-SAME',
            title: 'Earlier report',
          },
        ],
        duplicateBacklinkInaccessibleProjects: ['Offline project'],
      }),
    );
    expect(markup).toContain('data-component="ticket-duplicate-backlinks"');
    expect(markup).toContain('Other project · HS2-SAME');
    expect(markup).toContain('data-item-id="@other/git-other:source"');
    expect(markup).toContain('Could not check Offline project for additional duplicates.');
  });

  it('renders marked description choices as the reader feedback surface', () => {
    const details = 'FEEDBACK NEEDED: Which direction?\n\nCHOICE:\n- Keep **A**\n- Use `B`';
    const sidebar = String(TicketInspector({ ...base, details, feedbackNeeded: true }));
    expect(sidebar).toContain('data-note-id="ticket-details"');
    expect(sidebar).toContain('data-feedback-needed="true"');
    expect(sidebar).toContain('Respond to Feedback');
    const reader = String(TicketInspector({ ...base, details, feedbackNeeded: true, presentation: 'reader' }));
    expect(reader).toContain('data-details-feedback="true"');
    expect(reader).toContain('ticket-inspector__details-feedback-header');
    expect(reader).toContain('Feedback needed');
    expect(reader).toContain('data-lucide="circle-alert"');
    expect(reader.match(/data-action="toggle-feedback-choice"/g)).toHaveLength(2);
    expect(reader).toContain('aria-label="Feedback response"');
    expect(reader).not.toContain('CHOICE:');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toMatchSource(
      /details-surface\[data-feedback-needed="true"\] \{[^}]*padding: var\(--kui-space-xs\);[^}]*warning-border-normal[^}]*warning-fill-quiet/,
    );
  });

  it('shows a visible and accessible derived attachment count on the attachments tab', () => {
    const markup = String(
      TicketInspector({
        ...base,
        attachments: [
          { id: 'one', name: 'one.png' },
          { id: 'two', name: 'two.md' },
        ],
      }),
    );
    expect(markup).toContain('ticket-inspector__tab-count');
    expect(markup).toContain('<span aria-hidden="true">2</span>');
    expect(markup).toContain('ticket-inspector__tab-count-label">2 attachments</span>');
    expect(String(TicketInspector({ ...base, attachments: [] }))).not.toContain('ticket-inspector__tab-count');
  });

  it('keeps attachment names shrinkable while preserving the compact menu trigger', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContainSource(
      '.ticket-inspector__attachment { display: flex; box-sizing: border-box; width: 100%; min-width: 0;',
    );
    expect(css).toContainSource('.ticket-inspector__attachment > span { min-width: 0; overflow: hidden; flex: 1;');
    expect(css).toContainSource(
      '.ticket-inspector__attachment-menu { display: inline-grid; width: remify(28px); height: remify(28px); margin-left: auto;',
    );
  });

  it('contains metadata and ticket content within narrow inspector bounds', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    const noteCss = readFileSync(resolve(import.meta.dirname, 'note-card.css'), 'utf8');
    const markup = String(TicketInspector({ ...base }));
    expect(inspectorCss).toMatch(/\.ticket-inspector \{[^}]*min-width: 0;[^}]*max-width: 100%/);
    expect(markup).toMatch(/ticket-inspector__tabs[^>]*data-allocation="fill"[^>]*data-presentation="inspector"/);
    expect(panelCss).toMatch(/\.ticket-inspector__content \{[^}]*min-width: 0;[^}]*overflow-x: hidden/);
    expect(panelCss).toContain('grid-template-columns: repeat(2, minmax(0, 1fr))');
    expect(panelCss).toContainSource('.ticket-inspector__metadata > .kui-select { width: 100%; min-width: 0; }');
    expect(noteCss).toMatch(/\.note-card__body \{[^}]*overflow-wrap: anywhere/);
    expect(noteCss).toMatchSource(/\.note-card\[data-kind="activity"\] \{[^}]*background: transparent/);
    expect(noteCss).toMatchSource(
      /\.note-card\[data-kind="activity"\] \.note-card__body \{[^}]*font-size: var\(--wa-font-size-xs\)/,
    );
    expect(inspectorCss).toContainSource(
      '@container (max-width: remify(832px)) { .ticket-inspector__tabs .ticket-inspector__tab .kui-app-tab__name { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; } }',
    );
  });

  it('contains equal full-width tab targets inside one compact inspector gutter (HS2-WKGMN4)', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    const markup = String(TicketInspector({ ...base }));
    expect(inspectorCss).toMatch(
      /\.ticket-inspector__tabs \{[^}]*margin: 0 var\(--kui-space-xs\) var\(--kui-space-xs\)/,
    );
    expect(markup).toMatch(/ticket-inspector__tabs[^>]*data-allocation="fill"[^>]*data-presentation="inspector"/);
    expect(markup).toMatch(/ticket-inspector__tab[^>]*data-presentation="segmented"[^>]*data-size="compact"/);
    expect(inspectorCss).toMatch(
      /\.ticket-inspector__tab \.kui-app-tab__select \{[^}]*flex: 1;[^}]*justify-content: center/,
    );
    expect(panelCss).toMatch(
      /\.ticket-inspector__content \{[^}]*padding: 0 0 var\(--kui-space-xs\);[^}]*gap: var\(--kui-space-l\);/,
    );
    // Each direct child sits 8px from the edge with no border/padding of its own; headers get a 1px
    // transparent border + 8px padding (17px text) and bordered surfaces own their border+padding at the
    // 8px column — no negative margins anywhere (HS2-R64ETQ).
    expect(panelCss).toMatchSource(/\.ticket-inspector__content > \* \{ margin-inline: var\(--kui-space-xs\); \}/);
    expect(panelCss).toContainSource('border-inline: 1px solid transparent; padding-inline: var(--kui-space-xs);');
    expect(panelCss).not.toContain('margin-inline: calc((remify(8px) + 1px) * -1)');
  });

  it('hides the Up Next action for ineligible lifecycle states', () => {
    expect(String(TicketInspector({ ...base }))).toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, status: 'completed' }))).not.toContain(
      'data-action="toggle-inspector-up-next"',
    );
    expect(String(TicketInspector({ ...base, upNextEligible: false }))).not.toContain(
      'data-action="toggle-inspector-up-next"',
    );
  });
});
