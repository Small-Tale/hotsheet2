import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { RepositoryFile, RepositoryFileChange, RepositoryStatus } from '../api';
import {
  ChangeEvidenceDialog,
  repositoryAbsolutePath,
  repositoryFilesForView,
  repositoryFileStatusLetter,
  RepositoryStatusPopover,
  repositoryStatusState,
} from './repository-status-popover';

const files: RepositoryFile[] = [
  { path: 'src/staged.ts', staged: 'added', untracked: false, conflicted: false },
  { path: 'src/changed.ts', unstaged: 'modified', untracked: false, conflicted: false },
  { path: 'src/new name.ts', original_path: 'src/old name.ts', staged: 'renamed', untracked: false, conflicted: false },
  { path: 'notes/new.md', untracked: true, conflicted: false },
  { path: 'src/conflict.ts', staged: 'unmerged', unstaged: 'unmerged', untracked: false, conflicted: true },
];
const status = (patch: Partial<RepositoryStatus> = {}): RepositoryStatus => ({
  branch: 'main',
  upstream: 'origin/main',
  ahead: 0,
  behind: 0,
  staged: 2,
  unstaged: 1,
  untracked: 1,
  conflicted: 1,
  files,
  root: '/work/demo',
  platform: 'macos',
  commit_count: 12,
  commits: [
    {
      sha: 'abc123456',
      short_sha: 'abc1234',
      subject: 'Ship repository browser',
      committed_at: '2026-09-04T00:00:00Z',
    },
  ],
  ranges: [],
  difftool: 'Glassbox',
  truncated: false,
  ...patch,
});

describe('RepositoryStatusPopover', () => {
  it('lays Repository Status out as a list-detail SplitView whose detail column owns the heading (HS2-3B8345)', () => {
    const markup = String(RepositoryStatusPopover({ status: status() }));
    const css = readFileSync(resolve(import.meta.dirname, 'repository-status-popover.css'), 'utf8');
    expect(markup).toContain('data-component="split-view"');
    expect(markup).not.toContain('data-component="nav-stack"');
    // The list Pane has its own quiet toolbar title; the heading follows inside the detail column.
    const list = markup.indexOf('>Repository<'),
      metadata = markup.indexOf('repository-status-popover__metadata'),
      heading = markup.indexOf('id="repository-status-title"'),
      detail = markup.indexOf('class="repository-status-popover__detail" aria-live="polite"');
    expect(list).toBeGreaterThan(-1);
    expect(list).toBeLessThan(metadata);
    expect(metadata).toBeLessThan(heading);
    expect(heading).toBeLessThan(detail);
    expect(markup).toContain('aria-labelledby="repository-status-title"');
    // Panes own scrolling and SplitView the divider; the old grid stays only for ChangeEvidenceDialog.
    expect(markup).not.toContain('<aside class="repository-status-popover__navigation">');
    expect(css).toMatch(/\.repository-status-popover__split \.repository-status-popover__navigation,/);
    expect(css).toMatch(/--kui-split-view-list-width: min\(remify\(432px\), 50%\)/);
    expect(css).toMatch(/:is\(:popover-open, \[data-embedded='true'\]\)/);
  });

  it('drills from the repository list into the selected view on compact devices (HS2-3B8345)', () => {
    const list = String(RepositoryStatusPopover({ status: status(), view: 'conflicted', compact: true }));
    expect(list).toContain('data-component="nav-stack"');
    // The stack toolbar carries the title and actions; the list keeps the state summary.
    expect(list).toContain('Repository Status');
    expect(list).toContain('repository-status-popover__compact-summary');
    expect(list).not.toContain('id="repository-status-title"');
    expect(list).toContain('aria-label="Repository Status"');
    expect(list).toContain('aria-label="Refresh repository status"');
    const detail = String(
      RepositoryStatusPopover({ status: status(), view: 'conflicted', compact: true, detailActive: true }),
    );
    expect(detail).toContain('Back to repository views');
    expect(detail).toContain('Conflicted');
    // Setup, loading, and error states keep one headed column on every device.
    const failed = String(RepositoryStatusPopover({ status: null, error: 'git failed', compact: true }));
    expect(failed).not.toContain('data-component="split-view"');
    expect(failed).toContain('id="repository-status-title"');
    expect(failed).toContain('git failed');
  });

  it('classifies every repository state without hiding orthogonal counts', () => {
    expect(repositoryStatusState(status({ staged: 0, unstaged: 0, untracked: 0, conflicted: 0 }))).toBe('clean');
    expect(repositoryStatusState(status({ conflicted: 0 }))).toBe('dirty');
    expect(repositoryStatusState(status({ conflicted: 0, ahead: 1 }))).toBe('ahead');
    expect(repositoryStatusState(status({ conflicted: 0, behind: 1 }))).toBe('behind');
    expect(repositoryStatusState(status({ conflicted: 0, ahead: 1, behind: 1 }))).toBe('diverged');
    expect(repositoryStatusState(status())).toBe('conflicted');
    expect(repositoryStatusState(null, '', false)).toBe('uninitialized');
    expect(repositoryStatusState(null, 'git failed')).toBe('error');
  });

  it('renders typed no-Git recovery instead of a raw repository error', () => {
    const markup = String(
      RepositoryStatusPopover({
        status: status({
          branch: undefined,
          upstream: undefined,
          staged: 0,
          unstaged: 0,
          untracked: 0,
          conflicted: 0,
          files: [],
        }),
        initialized: false,
      }),
    );
    expect(markup).toContain('data-state="uninitialized"');
    expect(markup).toContain('data-setup-step="initialize"');
    expect(markup).toMatch(
      /kui-toolbar-text[^>]*id="repository-status-title"[^>]*><span class="kui-toolbar-text__text">This folder is not a Git repository</,
    );
    expect(markup).not.toContain('>Repository Status<');
    expect(markup).toContain('data-action="initialize-repository"');
    expect(markup).not.toContain('<dt>Branch</dt>');
    expect(markup).not.toContain('fatal: not a git repository');
  });

  it('can present the optional origin step after initialization', () => {
    const markup = String(RepositoryStatusPopover({ status: status({ conflicted: 0 }), setupStep: 'remote' }));
    expect(markup).toContain('data-setup-step="remote"');
    expect(markup).toContain('data-action="connect-repository-remote"');
    expect(markup).toContain('data-action="skip-repository-remote"');
    expect(markup).toContain('Add origin');
    expect(markup).not.toContain('<dt>Branch</dt>');
  });

  it('renders value cells, selectable views, and iconic file status in a master-detail layout', () => {
    const markup = String(RepositoryStatusPopover({ status: status(), view: 'staged' }));
    for (const text of [
      '<span class="kui-value-table__label">Branch</span></dt><dd>main',
      '<span class="kui-value-table__label">Upstream</span></dt><dd>origin/main',
      '<span class="kui-value-table__label">Ahead</span>',
      '<span class="kui-value-table__label">Behind</span>',
      'Staged',
      'Unstaged',
      'Untracked',
      'Conflicted',
      'Commits',
    ])
      expect(markup).toContain(text);
    expect(markup.match(/class="kui-value-table__row"/g)).toHaveLength(4);
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('data-component="list-header"');
    expect(markup.match(/data-component="list-item"/g)).toHaveLength(7);
    expect(markup).toContain('data-action="select-repository-file" data-item-id="src/staged.ts"');
    expect(markup).toContain('data-action="open-repository-file-menu-trigger" data-item-id="src/staged.ts"');
    expect(markup).toContain('data-lucide="ellipsis"');
    expect(markup).toContain('data-state="renamed"');
    expect(markup).toContain('aria-label="src/old name.ts"');
    expect(markup).toContain('data-lucide="square-pen"');
    expect(markup).toContain('class="repository-status-popover__file-status" aria-hidden="true">A</span>');
    expect(markup).toContain('class="repository-status-popover__file-status" aria-hidden="true">R</span>');
    expect(markup).not.toContain('repository-status-popover__file-state');
    expect(markup).toContain('repository-status-popover__path');
    expect(markup).toContain('data-action="refresh-repository-status"');
    const css = readFileSync(resolve(import.meta.dirname, 'repository-status-popover.css'), 'utf8'),
      shared = readFileSync(resolve(import.meta.dirname, 'repository-status-popover.tsx'), 'utf8');
    expect(css).toMatch(/__layout \{[^}]*grid-template-columns:/);
    expect(css).toMatch(/__detail \{[^}]*overflow: auto;/);
    expect(shared).toContain('dividerSides=""');
  });

  it('uses canonical master-detail, connected-list, and menu spacing while retaining geometry', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'repository-status-popover.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.repository-status-popover \{[^}]*2 \* var\(--kui-space-l\)/);
    expect(css).toMatch(/__navigation \{[^}]*padding: var\(--kui-space-l\)/);
    expect(css).toMatchSource(/__values \+ \.repository-status-popover__values \{ margin-top: var\(--kui-space-m\)/);
    // The nav spaces its header from the app-owned views wrapper; no margin on Kerf's ListHeader.
    expect(css).toMatchSource(/nav \{ display: grid; row-gap: var\(--kui-space-2xs\)/);
    expect(css).toMatchSource(/__views \{ display: grid/);
    expect(css).not.toContain('.kui-list-header');
    expect(css).toMatch(
      /__context-menu button \{[^}]*padding: 0 var\(--kui-space-m\);[^}]*grid-template-columns: remify\(16px\)[^}]*gap: var\(--kui-space-xs\)/,
    );
    expect(css).toMatchSource(/__navigation, \.repository-status-popover__detail \{ padding: var\(--kui-space-m\)/);
  });

  it('uses the canonical Git status letter for every file change kind', () => {
    expect(
      ['added', 'copied', 'deleted', 'modified', 'renamed', 'type_changed', 'unmerged', 'untracked'].map((change) =>
        repositoryFileStatusLetter(change as RepositoryFileChange),
      ),
    ).toEqual(['A', 'C', 'D', 'M', 'R', 'T', 'U', '?']);
  });

  it('reuses the master-detail and Git-letter rows for ticket change evidence', () => {
    const markup = String(
      ChangeEvidenceDialog({
        view: 'tests',
        review: {
          difftool: 'Glassbox',
          truncated: false,
          ranges: [],
          commits: [],
          summary: { files: { total: 3, docs: 1, tests: 1, source: 1, other: 0 }, tests_added: 1, tests_modified: 0 },
          files: [
            { path: 'docs/design.md', change: 'modified', category: 'docs' },
            { path: 'clients/web/src/dialog.test.ts', change: 'added', category: 'tests' },
            { path: 'clients/web/src/dialog.ts', change: 'modified', category: 'source' },
          ],
        },
      }),
    );
    expect(markup).toContain('data-component="change-evidence-dialog"');
    expect(markup).toContain('aria-label="Change evidence views"');
    expect(markup).toContain('data-action="select-repository-file" data-item-id="clients/web/src/dialog.test.ts"');
    expect(markup).toContain('data-file-menu-source="ticket"');
    expect(markup).toContain('class="repository-status-popover__file-status" aria-hidden="true">A</span>');
    expect(markup).not.toContain('data-item-id="docs/design.md" class="repository-status-popover__file"');
  });

  it('filters files into non-overlapping working-tree views', () => {
    expect(repositoryFilesForView(files, 'staged').map((file) => file.path)).toEqual([
      'src/staged.ts',
      'src/new name.ts',
    ]);
    expect(repositoryFilesForView(files, 'unstaged').map((file) => file.path)).toEqual(['src/changed.ts']);
    expect(repositoryFilesForView(files, 'untracked').map((file) => file.path)).toEqual(['notes/new.md']);
    expect(repositoryFilesForView(files, 'conflicted').map((file) => file.path)).toEqual(['src/conflict.ts']);
  });

  it('reuses code-review commit and range actions in the commits view', () => {
    const markup = String(
      RepositoryStatusPopover({
        status: status({ ranges: [{ from: 'aaa1111', to: 'abc123456', count: 2 }] }),
        view: 'commits',
        comparison: { active: false, side: 'a' },
      }),
    );
    expect(markup).toContain('data-component="ticket-code-review"');
    expect(markup).toContain('Ship repository browser');
    expect(markup).toContain('data-action="open-repository-review"');
    expect(markup).toContain('data-review-mode="range"');
    expect(markup).toContain('aria-label="Compare two commits"');
    expect(markup).toContain('data-action="toggle-repository-comparison"');
    expect(markup).toMatch(
      /data-button-appearance="push"[^>]*data-single="true"[^>]*><button[^>]*toggle-repository-comparison/,
    );
    expect(markup).toMatch(
      /kui-toolbar__trailing[\s\S]*data-appearance="contained"[\s\S]*toggle-repository-comparison[\s\S]*refresh-repository-status/,
    );
    expect(markup).toMatch(
      /kui-toolbar__trailing"><div[^>]*aria-label="Repository comparison"[\s\S]*<\/div><div[^>]*aria-label="Repository refresh"/,
    );
    expect(markup).not.toMatch(/kui-toolbar__trailing"><div[^>]*data-component="toolbar-control-group"[^>]*><div/);
    expect(markup.match(/data-component="toolbar-control-group"/g)).toHaveLength(3);
    const popoverCss = readFileSync(resolve(import.meta.dirname, 'repository-status-popover.css'), 'utf8');
    expect(popoverCss).not.toMatch(/repository-status-popover__refresh[^}]*color:/);
  });

  it('keeps the comparison selector compact and separated from its open action', () => {
    const markup = String(
      RepositoryStatusPopover({ status: status(), view: 'commits', comparison: { active: true, side: 'a' } }),
    );
    expect(markup).toContain('aria-pressed="true"');
    expect(markup).toContain('data-button-appearance="push"');
    expect(markup).toContain('data-size="compact"');
    expect(markup).not.toContain('cancel-repository-comparison');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-code-review.css'), 'utf8');
    expect(css).toMatchSource(/__compare-banner \{[^}]*grid-template-columns: auto minmax\(0,1fr\) auto/);
    expect(css).toMatch(/__compare-toolbar \{[^}]*justify-self: start/);
    expect(css).toMatch(/__compare-open \{[^}]*grid-column: 3/);
  });

  it('renders bounded detail pages with an infinite-scroll sentinel', () => {
    const markup = String(
      RepositoryStatusPopover({ status: status(), view: 'staged', detailFiles: [files[0]], detailHasMore: true }),
    );
    expect(markup).toContain('data-item-id="src/staged.ts"');
    expect(markup).not.toContain('data-item-id="src/new name.ts"');
    expect(markup).toContain('data-repository-pagination-sentinel="true"');
    expect(
      String(RepositoryStatusPopover({ status: status(), view: 'commits', detailCommits: [], detailLoading: true })),
    ).toContain('Finding commits');
  });

  it('normalizes absolute paths and host-specific reveal copy', () => {
    expect(repositoryAbsolutePath('/work/demo/', 'src/file.ts', 'macos')).toBe('/work/demo/src/file.ts');
    expect(repositoryAbsolutePath('C:\\work\\demo', 'src/file.ts', 'windows')).toBe('C:\\work\\demo\\src\\file.ts');
    const menu = String(
      RepositoryStatusPopover({
        status: status(),
        fileMenu: { path: 'src/changed.ts', absolutePath: '/work/demo/src/changed.ts', x: 10, y: 20, diff: 'unstaged' },
      }),
    );
    expect(menu).toContain('Show in Finder');
    for (const label of ['Show Diff', 'Open', 'Copy Relative Path', 'Copy Absolute Path'])
      expect(menu).toContain(label);
    expect(menu).toContain('data-repository-file-action="show-diff"');
    expect(
      String(
        RepositoryStatusPopover({
          status: status({ platform: 'windows' }),
          fileMenu: { path: 'src/changed.ts', x: 10, y: 20 },
        }),
      ),
    ).toContain('Show in File Explorer');
    expect(
      String(
        ChangeEvidenceDialog({
          review: { commits: [], ranges: [], difftool: 'Glassbox', truncated: false, files: [] },
          fileMenu: { path: 'src/file.ts', x: 10, y: 20, diff: 'ticket' },
          platform: 'macos',
        }),
      ),
    ).toContain('data-component="repository-file-context-menu"');
    const multiple = String(
      RepositoryStatusPopover({
        status: status(),
        fileMenu: {
          path: 'src/changed.ts',
          paths: ['src/changed.ts', 'src/staged.ts'],
          x: 10,
          y: 20,
          diff: 'unstaged',
        },
      }),
    );
    expect(multiple).toMatch(/data-repository-file-action="show-diff"[^>]*><svg/);
    expect(multiple).toMatch(/data-repository-file-action="open"[^>]*disabled/);
    expect(multiple).toMatch(/data-repository-file-action="reveal"[^>]*disabled/);
  });

  it('renders an actionable error without inventing repository values', () => {
    const markup = String(RepositoryStatusPopover({ status: null, error: 'git status failed' }));
    expect(markup).toContain('data-state="error"');
    expect(markup).toContain('git status failed');
    expect(markup).not.toContain('<dt>Branch</dt>');
  });
});
