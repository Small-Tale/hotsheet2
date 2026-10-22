import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { Bug } from 'lucide';
import { describe, expect, it } from 'vitest';

import { groupAttachments, TicketAttachments } from './ticket-attachments';
import { categoryColorToken, TicketCategorySelect } from './ticket-category-select';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketPrioritySelect } from './ticket-priority-select';
import { statusMenuAnchorY, TicketStatusMenu } from './ticket-status-menu';
import { TicketTimeline } from './ticket-timeline';

describe('ticket metadata controls and inspector panels', () => {
  it('colors built-in categories by their shared token and custom ones by the fallback (HS2-86A5CR)', () => {
    expect(categoryColorToken('feature')).toBe('--hs-category-feature');
    expect(categoryColorToken('requirement_change')).toBe('--hs-category-requirement-change');
    expect(categoryColorToken('chore')).toBe('--hs-category-fallback');
    const select = String(TicketCategorySelect({ name: 'category', value: 'bug' }));
    expect(select).toContain('--hs-category-bug');
    expect(select).toContain('--hs-category-requirement-change');
  });
  it('reserves phone space above the Started submenu', () => {
    expect(statusMenuAnchorY(269, 390, 844)).toBeCloseTo(354.48);
    expect(statusMenuAnchorY(400, 390, 844)).toBe(400);
    expect(statusMenuAnchorY(269, 1280, 844)).toBe(269);
  });
  it('offers Started phases in the status submenu only for editable git tickets (HS2-HHKJSC)', () => {
    const ticket = {
      status: 'started' as const,
      priority: 'high' as const,
      category: 'feature',
      tags: [],
      details: '',
    };
    const editable = String(TicketInfoPanel({ ...ticket, startedPhase: 'planning', canEditStartedPhase: true }));
    expect(editable).not.toContain('name="inspector-started-phase"');
    expect(editable).toContain('aria-label="Change Status, Planning"');
    for (const phase of ['', 'analyzing', 'planning', 'working', 'initial_testing', 'integrating', 'final_testing'])
      expect(editable).toContain(`data-started-phase="${phase}"`);
    expect(editable).toContain('data-action="set-inspector-started-phase"');
    expect(String(TicketInfoPanel({ ...ticket, canEditStartedPhase: false }))).not.toContain('data-started-phase=');
    expect(String(TicketInfoPanel({ ...ticket, status: 'completed', canEditStartedPhase: true }))).toContain(
      'data-started-phase=',
    );
  });

  it('shows the testing-phase estimate on the detail status line after the status trigger (HS2-YDGM04)', () => {
    const ticket = {
      status: 'started' as const,
      priority: 'high' as const,
      category: 'feature',
      tags: [],
      details: '',
    };
    const estimate = { kind: 'estimate' as const, label: '~4m left', title: 'This phase typically takes about 9m' };
    const markup = String(TicketInfoPanel({ ...ticket, startedPhase: 'initial_testing', phaseEstimate: estimate }));
    expect(markup).toMatch(/data-status-line[^]*data-phase-estimate="estimate"/);
    expect(markup.indexOf('Change Status, Initial testing')).toBeLessThan(markup.indexOf('data-phase-estimate'));
    expect(markup).toContain('title="This phase typically takes about 9m"');
    expect(String(TicketInfoPanel({ ...ticket, status: 'completed', phaseEstimate: estimate }))).not.toContain(
      'data-phase-estimate',
    );
  });

  it('uses compact Details field-label geometry in preview and editing modes (HS2-S6S709)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-info-panel.css'), 'utf8');
    expect(css).toContainSource(
      '.ticket-info-panel__status-field, .ticket-info-panel__details-section { display: grid; min-width: 0; --kui-list-header-min-height: 0; --kui-list-header-title-min-height: 0; --kui-list-header-title-padding-block: 0; --kui-list-header-border-width: 0; --kui-list-header-title-padding-inline: 1px; gap: calc(var(--kui-font-xs) * 0.5); }',
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
      expect(markup).toContain('class="ticket-info-panel__section ticket-info-panel__details-section"');
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
    expect(readOnly).toContain('class="ticket-info-panel__section ticket-info-panel__details-section"');
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
    // Ordinary Select content must name Kerf's native combobox trigger too (HS2-Q6EM0B).
    expect(String(TicketCategorySelect({ name: 'category', value: 'bug', ariaLabel: 'Compact category' }))).toMatch(
      /class="kui-select__trigger" role="combobox"[^>]*aria-label="Compact category"/,
    );
    expect(String(TicketPrioritySelect({ name: 'priority', value: 'urgent', ariaLabel: 'Compact priority' }))).toMatch(
      /class="kui-select__trigger" role="combobox"[^>]*aria-label="Compact priority"/,
    );
    const status = String(TicketStatusMenu({ value: 'completed' }));
    expect(status).toContain('aria-label="Change Status, Completed"');
    // The app-owned wrapper carries the placement class; the Kerf Select root keeps only its own classes.
    expect(status).toContain('<span class="ticket-status-menu"');
    expect(status).toContain('class="status-badge" data-component="status-badge" data-status="completed"');
    expect(status).toContain('data-inspector-status-menu="true"');
    expect(status).toContain('data-ticket-status="verified"');
    expect(status).toContain('class="kui-popup-menu__divider" role="separator"');
    expect(status).toContain('data-ticket-status="archive"');
    expect(status).toContain('data-lucide="badge-check"');
    // One icon per status (and the badge); Kerf adds its own check marks to the checkable items.
    expect(status.match(/data-lucide="(?!check")/g)).toHaveLength(7);
    // Kerf owns the trigger button and wraps the app-owned status badge.
    expect(status).toContain('data-action="open-inspector-status-menu"');
    expect(status).toContain('aria-haspopup="menu"');
    expect(status).toContain('class="kui-popup-menu__trigger-content"');
    expect(status).not.toContain('data-interactive');
    expect(existsSync(new URL('./status-badge.css', import.meta.url))).toBe(false);
    // Kerf beta.96 owns the submenu safe-triangle fix (KF-YAHAZK), so the menu ships no CSS (HS2-80E3X0).
    expect(existsSync(new URL('./ticket-status-menu.css', import.meta.url))).toBe(false);
  });

  it('renders inspector sections independently of the inspector shell', () => {
    const info = String(
      TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: ['ux'], details: 'Details' }),
    );
    expect(info).toContain('data-component="ticket-info-panel"');
    expect(info.match(/data-component="list-header"/g)).toHaveLength(4);
    expect(info).toContain('data-font="default" data-border="none">Status</h2>');
    expect(info).toContain('class="kui-list-inset-control"');
    expect(info).toContain('data-status-line="true"');
    expect(info).toMatch(
      /<kui-select[^>]*name="inspector-category"[^>]*><span class="kui-select__label"[^>]*>Category</,
    );
    expect(info).toMatch(
      /<kui-select[^>]*name="inspector-priority"[^>]*><span class="kui-select__label"[^>]*>Priority</,
    );
    expect(info).toContain('data-font="default" data-border="none">Details</h2>');
    expect(info).toContain('data-font="default" data-border="none">Tags</h2>');
    expect(info).toMatch(
      /data-tag-suggestions="ticket-tag-popover-suggestions"[^>]*aria-haspopup="dialog"[^>]*data-action="open-ticket-tag-popover"[^>]*aria-label="Add tag"/,
    );
    expect(info).toContain('data-action="edit-blocked-reason"');
    expect(info).toContain('Block ticket');
    expect(info).not.toContain('<h2>Blocked Reason</h2>');
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
    expect(deleted).toContain('data-status-line="true"');
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
    expect(blocked).toContain('data-font="default" data-border="none">Blocked Reason</h2>');
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
    expect(readOnly).toContain('data-status="started"');
    expect(readOnly).not.toContain('data-inspector-status-menu');
    const timeline = String(
      TicketTimeline({ entries: [{ id: 'one', time: 'Now', title: 'One event', subtitle: 'Optional detail' }] }),
    );
    expect(timeline.match(/<li/g)).toHaveLength(1);
    expect(timeline).toContain('One event');
    expect(timeline).toContain('Optional detail');
    expect(timeline).toContain('1 event total');
    const timedTimeline = String(
      TicketTimeline({ entries: [{ id: 'timed', time: 'Now', title: 'Timed event', usageToNext: '38K tokens' }] }),
    );
    expect(timedTimeline).toMatch(/class="ticket-timeline__duration"[^>]*><span class="ticket-timeline__usage"/);
    expect(timedTimeline).toContain('38K tokens');
    const attachments = String(
      TicketAttachments({ attachments: [{ id: 'one', name: 'one.png', url: '/attachment/one', annotationCount: 2 }] }),
    );
    expect(attachments.match(/class="ticket-attachments__item"/g)).toHaveLength(1);
    // Kerf ListHeader owns the heading and its count (HS2-QK4HES).
    expect(attachments).toMatch(/data-component="list-header"[^>]*data-has-count/u);
    expect(attachments).toContain('1 attachment');
    expect(attachments).not.toContain('attachment total');
    expect(attachments).toContain('data-attachment-drop-target="true"');
    expect(attachments).toContain('aria-label="Browse and add attachments"');
    expect(attachments).toContain('aria-label="Drop or browse attachments"');
    // Kerf FilePickerTrigger and FileDropZone own the Add trigger and drop zone (HS2-VHY325).
    expect(attachments).toMatch(
      /data-component="file-picker-trigger"[^>]*data-file-picker-id="ticket-attachments"[\s\S]*?name="ticket-attachments"/u,
    );
    expect(attachments).toMatch(
      /data-component="file-drop-zone"[^>]*data-file-picker-id="ticket-attachments"[\s\S]*?name="ticket-attachments"/u,
    );
    expect(attachments).not.toContain('ticket-attachments__browse');
    expect(attachments).not.toContain('ticket-attachments__drop');
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
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-attachments.css'), 'utf8');
    // Media grid items cap at 220px wide so they wrap instead of stretching full width (HS2-KEZ0EY).
    expect(css).toMatch(/ticket-attachments__image-grid button \{[^}]*max-width: remify\(220px\)/);
    expect(css).toMatchSource(/ticket-attachments__image-grid :is\(img,video\) \{[^}]*object-fit: contain/);
    expect(css).not.toContain('ticket-attachments__count');
    // Kerf ContentItem, Text, and icon-only Button own the row chrome inside the app-owned wrapper (HS2-K9HSXZ).
    expect(attachments).toMatch(
      /data-component="ticket-attachment-item"[^>]*>\s*<div[^>]*data-component="content-item"[^>]*data-appearance="neutral"/u,
    );
    expect(attachments).toMatch(
      /data-component="button"[^>]*data-icon-only=""[^>]*data-action="open-attachment-menu"/u,
    );
    expect(attachments).toContain('More Actions for one.png');
    expect(attachments).toContain('title="More actions for one.png"');
    expect(attachments).toContain('title="one.png — double-click to open"');
    expect(attachments).toContain('data-lucide="more-horizontal"');
    const unsupported = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png' }], enabled: false }));
    expect(unsupported).toContain('does not support attachment actions');
    expect(unsupported).not.toContain('name="ticket-attachments"');
    expect(unsupported).not.toContain('data-action="open-attachment-menu"');
    expect(unsupported).not.toContain('data-action="open-attachment-row"');
    expect(unsupported).not.toContain('data-drag-attachment-id');
    expect(unsupported).not.toContain('data-action="edit-attachment-batch-label"');
    expect(unsupported).toMatch(/data-component="list-header"[\s\S]*?<h3[^>]*>[\s\S]*?Legacy \/ Uncategorized/u);
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
    expect(appendOnly).toMatch(/<kui-select[^>]*name="attachment-batch-purpose"[^>]*disabled/u);
    expect(appendOnly).toContain('Purpose for Legacy / Uncategorized');
    expect(appendOnly).toContain(
      '<a href="/attachment/two" target="_blank" rel="noopener" title="Open log.txt">log.txt</a>',
    );
    expect(appendOnly).toContain('data-action="open-attachment-gallery"');
    // A delete-only provider (Jira, HS2-51ADSG) keeps rows read-only links but gives each the
    // reduced `remote` row menu; nothing becomes draggable, relabelable, or locally openable.
    const deleteOnly = String(
      TicketAttachments({
        attachments: [{ id: '10001', name: 'log.txt', url: '/attachment/10001' }],
        enabled: true,
        editable: false,
        removable: true,
      }),
    );
    expect(deleteOnly).toContain('data-attachment-menu-kind="remote"');
    expect(deleteOnly).toContain('data-attachment-action-id="10001"');
    expect(deleteOnly).toContain('data-action="open-attachment-menu"');
    expect(deleteOnly).not.toContain('data-action="open-attachment-row"');
    expect(deleteOnly).not.toContain('data-drag-attachment-id');
    expect(deleteOnly).not.toContain('data-action="edit-attachment-batch-label"');
    expect(deleteOnly).toContain(
      '<a href="/attachment/10001" target="_blank" rel="noopener" title="Open log.txt">log.txt</a>',
    );
    // A disabled surface never offers the menu, even when the provider could delete.
    const disabled = String(
      TicketAttachments({ attachments: [{ id: '10001', name: 'log.txt' }], enabled: false, removable: true }),
    );
    expect(disabled).not.toContain('data-action="open-attachment-menu"');
  });

  it('keeps labelled batches immutable on a disabled reader despite provider edit support (HS2-QZFZA8)', () => {
    const markup = String(
      TicketAttachments({
        attachments: [
          {
            id: 'linked',
            name: 'linked.png',
            batch_id: 'batch',
            batch_label: 'Linked evidence',
            purpose: 'problem_evidence',
          },
        ],
        enabled: false,
        editable: true,
        editingLabelBatch: 'batch',
        editingLabelDraft: 'Unreachable draft',
        message: 'Attachments are view-only in linked ticket readers.',
      }),
    );
    expect(markup).toMatch(/<h3[^>]*>[\s\S]*?Linked evidence/u);
    expect(markup).toContain('value="Linked evidence"');
    expect(markup).not.toContain('Unreachable draft');
    expect(markup).not.toContain('data-editing-label');
    expect(markup).not.toContain('data-action="edit-attachment-batch-label"');
    expect(markup).not.toContain('data-attachment-group-drop-target');
    expect(markup).not.toContain('data-drag-attachment-id');
    expect(markup).toMatch(/<input[^>]*name="attachment-batch-label"[^>]*disabled/u);
    expect(markup).toMatch(/<kui-select[^>]*name="attachment-batch-purpose"[^>]*disabled/u);
    expect(markup).toContain('Attachments are view-only in linked ticket readers.');
    expect(markup).not.toContain('This provider does not support');
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
    expect(markup).toContain('data-attachment-batch-title="true"');
    expect(markup).toMatch(/<h3[^>]*data-component="text"[^>]*data-size="medium"[^>]*>/u);
    expect(markup).toContain('Edit Batch Label AI · Round 1 · Correctness evidence');
    expect(markup).toMatch(
      /<div hidden><label[^>]*data-component="text-field"[^>]*>[\s\S]*?<input[^>]*name="attachment-batch-label"/u,
    );
    expect(markup).toMatch(/<kui-select[^>]*name="attachment-batch-purpose"/u);
    // Uncategorized carries a sentinel value; Web Awesome treats an empty option value as no choice.
    expect(markup).toContain('value="uncategorized"');
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

  it('styles only app-owned attachment markup: the row wrapper cursor and drag targets (HS2-K9HSXZ)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-attachments.css'), 'utf8');
    expect(css).toContainSource('.ticket-attachments__item[data-action="open-attachment-row"] { cursor: pointer; }');
    expect(css).toContainSource(
      ':is(.ticket-attachments__batch, .ticket-attachments__new-group)[data-drag-over="true"] { outline: var(--wa-focus-ring)',
    );
    // Kerf ContentItem, icon-only Button, ListHeader, TextField, and Select own the former row, menu, title,
    // editor, and purpose pill chrome.
    for (const removed of ['ticket-attachments__item-menu', 'batch-title-editor', 'header select', 'field-sizing'])
      expect(css).not.toContain(removed);
    expect(css).not.toMatch(/\.kui-/u);
  });
});
