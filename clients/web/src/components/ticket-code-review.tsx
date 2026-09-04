import './ticket-code-review.css';

import { ExternalLink, GitCommitHorizontal, GitCompare, GitCompareArrows, X } from 'lucide';

import type { CodeReview, CodeReviewTarget } from '../api';
import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';
import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';

export interface CodeReviewComparison {
  active: boolean;
  side: 'a' | 'b';
  a?: string;
  b?: string;
}

export interface TicketCodeReviewProps {
  review?: CodeReview;
  loading?: boolean;
  message?: string;
  title?: string;
  emptyMessage?: string;
  loadingMessage?: string;
  action?: string;
  embedded?: boolean;
  comparison?: CodeReviewComparison;
  expandedCommits?: readonly string[];
}

export function TicketCodeReview({ review, loading = false, message = '', title = 'Code Review', emptyMessage = 'No commits with this ticket in the subject were found.', loadingMessage = 'Finding ticket commits…', action = 'open-code-review', embedded = false, comparison, expandedCommits = [] }: TicketCodeReviewProps) {
  const enabled = Boolean(review?.difftool);
  const compareReady = Boolean(comparison?.a && comparison.b && comparison.a !== comparison.b);
  const heading = <div class="ticket-code-review__heading"><h2>{title}</h2>{review?.difftool && <span>Opens in {review.difftool}</span>}</div>;
  const compareButton = comparison && <ToolbarControlGroup appearance="borderless" single><button type="button" data-action="start-repository-comparison" aria-label="Compare two commits" title="Compare two commits" aria-pressed={comparison.active}><LucideIcon icon={GitCompare} name="git-compare" /></button></ToolbarControlGroup>;
  return <div class={`${embedded ? '' : 'ticket-inspector__content '}ticket-code-review`} data-component="ticket-code-review">
    <section>
      <Toolbar className="ticket-code-review__header" divider={false} leading={heading} trailing={compareButton}/>
      {loading && <p role="status">{loadingMessage}</p>}
      {!loading && review && review.commits.length === 0 && <div class="ticket-code-review__empty"><LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" /><p>{emptyMessage}</p></div>}
      {!loading && review && review.commits.length > 0 && <>
        {!enabled && <p class="ticket-code-review__notice" role="status">No Git diff tool is configured for this checkout. Set <code>diff.tool</code> to enable review actions.</p>}
        {comparison?.active && <div class="ticket-code-review__compare-banner" role="status">
          <div><LucideIcon icon={GitCompare} name="git-compare"/><span>Select the <strong>{comparison.side.toUpperCase()}</strong> side of the comparison.</span></div>
          <ToolbarControlGroup label="Comparison side"><button type="button" data-action="set-repository-comparison-side" data-comparison-side="a" data-selected={String(comparison.side==='a')} aria-pressed={comparison.side==='a'}>A</button><button type="button" data-action="set-repository-comparison-side" data-comparison-side="b" data-selected={String(comparison.side==='b')} aria-pressed={comparison.side==='b'}>B</button></ToolbarControlGroup>
          <button type="button" class="ticket-code-review__compare-cancel" data-action="cancel-repository-comparison"><LucideIcon icon={X} name="x"/>Cancel</button>
          <button type="button" class="ticket-code-review__compare-open" data-action={action} data-review-mode="compare" data-review-from={comparison.a} data-review-to={comparison.b} disabled={!enabled||!compareReady} aria-label={`Open comparison in ${review.difftool??'configured diff tool'}`}><LucideIcon icon={ExternalLink} name="external-link"/>Open</button>
        </div>}
        {review.ranges.filter(range => range.count > 1).map(range => <button type="button" class="ticket-code-review__range" data-action={action} data-review-mode="range" data-review-from={range.from} data-review-to={range.to} disabled={!enabled} aria-label={`Open ${range.count} commit bundle ${shortSha(range.from)} through ${shortSha(range.to)} in ${review.difftool ?? 'configured diff tool'}`}><LucideIcon icon={GitCompareArrows} name="git-compare-arrows" /><span>Open {range.count}-commit bundle<small>{shortSha(range.from)} → {shortSha(range.to)}</small></span><LucideIcon icon={ExternalLink} name="external-link" /></button>)}
        <ol class="ticket-code-review__commits">{review.commits.map(commit => {const expanded=expandedCommits.includes(commit.sha),body=commit.body?.trim()??'',labels=[comparison?.a===commit.sha?'A':'',comparison?.b===commit.sha?'B':''].filter(Boolean);return <li data-commit-sha={commit.sha} data-expanded={String(expanded)} data-compared={labels.length?labels.join('').toLowerCase():undefined}>
          <span class="ticket-code-review__graph" aria-hidden="true"><LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" /></span>
          <div class="ticket-code-review__commit-summary" data-action={comparison?.active?'select-repository-comparison-commit':'toggle-code-review-commit'} data-commit-sha={commit.sha} role="button" tabIndex={0} aria-expanded={body?String(expanded):undefined}><strong>{commit.subject}</strong>{body&&<div class="ticket-code-review__commit-body"><MarkdownPreview source={expanded?body:commitBodyPreview(body)}/></div>}<span><code>{commit.short_sha}</code><time dateTime={commit.committed_at}>{formatCommitDate(commit.committed_at)}</time>{labels.map(label=><b class="ticket-code-review__compare-label">{label}</b>)}</span></div>
          <button type="button" data-action={action} data-review-mode="commit" data-review-commit={commit.sha} disabled={!enabled} aria-label={`Open commit ${commit.short_sha} in ${review.difftool ?? 'configured diff tool'}`}><LucideIcon icon={ExternalLink} name="external-link" /></button>
        </li>})}</ol>
        {review.truncated && <p class="ticket-code-review__notice">Showing matches from the newest 2,000 commits.</p>}
      </>}
      {message && <p class="ticket-code-review__message" role="status">{message}</p>}
    </section>
  </div>;
}

export function codeReviewTarget(data: DOMStringMap): CodeReviewTarget | undefined {
  if (data.reviewMode === 'commit' && data.reviewCommit) return { mode: 'commit', commit: data.reviewCommit };
  if (data.reviewMode === 'range' && data.reviewFrom && data.reviewTo) return { mode: 'range', from: data.reviewFrom, to: data.reviewTo };
  if (data.reviewMode === 'compare' && data.reviewFrom && data.reviewTo && data.reviewFrom !== data.reviewTo) return { mode: 'compare', from: data.reviewFrom, to: data.reviewTo };
  return undefined;
}

function commitBodyPreview(body: string): string {
  return body.split(/\r?\n/).filter(line => line.trim()).slice(0, 2).join('\n');
}

function formatCommitDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' }).format(date);
}

function shortSha(value: string): string {
  return value.slice(0, 7);
}
