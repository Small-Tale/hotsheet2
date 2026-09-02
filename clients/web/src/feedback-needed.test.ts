import { describe, expect, it } from 'vitest';

import { isFeedbackNeeded } from './feedback-needed';

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
});
