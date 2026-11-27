import { describe, expect, it } from 'vitest';

import { type DetailsRetryContext, settleFailedDetailsSave, shouldRetryFailedDetailsSave } from './details-save-retry';

const failed = { ticketId: 't1', generation: 3 };
const retryable: DetailsRetryContext = {
  failed,
  ticketId: 't1',
  generation: 3,
  writing: true,
  pending: false,
  conflictOpen: false,
  draft: 'Intro\n- typed',
  current: 'Intro',
};

describe('retrying a failed Details save on the next focus loss (HS2-SJ5TCW)', () => {
  it('retries the same edit whose save failed while its text still differs from the ticket', () => {
    expect(shouldRetryFailedDetailsSave(retryable)).toBe(true);
  });

  it('never retries without a failure, for another ticket, or for a later edit', () => {
    expect(shouldRetryFailedDetailsSave({ ...retryable, failed: undefined })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, ticketId: 't2' })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, ticketId: undefined })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, failed: { ticketId: undefined, generation: 3 } })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, generation: 4 })).toBe(false);
  });

  it('leaves newer typing, a closed editor, and the conflict card to their own paths', () => {
    expect(shouldRetryFailedDetailsSave({ ...retryable, pending: true })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, writing: false })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, conflictOpen: true })).toBe(false);
  });

  it('has nothing to send once the text matches the ticket (Use Theirs, a successful Apply)', () => {
    expect(shouldRetryFailedDetailsSave({ ...retryable, draft: 'Intro' })).toBe(false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, current: undefined })).toBe(false);
  });

  it('remembers a plain failure, forgets it on success, and hands a conflict to the card', () => {
    const task = { ticketId: 't1', generation: 5 };
    expect(settleFailedDetailsSave(undefined, task, false, false)).toEqual(task);
    expect(settleFailedDetailsSave(failed, task, false, false)).toEqual(task);
    expect(settleFailedDetailsSave(failed, task, true, false)).toBeUndefined();
    expect(settleFailedDetailsSave(failed, task, false, true)).toBeUndefined();
    // A save that had no ticket keeps whatever failure was already waiting.
    expect(settleFailedDetailsSave(failed, { ticketId: undefined, generation: 5 }, false, false)).toBe(failed);
  });

  it('retries again after a repeated failure and stops after a success (fail, fail, succeed)', () => {
    let pending = settleFailedDetailsSave(undefined, failed, false, false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, failed: pending })).toBe(true);
    pending = settleFailedDetailsSave(pending, failed, false, false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, failed: pending })).toBe(true);
    pending = settleFailedDetailsSave(pending, failed, true, false);
    expect(shouldRetryFailedDetailsSave({ ...retryable, failed: pending })).toBe(false);
  });
});
