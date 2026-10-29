import { describe, expect, it } from 'vitest';

import {
  DETAILS_FEEDBACK_ID,
  fullTicketFeedbackNeeded,
  isFeedbackNeeded,
  isStaleFeedbackDismissal,
  NO_RESPONSE_NEEDED_NOTE,
  presentedNoteKind,
  textRequestsFeedback,
} from './feedback-needed';

const note = (id: string, kind: 'regular' | 'activity' | 'feedback_needed', created_at: string) => ({
  id,
  kind,
  created_at,
});

describe('isFeedbackNeeded', () => {
  it('uses the latest regular or feedback-needed note as the response state', () => {
    expect(isFeedbackNeeded([])).toBe(false);
    expect(isFeedbackNeeded([note('1', 'feedback_needed', '2026-09-02T00:00:00Z')])).toBe(true);
    expect(
      isFeedbackNeeded([
        note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
        note('2', 'activity', '2026-09-02T00:01:00Z'),
      ]),
    ).toBe(true);
    expect(
      isFeedbackNeeded([
        note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
        note('2', 'regular', '2026-09-02T00:02:00Z'),
      ]),
    ).toBe(false);
    expect(
      isFeedbackNeeded([
        note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
        note('2', 'regular', '2026-09-02T00:02:00Z'),
        note('3', 'feedback_needed', '2026-09-02T00:03:00Z'),
      ]),
    ).toBe(true);
  });

  it('keeps an open feedback request open across AI thumbs ratings', () => {
    const ask = note('ask', 'feedback_needed', '2026-09-02T00:00:00Z');
    const rating = {
      ...note('rating', 'regular', '2026-09-02T00:01:00Z'),
      text: 'AI feedback for note:source: Helpful — keep suggestions like this.',
    };
    expect(isFeedbackNeeded([ask, rating])).toBe(true);
    expect(presentedNoteKind(ask, [ask, rating])).toBe('feedback_needed');
    expect(fullTicketFeedbackNeeded({ details: '', notes: [ask, rating] })).toBe(true);
    expect(isFeedbackNeeded([ask, { ...rating, text: 'Edited rating', feedback_for: 'source' }])).toBe(true);
  });

  it('presents answered asks as regular notes without changing later unanswered asks', () => {
    const ask = note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
      response = note('2', 'regular', '2026-09-02T00:01:00Z'),
      next = note('3', 'feedback_needed', '2026-09-02T00:02:00Z'),
      notes = [ask, response, next];
    expect(presentedNoteKind(ask, notes)).toBe('regular');
    expect(presentedNoteKind(response, notes)).toBe('regular');
    expect(presentedNoteKind(next, notes)).toBe('feedback_needed');
  });
  it('keeps independently tracked requests open through unrelated and repeated replies in either order', () => {
    const oldest = { ...note('oldest', 'feedback_needed', '2026-09-02T00:00:00Z'), feedback_request: true };
    const newest = { ...note('newest', 'feedback_needed', '2026-09-02T00:01:00Z'), feedback_request: true };
    const unrelated = note('unrelated', 'regular', '2026-09-02T00:02:00Z');
    for (const [first, second] of [
      [oldest, newest],
      [newest, oldest],
    ]) {
      const pending = [oldest, newest, unrelated];
      expect(isFeedbackNeeded(pending)).toBe(true);
      expect(presentedNoteKind(oldest, pending)).toBe('feedback_needed');
      expect(presentedNoteKind(newest, pending)).toBe('feedback_needed');
      const reply = { ...note('reply', 'regular', '2026-09-02T00:03:00Z'), response_to: first.id };
      const partlyAnswered = [...pending, reply, { ...reply, id: 'repeat' }];
      expect(isFeedbackNeeded(partlyAnswered)).toBe(true);
      expect(presentedNoteKind(first, partlyAnswered)).toBe('regular');
      expect(presentedNoteKind(second, partlyAnswered)).toBe('feedback_needed');
      const allAnswered = [...partlyAnswered, { ...reply, id: 'other', response_to: second.id }];
      expect(isFeedbackNeeded(allAnswered)).toBe(false);
      expect(fullTicketFeedbackNeeded({ details: '', notes: allAnswered })).toBe(false);
    }
  });
  it('matches the core marker rule for feedback requests in descriptions', () => {
    expect(textRequestsFeedback('Context. FEEDBACK NEEDED: choose one')).toBe(true);
    expect(textRequestsFeedback('feedback needed from someone')).toBe(false);
  });
  it('ignores the marker inside quoted reply lines but honors an unquoted new request (HS2-HG7FZ0)', () => {
    expect(textRequestsFeedback('> Context. FEEDBACK NEEDED: choose one\n\nGo with A.')).toBe(false);
    expect(textRequestsFeedback('>> deeply quoted FEEDBACK NEEDED\n\nThanks.')).toBe(false);
    expect(textRequestsFeedback('> quoted FEEDBACK NEEDED\n\nActually FEEDBACK NEEDED: which one?')).toBe(true);
  });
  it('infers full-ticket state for older servers while trusting an explicit new-server value', () => {
    expect(fullTicketFeedbackNeeded({ details: 'FEEDBACK NEEDED: choose one', notes: [] })).toBe(true);
    expect(
      fullTicketFeedbackNeeded({
        details: 'FEEDBACK NEEDED: choose one',
        notes: [note('1', 'regular', '2026-09-02T00:00:00Z')],
      }),
    ).toBe(false);
    expect(
      fullTicketFeedbackNeeded({ feedback_needed: false, details: 'FEEDBACK NEEDED: choose one', notes: [] }),
    ).toBe(false);
  });
});

describe('isStaleFeedbackDismissal (HS2-VBFSZJ)', () => {
  const request = { id: 'R1', kind: 'feedback_needed' as const, created_at: '2026-10-27T00:00:00Z' };
  const dismissal = { note: NO_RESPONSE_NEEDED_NOTE, note_kind: 'regular', response_to: 'R1' };

  it('keeps an explicit dismissal of a still-open request', () => {
    expect(isStaleFeedbackDismissal('R1', dismissal, [request])).toBe(false);
  });

  it('drops only an explicit dismissal whose exact request became regular, was removed, or was answered', () => {
    expect(isStaleFeedbackDismissal('R1', dismissal, [{ ...request, kind: 'regular' }])).toBe(true);
    expect(isStaleFeedbackDismissal('R1', dismissal, [])).toBe(true);
    const answer = { id: 'A1', kind: 'regular' as const, created_at: '2026-10-27T00:01:00Z', response_to: 'R1' };
    expect(isStaleFeedbackDismissal('R1', dismissal, [request, answer])).toBe(true);
    expect(isStaleFeedbackDismissal('R2', dismissal, [])).toBe(false);
  });

  it('never infers dismissal from typed text, including the exact button body', () => {
    expect(isStaleFeedbackDismissal(undefined, dismissal, [])).toBe(false);
    expect(isStaleFeedbackDismissal(undefined, { ...dismissal, note: 'Use option B' }, [])).toBe(false);
    expect(isStaleFeedbackDismissal('R1', { note: NO_RESPONSE_NEEDED_NOTE }, [])).toBe(false);
    expect(isStaleFeedbackDismissal('R1', { note: NO_RESPONSE_NEEDED_NOTE, response_to: 7 }, [])).toBe(false);
  });
  it('does not mistake description feedback for a removed note target (HS2-M6ETWA)', () => {
    expect(isStaleFeedbackDismissal(DETAILS_FEEDBACK_ID, { ...dismissal, response_to: DETAILS_FEEDBACK_ID }, [])).toBe(
      false,
    );
  });
});
