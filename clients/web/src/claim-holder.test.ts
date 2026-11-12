import { describe, expect, it } from 'vitest';

import { claimHolderName, REMOTE_CLAIM_HOLDER } from './claim-holder';

describe('claim holder name (HS2-DQJPW0)', () => {
  it('names a remote server instead of its local worker and keeps local and unknown claims', () => {
    expect(claimHolderName({ claimed_by: 'codex-1', worker_label: 'Worktree 2', claim_origin: 'remote' })).toBe(
      REMOTE_CLAIM_HOLDER,
    );
    expect(claimHolderName({ claimed_by: 'codex-1', worker_label: 'Worktree 2', claim_origin: 'local' })).toBe(
      'Worktree 2',
    );
    expect(claimHolderName({ claimed_by: 'codex-1' })).toBe('codex-1');
    expect(claimHolderName({})).toBe('AI');
  });
});
