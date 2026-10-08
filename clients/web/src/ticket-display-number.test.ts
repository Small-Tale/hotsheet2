import { describe, expect, it } from 'vitest';

import { ticketDisplayNumber } from './ticket-display-number';

describe('ticketDisplayNumber', () => {
  it('shows only the number for a GitHub issue in its local project context', () => {
    expect(ticketDisplayNumber('Small-Tale/hotsheet2#5', 'github')).toBe('#5');
    expect(ticketDisplayNumber('acme/widgets#123', 'github')).toBe('#123');
  });

  it('retains non-GitHub and unrecognized identities', () => {
    expect(ticketDisplayNumber('Small-Tale/hotsheet2#5', 'gitlab')).toBe('Small-Tale/hotsheet2#5');
    expect(ticketDisplayNumber('HS2-TEST', 'git')).toBe('HS2-TEST');
    expect(ticketDisplayNumber('acme/widgets#0', 'github')).toBe('acme/widgets#0');
    expect(ticketDisplayNumber('unrecognized', 'github')).toBe('unrecognized');
  });
});
