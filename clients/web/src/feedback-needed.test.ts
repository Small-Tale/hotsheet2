import { describe, expect, it } from 'vitest';

import { fullTicketFeedbackNeeded,isFeedbackNeeded, presentedNoteKind,textRequestsFeedback } from './feedback-needed';

const note = (id: string, kind: 'regular' | 'activity' | 'feedback_needed', created_at: string) => ({ id, kind, created_at });

describe('isFeedbackNeeded', () => {
  it('uses the latest regular or feedback-needed note as the response state', () => {
    expect(isFeedbackNeeded([])).toBe(false);
    expect(isFeedbackNeeded([note('1', 'feedback_needed', '2026-09-02T00:00:00Z')])).toBe(true);
    expect(isFeedbackNeeded([
      note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
      note('2', 'activity', '2026-09-02T00:01:00Z'),
    ])).toBe(true);
    expect(isFeedbackNeeded([
      note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
      note('2', 'regular', '2026-09-02T00:02:00Z'),
    ])).toBe(false);
    expect(isFeedbackNeeded([
      note('1', 'feedback_needed', '2026-09-02T00:00:00Z'),
      note('2', 'regular', '2026-09-02T00:02:00Z'),
      note('3', 'feedback_needed', '2026-09-02T00:03:00Z'),
    ])).toBe(true);
  });

  it('presents answered asks as regular notes without changing later unanswered asks',()=>{const ask=note('1','feedback_needed','2026-09-02T00:00:00Z'),response=note('2','regular','2026-09-02T00:01:00Z'),next=note('3','feedback_needed','2026-09-02T00:02:00Z'),notes=[ask,response,next];expect(presentedNoteKind(ask,notes)).toBe('regular');expect(presentedNoteKind(response,notes)).toBe('regular');expect(presentedNoteKind(next,notes)).toBe('feedback_needed')});
  it('matches the core marker rule for feedback requests in descriptions',()=>{expect(textRequestsFeedback('Context. FEEDBACK NEEDED: choose one')).toBe(true);expect(textRequestsFeedback('feedback needed from someone')).toBe(false)});
  it('infers full-ticket state for older servers while trusting an explicit new-server value',()=>{
    expect(fullTicketFeedbackNeeded({details:'FEEDBACK NEEDED: choose one',notes:[]})).toBe(true);
    expect(fullTicketFeedbackNeeded({details:'FEEDBACK NEEDED: choose one',notes:[note('1','regular','2026-09-02T00:00:00Z')]})).toBe(false);
    expect(fullTicketFeedbackNeeded({feedback_needed:false,details:'FEEDBACK NEEDED: choose one',notes:[]})).toBe(false);
  });
});
