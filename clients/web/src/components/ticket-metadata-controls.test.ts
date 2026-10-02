import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { Bug } from 'lucide';
import { describe, expect, it } from 'vitest';

import { groupAttachments, TicketAttachments } from './ticket-attachments';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketPrioritySelect } from './ticket-priority-select';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTimeline } from './ticket-timeline';

describe('ticket metadata controls and inspector panels', () => {
  it('uses compact Details field-label geometry in preview and editing modes (HS2-S6S709)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContainSource(
      '.ticket-inspector__content .ticket-inspector__details-section { gap: calc(var(--kui-font-xs) * 0.5); }',
    );
    expect(css).toContainSource(
      '.ticket-inspector__status-field, .ticket-inspector__details-section { --kui-list-header-min-height: 0; --kui-list-header-title-min-height: 0; --kui-list-header-title-padding-block: 0; --kui-list-header-border-width: 0; --kui-list-header-title-padding-inline: 1px; }',
    );
    expect(css).not.toMatch(/\.kui-list-header(__title)? \{/);
    for (const detailsMode of ['preview', 'write'] as const) {
      const markup = String(
        TicketInfoPanel({
          status: 'started',
          priority: 'high',
          category: 'feature',
          tags: [],
          details: 'Body',
          detailsMode,
        }),
      );
      expect(markup).toContain('class="ticket-inspector__section ticket-inspector__details-section"');
      expect(markup).toContain('class="kui-text" data-component="text"');
      expect(markup).toContain('data-font="default" data-border="none">Details</h2>');
      expect(markup).toContain(`data-mode="${detailsMode}"`);
    }
    const readOnly = String(
      TicketInfoPanel({
        status: 'started',
        priority: 'high',
        category: 'feature',
        tags: [],
        details: 'Body',
        canEditText: false,
      }),
    );
    expect(readOnly).toContain('class="ticket-inspector__section ticket-inspector__details-section"');
    expect(readOnly).not.toContain('aria-label="Edit Ticket details"');
  });

  it('renders colored category icons and semantic priority icons', () => {
    const category = String(TicketCategorySelect({ name: 'category', value: 'bug' }));
    expect(category).toContain('data-component="select"');
    expect(category).toContain('data-lucide="bug"');
    expect(category).toContain('color:var(--hs-category-bug)');
    expect(
      String(
        TicketCategorySelect({
          name: 'category',
          value: 'custom',
          choices: [{ value: 'custom', label: 'Custom', color: '#123456', icon: Bug, iconName: 'bug' }],
        }),
      ),
    ).toContain('color:var(--hs-category-fallback)');
    const priority = String(TicketPrioritySelect({ name: 'priority', value: 'urgent' }));
    expect(priority).toContain('data-lucide="chevrons-up"');
    expect(priority).toContain('data-lucide="minus"');
    expect(String(TicketCategorySelect({ name: 'category', value: 'bug', ariaLabel: 'Compact category' }))).toContain(
      'aria-label="Compact category"',
    );
    expect(
      String(TicketPrioritySelect({ name: 'priority', value: 'urgent', ariaLabel: 'Compact priority' })),
    ).toContain('aria-label="Compact priority"');
    // Ordinary Select content must name Web Awesome's actual shadow combobox too (HS2-Q6EM0B).
    expect(String(TicketCategorySelect({ name: 'category', value: 'bug', ariaLabel: 'Compact category' }))).toMatch(
      /<wa-select\b[^>]*\slabel="Compact category"/,
    );
    expect(String(TicketPrioritySelect({ name: 'priority', value: 'urgent', ariaLabel: 'Compact priority' }))).toMatch(
      /<wa-select\b[^>]*\slabel="Compact priority"/,
    );
    const status = String(TicketStatusMenu({ value: 'completed' }));
    expect(status).toContain('aria-label="Change status, Completed"');
    // The app-owned wrapper carries the placement class; the Kerf Select root keeps only its own classes.
    expect(status).toContain('<span class="ticket-status-menu">');
    expect(status).toContain('kui-select kui-select--custom-selected kui-select--label-hidden"');
    expect(status).toContain('name="inspector-status"');
    expect(status).toMatch(
      /<span[^>]*slot="start" class="kui-select__custom-selected"><span class="kui-select__custom-selected-content"><span class="status-badge status-badge--completed/,
    );
    expect(status).toMatch(/<wa-option value="verified"><span[^>]*slot="start" class="kui-select__icon"/);
    // Kerf 5.0.0-beta.56 renders Web Awesome's reflected divider defaults (separator role).
    expect(status).toMatch(/<wa-divider[^>]*role="separator"[^>]*><\/wa-divider><wa-option value="backlog"/);
    expect(status).toContain('<wa-option value="archive"');
    expect(status).toContain('data-lucide="badge-check"');
    expect(status.match(/data-lucide=/g)).toHaveLength(7);
    // Kerf props, not consumer part overrides, drop the trigger chrome and caret; the badge is semibold
    // through its own prop (HS2-4APEJP).
    expect(status).toContain('data-presentation="toolbar-borderless"');
    expect(status).toContain('data-caret="false"');
    expect(status).toContain('status-badge--semibold');
    const css = readFileSync(new URL('./ticket-status-menu.css', import.meta.url), 'utf8');
    expect(css).not.toContain('status-badge');
    expect(css).not.toMatch(/::part\((start|expand-icon|label)\)/);
    expect(css).toContain('KF-V2Y51V');
  });

  it('renders inspector sections independently of the inspector shell', () => {
    const info = String(
      TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: ['ux'], details: 'Details' }),
    );
    expect(info).toContain('data-component="ticket-info-panel"');
    expect(info.match(/data-component="list-header"/g)).toHaveLength(4);
    expect(info).toContain('data-font="default" data-border="none">Status</h2>');
    expect(info).toContain('class="kui-list-inset-control"');
    expect(info).toContain('<div class="ticket-inspector__status-line">');
    expect(info).toContain('name="inspector-category" label="Category"');
    expect(info).toContain('name="inspector-priority" label="Priority"');
    expect(info).toContain('data-font="default" data-border="none">Details</h2>');
    expect(info).toContain('data-font="default" data-border="none">Tags</h2>');
    expect(info).toMatch(
      /popoverTarget="ticket-tag-popover"[^>]*aria-controls="ticket-tag-popover"[^>]*aria-haspopup="dialog"[^>]*data-action="open-ticket-tag-popover"[^>]*aria-label="Add tag"/,
    );
    expect(info).toContain('data-action="edit-blocked-reason"');
    expect(info).toContain('Block ticket');
    expect(info).not.toContain('<h2>Blocked reason</h2>');
    // The block action is a plain Kerf ListItem; no app class sits on its root (KUI-L022).
    expect(info).not.toContain('ticket-inspector__block-action');
    expect(info).toContain('class="kui-list-item" data-component="list-item"');
    const deleted = String(
      TicketInfoPanel({ status: 'deleted', priority: 'default', category: 'issue', tags: [], details: '' }),
    );
    expect(deleted).toContain('data-status="deleted"');
    expect(deleted).toContain('data-lucide="trash-2"');
    expect(deleted).toContain('Deleted');
    expect(deleted).toContain('class="kui-list-inset-control"');
    expect(deleted).toContain('<div class="ticket-inspector__status-line">');
    expect(deleted).not.toContain('name="inspector-status"');
    expect(info).toContain('aria-label="Notes, 0 notes" class="kui-text" data-component="text"');
    expect(info).toMatch(/<span class="kui-badge"[^>]*aria-hidden="true">0<\/span>/);
    expect(info).toContain('class="kui-list-item ticket-notes__add" data-component="list-item"');
    const blocked = String(
      TicketInfoPanel({
        status: 'started',
        priority: 'high',
        category: 'feature',
        tags: [],
        details: '',
        blockedReason: 'Waiting',
      }),
    );
    expect(blocked).toContain('data-font="default" data-border="none">Blocked reason</h2>');
    expect(blocked).toContain('data-edit-blocked-reason="true"');
    expect(blocked).toContain('aria-label="Edit blocked reason"');
    expect(blocked).not.toContain('ticket-inspector__text-action');
    expect(blocked).toContain('Waiting');
    expect(blocked.match(/data-component="list-header"/g)).toHaveLength(5);
    expect(info).toContain('data-component="ticket-notes"');
    const readOnly = String(
      TicketInfoPanel({
        status: 'started',
        priority: 'high',
        category: 'feature',
        tags: [],
        details: '',
        canUpdate: false,
        canAddNotes: false,
      }),
    );
    expect(readOnly).toContain('data-font="default" data-border="none">Tags</h2>');
    expect(readOnly).not.toContain('data-action="open-ticket-tag-popover"');
    expect(readOnly).not.toContain('data-action="add-ticket-note"');
    expect(readOnly).toContain('aria-label="Status, Started"');
    expect(readOnly).toMatch(/name="inspector-status"[^>]*disabled/);
    const timeline = String(
      TicketTimeline({ entries: [{ id: 'one', time: 'Now', title: 'One event', subtitle: 'Optional detail' }] }),
    );
    expect(timeline.match(/<li/g)).toHaveLength(1);
    expect(timeline).toContain('One event');
    expect(timeline).toContain('Optional detail');
    expect(timeline).toContain('1 event total');
    const attachments = String(
      TicketAttachments({ attachments: [{ id: 'one', name: 'one.png', url: '/attachment/one', annotationCount: 2 }] }),
    );
    expect(attachments.match(/class="ticket-inspector__attachment"/g)).toHaveLength(1);
    expect(attachments).toContain('class="ticket-attachments__count" aria-label="1 attachment">1</span>');
    expect(attachments).not.toContain('attachment total');
    expect(attachments).toContain('data-attachment-drop-target="true"');
    expect(attachments).toContain('aria-label="Browse and add attachments"');
    expect(attachments).toContain('aria-label="Drop or browse attachments"');
    expect(attachments).toContain('data-action="open-attachment-row"');
    expect(attachments).toContain('data-action="open-attachment-menu"');
    expect(attachments).toContain('data-attachment-menu-kind="item"');
    expect(attachments).toContain('class="ticket-attachments__image-grid"');
    expect(attachments).toContain('data-action="open-attachment-gallery"');
    expect(attachments).toContain('data-drag-attachment-id="one"');
    expect(attachments).toContain('<img draggable="false"');
    expect(attachments).toContain('Open one.png in media gallery, 2 annotations');
    expect(attachments).toContain('ticket-attachments__annotation-marker');
    expect(attachments).toContain('data-lucide="pencil"');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    // Media grid items cap at 220px wide so they wrap instead of stretching full width (HS2-KEZ0EY).
    expect(css).toMatch(/ticket-attachments__image-grid button \{[^}]*max-width: remify\(220px\)/);
    expect(css).toMatchSource(/ticket-attachments__image-grid :is\(img,video\) \{[^}]*object-fit: contain/);
    expect(css).toMatch(/ticket-attachments__count \{[^}]*background: var\(--wa-color-neutral-fill-quiet\)/);
    expect(attachments).toContain('aria-label="More actions for one.png" title="More actions for one.png"');
    expect(attachments).toContain('title="one.png — double-click to open"');
    expect(attachments).toContain('data-lucide="more-horizontal"');
    const unsupported = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png' }], enabled: false }));
    expect(unsupported).toContain('does not support attachment actions');
    expect(unsupported).not.toContain('name="ticket-attachments"');
    expect(unsupported).not.toContain('data-action="open-attachment-menu"');
    expect(unsupported).not.toContain('data-action="open-attachment-row"');
    expect(unsupported).not.toContain('data-drag-attachment-id');
    expect(unsupported).not.toContain('data-action="edit-attachment-batch-label"');
    expect(unsupported).toContain('<h3 class="ticket-attachments__batch-title">Legacy / Uncategorized</h3>');
    // An append-only provider (GitHub assets repository, HS2-HSA64D) adds files and opens existing
    // ones by link, but offers no regrouping, relabelling, renaming, deletion, or local open.
    const appendOnly = String(
      TicketAttachments({
        attachments: [
          { id: 'one', name: 'one.png', url: '/attachment/one' },
          { id: 'two', name: 'log.txt', url: '/attachment/two' },
        ],
        enabled: true,
        editable: false,
      }),
    );
    expect(appendOnly).toContain('aria-label="Browse and add attachments"');
    expect(appendOnly).toContain('aria-label="Drop or browse attachments"');
    expect(appendOnly).toContain('data-attachment-drop-target="true"');
    expect(appendOnly).not.toContain('does not support attachment actions');
    expect(appendOnly).not.toContain('data-action="open-attachment-menu"');
    expect(appendOnly).not.toContain('data-action="open-attachment-row"');
    expect(appendOnly).not.toContain('data-drag-attachment-id');
    expect(appendOnly).not.toContain('data-action="edit-attachment-batch-label"');
    // No regroup targets: neither group drop targets nor the New group zone (HS2-0RTH3J).
    expect(appendOnly).not.toContain('data-attachment-group-drop-target');
    expect(appendOnly).not.toContain('data-attachment-new-group-drop-target');
    expect(appendOnly).toContain(
      'name="attachment-batch-purpose" aria-label="Purpose for Legacy / Uncategorized" disabled',
    );
    expect(appendOnly).toContain(
      '<a href="/attachment/two" target="_blank" rel="noopener" title="Open log.txt">log.txt</a>',
    );
    expect(appendOnly).toContain('data-action="open-attachment-gallery"');
  });

  it('shows videos in the media grid without starting playback', () => {
    const attachments = String(
      TicketAttachments({
        attachments: [
          { id: 'clip', name: 'walkthrough.webm', url: '/attachment/clip', thumbnailUrl: '/attachment/clip/thumbnail' },
        ],
      }),
    );
    expect(attachments).toContain('aria-label="Attached media"');
    expect(attachments).toContain('<video');
    expect(attachments).toContain('preload="none"');
    expect(attachments).toContain('src="/attachment/clip#t=0.1"');
    expect(attachments).toContain('poster="/attachment/clip/thumbnail"');
    expect(attachments).not.toContain('autoplay');
    expect(attachments).toContain('Open walkthrough.webm in media gallery');
  });

  it('groups stable batches and keeps old/provider attachments explicitly legacy', () => {
    const groups = groupAttachments([
      { id: 'old', name: 'old.png' },
      {
        id: 'a',
        name: 'a.png',
        batch_id: 'fix',
        actor: { identity: 'codex', role: 'ai' },
        purpose: 'correctness_evidence',
      },
      {
        id: 'b',
        name: 'b.png',
        batch_id: 'fix',
        actor: { identity: 'codex', role: 'ai' },
        purpose: 'correctness_evidence',
      },
      {
        id: 'c',
        name: 'c.png',
        batch_id: 'human',
        batch_label: 'More feedback',
        actor: { display_name: 'Brian', role: 'human' },
        purpose: 'problem_evidence',
      },
      {
        id: 'd',
        name: 'diagnostics.json',
        batch_id: 'automatic',
        actor: { role: 'system' },
        purpose: 'problem_evidence',
      },
    ]);
    expect(groups.map((group) => [group.label, group.items.length])).toEqual([
      ['Legacy / Uncategorized', 1],
      ['AI · Round 1 · Correctness evidence', 2],
      ['More feedback', 1],
      ['System · Batch 1 · Problem evidence', 1],
    ]);
    const markup = String(TicketAttachments({ attachments: groups.flatMap((group) => group.items) }));
    expect(markup).not.toContain('type="checkbox"');
    expect(markup).not.toContain('Merge selected');
    expect(markup).toContain('draggable="true"');
    expect(markup).toContain('data-drag-attachment-id="a"');
    expect(markup).toContain('data-attachment-group-drop-target="true"');
    expect(markup).toContain('data-attachment-new-group-drop-target="true"');
    expect(markup).toContain('New group');
    expect(markup).not.toContain('data-lucide="grip-vertical"');
    expect(markup).toContain('data-action="edit-attachment-batch-label"');
    expect(markup).toContain('Double-click to edit batch label');
    expect(markup).toContain('ticket-attachments__batch-title-editor');
    expect(markup).toContain('name="attachment-batch-purpose"');
  });

  it('coalesces separate upload batches into one round until workflow state changes', () => {
    const groups = groupAttachments([
      { id: 'first', name: 'first.png', batch_id: 'upload-one', actor: { role: 'human' }, round: 1 },
      { id: 'second', name: 'second.png', batch_id: 'upload-two', actor: { role: 'human' }, round: 1 },
      { id: 'third', name: 'third.png', batch_id: 'upload-three', actor: { role: 'human' }, round: 2 },
    ]);
    expect(groups.map((group) => [group.label, group.items.map((item) => item.id)])).toEqual([
      ['Human · Round 1', ['first', 'second']],
      ['Human · Round 2', ['third']],
    ]);
  });

  it('gives the attachment menu trigger visible hover and keyboard-focus feedback', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContainSource('.ticket-inspector__attachment[data-action="open-attachment-row"]:hover');
    expect(css).toContainSource(
      '.ticket-inspector__attachment-menu:hover, .ticket-inspector__attachment-menu:focus-visible',
    );
    expect(css).toContain('outline: var(--wa-focus-ring)');
  });

  it('presents attachment groups as transparent titled sections with compact purpose tags', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContain('.ticket-attachments__batch {');
    expect(css).toContain('background: transparent');
    expect(css).toContainSource('.ticket-attachments__batch[data-drag-over="true"] { outline: var(--wa-focus-ring)');
    expect(css).toContain('.ticket-attachments__batch-title {');
    expect(css).toContain('font-size: var(--wa-font-size-m)');
    expect(css).toContain('height: auto');
    expect(css).toContain('overflow-wrap: anywhere');
    expect(css).toContain('white-space: normal');
    expect(css).toContainSource('.ticket-attachments__batch > header select { width: auto');
    expect(css).toContain('field-sizing: content');
  });
});
