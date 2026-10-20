import { describe, expect, it } from 'vitest';

import { TicketReader } from './ticket-reader';

const ticket = {
  slug: 'HS2-1',
  title: 'Reader',
  status: 'started' as const,
  priority: 'default' as const,
  category: 'task',
  tags: [],
  details: 'Details',
};

describe('TicketReader controlled layers', () => {
  it('uses Kerf geometry and names the active native modal', () => {
    const markup = String(TicketReader({ ...ticket, frameId: 'front', stackPosition: 2, stackSize: 2 }));
    expect(markup).toContain('<dialog');
    expect(markup).toContain('class="kui-dialog-surface ticket-reader-dialog"');
    expect(markup).toContain('aria-label="Read and edit HS2-1"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain('data-stack-open="true"');
    expect(markup).toContain('data-stack-active="true"');
    expect(markup).toContain('data-stack-position="2"');
    expect(markup).toContain('data-stack-count="2"');
    expect(markup).toContain('data-height=""');
    expect(markup).toContain('data-body-layout="fill"');
    expect(markup).toContain('data-reader-frame-id="front"');
    expect(markup).toContain('data-component="ticket-reader"');
  });

  it('keeps covered and closed layers inaccessible without losing their identity', () => {
    const covered = String(TicketReader({ ...ticket, active: false, frameId: 'back', stackSize: 2 }));
    expect(covered).toContain('aria-modal="false"');
    expect(covered).toContain('aria-hidden="true"');
    expect(covered).toContain(' inert');
    expect(covered).toContain('-0.625rem');
    const closed = String(TicketReader({ ...ticket, open: false, frameId: 'back' }));
    expect(closed).toContain('data-stack-open="false"');
    expect(closed).toContain('data-stack-active="false"');
    expect(closed).not.toContain('data-component="ticket-reader"');
    expect(closed).toContain('data-reader-frame-id="back"');
  });
});
