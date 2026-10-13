import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { type RepositoryBrowserBindings, wireRepositoryBrowser } from './repository-status-interactions';
import type { ChangeEvidenceView, RepositoryFileMenu, RepositoryStatusView } from './repository-status-popover';
import type { CodeReviewComparison } from './ticket-code-review';

interface Registration {
  root: unknown;
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ root, event, selector, handle });
    return () => undefined;
  },
}));
const root = {} as HTMLElement;
function handler(event: string, selector: string) {
  const found = registrations.find((item) => item.event === event && item.selector === selector);
  expect(found, `${event} ${selector}`).toBeDefined();
  expect(found!.root).toBe(root);
  return found!.handle;
}
/** A minimal row element: its dataset, plus `closest` answering for the dialog and the file list. */
function row(dataset: Record<string, string>, options: { ticket?: boolean; rows?: Element[] } = {}) {
  const element = {
    dataset,
    closest: (selector: string) => {
      if (selector.includes('change-evidence-dialog')) return options.ticket ? {} : null;
      if (selector === '[data-repository-file-list]') return { querySelectorAll: () => options.rows ?? [] };
      if (selector.includes('select-repository-file')) return element;
      return null;
    },
    getBoundingClientRect: () => ({ right: 100, bottom: 40 }),
  };
  return element as unknown as Element;
}

function bindings(overrides: Partial<RepositoryBrowserBindings> = {}) {
  let anchor: string | undefined;
  const value: RepositoryBrowserBindings = {
    view: signal<RepositoryStatusView>('staged'),
    detailActive: signal(false),
    fileMenu: signal<RepositoryFileMenu | undefined>(undefined),
    selectedFiles: signal<string[]>([]),
    selectionAnchor: {
      get: () => anchor,
      set: (path) => {
        anchor = path;
      },
    },
    comparison: signal<CodeReviewComparison>({ active: false, side: 'a' }),
    expandedCommits: signal<string[]>([]),
    changeEvidenceView: signal<ChangeEvidenceView>('docs'),
    onViewSelected: vi.fn(),
    onComparisonStarted: vi.fn(),
    canDiff: () => true,
    absolutePath: (path) => `/repo/${path}`,
    onFileAction: vi.fn(),
    onFileDiff: vi.fn(),
    onCopyPaths: vi.fn(),
    ...overrides,
  };
  return value;
}

beforeEach(() => {
  registrations.length = 0;
  vi.stubGlobal('window', { innerWidth: 1440, innerHeight: 900 });
});
afterEach(() => vi.unstubAllGlobals());

describe('wireRepositoryBrowser (HS2-RWY78R)', () => {
  it('selects a view, drills into its detail, and reports it to the owner', () => {
    const state = bindings();
    state.selectedFiles.value = ['a'];
    wireRepositoryBrowser(root, state);
    handler('click', '[data-action="select-repository-view"]')(new Event('click'), row({ itemId: 'commits' }));
    expect(state.view.value).toBe('commits');
    expect(state.detailActive.value).toBe(true);
    expect(state.selectedFiles.value).toEqual([]);
    expect(state.onViewSelected).toHaveBeenCalledWith('commits');
  });

  it('walks comparison on, both sides, and off again', () => {
    const state = bindings();
    wireRepositoryBrowser(root, state);
    const toggle = handler('click', '[data-action="toggle-repository-comparison"]'),
      pick = handler('click', '[data-action="select-repository-comparison-commit"]');
    toggle(new Event('click'), row({}));
    expect(state.comparison.value).toEqual({ active: true, side: 'a' });
    expect(state.view.value).toBe('commits');
    expect(state.onComparisonStarted).toHaveBeenCalledOnce();
    pick(new Event('click'), row({ commitSha: 'aaa' }));
    pick(new Event('click'), row({ commitSha: 'bbb' }));
    expect(state.comparison.value).toEqual({ active: true, side: 'b', a: 'aaa', b: 'bbb' });
    toggle(new Event('click'), row({}));
    expect(state.comparison.value).toEqual({ active: false, side: 'a' });
    expect(state.onComparisonStarted).toHaveBeenCalledOnce();
  });

  it('extends a shift-click range from the anchor across the visible rows (HS2-2Y02T1)', () => {
    const state = bindings();
    wireRepositoryBrowser(root, state);
    const click = handler('click', '[data-action="select-repository-file"]'),
      rows = ['a', 'b', 'c', 'd'].map((itemId) => ({ dataset: { itemId } }) as unknown as Element),
      file = (itemId: string) => row({ itemId }, { rows });
    click(Object.assign(new Event('click'), { shiftKey: false }), file('b'));
    expect(state.selectedFiles.value).toEqual(['b']);
    click(Object.assign(new Event('click'), { shiftKey: true }), file('d'));
    expect(state.selectedFiles.value).toEqual(['b', 'c', 'd']);
    click(Object.assign(new Event('click'), { shiftKey: true }), file('a'));
    expect(state.selectedFiles.value).toEqual(['a', 'b']);
  });

  it('opens a staged file menu with the selection, then routes each action to its callback', () => {
    const state = bindings();
    wireRepositoryBrowser(root, state);
    const file = row({ itemId: 'src/a.ts' });
    handler('contextmenu', '[data-action="select-repository-file"]')(
      Object.assign(new Event('contextmenu'), { clientX: 10, clientY: 20 }),
      file,
    );
    expect(state.fileMenu.value).toMatchObject({
      path: 'src/a.ts',
      paths: ['src/a.ts'],
      absolutePaths: ['/repo/src/a.ts'],
      diff: 'staged',
      source: 'repository',
    });
    const act = handler('click', '[data-repository-file-action]');
    act(new Event('click'), row({ repositoryFileAction: 'copy-absolute-path' }));
    expect(state.onCopyPaths).toHaveBeenCalledWith(['/repo/src/a.ts'], 'copy-absolute-path');
    expect(state.fileMenu.value).toBeUndefined();
    // A closed menu ignores a stale action.
    act(new Event('click'), row({ repositoryFileAction: 'open' }));
    expect(state.onFileAction).not.toHaveBeenCalled();
    handler('click', '[data-action="open-repository-file-menu-trigger"]')(new Event('click'), file);
    act(new Event('click'), row({ repositoryFileAction: 'show-diff' }));
    expect(state.onFileDiff).toHaveBeenCalledWith(['src/a.ts'], 'staged');
    handler('click', '[data-action="open-repository-file-menu-trigger"]')(new Event('click'), file);
    act(new Event('click'), row({ repositoryFileAction: 'reveal' }));
    expect(state.onFileAction).toHaveBeenCalledWith('src/a.ts', 'reveal');
  });

  it('marks a change-evidence file as a ticket diff and omits diff when no tool is configured', () => {
    const state = bindings({ canDiff: (source) => source === 'ticket' });
    wireRepositoryBrowser(root, state);
    const open = handler('contextmenu', '[data-action="select-repository-file"]');
    open(new Event('contextmenu'), row({ itemId: 'docs/a.md' }, { ticket: true }));
    expect(state.fileMenu.value).toMatchObject({ source: 'ticket', diff: 'ticket' });
    open(new Event('contextmenu'), row({ itemId: 'docs/a.md' }));
    expect(state.fileMenu.value).toMatchObject({ source: 'repository', diff: undefined });
  });

  it('opens a file on double-click unless the menu trigger was double-clicked', () => {
    const state = bindings();
    wireRepositoryBrowser(root, state);
    const dblclick = handler('dblclick', '[data-action="select-repository-file"]');
    dblclick({ target: { closest: () => null } } as unknown as Event, row({ itemId: 'a' }));
    expect(state.onFileAction).toHaveBeenCalledWith('a', 'open');
    dblclick({ target: { closest: () => ({}) } } as unknown as Event, row({ itemId: 'b' }));
    expect(state.onFileAction).toHaveBeenCalledTimes(1);
  });

  it('switches the change-evidence view and clears the file selection', () => {
    const state = bindings();
    state.selectedFiles.value = ['a'];
    state.selectionAnchor.set('a');
    wireRepositoryBrowser(root, state);
    handler('click', '[data-action="select-change-evidence-view"]')(new Event('click'), row({ itemId: 'tests' }));
    expect(state.changeEvidenceView.value).toBe('tests');
    expect(state.selectedFiles.value).toEqual([]);
    expect(state.selectionAnchor.get()).toBeUndefined();
  });
});
