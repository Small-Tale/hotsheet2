import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketInspector, ticketInspectorPanel } from './ticket-inspector';

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
    expect(css).toMatch(/\.ticket-inspector__header \{[^}]*padding: 0;/);
    expect(css).toMatch(
      /\.ticket-inspector__header h1 \{[^}]*margin: var\(--kui-space-2xs\) var\(--kui-space-m\) var\(--kui-space-m\)/,
    );
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
      const markup = String(TicketInspector({ ...base, activeTab: tab, collapseControl: true }));
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
      // Icon-only tabs keep their name as the accessible name (Kerf beta.60 presentation).
      expect(markup).toMatch(
        new RegExp(
          `role="tab" aria-selected="true" aria-label="[^"]+"[^>]*data-action="set-inspector-tab" data-tab-id="${tab}" tabindex="0"`,
        ),
      );
      expect(markup).toContain('aria-label="Hide ticket inspector"');
      expect(markup).toContain('data-action="toggle-ticket-inspector"');
      expect(markup).toContain('data-lucide="panel-right-close"');
      expect(markup).toContain(
        'data-component="toolbar-text" data-size="small"><span class="kui-toolbar-text__text">HS2-TEST',
      );
      expect(markup).toContain(
        'data-action="copy-ticket-slug" data-ticket-slug="HS2-TEST" aria-label="Copy ticket number HS2-TEST"',
      );
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

  it('names every non-duplicate close outcome in human language (HS2-N11T22)', () => {
    const outcome = (closeReason: 'not_planned' | 'obsolete' | 'works_as_designed') =>
      /data-close-reason="[^"]+"[^>]*>\s*<span>([^<]+)<\/span>/u.exec(
        String(TicketInspector({ ...base, closeReason })),
      )?.[1];
    expect(outcome('not_planned')).toBe('Closed as not planned');
    expect(outcome('obsolete')).toBe('Closed as obsolete');
    expect(outcome('works_as_designed')).toBe('Closed as works as designed');
  });

  it('shows the live claim and its ETA in the header only while a claim is live (HS2-QKNQXC)', () => {
    const idle = String(TicketInspector(base));
    expect(idle).not.toContain('data-component="live-claim-notice"');
    const working = String(
      TicketInspector({
        ...base,
        feedbackNeeded: true,
        liveClaim: {
          agentName: 'Codex',
          eta: { kind: 'estimate', percent: 50, label: '~10m left', title: 'Estimated to finish 10:00' },
        },
      }),
    );
    expect(working).toContain('<span class="live-claim-notice__agent">Codex</span> is working on this');
    expect(working).toContain('data-claim-eta="estimate"');
    // The live-work notice leads the header notices, above Needs review.
    expect(working.indexOf('data-component="live-claim-notice"')).toBeLessThan(
      working.indexOf('class="ticket-inspector__feedback"'),
    );
    const reader = String(TicketInspector({ ...base, presentation: 'reader', liveClaim: { agentName: 'Codex' } }));
    expect(reader).toContain('data-component="live-claim-notice"');
  });

  it('shows the derived completion confidence only on completed or verified tickets (HS2-DWTJ43)', () => {
    const row = (props: Partial<Parameters<typeof TicketInspector>[0]>) =>
      /<div class="ticket-inspector__confidence"[^>]*>[\s\S]*?<\/div>/u.exec(
        String(TicketInspector({ ...base, ...props })),
      )?.[0];
    expect(row({ status: 'completed' })).toBeUndefined();
    for (const status of ['not_started', 'started', 'backlog', 'archive'] as const) {
      expect(row({ status, latestConfidence: 82 })).toBeUndefined();
    }
    for (const [status, value, band] of [
      ['completed', 82, 'assumed'],
      ['verified', 95, 'verified'],
      ['completed', 0, 'unverified'],
    ] as const) {
      const markup = row({ status, latestConfidence: value });
      expect(markup).toContain(`data-confidence="${value}"`);
      expect(markup).toContain(`aria-label="Confidence ${value} percent"`);
      expect(markup).toContain(`data-band="${band}"`);
      expect(markup).toContain(`Confidence ${value}%`);
      expect(markup).toContain('data-lucide="gauge"');
      expect(markup).toContain('Reported by the completing AI');
    }
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
    expect(reader).toContain('ticket-info-panel__details-feedback-header');
    expect(reader).toContain('Feedback needed');
    expect(reader).toContain('data-lucide="circle-alert"');
    expect(reader.match(/data-action="toggle-feedback-choice"/g)).toHaveLength(2);
    expect(reader).toContain('aria-label="Feedback response"');
    expect(reader).not.toContain('CHOICE:');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-info-panel.css'), 'utf8');
    // The feedback state keeps the surface's own 8px padding and only recolors it.
    expect(css).toMatch(/\.ticket-info-panel__details-surface \{[^}]*padding: var\(--kui-space-xs\);/);
    expect(css).toMatchSource(
      /details-surface\[data-feedback-needed="true"\] \{[^}]*warning-border-normal[^}]*warning-fill-quiet/,
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
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-attachments.css'), 'utf8');
    expect(css).toContainSource(
      '.ticket-attachments__item { display: flex; box-sizing: border-box; width: 100%; min-width: 0;',
    );
    expect(css).toContainSource('.ticket-attachments__item > span { min-width: 0; overflow: hidden; flex: 1;');
    expect(css).toContainSource(
      '.ticket-attachments__item-menu { display: inline-grid; width: remify(28px); height: remify(28px); margin-left: auto;',
    );
  });

  it('contains metadata and ticket content within narrow inspector bounds', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    const infoCss = readFileSync(resolve(import.meta.dirname, 'ticket-info-panel.css'), 'utf8');
    const noteCss = readFileSync(resolve(import.meta.dirname, 'note-card.css'), 'utf8');
    const markup = String(TicketInspector({ ...base }));
    expect(inspectorCss).toMatch(/\.ticket-inspector \{[^}]*min-width: 0;[^}]*max-width: 100%/);
    expect(markup).toMatch(/ticket-inspector__tabs[^>]*data-allocation="fill"[^>]*data-presentation="inspector"/);
    expect(panelCss).toMatch(/\.ticket-inspector-panel \{[^}]*min-width: 0;[^}]*overflow-x: hidden/);
    expect(infoCss).toContain('grid-template-columns: repeat(2, minmax(0, 1fr))');
    // Category and Priority fill their grid cells through Kerf's trigger width prop (HS2-PKPGGZ).
    expect(infoCss).not.toContain('.kui-select');
    expect(markup).toContain('data-trigger-width="fill"');
    expect(noteCss).toMatch(/\.note-card__body \{[^}]*overflow-wrap: anywhere/);
    expect(noteCss).toMatchSource(/\.note-card\[data-kind="activity"\] \{[^}]*background: transparent/);
    expect(noteCss).toMatchSource(
      /\.note-card\[data-kind="activity"\] \.note-card__body \{[^}]*font-size: calc\(var\(--hotsheet-reading-scale, 1\) \* var\(--wa-font-size-xs\)\)/,
    );
    // Kerf beta.62 switches the reader's segmented tabs to icon-only below 832px itself.
    expect(inspectorCss).not.toContain('@container');
    expect(String(TicketInspector({ ...base, presentation: 'reader' }))).toMatch(
      /ticket-inspector__tabs[^>]*data-icon-only-at="wide"/,
    );
    expect(markup).not.toContain('data-icon-only-at');
  });

  it('contains equal full-width tab targets inside one compact inspector gutter (HS2-WKGMN4)', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    const markup = String(TicketInspector({ ...base }));
    // The inset is frame padding: a margin on Kerf's full-width TabBar overflowed the inspector.
    expect(inspectorCss).toMatch(
      /\.ticket-inspector__tabs-frame \{[^}]*padding: 0 var\(--kui-space-xs\) var\(--kui-space-xs\)/,
    );
    expect(inspectorCss).not.toMatch(/\.ticket-inspector__tabs \{[^}]*margin/);
    expect(markup).toMatch(/class="ticket-inspector__tabs-frame"><nav class="kui-tab-bar ticket-inspector__tabs"/);
    expect(markup).toMatch(/ticket-inspector__tabs[^>]*data-allocation="fill"[^>]*data-presentation="inspector"/);
    expect(markup).toMatch(/data-inspector-tab="info"[^>]*data-presentation="icon-only"[^>]*data-size="compact"/);
    expect(String(TicketInspector({ ...base, presentation: 'reader' }))).toMatch(
      /data-inspector-tab="info"[^>]*data-presentation="segmented"[^>]*data-size="compact"/,
    );
    // The tabs are configured through AppTab props and the tab strip; no app class sits on the tab root.
    expect(markup).not.toContain('ticket-inspector__tab"');
    // Fill allocation centers each tab; the icon-to-name gap is the public layout token.
    expect(inspectorCss).toMatch(/\.ticket-inspector__tabs-frame \{[^}]*--kui-layout-item-gap: var\(--kui-space-2xs\)/);
    expect(inspectorCss).not.toMatch(/\.kui-app-tab__select \{/);
    expect(panelCss).toMatch(
      /\.ticket-inspector-panel \{[^}]*padding: 0 0 var\(--kui-space-xs\);[^}]*gap: var\(--kui-space-l\);/,
    );
    // Each direct child sits 8px from the edge with no border/padding of its own; headers get a 1px
    // transparent border + 8px padding (17px text) and bordered surfaces own their border+padding at the
    // 8px column — no negative margins anywhere (HS2-R64ETQ).
    expect(panelCss).toMatchSource(/\.ticket-inspector-panel > \* \{ margin-inline: var\(--kui-space-xs\); \}/);
    // Each tab panel owns its own header inset (HS2-MGVE50).
    for (const file of [
      'ticket-info-panel.css',
      'ticket-timeline.css',
      'ticket-attachments.css',
      'ticket-code-review.css',
    ]) {
      const own = readFileSync(resolve(import.meta.dirname, file), 'utf8');
      expect(own).toMatch(
        /padding-inline: var\(--kui-space-xs\);\s*border-inline: 1px solid transparent;|padding: var\(--kui-space-2xs\) var\(--kui-space-xs\) 0;\s*border-inline: 1px solid transparent;/,
      );
      expect(own).not.toContain('margin-inline: calc((remify(8px) + 1px) * -1)');
    }
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

  it('exposes Workbench panel parts whose fixed header keeps the title and status notices (HS2-QQW6CT)', () => {
    const parts = ticketInspectorPanel({
      ...base,
      status: 'completed',
      latestConfidence: 82,
      feedbackNeeded: true,
      liveClaim: { agentName: 'Codex' },
    });
    expect(parts.toggle).toEqual({ action: 'toggle-ticket-inspector', name: 'ticket inspector' });
    expect(parts.label).toBe('HS2-TEST inspector');
    expect(String(parts.toolbar.leading)).toContain('data-action="copy-ticket-slug"');
    expect(String(parts.toolbar.trailing)).toContain('data-action="open-ticket-reader"');
    // The Workbench renders the toggle itself; the parts carry no hand-made hide control.
    expect(String(parts.toolbar.trailing)).not.toContain('toggle-ticket-inspector');
    const header = String(parts.header);
    expect(header).toContain('data-component="ticket-inspector-header"');
    expect(header).toContain('data-ticket-slug="HS2-TEST"');
    expect(header).toContain('data-needs-review="true"');
    expect(header).toContain('Inspect this ticket');
    expect(header).toContain('data-component="live-claim-notice"');
    expect(header).toContain('data-confidence="82"');
    expect(header).toContain('Needs review');
    expect(header).toContain('data-component="tab-bar"');
    const content = String(parts.content);
    expect(content).toContain('data-component="ticket-inspector-body"');
    expect(content).toContain('data-attachment-drop-target="true"');
    expect(content).toContain('data-presentation="sidebar"');
    expect(content).not.toContain('data-component="tab-bar"');
    // The reader keeps its own close control and never the rail toggle.
    const reader = String(TicketInspector({ ...base, presentation: 'reader', collapseControl: true }));
    expect(reader).not.toContain('toggle-ticket-inspector');
    expect(reader).toContain('data-action="close-ticket-reader"');
  });
});
