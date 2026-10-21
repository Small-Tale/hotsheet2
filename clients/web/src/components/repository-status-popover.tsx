import '@kerfjs/ui/badge.css';
import '@kerfjs/ui/empty-state.css';
import '@kerfjs/ui/layout.css';
import '@kerfjs/ui/list-action-row.css';
import '@kerfjs/ui/nav-stack.css';
import '@kerfjs/ui/split-view.css';
import '@kerfjs/ui/state-banner.css';
import '@kerfjs/ui/surface-scaffold.css';

import { Badge } from '@kerfjs/ui/badge';
import { remify, space, uiColor } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane } from '@kerfjs/ui/pane';
import { PopupMenu, type PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { SplitView } from '@kerfjs/ui/split-view';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { PopoverDialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
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
import { contextMenuAnchor } from '../context-menu-position';
import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';
import type { RepositoryStatusView } from '../repository-state';
import { RepositorySetup, type RepositorySetupStep } from './repository-setup';
import { type CodeReviewComparison, DiffToolMissingNotice, TicketCodeReview } from './ticket-code-review';

export type RepositoryStatusState =
  'clean' | 'dirty' | 'ahead' | 'behind' | 'diverged' | 'conflicted' | 'uninitialized' | 'error';
export type { RepositoryStatusView } from '../repository-state';
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
  hostActionsAvailable = hostActionsAvailableHere(),
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
  /** False on a remote client: open, reveal, and diff-tool actions render disabled (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
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
        <ToolbarControlGroup label="Repository Comparison" buttonAppearance="push" single>
          <button
            type="button"
            {...REPOSITORY_ACTIONS.toggleRepositoryComparison.attrs}
            aria-label="Compare Two Commits"
            title="Compare two commits"
            aria-pressed={String(comparison.active)}
          >
            <LucideIcon icon={GitCompare} name="git-compare" />
          </button>
        </ToolbarControlGroup>
      )}
      <ToolbarControlGroup label="Repository Refresh" single>
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
            description={summary}
          />
        </>
      }
      trailing={actions}
    />
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
      <nav aria-label="Repository Views">
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
          hostActionsAvailable={hostActionsAvailable}
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
    // Kerf's PopoverDialogSurface owns the chrome, viewport-bounded size, fill, and embedded flow
    // (KF-XHECB9, HS2-5242DR).
    <PopoverDialogSurface
      id={embedded ? undefined : 'repository-status-popover'}
      className="repository-status-popover"
      presentation={embedded ? 'embedded' : 'overlay'}
      preferredWidth={remify(1024)}
      height={remify(640)}
      viewportGutter={space('xs')}
      bodyLayout="fill"
      rootAttributes={{
        'data-repository-surface': 'repository-status',
        'data-state': state,
        'data-view': view,
        ...(recoveryStep ? { 'data-setup-step': recoveryStep } : {}),
      }}
      {...(compactSplit ? { label: 'Repository Status' } : { labelledBy: 'repository-status-title' })}
    >
      {status && !recoveryStep ? (
        <SplitView
          id="repository-status-split"
          label="Repository status"
          className="repository-status-popover__split"
          listWidth={{ size: 432, maxFraction: 0.5 }}
          compact={compact}
          detailActive={detailActive}
          listTitle={compact ? 'Repository Status' : 'Repository'}
          detailTitle={viewLabel}
          backLabel="Back to Repository Views"
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
          {!recoveryStep && !status && !error && <EmptyState busy title="Loading Repository Status…" />}
          {!recoveryStep && Boolean(error) && <StateBanner tone="danger" urgency="alert" title={error} />}
        </Pane>
      )}
      {fileMenu && fileMenu.source !== 'ticket' && (
        <RepositoryFileContextMenu
          menu={fileMenu}
          platform={status?.platform}
          hostActionsAvailable={hostActionsAvailable}
        />
      )}
    </PopoverDialogSurface>
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
    <div role="list" aria-label={label} data-repository-file-list>
      {files.map((file, index) => (
        <ListActionRow
          action="select-repository-file"
          itemId={file.path}
          // Dividers only between rows, so Kerf rounds the last/only row's outer corners (KF-AQN4H7).
          divider={index < files.length - 1 ? 'after' : 'none'}
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
  hostActionsAvailable = hostActionsAvailableHere(),
}: {
  review?: CodeReview;
  view?: ChangeEvidenceView;
  embedded?: boolean;
  /** A compact device class stacks the views above the files in one column (HS2-B2MD8Z). */
  compact?: boolean;
  fileMenu?: RepositoryFileMenu;
  selectedFiles?: readonly string[];
  platform?: RepositoryStatus['platform'];
  /** False on a remote client: open, reveal, and diff-tool actions render disabled (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}) {
  const files = review?.files ?? [],
    visible = files.filter((file) => file.category === view);
  const heading = (
    <Toolbar
      dividerSides=""
      leading={
        <>
          <ToolbarControlGroup single tileTone="brand">
            <LucideIcon color={uiColor('brand-on-quiet')} icon={GitCompare} name="git-compare" />
          </ToolbarControlGroup>
          <ToolbarText
            text="Change Evidence"
            id="change-evidence-title"
            size="xlarge"
            description="Files changed across the ticket's complete commit range"
          />
        </>
      }
    />
  );
  const navigation = (
    <nav aria-label="Change Evidence Views">
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
    <PopoverDialogSurface
      id={embedded ? undefined : 'change-evidence-dialog'}
      className="change-evidence-dialog"
      presentation={embedded ? 'embedded' : 'overlay'}
      preferredWidth={remify(832)}
      height={remify(512)}
      viewportGutter={space('xs')}
      bodyLayout="fill"
      rootAttributes={{ 'data-repository-surface': 'change-evidence', 'data-view': view }}
      labelledBy="change-evidence-title"
    >
      {compact ? (
        <Pane label="Change Evidence" header={heading} contentLabel={`${view} Change Evidence`}>
          {navigation}
          {fileRows}
        </Pane>
      ) : (
        <SplitView
          id="change-evidence-split"
          label="Change Evidence"
          listWidth={{ size: 240, maxFraction: 0.4 }}
          list={
            <Pane
              label="Change Evidence Views"
              header={<Toolbar label="Views" leading={<ToolbarText text="Views" />} />}
            >
              {navigation}
            </Pane>
          }
          detail={
            <Pane label="Change Evidence" header={heading} contentLabel={`${view} Change Evidence`}>
              {fileRows}
            </Pane>
          }
        />
      )}
      {fileMenu?.source === 'ticket' && (
        <RepositoryFileContextMenu menu={fileMenu} platform={platform} hostActionsAvailable={hostActionsAvailable} />
      )}
    </PopoverDialogSurface>
  );
}

const fileMenuItem = (
  action: string,
  path: string,
  label: string,
  icon: Parameters<typeof LucideIcon>[0]['icon'],
  iconName: string,
  disabled = false,
  disabledReason?: string,
): PopupMenuItem => ({
  label,
  disabled: disabled || Boolean(disabledReason),
  ...(disabledReason ? { disabledReason } : {}),
  icon: <LucideIcon size="s" icon={icon} name={iconName} />,
  attributes: { 'data-repository-file-action': action, 'data-repository-file-path': path },
});

function RepositoryFileContextMenu({
  menu,
  platform,
  hostActionsAvailable,
}: {
  menu: RepositoryFileMenu;
  platform?: RepositoryStatus['platform'];
  hostActionsAvailable: boolean;
}) {
  const multiple = (menu.paths?.length ?? 1) > 1;
  const reveal =
    platform === 'macos' ? 'Show in Finder' : platform === 'windows' ? 'Show in File Explorer' : 'Show in file manager';
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(menu.x, menu.y)}
      label="File Actions"
      rootAttributes={{ 'data-context-menu': 'repository-file' }}
      items={[
        fileMenuItem('show-diff', menu.path, 'Show Diff', GitCompare, 'git-compare', !menu.diff),
        fileMenuItem(
          'open',
          menu.path,
          'Open',
          ExternalLink,
          'external-link',
          multiple,
          hostOnlyDisabledReason('open-file', hostActionsAvailable),
        ),
        fileMenuItem(
          'reveal',
          menu.path,
          reveal,
          FolderOpen,
          'folder-open',
          multiple,
          hostOnlyDisabledReason('reveal-file', hostActionsAvailable),
        ),
        { kind: 'divider' },
        fileMenuItem('copy-path', menu.path, 'Copy Relative Path', Clipboard, 'clipboard'),
        ...(menu.absolutePath
          ? [fileMenuItem('copy-absolute-path', menu.absolutePath, 'Copy Absolute Path', Copy, 'copy')]
          : []),
      ]}
    />
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

/** A path truncated in the middle, keeping its file name visible (Kerf Text, KF-XYA6VR). */
function middleEllipsisPath(path: string) {
  return (
    <Text variant="span" border="none" wrap="truncate" truncate="middle" truncateKeep="segment">
      {path}
    </Text>
  );
}
