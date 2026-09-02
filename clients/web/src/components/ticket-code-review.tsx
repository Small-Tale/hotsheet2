import './ticket-code-review.css';

import { ExternalLink, GitCommitHorizontal, GitCompareArrows } from 'lucide';

import type { CodeReview, CodeReviewTarget } from '../api';
import { LucideIcon } from './lucide-icon';

export interface TicketCodeReviewProps {
  review?: CodeReview;
  loading?: boolean;
  message?: string;
}

export function TicketCodeReview({ review, loading = false, message = '' }: TicketCodeReviewProps) {
  const enabled = Boolean(review?.difftool);
  return <div class="ticket-inspector__content ticket-code-review" data-component="ticket-code-review">
    <section>
      <header class="ticket-code-review__header"><div><h2>Code Review</h2>{review?.difftool && <span>Opens in {review.difftool}</span>}</div></header>
      {loading && <p role="status">Finding ticket commits…</p>}
      {!loading && review && review.commits.length === 0 && <div class="ticket-code-review__empty"><LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" /><p>No commits with this ticket in the subject were found.</p></div>}
      {!loading && review && review.commits.length > 0 && <>
        {!enabled && <p class="ticket-code-review__notice" role="status">No Git diff tool is configured for this checkout. Set <code>diff.tool</code> to enable review actions.</p>}
        {review.ranges.filter(range => range.count > 1).map(range => <button type="button" class="ticket-code-review__range" data-action="open-code-review" data-review-mode="range" data-review-from={range.from} data-review-to={range.to} disabled={!enabled} aria-label={`Open ${range.count} commit range in ${review.difftool ?? 'configured diff tool'}`}><LucideIcon icon={GitCompareArrows} name="git-compare-arrows" /><span>Open {range.count}-commit range</span><LucideIcon icon={ExternalLink} name="external-link" /></button>)}
        <ol class="ticket-code-review__commits">{review.commits.map(commit => <li data-commit-sha={commit.sha}>
          <span class="ticket-code-review__graph" aria-hidden="true"><LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" /></span>
          <div><strong>{commit.subject}</strong><span><code>{commit.short_sha}</code><time dateTime={commit.committed_at}>{formatCommitDate(commit.committed_at)}</time></span></div>
          <button type="button" data-action="open-code-review" data-review-mode="commit" data-review-commit={commit.sha} disabled={!enabled} aria-label={`Open commit ${commit.short_sha} in ${review.difftool ?? 'configured diff tool'}`}><LucideIcon icon={ExternalLink} name="external-link" /></button>
        </li>)}</ol>
        {review.truncated && <p class="ticket-code-review__notice">Showing matches from the newest 2,000 commits.</p>}
      </>}
      {message && <p class="ticket-code-review__message" role="status">{message}</p>}
    </section>
  </div>;
}

export function codeReviewTarget(data: DOMStringMap): CodeReviewTarget | undefined {
  if (data.reviewMode === 'commit' && data.reviewCommit) return { mode: 'commit', commit: data.reviewCommit };
  if (data.reviewMode === 'range' && data.reviewFrom && data.reviewTo) return { mode: 'range', from: data.reviewFrom, to: data.reviewTo };
  return undefined;
}

function formatCommitDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' }).format(date);
}
