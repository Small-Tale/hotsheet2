import { describe, expect, it } from 'vitest';

import {
  feedbackPrimaryMenuItem,
  feedbackPrimaryToast,
  RECEIVE_FEEDBACK_LABEL,
  STOP_FEEDBACK_LABEL,
} from './terminal-feedback-primary';

const ai = { kind: 'ai' as const, alive: true };

describe('feedback-primary menu command (HS2-H3S7YQ)', () => {
  it('offers an eligible AI session the choice', () => {
    expect(feedbackPrimaryMenuItem({ ...ai, feedback_eligible: true })).toEqual({
      command: 'choose',
      label: RECEIVE_FEEDBACK_LABEL,
      description: 'Instead of the oldest AI session.',
    });
  });

  it('offers the chosen session a way back to the derived rule', () => {
    const item = feedbackPrimaryMenuItem({
      ...ai,
      feedback_eligible: true,
      feedback_primary: true,
      feedback_primary_chosen: true,
    });
    expect(item.command).toBe('clear');
    expect(item.label).toBe(STOP_FEEDBACK_LABEL);
    expect(item.disabledReason).toBeUndefined();
  });

  it.each([
    [{ kind: 'shell' as const, alive: true, feedback_eligible: true }, 'A shell terminal'],
    [{ alive: true }, 'A shell terminal'],
    [{ ...ai, alive: false, feedback_eligible: true }, 'has ended'],
    [{ ...ai, feedback_eligible: true, feedback_primary: true }, 'already receives'],
    [{ ...ai }, 'cannot receive pushed messages'],
    [{ ...ai, feedback_eligible: false }, 'cannot receive pushed messages'],
  ])('disables the choice with a reason: %o', (session, reason) => {
    const item = feedbackPrimaryMenuItem(session);
    expect(item.command).toBe('choose');
    expect(item.label).toBe(RECEIVE_FEEDBACK_LABEL);
    expect(item.disabledReason).toContain(reason);
  });

  it('follows a session through choose, server promotion, and clear', () => {
    // Eligible, then chosen, then cleared while it is not the oldest, then derived primary.
    expect(feedbackPrimaryMenuItem({ ...ai, feedback_eligible: true }).command).toBe('choose');
    expect(
      feedbackPrimaryMenuItem({ ...ai, feedback_eligible: true, feedback_primary: true, feedback_primary_chosen: true })
        .command,
    ).toBe('clear');
    expect(feedbackPrimaryMenuItem({ ...ai, feedback_eligible: true }).disabledReason).toBeUndefined();
    expect(
      feedbackPrimaryMenuItem({ ...ai, feedback_eligible: true, feedback_primary: true }).disabledReason,
    ).toContain('already receives');
  });

  it('names the outcome in its toast', () => {
    expect(feedbackPrimaryToast('choose', 'Claude 2')).toBe('Claude 2 now receives feedback notifications.');
    expect(feedbackPrimaryToast('clear', 'Claude 2')).toBe('Feedback notifications go to the oldest AI session again.');
  });
});
