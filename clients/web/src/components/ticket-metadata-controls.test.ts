import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { groupAttachments,TicketAttachments } from './ticket-attachments';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketPrioritySelect } from './ticket-priority-select';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTimeline } from './ticket-timeline';

describe('ticket metadata controls and inspector panels', () => {
  it('renders colored category icons and semantic priority icons', () => {
    const category = String(TicketCategorySelect({ name: 'category', value: 'bug' }));
    expect(category).toContain('data-component="select"');
    expect(category).toContain('data-lucide="bug"');
    expect(category).toContain('color:#ef4444');
    const priority = String(TicketPrioritySelect({ name: 'priority', value: 'urgent' }));
    expect(priority).toContain('data-lucide="chevrons-up"');
    expect(priority).toContain('data-lucide="minus"');
    const status = String(TicketStatusMenu({ value: 'completed' }));
    expect(status).toContain('aria-label="Change status, Completed"');
    expect(status).toContain('select select--custom-selected ticket-status-menu');
    expect(status).toContain('name="inspector-status"');
    expect(status).toContain('<span slot="start" class="select__custom-selected"><span class="status-badge status-badge--completed');
    expect(status).toContain('<wa-option value="verified"><span slot="start" class="select__icon"');
    expect(status).toContain('<wa-divider></wa-divider><wa-option value="backlog"');
    expect(status).toContain('<wa-option value="archive"');
    expect(status).toContain('data-lucide="badge-check"');
    expect(status.match(/data-lucide=/g)).toHaveLength(7);
  });

  it('renders inspector sections independently of the inspector shell', () => {
    const info = String(TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: ['ux'], details: 'Details' }));
    expect(info).toContain('data-component="ticket-info-panel"');
    expect(info.match(/data-component="menu-header"/g)).toHaveLength(3);
    expect(info).toContain('<header class="menu-header" data-component="menu-header"><h2>Details</h2>');
    expect(info).toContain('<header class="menu-header" data-component="menu-header"><h2>Tags</h2>');
    expect(info).toContain('data-action="open-ticket-tag-popover" popoverTarget="ticket-tag-popover" aria-haspopup="dialog" aria-controls="ticket-tag-popover" aria-label="Add tag"');
    expect(info).toContain('data-action="edit-blocked-reason"');
    expect(info).toContain('Block ticket');
    expect(info).not.toContain('<h2>Blocked reason</h2>');
    expect(info).toContain('ticket-inspector__block-action');
    expect(info).toContain('class="menu-item ticket-inspector__block-action" data-component="menu-item"');
    expect(info).toContain('<h2>Notes 0</h2>');
    expect(info).toContain('class="menu-item ticket-notes__add" data-component="menu-item"');
    const blocked = String(TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: [], details: '', blockedReason: 'Waiting' }));
    expect(blocked).toContain('<header class="menu-header" data-component="menu-header"><h2>Blocked reason</h2>');
    expect(blocked).toContain('data-edit-blocked-reason="true"');
    expect(blocked).toContain('aria-label="Edit blocked reason"');
    expect(blocked).not.toContain('ticket-inspector__text-action');
    expect(blocked).toContain('Waiting');
    expect(blocked.match(/data-component="menu-header"/g)).toHaveLength(4);
    expect(info).toContain('data-component="ticket-notes"');
    const timeline = String(TicketTimeline({ entries: [{ id: 'one', time: 'Now', title: 'One event', subtitle: 'Optional detail' }] }));
    expect(timeline.match(/<li/g)).toHaveLength(1);
    expect(timeline).toContain('One event');
    expect(timeline).toContain('Optional detail');
    expect(timeline).toContain('1 event total');
    const attachments = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png',url:'/attachment/one',annotationCount:2 }] }));
    expect(attachments.match(/class="ticket-inspector__attachment"/g)).toHaveLength(1);
    expect(attachments).toContain('1 attachment total');
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
    const css=readFileSync(resolve(import.meta.dirname,'ticket-inspector-panel.css'),'utf8');
    expect(css).toMatch(/ticket-attachments__image-grid :is\(img,video\) \{[^}]*object-fit: contain/);
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
  });

  it('shows videos in the media grid without starting playback', () => {
    const attachments = String(TicketAttachments({ attachments: [{ id: 'clip', name: 'walkthrough.webm',url:'/attachment/clip',thumbnailUrl:'/attachment/clip/thumbnail' }] }));
    expect(attachments).toContain('aria-label="Attached media"');
    expect(attachments).toContain('<video');
    expect(attachments).toContain('preload="metadata"');
    expect(attachments).toContain('src="/attachment/clip#t=0.1"');
    expect(attachments).toContain('poster="/attachment/clip/thumbnail"');
    expect(attachments).not.toContain('autoplay');
    expect(attachments).toContain('Open walkthrough.webm in media gallery');
  });

  it('groups stable batches and keeps old/provider attachments explicitly legacy',()=>{
    const groups=groupAttachments([
      {id:'old',name:'old.png'},
      {id:'a',name:'a.png',batch_id:'fix',actor:{identity:'codex',role:'ai'},purpose:'correctness_evidence'},
      {id:'b',name:'b.png',batch_id:'fix',actor:{identity:'codex',role:'ai'},purpose:'correctness_evidence'},
      {id:'c',name:'c.png',batch_id:'human',batch_label:'More feedback',actor:{display_name:'Brian',role:'human'},purpose:'problem_evidence'},
      {id:'d',name:'diagnostics.json',batch_id:'automatic',actor:{role:'system'},purpose:'problem_evidence'},
    ]);
    expect(groups.map(group=>[group.label,group.items.length])).toEqual([
      ['Legacy / Uncategorized',1],
      ['AI · Round 1 · Correctness evidence',2],
      ['More feedback',1],
      ['System · Batch 1 · Problem evidence',1],
    ]);
    const markup=String(TicketAttachments({attachments:groups.flatMap(group=>group.items)}));
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

  it('coalesces separate upload batches into one round until workflow state changes',()=>{
    const groups=groupAttachments([
      {id:'first',name:'first.png',batch_id:'upload-one',actor:{role:'human'},round:1},
      {id:'second',name:'second.png',batch_id:'upload-two',actor:{role:'human'},round:1},
      {id:'third',name:'third.png',batch_id:'upload-three',actor:{role:'human'},round:2},
    ]);
    expect(groups.map(group=>[group.label,group.items.map(item=>item.id)])).toEqual([
      ['Human · Round 1',['first','second']],
      ['Human · Round 2',['third']],
    ]);
  });

  it('gives the attachment menu trigger visible hover and keyboard-focus feedback', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContain('.ticket-inspector__attachment[data-action="open-attachment-row"]:hover');
    expect(css).toContain('.ticket-inspector__attachment-menu:hover, .ticket-inspector__attachment-menu:focus-visible');
    expect(css).toContain('outline: var(--wa-focus-ring)');
  });

  it('presents attachment groups as transparent titled sections with compact purpose tags', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContain('.ticket-attachments__batch {');
    expect(css).toContain('background: transparent');
    expect(css).toContain('.ticket-attachments__batch[data-drag-over="true"] { outline: var(--wa-focus-ring)');
    expect(css).toContain('.ticket-attachments__batch-title {');
    expect(css).toContain('font-size: var(--wa-font-size-m)');
    expect(css).toContain('height: auto');
    expect(css).toContain('overflow-wrap: anywhere');
    expect(css).toContain('white-space: normal');
    expect(css).toContain('.ticket-attachments__batch > header select { width: auto');
    expect(css).toContain('field-sizing: content');
  });
});
