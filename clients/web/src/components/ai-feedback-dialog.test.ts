import { describe, expect, it } from 'vitest';

import { AiFeedbackDialog } from './ai-feedback-dialog';

const state = {
  ticketId: 'ticket-1',
  target: 'activity:one',
  rating: 'not_helpful' as const,
  rater: 'person-1',
  explanation: 'First line\n\n**Markdown**',
  revising: true,
};

describe('AiFeedbackDialog', () => {
  it('renders an editable multiline Markdown draft and app controls', () => {
    const markup = String(AiFeedbackDialog({ state }));
    expect(markup).toContain('label="Not helpful feedback"');
    expect(markup).toContain('What should Hot Sheet change or stop doing?');
    expect(markup).toContain('name="ai-feedback-explanation"');
    expect(markup).toContain('First line\n\n**Markdown**');
    expect(markup).toContain('data-action="cancel-ai-feedback"');
    expect(markup).toContain('data-action="submit-ai-feedback"');
    expect(markup).toContain('Update feedback');
  });

  it('unmounts when closed and labels a new helpful rating', () => {
    expect(String(AiFeedbackDialog({}))).toBe('');
    const markup = String(AiFeedbackDialog({ state: { ...state, rating: 'helpful', revising: false } }));
    expect(markup).toContain('What should Hot Sheet keep doing?');
    expect(markup).toContain('Save feedback');
  });
});
