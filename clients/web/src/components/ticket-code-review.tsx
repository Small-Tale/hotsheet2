import './ticket-code-review.css';

import { Chip } from '@kerfjs/ui/chip';
import { foregroundColorVar, uiColor } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import {
  CircleHelp,
  ExternalLink,
  FileCode2,
  FileText,
  FlaskConical,
  FolderGit2,
  GitBranch,
  GitCommitHorizontal,
  GitCompare,
  GitCompareArrows,
  Tag,
} from 'lucide';

import type { CodeReview, CodeReviewTarget, CommitRef, UnmergedSource } from '../api';
import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';
import { MarkdownPreview } from './markdown-preview';
import { TicketInspectorPanel, type TicketInspectorPanelPresentation } from './ticket-inspector-panel';

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
  /** Rendered inside another surface (the repository popover) rather than as an inspector tab panel. */
  embedded?: boolean;
  /** The inspector tab panel presentation when not `embedded`. */
  presentation?: TicketInspectorPanelPresentation;
  comparison?: CodeReviewComparison;
  expandedCommits?: readonly string[];
  /** False on a remote client: diff-tool actions render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}

export function TicketCodeReview({
  review,
  loading = false,
  message = '',
  title = 'Code Review',
  emptyMessage = 'No commits referencing this ticket were found.',
  loadingMessage = 'Finding ticket commits…',
  action = 'open-code-review',
  embedded = false,
  presentation = 'sidebar',
  comparison,
  expandedCommits = [],
  hostActionsAvailable = hostActionsAvailableHere(),
}: TicketCodeReviewProps) {
  const enabled = Boolean(review?.difftool) && hostActionsAvailable,
    hostReason = hostOnlyDisabledReason('diff-tool', hostActionsAvailable);
  const compareReady = Boolean(comparison?.a && comparison.b && comparison.a !== comparison.b);
  const heading = <ToolbarText text={title} size="xlarge" headingLevel={2} />;
  const body = (
    <List gap={embedded ? 'none' : 'xs'} className="ticket-code-review__section">
      <div class="ticket-code-review__header">
        <Toolbar dividerSides="" leading={heading} />
      </div>
      {loading && (
        <Text variant="p" tone="quiet" size="compact" role="status">
          {loadingMessage}
        </Text>
      )}
      {!loading && review && review.commits.length === 0 && (
        <EmptyState
          title={emptyMessage}
          icon={<LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" />}
        />
      )}
      {!loading && review && review.commits.length > 0 && (
        <>
          {review.summary && (
            <button
              type="button"
              class="ticket-code-review__evidence"
              {...REPOSITORY_ACTIONS.openChangeEvidence.attrs}
              aria-label="Open Change Evidence"
            >
              <div class="ticket-code-review__evidence-heading">
                <Text variant="h3" size="compact" flush>
                  Change Evidence
                </Text>
              </div>
              <div class="ticket-code-review__evidence-grid">
                <span>
                  <LucideIcon size="s" icon={FileText} name="file-text" />
                  <strong>{review.summary.files.docs}</strong> docs
                </span>
                <span>
                  <LucideIcon size="s" icon={FlaskConical} name="flask-conical" />
                  <strong>{review.summary.files.tests}</strong> tests
                </span>
                <span>
                  <LucideIcon size="s" icon={FileCode2} name="file-code-2" />
                  <strong>{review.summary.files.source}</strong> source
                </span>
                {review.summary.files.other > 0 && (
                  <span>
                    <LucideIcon size="s" icon={CircleHelp} name="circle-help" />
                    <strong>{review.summary.files.other}</strong> other
                  </span>
                )}
              </div>
              <p data-tests-modified={review.summary.tests_modified > 0 ? 'true' : 'false'}>
                {review.summary.tests_added} new test file{review.summary.tests_added === 1 ? '' : 's'} ·{' '}
                {review.summary.tests_modified} existing test file{review.summary.tests_modified === 1 ? '' : 's'}{' '}
                modified
              </p>
            </button>
          )}
          {!review.difftool && <DiffToolMissingNotice />}
          {comparison?.active && (
            <div class="ticket-code-review__compare-banner" role="status">
              <div class="ticket-code-review__compare-prompt">
                <Row gap="2xs" vAlign="middle">
                  <span class="ticket-code-review__compare-icon">
                    <LucideIcon
                      icon={GitCompare}
                      name="git-compare"
                      size={16}
                      color={foregroundColorVar('--hs-ticket-state-needs-review')}
                    />
                  </span>
                  <Text variant="span" size="compact">
                    Select the <strong>{comparison.side.toUpperCase()}</strong> side of the comparison.
                  </Text>
                </Row>
              </div>
              <div class="ticket-code-review__compare-toolbar">
                <Toolbar
                  leading={
                    <ToolbarControlGroup label="Comparison Side" size="compact" selectedChrome="filled">
                      <button
                        type="button"
                        {...REPOSITORY_ACTIONS.setRepositoryComparisonSide.attrs}
                        data-comparison-side="a"
                        data-selected={String(comparison.side === 'a')}
                        aria-pressed={String(comparison.side === 'a')}
                      >
                        A
                      </button>
                      <button
                        type="button"
                        {...REPOSITORY_ACTIONS.setRepositoryComparisonSide.attrs}
                        data-comparison-side="b"
                        data-selected={String(comparison.side === 'b')}
                        aria-pressed={String(comparison.side === 'b')}
                      >
                        B
                      </button>
                    </ToolbarControlGroup>
                  }
                />
              </div>
              <button
                type="button"
                class="ticket-code-review__compare-open"
                data-action={action}
                data-review-mode="compare"
                data-review-from={comparison.a}
                data-review-to={comparison.b}
                disabled={!enabled || !compareReady}
                title={hostReason}
                aria-label={`Open comparison in ${review.difftool ?? 'configured diff tool'}`}
              >
                <LucideIcon size={14.4} icon={ExternalLink} name="external-link" />
                Open
              </button>
            </div>
          )}
          {unmergedNotice(review)}
          <ol class="ticket-code-review__commits">
            {review.commits.flatMap((commit) => {
              const expanded = expandedCommits.includes(commit.sha),
                body = commit.body?.trim() ?? '',
                labels = [comparison?.a === commit.sha ? 'A' : '', comparison?.b === commit.sha ? 'B' : ''].filter(
                  Boolean,
                ),
                ranges = review.ranges.filter((range) => range.count > 1 && range.to === commit.sha);
              return [
                ...ranges.map((range) => (
                  <li class="ticket-code-review__range-item">
                    <button
                      type="button"
                      class="ticket-code-review__range"
                      data-action={action}
                      data-review-mode="range"
                      data-review-from={range.from}
                      data-review-to={range.to}
                      disabled={!enabled}
                      title={hostReason}
                      aria-label={`Open ${range.count} commit bundle ${shortSha(range.from)} through ${shortSha(range.to)} in ${review.difftool ?? 'configured diff tool'}`}
                    >
                      <LucideIcon size={14.4} icon={GitCompareArrows} name="git-compare-arrows" />
                      <span>
                        Open {range.count}-commit bundle
                        <small>
                          {shortSha(range.from)} → {shortSha(range.to)}
                        </small>
                      </span>
                      <LucideIcon size={14.4} icon={ExternalLink} name="external-link" />
                    </button>
                  </li>
                )),
                <li
                  class="ticket-code-review__commit"
                  data-commit-sha={commit.sha}
                  data-expanded={String(expanded)}
                  data-compared={labels.length ? labels.join('').toLowerCase() : undefined}
                >
                  <span class="ticket-code-review__graph" aria-hidden="true">
                    <LucideIcon
                      size="s"
                      icon={GitCommitHorizontal}
                      name="git-commit-horizontal"
                      color={uiColor('neutral-on-quiet')}
                    />
                  </span>
                  <div
                    class="ticket-code-review__commit-summary"
                    data-action={
                      comparison?.active ? 'select-repository-comparison-commit' : 'toggle-code-review-commit'
                    }
                    data-commit-sha={commit.sha}
                    role="button"
                    tabIndex={0}
                    aria-expanded={body ? String(expanded) : undefined}
                  >
                    <Text variant="span" size="compact" lineHeight="tight" wrap="anywhere">
                      <strong>{commit.subject}</strong>
                    </Text>
                    {unmergedChip(commit.unmerged)}
                    {commitRefs(commit.refs)}
                    {body && (
                      <div class="ticket-code-review__commit-body">
                        <MarkdownPreview
                          source={expanded ? body : commitBodyPreview(body)}
                          tone="inherit"
                          size="small"
                          density="compact"
                        />
                      </div>
                    )}
                    <div class="ticket-code-review__commit-metadata">
                      <Row wrap gap="xs" vAlign="middle">
                        <code class="ticket-code-review__commit-sha">{commit.short_sha}</code>
                        <time dateTime={commit.committed_at}>{formatCommitDate(commit.committed_at)}</time>
                        {labels.map((label) => (
                          <b class="ticket-code-review__compare-label">{label}</b>
                        ))}
                      </Row>
                    </div>
                  </div>
                  <button
                    type="button"
                    data-action={action}
                    data-review-mode="commit"
                    data-review-commit={commit.sha}
                    disabled={!enabled}
                    title={hostReason}
                    aria-label={`Open Commit ${commit.short_sha} in ${review.difftool ?? 'configured diff tool'}`}
                  >
                    <LucideIcon size={14.4} icon={ExternalLink} name="external-link" />
                  </button>
                </li>,
              ];
            })}
          </ol>
          {review.truncated && (
            <div class="ticket-code-review__notice">
              <Text variant="p" size="compact" tone="quiet" flush>
                Showing matches from the newest 2,000 commits.
              </Text>
            </div>
          )}
        </>
      )}
      {!loading && review?.difftool ? (
        // The diff tool is a footnote below the commits rather than a heading label (HS2-SHV8J7).
        <footer class="ticket-code-review__footnotes">
          <Text variant="p" size="compact" tone="quiet" flush>
            Code reviews open in {review.difftool}
          </Text>
        </footer>
      ) : undefined}
      {message ? (
        <Text variant="p" tone="quiet" size="compact" role="status" className="ticket-code-review__message">
          {message}
        </Text>
      ) : undefined}
    </List>
  );
  return embedded ? (
    <div
      class="ticket-code-review ticket-code-review--embedded"
      data-component="ticket-code-review"
      data-embedded="true"
    >
      {body}
    </div>
  ) : (
    <TicketInspectorPanel
      component="ticket-code-review"
      className="ticket-code-review ticket-code-review--panel"
      presentation={presentation}
    >
      {body}
    </TicketInspectorPanel>
  );
}

/**
 * The notice shown where review actions need a configured Git diff tool. Code review owns it, and
 * the change-evidence dialog composes it rather than rendering this component's class (HS2-WP69TD).
 */
export function DiffToolMissingNotice() {
  return (
    <div class="ticket-code-review__notice" role="status">
      <Text variant="p" size="compact" tone="quiet" flush>
        No Git diff tool is configured for this checkout. Set <code>diff.tool</code> to enable review actions.
      </Text>
    </div>
  );
}

export function codeReviewTarget(data: DOMStringMap): CodeReviewTarget | undefined {
  if (data.reviewMode === 'commit' && data.reviewCommit) return { mode: 'commit', commit: data.reviewCommit };
  if (data.reviewMode === 'range' && data.reviewFrom && data.reviewTo)
    return { mode: 'range', from: data.reviewFrom, to: data.reviewTo };
  if (data.reviewMode === 'compare' && data.reviewFrom && data.reviewTo && data.reviewFrom !== data.reviewTo)
    return { mode: 'compare', from: data.reviewFrom, to: data.reviewTo };
  if (data.reviewMode === 'ticket_file' && data.reviewPath) return { mode: 'ticket_file', paths: [data.reviewPath] };
  if (
    data.reviewMode === 'worktree_file' &&
    data.reviewPath &&
    (data.reviewArea === 'staged' || data.reviewArea === 'unstaged')
  )
    return { mode: 'worktree_file', paths: [data.reviewPath], area: data.reviewArea };
  return undefined;
}

function commitBodyPreview(body: string): string {
  return body
    .split(/\r?\n/)
    .filter((line) => line.trim())
    .slice(0, 2)
    .join('\n');
}

function formatCommitDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' }).format(date);
}

function shortSha(value: string): string {
  return value.slice(0, 7);
}

function commitRefKindLabel(kind: CommitRef['kind']): string {
  switch (kind) {
    case 'tag':
      return 'Tag';
    case 'remote':
      return 'Remote branch';
    case 'head':
      return 'Current HEAD';
    case 'branch':
      return 'Branch';
    default:
      return kind satisfies never;
  }
}

/** Each ref kind's Kerf Chip tone (HS2-GQXT52). */
const REF_TONES = {
  head: 'success',
  branch: 'info',
  remote: 'neutral',
  tag: 'warning',
} as const satisfies Record<CommitRef['kind'], string>;

/** How many ticket commits are not on this checkout's branch yet, and where (HS2-0WZRK5). */
function unmergedNotice(review: CodeReview) {
  const unmerged = review.commits.filter((commit) => commit.unmerged);
  if (!unmerged.length) return undefined;
  const places = [...new Set(unmerged.map((commit) => unmergedLabel(commit.unmerged!)))];
  const count = unmerged.length === 1 ? '1 commit is' : `${unmerged.length} commits are`;
  return (
    <div class="ticket-code-review__notice" data-unmerged-notice="true" role="status">
      <Text variant="p" size="compact" tone="quiet" flush>
        {count} not merged into this checkout yet ({places.join(', ')}).
      </Text>
    </div>
  );
}

/** A short place name for unmerged work: the worktree folder, else the branch. */
function unmergedLabel(source: UnmergedSource): string {
  const folder = source.worktree?.split(/[\\/]/).filter(Boolean).pop();
  if (folder && source.branch && folder !== source.branch) return `${source.branch} in ${folder}`;
  return folder ?? source.branch ?? 'another worktree';
}

function unmergedTitle(source: UnmergedSource): string {
  const parts = ['Not merged into this checkout yet'];
  if (source.branch) parts.push(`branch ${source.branch}`);
  if (source.worktree) parts.push(`worktree ${source.worktree}`);
  return parts.join(' · ');
}

/** The Unmerged chip for a commit that is only on another branch or worktree (HS2-0WZRK5). */
function unmergedChip(unmerged: UnmergedSource | undefined) {
  if (!unmerged) return undefined;
  return (
    <span class="ticket-code-review__unmerged" title={unmergedTitle(unmerged)}>
      <Chip tone="pop" size="compact" truncate icon={<LucideIcon icon={FolderGit2} name="folder-git-2" />}>
        {`Unmerged · ${unmergedLabel(unmerged)}`}
      </Chip>
    </span>
  );
}

/** Git ref decorations (HEAD/branches/remotes/tags) for a commit, toned by kind (HS2-SFJ5TE). */
function commitRefs(refs: readonly CommitRef[] | undefined) {
  if (!refs?.length) return undefined;
  return (
    <span class="ticket-code-review__refs">
      {refs.map((ref) => (
        <span
          class="ticket-code-review__ref"
          data-ref-kind={ref.kind}
          title={`${commitRefKindLabel(ref.kind)}: ${ref.label}`}
        >
          <Chip
            tone={REF_TONES[ref.kind]}
            size="compact"
            truncate
            icon={
              <LucideIcon
                icon={ref.kind === 'tag' ? Tag : GitBranch}
                name={ref.kind === 'tag' ? 'tag' : 'git-branch'}
              />
            }
          >
            {ref.label}
          </Chip>
        </span>
      ))}
    </span>
  );
}
