import { describe, expect, it } from 'vitest';

import type { ChangeEvent } from './api';
import { LocalTicketChangeAcknowledgements } from './local-ticket-changes';

const event = (id: string, kind = 'updated'): ChangeEvent => ({ store: 'local', id, slug: `HS2-${id}`, kind });

describe('local ticket change acknowledgements', () => {
  it('consumes one exact stream event for each locally acknowledged write', () => {
    const acknowledgements = new LocalTicketChangeAcknowledgements();
    acknowledgements.acknowledge('project-one', event('one'));
    expect(acknowledgements.unacknowledged('project-one', [event('one'), event('one'), event('two')])).toEqual([event('one'), event('two')]);
  });

  it('does not hide a different event kind for the same ticket', () => {
    const acknowledgements = new LocalTicketChangeAcknowledgements();
    acknowledgements.acknowledge('project-one', event('one', 'created'));
    expect(acknowledgements.unacknowledged('project-one', [event('one', 'updated')])).toEqual([event('one', 'updated')]);
  });

  it('expires acknowledgements that were never observed', () => {
    const acknowledgements = new LocalTicketChangeAcknowledgements();
    acknowledgements.acknowledge('project-one', event('one'), 1_000);
    expect(acknowledgements.unacknowledged('project-one', [event('one')], 31_001)).toEqual([event('one')]);
  });

  it('does not consume the corresponding invalidation for another open project', () => {
    const acknowledgements = new LocalTicketChangeAcknowledgements();
    acknowledgements.acknowledge('project-one', event('one'));
    expect(acknowledgements.unacknowledged('project-two', [event('one')])).toEqual([event('one')]);
  });
});
