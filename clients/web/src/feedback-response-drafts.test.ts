import { describe, expect, it, vi } from 'vitest';

import type { Note } from './api';
import { NoteCard } from './components/note-card';
import { coalescedDraft, flushDraftInputs } from './draft-input-coalescing';
import type { FeedbackResponseOwner } from './feedback-response-drafts';
import { createFeedbackResponseDrafts } from './feedback-response-drafts';

const request: Note = {
  id: 'request',
  kind: 'feedback_needed',
  text: 'Synthetic request',
  created_at: '2026-10-27',
  edited_at: '2026-10-27',
};
const scope = { projectId: 'alpha', qualifiedId: 'source:ticket', surface: 'workspace-reader' };

describe('HS2-VBFSZJ local response owners', () => {
  it('separates qualified owners, retains captured context, and resets empty then refill', () => {
    const drafts = createFeedbackResponseDrafts(),
      first = drafts.begin(scope, request);
    expect(drafts.begin(scope, { ...request, kind: 'regular' })).toBe(first);
    expect(first.request.kind).toBe('feedback_needed');
    for (const other of [
      { ...scope, projectId: 'beta' },
      { ...scope, qualifiedId: 'other:ticket' },
      { ...scope, surface: 'linked-frame' },
    ]) {
      expect(drafts.get(other)).toBeUndefined();
      drafts.begin(other, request);
    }
    drafts.retain([scope]);
    expect(drafts.get(scope)).toBe(first);
    drafts.retain([]);
    expect(drafts.get(scope)).toBeUndefined();
    const reopened = drafts.begin(scope, request);
    expect(reopened).not.toBe(first);
    expect(drafts.clear(first)).toBe(false);
    expect(drafts.get(scope)).toBe(reopened);
    expect(drafts.clear(reopened)).toBe(true);
    expect(drafts.get(scope)).toBeUndefined();
  });
  it('fences an earlier submission after further typing, reset, or a newer request', () => {
    const drafts = createFeedbackResponseDrafts(),
      sent = drafts.begin(scope, request, 'First input'),
      edited = drafts.touch(sent, 'Latest input before the coalescer');
    expect(drafts.get(scope)?.draft).toBe('Latest input before the coalescer');
    expect(drafts.isCurrent(sent)).toBe(false);
    expect(drafts.acceptsInput(sent)).toBe(false);
    const metadata = drafts.touch(edited);
    expect(drafts.isCurrent(edited)).toBe(false);
    expect(drafts.acceptsInput(edited)).toBe(true);
    expect(drafts.isCurrent(metadata)).toBe(true);
    expect(drafts.clear(sent)).toBe(false);
    expect(drafts.get(scope)).toBe(metadata);
    const newer = drafts.begin(scope, { ...request, id: 'next-request' });
    expect(drafts.clear(edited)).toBe(false);
    expect(drafts.touch(edited)).toBe(edited);
    expect(drafts.get(scope)).toBe(newer);
    drafts.clear(undefined);
    drafts.retain([]);
    expect(drafts.clear(newer)).toBe(false);
  });
  it('renders pending input before its 150 ms publication and refuses a closed owner at flush (HS2-VBFSZJ)', () => {
    vi.useFakeTimers();
    const drafts = createFeedbackResponseDrafts(),
      published = vi.fn(),
      input = coalescedDraft<FeedbackResponseOwner>((owner) => {
        if (drafts.acceptsInput(owner)) published(owner.draft);
      });
    try {
      const owner = drafts.begin(scope, request, 'Input not yet in the draft signal');
      input.schedule(owner);
      vi.advanceTimersByTime(149);
      expect(published).not.toHaveBeenCalled();
      const rendered = String(
        NoteCard({
          id: request.id,
          kind: 'regular',
          author: 'Hot Sheet',
          time: 'Now',
          body: request.text,
          readerMode: true,
          responseDraft: true,
          editing: true,
          draft: drafts.get(scope)?.draft,
        }),
      );
      expect(rendered).toContain('Input not yet in the draft signal');
      flushDraftInputs();
      expect(published).toHaveBeenLastCalledWith('Input not yet in the draft signal');
      input.schedule(drafts.touch(owner, 'Late old input'));
      drafts.retain([]);
      const reopened = drafts.begin(scope, request, 'New owner input');
      flushDraftInputs();
      expect(published).toHaveBeenCalledTimes(1);
      expect(drafts.get(scope)).toBe(reopened);
      expect(drafts.get(scope)?.draft).toBe('New owner input');
    } finally {
      input.cancel();
      vi.useRealTimers();
    }
  });
});
