import { describe, expect, it } from 'vitest';

import { ticketMilestone } from './announcer-milestone';

describe('ticketMilestone (HS2-G3HJZH)', () => {
  it('reads the server milestone fields', () => {
    expect(
      ticketMilestone({ detail: { milestone: 'question', spoken: ' I have a question (QXVHPB). ', speak: true } }),
    ).toEqual({ kind: 'question', spoken: 'I have a question (QXVHPB).', speak: true });
    expect(ticketMilestone({ detail: { milestone: 'started', spoken: 'I started.', speak: false } })?.speak).toBe(
      false,
    );
    expect(ticketMilestone({ detail: { milestone: 'started', spoken: 'I started.' } })?.speak).toBe(false);
  });

  it('ignores ordinary tool events and malformed details', () => {
    for (const detail of [
      null,
      undefined,
      'text',
      [],
      { path: 'src/a.ts' },
      { milestone: '', spoken: 'x' },
      { milestone: 'started', spoken: '  ' },
      { milestone: 'started', spoken: 3 },
      { milestone: 7, spoken: 'x' },
    ])
      expect(ticketMilestone({ detail })).toBeUndefined();
  });
});
