import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { ConfidenceBadge, confidenceBand, NoteCard, NoteEditor, RespondToFeedbackButton } from './note-card';

describe('NoteCard', () => {
  it.each([
    ['regular', 'message-square-text', 'Note'],
    ['status', 'refresh-cw', 'Status update'],
    ['feedback_needed', 'circle-alert', 'Feedback needed'],
    ['feedback_draft', 'file-pen-line', 'Feedback draft'],
    ['activity', 'activity', 'Activity'],
  ] as const)('renders the complete %s presentation', (kind, icon, label) => {
    const markup = String(NoteCard({ id: kind, kind, author: 'Codex', time: 'Now', body: 'Body' }));
    expect(markup).toContain(`data-kind="${kind}"`);
    expect(markup).toContain(`data-lucide="${icon}"`);
    expect(markup).toContain(label);
    expect(markup).toContain('Codex');
    expect(markup).toContain('Body');
  });

  it('HS2-VBFSZJ keeps local append purpose after correction/removal without a stale dismissal action', () => {
    const props = {
      id: 'request',
      kind: 'regular' as const,
      author: 'Hot Sheet',
      time: 'Now',
      body: 'Captured request',
      readerMode: true,
      editing: true,
      draft: 'Unsent response',
    };
    const ordinary = String(NoteCard(props));
    expect(ordinary).toContain('aria-label="Note body"');
    const response = String(NoteCard({ ...props, responseDraft: true }));
    expect(response).toContain('aria-label="Feedback response"');
    expect(response).toContain('data-note-response="true"');
    expect(response).toContain('No Response Needed');
    const removed = String(
      NoteCard({
        ...props,
        editing: false,
        feedbackPrompt: true,
        aiAuthored: true,
        title: 'Unsent response to removed request',
        responseDraft: true,
        responseRemoved: true,
      }),
    );
    expect(removed).toContain('Unsent response to removed request');
    expect(removed).toContain('aria-label="Feedback response"');
    expect(removed).toContain('Respond');
    expect(removed).not.toContain('No Response Needed');
    expect(removed).not.toContain('Edit Note');
    expect(removed).not.toContain('data-ai-feedback-target');
    const actualNote = String(
      NoteCard({ ...props, editing: false, feedbackPrompt: true, aiAuthored: true, responseDraft: true }),
    );
    expect(actualNote).toContain('Edit Note');
    expect(actualNote).toContain('data-ai-feedback-target');
    expect(String(NoteCard(props))).not.toContain('data-note-response="true"');
  });

  it('hides only the stored activity-distillation provenance from the reading view', () => {
    const body = 'Connected the client\nLoaded tickets.\n\n<!-- hotsheet:activity-distillation:v1:demo -->';
    const note = { id: 'summary', kind: 'activity' as const, author: 'Hot Sheet AI', time: 'Now', body };
    const markup = String(NoteCard(note));
    expect(markup).toContain('Connected the client');
    expect(markup).toContain('Loaded tickets.');
    expect(markup).not.toContain('activity-distillation');

    const editing = String(NoteCard({ ...note, editing: true }));
    expect(editing).toContain('activity-distillation');
    const ordinary = String(NoteCard({ ...note, kind: 'regular' }));
    expect(ordinary).toContain('activity-distillation');
    const embedded = String(NoteCard({ ...note, body: `${body}\nMore prose.` }));
    expect(embedded).toContain('activity-distillation');
  });

  it('renders a banded confidence badge only for scored notes (HS2-DWTJ43)', () => {
    const unscored = String(NoteCard({ id: 'plain', kind: 'regular', author: 'Codex', time: 'Now', body: 'Body' }));
    expect(unscored).not.toContain('confidence-badge');
    expect(unscored).not.toContain('data-confidence');
    for (const [value, band] of [
      [100, 'verified'],
      [90, 'verified'],
      [89, 'assumed'],
      [70, 'assumed'],
      [69, 'partial'],
      [40, 'partial'],
      [39, 'unverified'],
      [0, 'unverified'],
    ] as const) {
      expect(confidenceBand(value)).toBe(band);
      const markup = String(
        NoteCard({ id: `n${value}`, kind: 'regular', author: 'Codex', time: 'Now', body: 'Body', confidence: value }),
      );
      expect(markup).toContain(`data-confidence="${value}"`);
      expect(markup).toContain(`data-band="${band}"`);
      expect(markup).toContain(`aria-label="Confidence ${value} percent"`);
      expect(markup).toContain('data-lucide="gauge"');
      expect(markup).toContain(`${value}%`);
      // The badge sits in the header beside the timestamp, not in the body.
      expect(markup).toMatch(/note-card__header-end"[\s\S]*confidence-badge[\s\S]*<time class="note-card__time">/u);
    }
    // Kerf's Badge tints each band through its semantic tone; there is no stylesheet (HS2-MBFKJF).
    expect(existsSync(resolve(import.meta.dirname, 'confidence-badge.css'))).toBe(false);
    for (const [value, tone] of [
      [95, 'success'],
      [75, 'info'],
      [50, 'warning'],
      [10, 'danger'],
    ] as const) {
      expect(String(ConfidenceBadge({ value }))).toMatch(
        new RegExp(`data-component="badge" data-tone="${tone}"[^>]*data-size="compact"`, 'u'),
      );
    }
    expect(String(ConfidenceBadge({ value: 82, appearance: 'labeled' }))).toContain('data-size="default"');
    expect(String(ConfidenceBadge({ value: 82, appearance: 'labeled' }))).toContain('Confidence 82%');
  });

  it('renders ConfidenceBadge as a single Kerf Badge root with no wrapper element (HS2-PQHN1J)', () => {
    for (const appearance of ['compact', 'labeled'] as const) {
      const markup = String(ConfidenceBadge({ value: 82, appearance }));
      // Badge is the root: it carries the identity, band, labeled-image name, and title itself.
      expect(markup).toMatch(
        new RegExp(
          `^<span data-confidence-badge="${appearance}" data-band="assumed" data-confidence="82" class="kui-badge" data-component="badge"`,
          'u',
        ),
      );
      expect(markup).toContain('data-has-icon="true"');
      expect(markup).toContain('role="img" aria-label="Confidence 82 percent"');
      expect(markup).toContain('title="AI-reported confidence 82%: verified with minor assumptions"');
      expect(markup).not.toContain('aria-hidden="true" data-component');
      expect(markup).not.toContain('class="confidence-badge"');
      expect(markup).not.toContain('data-component="row"');
      expect(markup.match(/<span /gu)).toHaveLength(2);
    }
  });

  it('renders a controlled editor only when editing is requested', () => {
    const markup = String(
      NoteCard({
        id: 'editable',
        kind: 'regular',
        author: 'Codex',
        time: 'Now',
        body: 'Saved body',
        editing: true,
        draft: 'Draft body',
      }),
    );
    expect(markup).toContain('note-card--editing');
    expect(markup).toContain('aria-label="Note body"');
    expect(markup).toContain('Draft body');
    // A regular note editor autosaves silently — no redundant "Changes save automatically"
    // hint and no Save/Cancel buttons (HS2-5NJRV9).
    expect(markup).not.toContain('Changes save automatically');
    expect(markup).not.toContain('data-action="save-note-edit"');
    expect(markup).not.toContain('data-action="cancel-note-edit"');
  });

  it('never offers in-place editing of a feedback prompt and keeps reader responses empty', () => {
    // HS2-MB2Q10: clicking the prompt opened the plain note editor, so a typed reply overwrote it.
    const sidebar = String(
      NoteCard({
        id: 'feedback-edit',
        kind: 'feedback_needed',
        author: 'Codex',
        time: 'Now',
        body: 'Existing feedback question',
        respondToFeedback: true,
      }),
    );
    expect(sidebar).toContain('Existing feedback question');
    expect(sidebar).not.toContain('data-edit-on-click');
    expect(sidebar).not.toContain('aria-label="Edit note"');
    expect(sidebar).not.toContain('name="note-body"');
    expect(sidebar).toContain('data-action="respond-to-feedback"');
    const responding = String(
      NoteCard({
        id: 'feedback-response',
        kind: 'feedback_needed',
        author: 'Codex',
        time: 'Now',
        body: 'Existing feedback question',
        readerMode: true,
      }),
    );
    expect(responding).toContain('aria-label="Feedback response"');
    expect(responding).toContain(
      '<textarea class="native-textarea" name="note-body" data-note-id="feedback-response" data-note-response="true" aria-label="Feedback response"></textarea>',
    );
  });

  it('opens a feedback prompt for editing only through its explicit Edit Note action (HS2-ZXGEFE)', () => {
    const prompt = { id: 'p1', kind: 'feedback_needed' as const, author: 'Codex', time: 'Now', body: 'Which token?' };
    const sidebar = String(NoteCard({ ...prompt, respondToFeedback: true }));
    expect(sidebar).toContain('data-action="edit-feedback-prompt"');
    expect(sidebar).toContain('aria-label="Edit Note"');
    expect(sidebar).toContain('data-lucide="pencil"');
    expect(sidebar).toContain('data-feedback-prompt-body="true"');
    expect(sidebar).toContain('title="Double-click to edit"');
    // A plain click still never edits the prompt (HS2-MB2Q10).
    expect(sidebar).not.toContain('data-edit-on-click');
    // The reader keeps its response editor and offers the same explicit edit action.
    const reader = String(NoteCard({ ...prompt, readerMode: true }));
    expect(reader).toContain('data-note-response="true"');
    expect(reader).toContain('data-action="edit-feedback-prompt"');
    // While the prompt is being edited, the ordinary note editor replaces the response editor.
    for (const readerMode of [false, true]) {
      const editing = String(
        NoteCard({
          ...prompt,
          readerMode,
          editing: true,
          editingPrompt: true,
          draft: 'Which PAT?',
          respondToFeedback: true,
        }),
      );
      expect(editing).not.toContain('data-action="respond-to-feedback"');
      expect(editing).toContain('name="note-body"');
      expect(editing).toContain('Which PAT?');
      expect(editing).not.toContain('data-note-response="true"');
      expect(editing).not.toContain('data-action="edit-feedback-prompt"');
      expect(editing).not.toContain('No Response Needed');
    }
    // A reply being drafted also sets `editing`; without an explicit prompt edit the reader keeps its response editor.
    expect(String(NoteCard({ ...prompt, readerMode: true, editing: true, draft: 'token stored' }))).toContain(
      'data-note-response="true"',
    );
    // A note that cannot be edited never offers the action.
    expect(String(NoteCard({ ...prompt, editable: false }))).not.toContain('edit-feedback-prompt');
  });

  it('turns feedback text into character-position insertion targets with removable responses', () => {
    const markup = String(
      NoteCard({
        id: 'inline-feedback',
        kind: 'feedback_needed',
        author: 'Codex',
        time: 'Now',
        body: 'Choose:\n\n1. First\n2. Second',
        readerMode: true,
        inlineReplies: [{ offset: 7, text: 'Reply after a character' }],
      }),
    );
    expect(markup.match(/data-action="add-inline-feedback-reply"/g)).toHaveLength(2);
    expect(markup).toContain('class="kui-list" data-component="list"');
    expect(markup).toContain('data-component="badge" data-tone="info"');
    expect(markup).toContain('aria-label="Response at character 7"');
    expect(markup).toContain('Reply after a character');
    expect(markup).toContain('data-action="remove-inline-feedback-reply"');
    expect(markup).toMatch(/data-component="button"[^>]*data-action="remove-inline-feedback-reply"/u);
    expect(markup).toContain('aria-label="Remove Response at Character 7"');
    expect(markup).toContain('aria-label="Feedback response"');
    expect(markup).toContain('placeholder="General response (optional)"');
  });

  it('renders selectable Markdown choices while preserving a freeform response', () => {
    const markup = String(
      NoteCard({
        id: 'choice-feedback',
        kind: 'feedback_needed',
        author: 'Codex',
        time: 'Now',
        body: 'Pick a direction.\n\nCHOICE:\n- **Keep** this\n- `attachment:proof.png`',
        readerMode: true,
        selectedChoices: ['choice-2'],
        attachmentContext: { checkout: 'project', ticket: 'HS2-CHOICE' },
      }),
    );
    expect(markup.match(/data-action="toggle-feedback-choice"/g)).toHaveLength(2);
    expect(markup).not.toContain('CHOICE:');
    expect(markup).toContain('<strong>Keep</strong>');
    // Kerf multi-select ContentItems in a labeled List grid own the selection chrome (HS2-3XV34B).
    expect(markup).toMatch(
      /role="grid"[^>]*aria-label="Feedback Choices"|aria-label="Feedback Choices"[^>]*role="grid"/,
    );
    expect(markup).toContain('aria-selected="false"');
    expect(markup).toContain('aria-selected="true"');
    expect(markup).toContain('data-selection-mode="multiple"');
    expect(markup).not.toContain('note-card__choice');
    expect(markup).toContain('src="/checkouts/project/tickets/HS2-CHOICE/attachments/by-name/proof.png"');
    expect(markup).toContain('placeholder="Additional response (optional)"');
  });

  it('exposes content editing without a redundant edit button', () => {
    const markup = String(NoteCard({ id: 'actions', kind: 'regular', author: 'Codex', time: 'Now', body: 'Body' }));
    expect(markup).toContain('data-edit-on-click="true"');
    expect(markup).toContain('aria-label="Edit note"');
    expect(markup).not.toContain('data-action="edit-note"');
    expect(markup).not.toContain('open-ticket-reader');
    expect(markup).toContain('class="kui-row note-card__header"');
    expect(markup).toContain('class="kui-row note-card__header-end"');
    expect(markup).toContain('data-component="text"');
    expect(markup).toContain('data-action="delete-note"');
    expect(markup).toContain('data-component="button"');
  });

  it('renders note Markdown through the shared safe new-tab boundary', () => {
    const markup = String(
      NoteCard({
        id: 'link',
        kind: 'regular',
        author: 'Codex',
        time: 'Now',
        body: 'Read the [runbook](/docs/runbook).',
      }),
    );
    expect(markup).toContain('href="/docs/runbook" target="_blank" rel="noopener noreferrer"');
  });

  it('renders feedback-needed notes with the same Markdown support as regular notes', () => {
    const markup = String(
      NoteCard({
        id: 'feedback-markdown',
        kind: 'feedback_needed',
        author: 'Codex',
        time: 'Now',
        body: 'Choose one:\n\n1. **Keep** this\n2. Use `that`',
      }),
    );
    expect(markup).toContain('<ol>');
    expect(markup).toContain('<strong>Keep</strong>');
    expect(markup).toContain('<code>that</code>');
  });

  it('keeps regular reader notes directly editable while preserving feedback response behavior', () => {
    const regular = String(
      NoteCard({ id: 'regular', kind: 'regular', author: 'Codex', time: 'Now', body: 'Read only', readerMode: true }),
    );
    expect(regular).toContain('data-edit-on-click="true"');
    expect(regular).toContain('aria-label="Edit note"');
    expect(regular).not.toContain('data-action="edit-note"');
    const needed = String(
      NoteCard({
        id: 'needed',
        kind: 'feedback_needed',
        author: 'Codex',
        time: 'Now',
        body: 'Please answer',
        readerMode: true,
      }),
    );
    expect(needed).toContain('Please answer');
    expect(needed).toContain('aria-label="Feedback response"');
    expect(needed).toContain('data-note-response="true"');
    expect(needed).toContain('Respond');
    expect(needed).toContain('data-action="dismiss-feedback"');
    expect(needed).toContain('No Response Needed');
    expect(needed).toContain('data-size="compact"');
    expect(needed).toContain('data-emphasis="secondary"');
    expect(needed).toContain('data-emphasis="primary"');
    expect(needed).not.toContain('data-lucide="circle-check"');
    const draft = String(
      NoteCard({
        id: 'draft',
        kind: 'feedback_draft',
        author: 'You',
        time: 'Now',
        body: 'Continue me',
        readerMode: true,
      }),
    );
    expect(draft).toContain('aria-label="Note body"');
    expect(draft).toContain('Continue me');
    expect(draft).toContain('Submit');
  });

  it('renders feedback reader navigation only on an opted-in inspector note', () => {
    const props = {
      id: 'needed',
      kind: 'feedback_needed' as const,
      author: 'Codex',
      time: 'Now',
      body: 'Please answer',
      respondToFeedback: true,
    };
    expect(String(NoteCard(props))).toContain('data-action="respond-to-feedback"');
    expect(String(NoteCard({ ...props, readerMode: true }))).not.toContain('respond-to-feedback');
  });

  it('marks the exact regular acknowledgement for subtle presentation', () => {
    const acknowledgement = String(
      NoteCard({ id: 'ack', kind: 'regular', author: 'You', time: 'Now', body: 'No response needed' }),
    );
    expect(acknowledgement).toContain('data-acknowledgement="true"');
    expect(
      String(NoteCard({ id: 'other', kind: 'regular', author: 'You', time: 'Now', body: 'No response needed here' })),
    ).not.toContain('data-acknowledgement="true"');
  });

  it('uses canonical semantic spacing while retaining note-control geometry', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'note-card.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.note-card \{[^}]*gap: var\(--kui-space-xs\)/);
    expect(css).not.toContain('.note-card__header {');
    expect(css).not.toContain('.note-card__header-end {');
    expect(css).not.toContain('.note-card__editor > div {');
    expect(css).not.toContain('note-card__choice');
    expect(css).not.toContain('.note-card__inline-reply-row button');
    // The response badge carries a Lucide icon, never a font glyph (HS2-MDJRCM).
    expect(css).not.toMatch(/feedback-block::after|content: '\+'/);
    expect(css).toMatchSource(/\[data-kind="activity"\] \{[^}]*gap: var\(--kui-space-2xs\)/);
  });

  it('frames every note kind in the Kerf ContentItem appearance chosen by HS2-PATEA0', () => {
    const base = { id: 'n', author: 'A', time: 'now', body: 'Body' };
    const appearance = (props: Partial<Parameters<typeof NoteCard>[0]>) =>
      /data-component="content-item" data-appearance="([a-z-]+)"/.exec(
        String(NoteCard({ ...base, kind: 'regular', ...props })),
      )?.[1];
    expect(appearance({ kind: 'regular' })).toBe('surface');
    expect(appearance({ kind: 'status' })).toBe('neutral');
    expect(appearance({ kind: 'activity' })).toBe('transparent');
    expect(appearance({ kind: 'feedback_needed' })).toBe('warning');
    expect(appearance({ kind: 'feedback_draft' })).toBe('warning');
    expect(appearance({ kind: 'regular', body: 'No response needed' })).toBe('neutral');
    expect(appearance({ kind: 'regular', density: 'compact' })).toBe('surface');
    // A nested AI-feedback entry renders unframed inside its parent's single frame.
    const nested = String(
      NoteCard({ ...base, kind: 'regular', aiFeedback: [{ ...base, id: 'f', kind: 'regular', body: 'Helpful' }] }),
    );
    expect(nested.match(/data-component="content-item"/g)).toHaveLength(1);
    expect(nested.match(/data-component="note-card"/g)).toHaveLength(2);
  });

  it('selects the compact list inset through the density prop and picks Markdown variants itself', () => {
    const base = { id: 'n', author: 'A', time: 'now', body: 'Body' };
    expect(String(NoteCard({ ...base, kind: 'regular' }))).not.toMatch(/<article [^>]*data-density/);
    const compact = String(NoteCard({ ...base, kind: 'activity', density: 'compact' }));
    expect(compact).toContain('data-kind="activity" data-density="compact"');
    expect(compact).toContain('data-component="markdown-preview" data-size="inherit"');
    const css = readFileSync(resolve(import.meta.dirname, 'note-card.css'), 'utf8');
    // Compact only tightens the internal stack; Kerf ContentItem owns the one 8px inset (HS2-GZDKM7).
    expect(css).toMatchSource(/\.note-card\[data-density='compact'\] \{ gap: var\(--kui-space-2xs\); \}/);
    expect(css).not.toMatch(/\.note-card(\[[^\]]*\])* \{[^}]*(padding|border|background):/);
    expect(css).not.toContain('.markdown-preview');
    const choices = String(
      NoteCard({
        ...base,
        kind: 'feedback_needed',
        readerMode: true,
        body: 'Pick one\n\nCHOICE:\n- First\n- Second',
      }),
    );
    expect(choices).toContain('data-size="inherit" data-media="thumbnail"');
  });

  it('keeps AI attribution, limitations, and feedback in the note accessible name', () => {
    const markup = String(
      NoteCard({
        id: 'distilled',
        kind: 'activity',
        author: 'Hot Sheet AI',
        aiAuthored: true,
        aiTool: 'Codex',
        time: 'Now',
        body: 'Summarized the test run.',
      }),
    );
    expect(markup).toContain('data-ai-authored="true"');
    expect(markup).toContain('aria-label="AI-generated activity by Codex; may contain errors"');
    expect(markup).toContain('data-component="ai-content-label"');
    expect(markup).toContain('data-ai-feedback-target="note:distilled"');
  });
  it('announces human edits in the note and attribution accessible names', () => {
    const markup = String(
      NoteCard({
        id: 'edited',
        kind: 'regular',
        author: 'Codex',
        aiAuthored: true,
        humanEdited: true,
        time: 'Now',
        body: 'Human revision',
      }),
    );
    expect(markup).toContain('aria-label="AI-generated note by Codex; edited by human"');
    expect(markup).toContain('aria-label="AI-generated by Codex; edited by human"');
  });

  it('shows AI attribution for prose while leaving automatic event labels plain', () => {
    const base = { id: 'event', kind: 'activity' as const, author: 'Codex', aiAuthored: true, time: 'Now' };
    const event = String(NoteCard({ ...base, body: 'Status changed from Started to Completed' }));
    expect(event).toContain('data-ai-authored="true"');
    expect(event).not.toContain('AI-generated');
    expect(event).not.toContain('data-component="ai-content-label"');
    expect(event).toMatch(
      /<footer class="note-card__footer">[\s\S]*data-component="text"[\s\S]*Codex<\/span><\/footer>/u,
    );

    const mixed = String(NoteCard({ ...base, body: 'Status changed from Started to Completed\n\nVerified the fix.' }));
    expect(mixed).toContain('aria-label="AI-generated activity by Codex; may contain errors"');
    expect(mixed).toContain('data-component="ai-content-label"');
    const readerEvent = String(
      NoteCard({ ...base, body: 'Status changed from Started to Completed', readerMode: true }),
    );
    const readerMixed = String(
      NoteCard({ ...base, body: 'Status changed from Started to Completed\n\nVerified the fix.', readerMode: true }),
    );
    expect(readerEvent).not.toContain('AI-generated');
    expect(readerMixed).toContain('aria-label="AI-generated activity by Codex; may contain errors"');

    const status = String(NoteCard({ ...base, kind: 'status', body: 'Claim expired — reclaimed' }));
    expect(status).not.toContain('AI-generated');
    expect(
      String(NoteCard({ ...base, kind: 'status', body: 'Claim expired — reclaimed\n\nReviewed the cause.' })),
    ).toContain('AI-generated status update by Codex');
    expect(String(NoteCard({ ...base, kind: 'regular', body: '  ' }))).not.toContain('AI-generated');
  });
});

describe('NoteEditor and RespondToFeedbackButton (HS2-3X404M)', () => {
  it('renders a plain note editor without submit actions', () => {
    const markup = String(NoteEditor({ id: 'n1', source: 'Draft' }));
    expect(markup).toContain('class="note-card__editor"');
    expect(markup).toContain('aria-label="Note body"');
    expect(markup).toContain('>Draft</textarea>');
    expect(markup).not.toContain('wa-button');
    expect(markup).not.toContain('data-note-response');
  });

  it('renders a feedback response editor with its dismiss and respond actions', () => {
    const markup = String(
      NoteEditor({
        id: 'details',
        source: '',
        response: true,
        actions: true,
        placeholder: 'Additional response (optional)',
      }),
    );
    expect(markup).toContain('aria-label="Feedback response"');
    expect(markup).toContain('placeholder="Additional response (optional)"');
    expect(markup.match(/data-note-response="true"/g)).toHaveLength(2);
    expect(markup).toContain('No Response Needed');
    expect(markup).toContain('data-note-id="details"');
    expect(markup).toContain('>Respond</span>');
  });

  it('draws the AI feedback disclosure arrows with Kerf DisclosureArrow (HS2-XSBED3)', () => {
    const feedback = { id: 'fb', kind: 'regular' as const, author: 'Human', time: 'Now', body: 'Helpful' };
    const markup = String(
      NoteCard({ id: 'parent', kind: 'activity', author: 'AI', time: 'Now', body: 'Body', aiFeedback: [feedback] }),
    );
    expect(markup).toMatch(/note-card__ai-feedback-show">[\s\S]*?kui-disclosure-arrow[\s\S]*?Show AI Feedback/);
    expect(markup).toMatch(/note-card__ai-feedback-hide">[\s\S]*?kui-disclosure-arrow[\s\S]*?Hide AI Feedback/);
    const css = readFileSync(resolve(import.meta.dirname, 'note-card.css'), 'utf8');
    expect(css).not.toMatch(/summary::before/);
  });

  it('labels a non-response editor action Submit', () => {
    expect(String(NoteEditor({ id: 'd', source: 'x', actions: true }))).toMatch(/>\s*Submit\s*</);
  });

  it('renders the full-width Respond to Feedback action for a note id', () => {
    const markup = String(RespondToFeedbackButton({ id: 'feedback-1' }));
    expect(markup).toContain('class="note-card__respond"');
    expect(markup).toContain('data-note-id="feedback-1"');
    expect(markup).toContain('Respond to Feedback');
  });
});
