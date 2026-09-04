import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { CodeReview } from '../api';
import { codeReviewTarget, TicketCodeReview } from './ticket-code-review';

const review: CodeReview = {
  difftool: 'glassbox',
  truncated: false,
  ranges: [{ from: 'aaa1111', to: 'bbb2222', count: 2 }, { from: 'ccc3333', to: 'ddd4444', count: 2 }],
  commits: [
    { sha: 'ddd4444', short_sha: 'ddd4444', subject: 'HS2-TEST: finish later review run', body: 'First detail line\n\nSecond **Markdown** line\nThird hidden line', committed_at: '2026-09-02T10:00:00Z' },
    { sha: 'ccc3333', short_sha: 'ccc3333', subject: 'HS2-TEST: start later review run', committed_at: '2026-09-02T09:00:00Z' },
    { sha: 'bbb2222', short_sha: 'bbb2222', subject: 'HS2-TEST: finish review UI', committed_at: '2026-09-02T08:00:00Z' },
    { sha: 'aaa1111', short_sha: 'aaa1111', subject: 'HS2-TEST: add server route', committed_at: '2026-09-01T08:00:00Z' },
  ],
};

describe('TicketCodeReview', () => {
  it('resets the native list-item indent so commit rows align with their list', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).toMatch(/\.ticket-code-review__commits li \{[^}]*margin-inline-start: 0;/);
  });

  it('lists commit messages and exposes only server-provided commit and range targets', () => {
    const markup = String(TicketCodeReview({ review }));
    expect(markup).toContain('Opens in glassbox');
    expect(markup).toContain('HS2-TEST: finish review UI');
    expect(markup).toContain('data-review-mode="range" data-review-from="aaa1111" data-review-to="bbb2222"');
    expect(markup).toContain('data-review-mode="range" data-review-from="ccc3333" data-review-to="ddd4444"');
    expect(markup.match(/ticket-code-review__range/g)).toHaveLength(2);
    expect(markup).toContain('Open 2-commit bundle<small>aaa1111 → bbb2222</small>');
    expect(markup).toContain('Open 2-commit bundle<small>ccc3333 → ddd4444</small>');
    expect(markup).toContain('data-review-mode="commit" data-review-commit="bbb2222"');
    expect(markup).toContain('data-lucide="external-link"');
  });

  it('keeps history readable but disables launching without a configured tool', () => {
    const markup = String(TicketCodeReview({ review: { ...review, difftool: undefined } }));
    expect(markup).toContain('No Git diff tool is configured');
    expect(markup).toContain('HS2-TEST: add server route');
    expect(markup.match(/ disabled/g)).toHaveLength(6);
  });

  it('renders loading and empty states', () => {
    expect(String(TicketCodeReview({ loading: true }))).toContain('Finding ticket commits');
    expect(String(TicketCodeReview({ review: { commits: [], ranges: [], difftool: 'meld', truncated: false } }))).toContain('No commits with this ticket');
  });

  it('supports the repository dialog while preserving the shared commit presentation', () => {
    const markup = String(TicketCodeReview({ review, embedded: true, title: 'Commits', action: 'open-repository-review', comparison: {active:true,side:'b',a:'aaa1111',b:'ddd4444'}, expandedCommits:['ddd4444'] }));
    expect(markup).not.toContain('ticket-inspector__content');
    expect(markup).toContain('<h2>Commits</h2>');
    expect(markup).toContain('data-action="open-repository-review"');
    expect(markup).toContain('data-lucide="git-compare"');
    expect(markup).toContain('Select the <strong>B</strong> side');
    expect(markup).toContain('data-review-mode="compare" data-review-from="aaa1111" data-review-to="ddd4444"');
    expect(markup).toContain('class="ticket-code-review__compare-label">A</b>');
    expect(markup).toContain('class="ticket-code-review__compare-label">B</b>');
    expect(markup).toContain('Third hidden line');
    expect(markup).not.toContain('cancel-repository-comparison');
    expect(markup).not.toContain('start-repository-comparison');
  });

  it('limits collapsed commit detail to two Markdown lines',()=>{
    const markup=String(TicketCodeReview({review}));
    expect(markup).toContain('First detail line');
    expect(markup).toContain('Second <strong>Markdown</strong> line');
    expect(markup).not.toContain('Third hidden line');
  });

  it('decodes only complete action datasets', () => {
    expect(codeReviewTarget({ reviewMode: 'commit', reviewCommit: 'abc' })).toEqual({ mode: 'commit', commit: 'abc' });
    expect(codeReviewTarget({ reviewMode: 'range', reviewFrom: 'abc', reviewTo: 'def' })).toEqual({ mode: 'range', from: 'abc', to: 'def' });
    expect(codeReviewTarget({ reviewMode: 'compare', reviewFrom: 'abc', reviewTo: 'def' })).toEqual({ mode: 'compare', from: 'abc', to: 'def' });
    expect(codeReviewTarget({ reviewMode: 'compare', reviewFrom: 'abc', reviewTo: 'abc' })).toBeUndefined();
    expect(codeReviewTarget({ reviewMode: 'range', reviewFrom: 'abc' })).toBeUndefined();
  });
});
