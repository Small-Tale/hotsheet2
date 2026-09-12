import './ticket-code-review.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { CircleHelp, ExternalLink, FileCode2, FileText, FlaskConical, GitCommitHorizontal, GitCompare, GitCompareArrows } from 'lucide';

import type { CodeReview, CodeReviewTarget } from '../api';
import { MarkdownPreview } from './markdown-preview';

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

export function TicketCodeReview({ review, loading = false, message = '', title = 'Code Review', emptyMessage = 'No commits referencing this ticket were found.', loadingMessage = 'Finding ticket commits…', action = 'open-code-review', embedded = false, comparison, expandedCommits = [] }: TicketCodeReviewProps) {
  const enabled = Boolean(review?.difftool);
  const compareReady = Boolean(comparison?.a && comparison.b && comparison.a !== comparison.b);
  const heading = <div class="ticket-code-review__heading"><h2>{title}</h2>{review?.difftool && <span>Opens in {review.difftool}</span>}</div>;
  return <div class={`${embedded ? '' : 'ticket-inspector__content '}ticket-code-review`} data-component="ticket-code-review">
    <section>
      <Toolbar className="ticket-code-review__header" divider={false} leading={heading}/>
      {loading && <p role="status">{loadingMessage}</p>}
      {!loading && review && review.commits.length === 0 && <div class="ticket-code-review__empty"><LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" /><p>{emptyMessage}</p></div>}
      {!loading && review && review.commits.length > 0 && <>
        {review.summary&&<button type="button" class="ticket-code-review__evidence" data-action="open-change-evidence" aria-label="Open change evidence"><h3>Change evidence</h3><div class="ticket-code-review__evidence-grid">
          <span><LucideIcon icon={FileText} name="file-text"/><strong>{review.summary.files.docs}</strong> docs</span>
          <span><LucideIcon icon={FlaskConical} name="flask-conical"/><strong>{review.summary.files.tests}</strong> tests</span>
          <span><LucideIcon icon={FileCode2} name="file-code-2"/><strong>{review.summary.files.source}</strong> source</span>
          {review.summary.files.other>0&&<span><LucideIcon icon={CircleHelp} name="circle-help"/><strong>{review.summary.files.other}</strong> other</span>}
        </div><p data-tests-modified={review.summary.tests_modified>0?'true':'false'}>{review.summary.tests_added} new test file{review.summary.tests_added===1?'':'s'} · {review.summary.tests_modified} existing test file{review.summary.tests_modified===1?'':'s'} modified</p></button>}
        {!enabled && <p class="ticket-code-review__notice" role="status">No Git diff tool is configured for this checkout. Set <code>diff.tool</code> to enable review actions.</p>}
        {comparison?.active && <div class="ticket-code-review__compare-banner" role="status">
          <div><LucideIcon icon={GitCompare} name="git-compare"/><span>Select the <strong>{comparison.side.toUpperCase()}</strong> side of the comparison.</span></div>
          <ToolbarControlGroup label="Comparison side"><button type="button" data-action="set-repository-comparison-side" data-comparison-side="a" data-selected={String(comparison.side==='a')} aria-pressed={comparison.side==='a'}>A</button><button type="button" data-action="set-repository-comparison-side" data-comparison-side="b" data-selected={String(comparison.side==='b')} aria-pressed={comparison.side==='b'}>B</button></ToolbarControlGroup>
          <button type="button" class="ticket-code-review__compare-open" data-action={action} data-review-mode="compare" data-review-from={comparison.a} data-review-to={comparison.b} disabled={!enabled||!compareReady} aria-label={`Open comparison in ${review.difftool??'configured diff tool'}`}><LucideIcon icon={ExternalLink} name="external-link"/>Open</button>
        </div>}
        <ol class="ticket-code-review__commits">{review.commits.flatMap(commit => {const expanded=expandedCommits.includes(commit.sha),body=commit.body?.trim()??'',labels=[comparison?.a===commit.sha?'A':'',comparison?.b===commit.sha?'B':''].filter(Boolean),ranges=review.ranges.filter(range=>range.count>1&&range.to===commit.sha);return [...ranges.map(range=><li class="ticket-code-review__range-item"><button type="button" class="ticket-code-review__range" data-action={action} data-review-mode="range" data-review-from={range.from} data-review-to={range.to} disabled={!enabled} aria-label={`Open ${range.count} commit bundle ${shortSha(range.from)} through ${shortSha(range.to)} in ${review.difftool ?? 'configured diff tool'}`}><LucideIcon icon={GitCompareArrows} name="git-compare-arrows" /><span>Open {range.count}-commit bundle<small>{shortSha(range.from)} → {shortSha(range.to)}</small></span><LucideIcon icon={ExternalLink} name="external-link" /></button></li>),<li class="ticket-code-review__commit" data-commit-sha={commit.sha} data-expanded={String(expanded)} data-compared={labels.length?labels.join('').toLowerCase():undefined}>
          <span class="ticket-code-review__graph" aria-hidden="true"><LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" /></span>
          <div class="ticket-code-review__commit-summary" data-action={comparison?.active?'select-repository-comparison-commit':'toggle-code-review-commit'} data-commit-sha={commit.sha} role="button" tabIndex={0} aria-expanded={body?String(expanded):undefined}><strong>{commit.subject}</strong>{body&&<div class="ticket-code-review__commit-body"><MarkdownPreview source={expanded?body:commitBodyPreview(body)}/></div>}<span><code>{commit.short_sha}</code><time dateTime={commit.committed_at}>{formatCommitDate(commit.committed_at)}</time>{labels.map(label=><b class="ticket-code-review__compare-label">{label}</b>)}</span></div>
          <button type="button" data-action={action} data-review-mode="commit" data-review-commit={commit.sha} disabled={!enabled} aria-label={`Open commit ${commit.short_sha} in ${review.difftool ?? 'configured diff tool'}`}><LucideIcon icon={ExternalLink} name="external-link" /></button>
        </li>]})}</ol>
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
  if (data.reviewMode === 'ticket_file' && data.reviewPath) return { mode: 'ticket_file', path: data.reviewPath };
  if (data.reviewMode === 'worktree_file' && data.reviewPath && (data.reviewArea === 'staged' || data.reviewArea === 'unstaged')) return { mode: 'worktree_file', path: data.reviewPath, area: data.reviewArea };
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
