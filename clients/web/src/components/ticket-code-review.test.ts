import { describe, expect, it } from 'vitest';

import type { CodeReview } from '../api';
import { codeReviewTarget, TicketCodeReview } from './ticket-code-review';

const review: CodeReview = {
  difftool: 'glassbox',
  truncated: false,
  ranges: [{ from: 'aaa', to: 'bbb', count: 2 }],
  commits: [
    { sha: 'bbb', short_sha: 'bbbbbbb', subject: 'HS2-TEST: finish review UI', committed_at: '2026-09-02T08:00:00Z' },
    { sha: 'aaa', short_sha: 'aaaaaaa', subject: 'HS2-TEST: add server route', committed_at: '2026-09-01T08:00:00Z' },
  ],
};

describe('TicketCodeReview', () => {
  it('lists commit messages and exposes only server-provided commit and range targets', () => {
    const markup = String(TicketCodeReview({ review }));
    expect(markup).toContain('Opens in glassbox');
    expect(markup).toContain('HS2-TEST: finish review UI');
    expect(markup).toContain('data-review-mode="range" data-review-from="aaa" data-review-to="bbb"');
    expect(markup).toContain('data-review-mode="commit" data-review-commit="bbb"');
    expect(markup).toContain('data-lucide="external-link"');
  });

  it('keeps history readable but disables launching without a configured tool', () => {
    const markup = String(TicketCodeReview({ review: { ...review, difftool: undefined } }));
    expect(markup).toContain('No Git diff tool is configured');
    expect(markup).toContain('HS2-TEST: add server route');
    expect(markup.match(/ disabled/g)).toHaveLength(3);
  });

  it('renders loading and empty states', () => {
    expect(String(TicketCodeReview({ loading: true }))).toContain('Finding ticket commits');
    expect(String(TicketCodeReview({ review: { commits: [], ranges: [], difftool: 'meld', truncated: false } }))).toContain('No commits with this ticket');
  });

  it('decodes only complete action datasets', () => {
    expect(codeReviewTarget({ reviewMode: 'commit', reviewCommit: 'abc' })).toEqual({ mode: 'commit', commit: 'abc' });
    expect(codeReviewTarget({ reviewMode: 'range', reviewFrom: 'abc', reviewTo: 'def' })).toEqual({ mode: 'range', from: 'abc', to: 'def' });
    expect(codeReviewTarget({ reviewMode: 'range', reviewFrom: 'abc' })).toBeUndefined();
  });
});
