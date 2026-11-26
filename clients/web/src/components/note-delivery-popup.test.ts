import { describe, expect, it } from 'vitest';

import { NoteDeliveryPopup } from './note-delivery-popup';

const prompt = {
  key: 'note-delivery:NOTE1',
  ticketId: 'source:ABC',
  slug: 'HS2-ABC123',
  title: 'Synthetic ticket',
  projectName: 'demo',
  sessions: [
    {
      id: 'session-123456',
      label: 'Codex 2 · demo · HS2-ABC123',
      description: 'Codex in demo (session session-123456)',
    },
  ],
};

describe('app-level note delivery prompt (HS2-6RXBS9, HS2-25TP3N)', () => {
  it('offers each session by its label in a top-layer card that names the ticket and project', () => {
    const html = String(NoteDeliveryPopup({ prompt: { ...prompt, kind: 'choose' } }));
    expect(html).toContain('data-component="note-delivery-popup"');
    expect(html).toContain('popover="manual"');
    expect(html).toContain('data-top-layer-presentation-key="note-delivery:NOTE1"');
    expect(html).toContain('Choose an AI Session');
    expect(html).toContain('HS2-ABC123');
    expect(html).toContain('demo');
    expect(html).toContain('Synthetic ticket');
    expect(html).toContain('data-action="choose-feedback-session"');
    expect(html).toContain('data-connection-id="session-123456"');
    expect(html).toContain('data-ticket-id="source:ABC"');
    expect(html).toContain('Codex 2 · demo · HS2-ABC123');
    expect(html).toContain('title="Codex in demo (session session-123456)"');
    expect(html).toContain('data-action="dismiss-feedback-delivery"');
    expect(html).not.toContain('start-feedback-session');
  });

  it('offers to start a session when none is running', () => {
    const html = String(NoteDeliveryPopup({ prompt: { ...prompt, kind: 'start', sessions: [] } }));
    expect(html).toContain('Start an AI Session');
    expect(html).toContain('data-action="start-feedback-session"');
    expect(html).not.toContain('choose-feedback-session');
  });
});
