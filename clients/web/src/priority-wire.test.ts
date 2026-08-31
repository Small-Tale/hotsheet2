import { describe,expect,it } from 'vitest';

import { prioritiesToWire, priorityFromWire, priorityToWire } from './priority-wire';

describe('priority wire adapter', () => {
  it('maps the client urgent label to the canonical highest wire value', () => {
    expect(priorityToWire('urgent')).toBe('highest');
    expect(priorityFromWire('highest')).toBe('urgent');
  });

  it('accepts legacy client-shaped responses and defaults unknown values safely', () => {
    expect(priorityFromWire('urgent')).toBe('urgent');
    expect(priorityFromWire('high')).toBe('high');
    expect(priorityFromWire('low')).toBe('low');
    expect(priorityFromWire('lowest')).toBe('default');
    expect(priorityFromWire(undefined)).toBe('default');
  });

  it('translates request patches without mutating their source', () => {
    const patch = { priority: 'urgent', status: 'started' };
    expect(prioritiesToWire(patch)).toEqual({ priority: 'highest', status: 'started' });
    expect(patch.priority).toBe('urgent');
    expect(prioritiesToWire({ status: 'started' })).toEqual({ status: 'started' });
  });
});
