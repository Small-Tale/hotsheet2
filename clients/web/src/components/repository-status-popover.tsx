import '@kerfjs/ui/badge.css';
import '@kerfjs/ui/empty-state.css';
import '@kerfjs/ui/layout.css';
import '@kerfjs/ui/list-action-row.css';
import '@kerfjs/ui/nav-stack.css';
import '@kerfjs/ui/split-view.css';
import '@kerfjs/ui/state-banner.css';
import './heading.css';
import './repository-status-popover.css';
import './native-popover-dialog.css';

import { Badge } from '@kerfjs/ui/badge';
import { uiColor } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane } from '@kerfjs/ui/pane';
import { PopupMenu, type PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { SplitView } from '@kerfjs/ui/split-view';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import {
  ArrowDown,
  ArrowUp,
  CircleCheck,
  CircleHelp,
  Clipboard,
  Copy,
  Ellipsis,
  ExternalLink,
  FileCode2,
  FileText,
  FlaskConical,
  FolderOpen,
  GitBranch,
  GitCommitHorizontal,
  GitCompare,
  RefreshCw,
  SquareMinus,
  SquarePen,
  SquarePlus,
  SquareX,
  TriangleAlert,
} from 'lucide';

import type { CodeReview, RepositoryFile, RepositoryFileChange, RepositoryStatus } from '../api';
import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';
import { ContextMenuAnchor } from './context-menu-anchor';
import { RepositorySetup, type RepositorySetupStep } from './repository-setup';
import { type CodeReviewComparison, DiffToolMissingNotice, TicketCodeReview } from './ticket-code-review';

export type RepositoryStatusState =
  'clean' | 'dirty' | 'ahead' | 'behind' | 'diverged' | 'conflicted' | 'uninitialized' | 'error';
export type RepositoryStatusView = 'staged' | 'unstaged' | 'untracked' | 'conflicted' | 'commits';
export type ChangeEvidenceView = 'docs' | 'tests' | 'source' | 'other';
export interface RepositoryFileMenu {
  path: string;
  paths?: string[];
  absolutePath?: string;
  absolutePaths?: string[];
  x: number;
  y: number;
  diff?: 'ticket' | 'staged' | 'unstaged';
  /** The surface that opened the menu; only that surface renders it (one live context menu). */
  source?: 'repository' | 'ticket';
}

export function repositoryStatusState(
  status: RepositoryStatus | null,
  error = '',
  initialized = true,
): RepositoryStatusState {
  if (error) return 'error';
  if (!initialized) return 'uninitialized';
  if (!status) return 'error';
  if (status.conflicted > 0) return 'conflicted';
  if (status.ahead > 0 && status.behind > 0) return 'diverged';
  if (status.behind > 0) return 'behind';
  if (status.ahead > 0) return 'ahead';
  if (status.staged + status.unstaged + status.untracked > 0) return 'dirty';
  return 'clean';
}

const stateCopy: Record<RepositoryStatusState, string> = {
  clean: 'Working tree is clean',
  dirty: 'Local changes have not been committed',
  ahead: 'Local commits have not been pushed',
  behind: 'Remote commits have not been integrated',
  diverged: 'Local and remote histories have diverged',
  conflicted: 'Repository has unresolved conflicts',
  uninitialized: 'Git has not been initialized for this folder',
  error: 'Repository status is unavailable',
};

/** A behavior-only hook (no CSS) on the detail Pane's scroll owner: the pagination observer's root. */
export const DETAIL_SCROLL_OWNER = 'repository-status-popover__detail';

const viewDefinitions = [
  { id: 'staged', label: 'Staged', icon: SquarePlus },
  { id: 'unstaged', label: 'Unstaged', icon: SquareMinus },
  { id: 'untracked', label: 'Untracked', icon: SquarePen },
  { id: 'conflicted', label: 'Conflicted', icon: SquareX },
  { id: 'commits', label: 'Commits', icon: GitCommitHorizontal },
] as const;

export function RepositoryStatusPopover({
  status,
  error = '',
  initialized = true,
  setupStep,
  setupBusy = false,
  setupError = '',
  refreshing = false,
  view = 'unstaged',
  fileMenu,
  selectedFiles = [],
  embedded = false,
  comparison,
  expandedCommits,
  detailFiles,
  detailCommits,
  detailLoading = false,
  detailError = '',
  detailHasMore = false,
  compact = false,
  detailActive = false,
}: {
  status: RepositoryStatus | null;
  error?: string;
  initialized?: boolean;
  setupStep?: RepositorySetupStep;
  setupBusy?: boolean;
  setupError?: string;
  refreshing?: boolean;
  view?: RepositoryStatusView;
  fileMenu?: RepositoryFileMenu;
  selectedFiles?: readonly string[];
  embedded?: boolean;
  comparison?: CodeReviewComparison;
  expandedCommits?: readonly string[];
  detailFiles?: RepositoryFile[];
  detailCommits?: CodeReview['commits'];
  detailLoading?: boolean;
  detailError?: string;
  detailHasMore?: boolean;
  /** A compact device class: the list-detail layout becomes a NavStack drill-down (HS2-3B8345). */
  compact?: boolean;
  /** In compact mode, whether the selected view's detail is pushed over the list. */
  detailActive?: boolean;
}) {
  const recoveryStep = error ? undefined : (setupStep ?? (!initialized ? 'initialize' : undefined)),
    state = repositoryStatusState(status, error, initialized),
    branch = status?.branch || 'No branch',
    upstream = status?.upstream || 'No upstream';
  const files = repositoryFilesForView(detailFiles ?? status?.files ?? [], view);
  const review: CodeReview | undefined = status
    ? {
        commits: detailCommits ?? status.commits ?? [],
        ranges: status.ranges ?? [],
        difftool: status.difftool,
        truncated: Boolean(status.truncated),
      }
    : undefined;
  const actions = (
    <>
      {comparison && !recoveryStep && (
        <ToolbarControlGroup label="Repository comparison" buttonAppearance="push" single>
          <button
            type="button"
            {...REPOSITORY_ACTIONS.toggleRepositoryComparison.attrs}
            aria-label="Compare two commits"
            title="Compare two commits"
            aria-pressed={String(comparison.active)}
          >
            <LucideIcon icon={GitCompare} name="git-compare" />
          </button>
        </ToolbarControlGroup>
      )}
      <ToolbarControlGroup label="Repository refresh" single>
        <button
          type="button"
          {...REPOSITORY_ACTIONS.refreshRepositoryStatus.attrs}
          disabled={refreshing}
          aria-label={refreshing ? 'Refreshing repository status' : 'Refresh repository status'}
        >
          <LucideIcon icon={RefreshCw} name="refresh-cw" />
        </button>
      </ToolbarControlGroup>
    </>
  );
  // The heading (icon, title, summary, and actions) belongs to the detail column of the list-detail
  // layout, not to a header spanning both columns (HS2-3B8345, Kerf recipe-list-detail-dialog).
  const troubled = state === 'error' || state === 'conflicted',
    stateColor =
      state === 'clean'
        ? uiColor('success-on-quiet')
        : troubled
          ? uiColor('danger-on-quiet')
          : uiColor('brand-on-quiet'),
    stateIcon = state === 'clean' ? CircleCheck : troubled ? TriangleAlert : GitBranch,
    stateIconName = state === 'clean' ? 'circle-check' : troubled ? 'triangle-alert' : 'git-branch',
    summary =
      recoveryStep === 'initialize'
        ? 'Initialize Git here to enable repository status'
        : recoveryStep === 'remote'
          ? 'Git is ready; add an origin remote or skip for now'
          : stateCopy[state];
  const heading = (
    <div class="app-heading" data-component="heading" data-has-icon="true">
      <Toolbar
        dividerSides=""
        leading={
          <>
            <ToolbarControlGroup single tileTone={state === 'clean' ? 'success' : troubled ? 'danger' : 'brand'}>
              <LucideIcon color={stateColor} icon={stateIcon} name={stateIconName} />
            </ToolbarControlGroup>
            <ToolbarText
              text={recoveryStep === 'initialize' ? 'This folder is not a Git repository' : 'Repository Status'}
              id="repository-status-title"
              size="xlarge"
            />
          </>
        }
        trailing={actions}
      />
      <p class="app-heading__summary">{summary}</p>
    </div>
  );
  const sidebar = (current: RepositoryStatus) => (
    <>
      <ValueTable label="Repository identity">
        <ValueTableRow label="Branch" value={branch} />
        <ValueTableRow label="Upstream" value={upstream} />
      </ValueTable>
      <ValueTable label="Repository synchronization">
        <ValueTableRow
          label="Ahead"
          value={
            <>
              <LucideIcon icon={ArrowUp} name="arrow-up" size={14.4} color={uiColor('text-quiet')} />
              {current.ahead}
            </>
          }
        />
        <ValueTableRow
          label="Behind"
          value={
            <>
              <LucideIcon icon={ArrowDown} name="arrow-down" size={14.4} color={uiColor('text-quiet')} />
              {current.behind}
            </>
          }
        />
      </ValueTable>
      <nav aria-label="Repository views">
        <ListHeader label="Views" />
        {viewDefinitions.map((item) => (
          <ListItem
            action="select-repository-view"
            itemId={item.id}
            selected={view === item.id}
            icon={
              <LucideIcon
                icon={item.icon}
                name={
                  item.id === 'commits'
                    ? 'git-commit-horizontal'
                    : item.id === 'untracked'
                      ? 'square-pen'
                      : item.id === 'conflicted'
                        ? 'square-x'
                        : item.id === 'staged'
                          ? 'square-plus'
                          : 'square-minus'
                }
              />
            }
            label={item.label}
            status={String(repositoryViewCount(current, item.id))}
          />
        ))}
      </nav>
    </>
  );
  const detail = (
    <>
      {view === 'commits' ? (
        <TicketCodeReview
          embedded
          title="Commits"
          emptyMessage="No commits were found in this repository."
          loadingMessage="Finding commits…"
          action="open-repository-review"
          review={review}
          comparison={comparison}
          expandedCommits={expandedCommits}
          loading={detailLoading && review?.commits.length === 0}
        />
      ) : (
        <RepositoryFileRows
          files={files.map((file) => ({ ...file, change: repositoryFileChange(file, view) }))}
          label={`${view} files`}
          source="repository"
          emptyMessage={detailLoading ? undefined : `No ${view} files.`}
          selectedFiles={selectedFiles}
        />
      )}
      {detailError && <StateBanner tone="danger" urgency="alert" title={detailError} />}
      {(detailHasMore || detailLoading) && (
        <div data-repository-pagination-sentinel="true">
          <EmptyState busy={detailLoading} title={detailLoading ? 'Loading more…' : 'Load more'} />
        </div>
      )}
    </>
  );
  const compactSplit = compact && Boolean(status) && !recoveryStep;
  const viewLabel = viewDefinitions.find((item) => item.id === view)?.label ?? 'Changes';
  return (
    <section
      popover={embedded ? undefined : 'auto'}
      id={embedded ? undefined : 'repository-status-popover'}
      class="dialog-surface repository-status-popover"
      data-component="repository-status-popover"
      data-state={state}
      data-view={view}
      data-setup-step={recoveryStep}
      data-embedded={embedded ? 'true' : undefined}
      role="dialog"
      aria-labelledby={compactSplit ? undefined : 'repository-status-title'}
      aria-label={compactSplit ? 'Repository Status' : undefined}
    >
      {status && !recoveryStep ? (
        <SplitView
          id="repository-status-split"
          label="Repository status"
          className="repository-status-popover__split"
          compact={compact}
          detailActive={detailActive}
          listTitle={compact ? 'Repository Status' : 'Repository'}
          detailTitle={viewLabel}
          backLabel="Back to repository views"
          compactStack={{ list: { toolbar: { trailing: actions } }, detail: { toolbar: { trailing: actions } } }}
          list={
            compact ? (
              <Pane label="Repository">
                {/* The stack toolbar carries the title and actions on compact devices; the state stays visible. */}
                <StateBanner
                  tone={state === 'clean' ? 'success' : troubled ? 'danger' : 'info'}
                  icon={<LucideIcon icon={stateIcon} name={stateIconName} color={stateColor} size="s" />}
                  title={summary}
                />
                {sidebar(status)}
              </Pane>
            ) : (
              <Pane
                label="Repository"
                header={<Toolbar label="Repository" leading={<ToolbarText text="Repository" />} />}
              >
                {sidebar(status)}
              </Pane>
            )
          }
          detail={
            compact ? (
              <Pane label={viewLabel} contentLabel={viewLabel} contentClassName={DETAIL_SCROLL_OWNER}>
                {detail}
              </Pane>
            ) : (
              <Pane
                label="Repository Status"
                header={heading}
                contentLabel={viewLabel}
                contentClassName={DETAIL_SCROLL_OWNER}
              >
                {detail}
              </Pane>
            )
          }
        />
      ) : (
        <Pane label="Repository Status" header={heading}>
          {recoveryStep && <RepositorySetup step={recoveryStep} busy={setupBusy} error={setupError} />}
          {!recoveryStep && !status && !error && <EmptyState busy title="Loading repository status…" />}
          {!recoveryStep && Boolean(error) && <StateBanner tone="danger" urgency="alert" title={error} />}
        </Pane>
      )}
      {fileMenu && fileMenu.source !== 'ticket' && (
        <RepositoryFileContextMenu menu={fileMenu} platform={status?.platform} />
      )}
    </section>
  );
}

type RepositoryFileRow = Pick<RepositoryFile, 'path' | 'original_path'> & { change: RepositoryFileChange };

const changeTone = (change: RepositoryFileChange) =>
  change === 'added' ? 'success' : change === 'deleted' || change === 'unmerged' ? 'danger' : 'neutral';

/** The divided, selectable file rows shared by Repository Status and Change evidence. */
function RepositoryFileRows({
  files,
  label,
  source,
  emptyMessage,
  selectedFiles,
}: {
  files: RepositoryFileRow[];
  label: string;
  source: 'repository' | 'ticket';
  /** Omit while the first page is still loading so no empty state flashes. */
  emptyMessage?: string;
  selectedFiles: readonly string[];
}) {
  if (files.length === 0)
    return emptyMessage ? (
      <EmptyState
        icon={<LucideIcon icon={CircleCheck} name="circle-check" size="l" color={uiColor('success-on-quiet')} />}
        title={emptyMessage}
      />
    ) : (
      <></>
    );
  return (
    <div role="list" aria-label={label}>
      {files.map((file) => (
        <ListActionRow
          action="select-repository-file"
          itemId={file.path}
          divider="after"
          state={file.change}
          pressed={selectedFiles.includes(file.path)}
          multiline
          title={`${file.path} — ${fileChangeLabel(file.change)}. Select; right-click for actions; double-click to open`}
          accessibleLabel={`${file.path}, ${fileChangeLabel(file.change)}. Select file`}
          icon={
            <Badge tone={changeTone(file.change)} shape="rounded" size="compact" ariaHidden>
              {repositoryFileStatusLetter(file.change)}
            </Badge>
          }
          label={middleEllipsisPath(file.path)}
          description={file.original_path ? <>from {middleEllipsisPath(file.original_path)}</> : undefined}
          trailingAction={REPOSITORY_ACTIONS.openRepositoryFileMenuTrigger.attrs['data-action']}
          trailingActionLabel={`Actions for ${file.path}`}
          trailingActionIcon={<LucideIcon size="s" icon={Ellipsis} name="ellipsis" />}
          trailingActionAttributes={{ 'data-file-menu-source': source, 'aria-haspopup': 'menu' }}
        />
      ))}
    </div>
  );
}

const evidenceViews = [
  { id: 'docs', label: 'Docs', icon: FileText, iconName: 'file-text' },
  { id: 'tests', label: 'Tests', icon: FlaskConical, iconName: 'flask-conical' },
  { id: 'source', label: 'Source', icon: FileCode2, iconName: 'file-code-2' },
  { id: 'other', label: 'Other', icon: CircleHelp, iconName: 'circle-help' },
] as const;

export function ChangeEvidenceDialog({
  review,
  view = 'docs',
  embedded = false,
  fileMenu,
  selectedFiles = [],
  platform,
  compact = false,
}: {
  review?: CodeReview;
  view?: ChangeEvidenceView;
  embedded?: boolean;
  /** A compact device class stacks the views above the files in one column (HS2-B2MD8Z). */
  compact?: boolean;
  fileMenu?: RepositoryFileMenu;
  selectedFiles?: readonly string[];
  platform?: RepositoryStatus['platform'];
}) {
  const files = review?.files ?? [],
    visible = files.filter((file) => file.category === view);
  const heading = (
    <div class="app-heading" data-component="heading" data-has-icon="true">
      <Toolbar
        dividerSides=""
        leading={
          <>
            <ToolbarControlGroup single tileTone="brand">
              <LucideIcon color={uiColor('brand-on-quiet')} icon={GitCompare} name="git-compare" />
            </ToolbarControlGroup>
            <ToolbarText text="Change evidence" id="change-evidence-title" size="xlarge" />
          </>
        }
      />
      <p class="app-heading__summary">Files changed across the ticket's complete commit range</p>
    </div>
  );
  const navigation = (
    <nav aria-label="Change evidence views">
      {compact ? <ListHeader label="Views" /> : <></>}
      {evidenceViews.map((item) => (
        <ListItem
          action="select-change-evidence-view"
          itemId={item.id}
          selected={view === item.id}
          icon={<LucideIcon icon={item.icon} name={item.iconName} />}
          label={item.label}
          status={String(files.filter((file) => file.category === item.id).length)}
        />
      ))}
    </nav>
  );
  const fileRows = (
    <>
      {review && !review.difftool && <DiffToolMissingNotice />}
      <RepositoryFileRows
        files={visible}
        label={`${view} change evidence`}
        source="ticket"
        emptyMessage={`No ${view} files.`}
        selectedFiles={selectedFiles}
      />
    </>
  );
  return (
    <section
      popover={embedded ? undefined : 'auto'}
      id={embedded ? undefined : 'change-evidence-dialog'}
      class="dialog-surface repository-status-popover change-evidence-dialog"
      data-component="change-evidence-dialog"
      data-view={view}
      data-embedded={embedded ? 'true' : undefined}
      role="dialog"
      aria-labelledby="change-evidence-title"
    >
      {compact ? (
        <Pane label="Change evidence" header={heading} contentLabel={`${view} change evidence`}>
          {navigation}
          {fileRows}
        </Pane>
      ) : (
        <SplitView
          id="change-evidence-split"
          label="Change evidence"
          list={
            <Pane
              label="Change evidence views"
              header={<Toolbar label="Views" leading={<ToolbarText text="Views" />} />}
            >
              {navigation}
            </Pane>
          }
          detail={
            <Pane label="Change evidence" header={heading} contentLabel={`${view} change evidence`}>
              {fileRows}
            </Pane>
          }
        />
      )}
      {fileMenu?.source === 'ticket' && <RepositoryFileContextMenu menu={fileMenu} platform={platform} />}
    </section>
  );
}

const fileMenuItem = (
  action: string,
  path: string,
  label: string,
  icon: Parameters<typeof LucideIcon>[0]['icon'],
  iconName: string,
  disabled = false,
): PopupMenuItem => ({
  label,
  disabled,
  icon: <LucideIcon size="s" icon={icon} name={iconName} />,
  attributes: { 'data-repository-file-action': action, 'data-repository-file-path': path },
});

function RepositoryFileContextMenu({
  menu,
  platform,
}: {
  menu: RepositoryFileMenu;
  platform?: RepositoryStatus['platform'];
}) {
  const multiple = (menu.paths?.length ?? 1) > 1;
  const reveal =
    platform === 'macos' ? 'Show in Finder' : platform === 'windows' ? 'Show in File Explorer' : 'Show in file manager';
  return (
    <ContextMenuAnchor x={menu.x} y={menu.y} component="repository-file-context-menu" label="File actions">
      <PopupMenu
        context
        label="File actions"
        rootAttributes={{ 'data-context-menu': 'repository-file' }}
        items={[
          fileMenuItem('show-diff', menu.path, 'Show Diff', GitCompare, 'git-compare', !menu.diff),
          fileMenuItem('open', menu.path, 'Open', ExternalLink, 'external-link', multiple),
          fileMenuItem('reveal', menu.path, reveal, FolderOpen, 'folder-open', multiple),
          { kind: 'divider' },
          fileMenuItem('copy-path', menu.path, 'Copy Relative Path', Clipboard, 'clipboard'),
          ...(menu.absolutePath
            ? [fileMenuItem('copy-absolute-path', menu.absolutePath, 'Copy Absolute Path', Copy, 'copy')]
            : []),
        ]}
      />
    </ContextMenuAnchor>
  );
}

export function repositoryFilesForView(files: RepositoryFile[], view: RepositoryStatusView): RepositoryFile[] {
  if (view === 'commits') return [];
  return files.filter((file) =>
    view === 'staged'
      ? Boolean(file.staged) && !file.conflicted
      : view === 'unstaged'
        ? Boolean(file.unstaged) && !file.untracked && !file.conflicted
        : view === 'untracked'
          ? file.untracked
          : file.conflicted,
  );
}

export function repositoryAbsolutePath(
  root: string | undefined,
  path: string,
  platform: RepositoryStatus['platform'],
): string | undefined {
  if (!root) return undefined;
  const separator = platform === 'windows' ? '\\' : '/';
  return `${root.replace(/[\\/]+$/, '')}${separator}${platform === 'windows' ? path.replaceAll('/', '\\') : path}`;
}

function repositoryViewCount(status: RepositoryStatus, view: RepositoryStatusView): number {
  if (view === 'commits') return status.commit_count ?? status.commits?.length ?? 0;
  return status[view];
}

function repositoryFileChange(
  file: RepositoryFile,
  view: Exclude<RepositoryStatusView, 'commits'>,
): RepositoryFileChange {
  if (view === 'conflicted') return 'unmerged';
  if (view === 'untracked') return 'untracked';
  return file[view] ?? 'modified';
}

export function repositoryFileStatusLetter(change: RepositoryFileChange): string {
  return change === 'untracked'
    ? '?'
    : change === 'type_changed'
      ? 'T'
      : change === 'unmerged'
        ? 'U'
        : change[0].toUpperCase();
}

function fileChangeLabel(change: RepositoryFileChange): string {
  return change === 'type_changed' ? 'Type changed' : change[0].toUpperCase() + change.slice(1);
}

function middleEllipsisPath(path: string) {
  const suffixLength = Math.min(20, Math.floor(path.length / 2)),
    splitAt = path.length - suffixLength;
  return (
    <span class="repository-status-popover__path" aria-label={path}>
      <span>{path.slice(0, splitAt)}</span>
      <span>{path.slice(splitAt)}</span>
    </span>
  );
}
