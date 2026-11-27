import { describe, expect, it } from 'vitest';

import { type FailedSaveRetryContext, settleFailedSave, shouldRetryFailedSave } from './failed-save-retry';

const destination = 'p1\u0000HS2-A\u0000details',
  failed = { destination };
const retryable: FailedSaveRetryContext = {
  failed,
  destination,
  pending: false,
  conflictOwns: false,
  text: 'Intro\n- typed',
  committed: 'Intro',
};

describe('retrying a failed ticket-editor save on the next focus loss (HS2-SJ5TCW, HS2-Y0VRXR)', () => {
  it('retries the same destination whose save failed while its text still differs from the ticket', () => {
    expect(shouldRetryFailedSave(retryable)).toBe(true);
  });

  it('never retries without a failure or into another ticket, field, or note', () => {
    expect(shouldRetryFailedSave({ ...retryable, failed: undefined })).toBe(false);
    expect(shouldRetryFailedSave({ ...retryable, destination: 'p1\u0000HS2-B\u0000details' })).toBe(false);
    expect(shouldRetryFailedSave({ ...retryable, destination: 'p1\u0000HS2-A\u0000title' })).toBe(false);
    expect(shouldRetryFailedSave({ ...retryable, destination: undefined })).toBe(false);
    const note = { destination: 'p1\u0000HS2-A\u0000note\u0000n1' };
    expect(shouldRetryFailedSave({ ...retryable, failed: note, destination: 'p1\u0000HS2-A\u0000note\u0000n2' })).toBe(
      false,
    );
    expect(shouldRetryFailedSave({ ...retryable, failed: note, destination: note.destination })).toBe(true);
  });

  it('leaves newer typing and a conflict-owned edit to their own paths', () => {
    expect(shouldRetryFailedSave({ ...retryable, pending: true })).toBe(false);
    expect(shouldRetryFailedSave({ ...retryable, conflictOwns: true })).toBe(false);
  });

  it('has nothing to send once the text matches the ticket (Use Theirs, a successful Apply)', () => {
    expect(shouldRetryFailedSave({ ...retryable, text: 'Intro' })).toBe(false);
    expect(shouldRetryFailedSave({ ...retryable, committed: undefined })).toBe(false);
    // An emptied field still differs from a ticket that holds text.
    expect(shouldRetryFailedSave({ ...retryable, text: '' })).toBe(true);
  });

  it('remembers a plain failure, forgets it on success, and hands a conflict to its owner', () => {
    const other = 'p1\u0000HS2-A\u0000blocked_reason';
    expect(settleFailedSave(undefined, destination, false, false)).toEqual(failed);
    expect(settleFailedSave(failed, other, false, false)).toEqual({ destination: other });
    expect(settleFailedSave(failed, destination, true, false)).toBeUndefined();
    expect(settleFailedSave(failed, destination, false, true)).toBeUndefined();
    // A save that had no destination keeps whatever failure was already waiting.
    expect(settleFailedSave(failed, undefined, false, false)).toBe(failed);
    expect(settleFailedSave(failed, undefined, true, false)).toBeUndefined();
  });

  it('retries again after a repeated failure and stops after a success (fail, fail, succeed)', () => {
    let pending = settleFailedSave(undefined, destination, false, false);
    expect(shouldRetryFailedSave({ ...retryable, failed: pending })).toBe(true);
    pending = settleFailedSave(pending, destination, false, false);
    expect(shouldRetryFailedSave({ ...retryable, failed: pending })).toBe(true);
    pending = settleFailedSave(pending, destination, true, false);
    expect(shouldRetryFailedSave({ ...retryable, failed: pending })).toBe(false);
  });
});
