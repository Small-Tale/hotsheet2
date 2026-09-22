import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { KEYBOARD_SHORTCUT_STORAGE_KEY, type ShortcutChord } from '../keyboard-shortcuts';
import { type CommandAndAiInteractionsDependencies, wireCommandAndAiInteractions } from './commands-and-ai';
import { type ProjectLifecycleInteractionsDependencies, wireProjectLifecycleInteractions } from './project-lifecycle';
import { type RepositoryInteractionsDependencies, wireRepositoryInteractions } from './repository';
import {
  type ViewAndSavedViewInteractionsDependencies,
  wireViewAndSavedViewInteractions,
} from './views-and-saved-views';

interface Registration {
  root: unknown;
  event: string;
  selector: string;
  capture: boolean;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (root: unknown, event: string, selector: string, handle: Registration['handle']) =>
    registrations.push({ root, event, selector, handle, capture: false }),
  delegateCapture: (root: unknown, event: string, selector: string, handle: Registration['handle']) =>
    registrations.push({ root, event, selector, handle, capture: true }),
}));
function handler(event: string, selector: string, capture = false) {
  const found = registrations.find(
    (registration) =>
      registration.event === event && registration.selector === selector && registration.capture === capture,
  );
  expect(found, `${event} ${selector} capture=${String(capture)}`).toBeDefined();
  expect(found!.root).toBe(document.body);
  return found!.handle;
}
const target = (dataset: Record<string, string>) => ({ dataset }) as unknown as Element;

beforeEach(() => {
  registrations.length = 0;
  vi.stubGlobal('document', { body: {}, addEventListener: vi.fn() });
});
afterEach(() => vi.unstubAllGlobals());

describe('extracted handlers retain live application bindings', () => {
  it('reads an externally replaced range anchor and writes its next selection back to the owner', () => {
    let anchor: string | undefined = 'a';
    const selected = signal<string[]>([]),
      menu = signal(undefined);
    // Only the exercised callback's dependencies are supplied; no other callback runs.
    const bindings = {
      repositorySelectedFiles: selected,
      repositoryFileMenu: menu,
      get repositoryFileSelectionAnchor() {
        return anchor;
      },
      set repositoryFileSelectionAnchor(value: string | undefined) {
        anchor = value;
      },
    } as RepositoryInteractionsDependencies;
    wireRepositoryInteractions(bindings);
    const click = handler('click', '[data-action="select-repository-file"]');
    const rows = ['a', 'b', 'c', 'd'].map((itemId) => target({ itemId }));
    const row = (index: number) => Object.assign(rows[index], { closest: () => ({ querySelectorAll: () => rows }) });
    click(new Event('click'), row(0));
    expect(selected.value).toEqual(['a']);
    expect(anchor).toBe('a');
    anchor = 'b'; // Another owning action changed it after wiring.
    click(Object.assign(new Event('click'), { shiftKey: true }), row(3));
    expect(selected.value).toEqual(['b', 'c', 'd']);
    expect(anchor).toBe('b');
    click(new Event('click'), row(2));
    expect(anchor).toBe('c');
    expect(selected.value).toEqual(['c']);
  });

  it('consumes a shared long-press flag once and preserves captured native dialog close', () => {
    let fired = false;
    const run = vi.fn(async () => {}),
      dialog = signal<string | undefined>('command'),
      confirmation = signal(true);
    const bindings: Partial<CommandAndAiInteractionsDependencies> = {
      runCommand: run,
      commandDialogId: dialog,
      commandStopConfirmation: confirmation,
      get commandLongPressFired() {
        return fired;
      },
      set commandLongPressFired(value: boolean) {
        fired = value;
      },
    };
    wireCommandAndAiInteractions(bindings as CommandAndAiInteractionsDependencies);
    const click = handler('click', '[data-action="run-command"]'),
      button = target({ itemId: 'check' });
    fired = true; // The global pointer handler owns the original plain binding.
    const suppressed = new Event('click', { cancelable: true });
    click(suppressed, button);
    expect(suppressed.defaultPrevented).toBe(true);
    expect(fired).toBe(false);
    expect(run).not.toHaveBeenCalled();
    click(new Event('click'), button);
    expect(run).toHaveBeenCalledExactlyOnceWith('check');
    const close = handler(
      'close',
      '[data-component="command-run-dialog"], [data-component="command-cancellation-dialog"]',
      true,
    );
    close(new Event('close'), button);
    expect(dialog.value).toBeUndefined();
    expect(confirmation.value).toBe(false);
    dialog.value = 'reopened';
    confirmation.value = true;
    close(new Event('close'), button);
    expect(dialog.value).toBeUndefined();
    expect(confirmation.value).toBe(false);
  });

  it('synchronizes repeated native project dismissal through the original signals', () => {
    const opened = signal(true),
      recovery = signal<ProjectLifecycleInteractionsDependencies['unhealthyServerRecovery']['value']>({
        store: '/store',
        expected: { pid: 7, url: 'http://localhost', started_at: '' },
      });
    wireProjectLifecycleInteractions({
      projectDialogOpen: opened,
      unhealthyServerRecovery: recovery,
    } as ProjectLifecycleInteractionsDependencies);
    const hide = handler('wa-hide', '[data-project-dialog]');
    hide(new Event('wa-hide'), target({}));
    expect(opened.value).toBe(false);
    expect(recovery.value).toBeUndefined();
    opened.value = true;
    recovery.value = { store: '/new-store', expected: { pid: 8, url: 'http://localhost', started_at: '' } };
    hide(new Event('wa-hide'), target({}));
    expect(opened.value).toBe(false);
    expect(recovery.value).toBeUndefined();
  });
  it('keeps saved-view name/query inputs separate through native dismissal, busy protection, and reset', () => {
    const opened = signal(true),
      busy = signal(false),
      name = signal(''),
      error = signal('Old error');
    const query = signal(''),
      tokens = signal([]),
      focusQuery = vi.fn();
    const close = vi.fn(() => {
      if (!busy.value) opened.value = false;
    });
    const read = vi.fn(() => ({ text: 'query first', tokens: [] }));
    wireViewAndSavedViewInteractions({
      savedViewName: name,
      savedViewError: error,
      savedViewQuery: query,
      savedViewQueryTokens: tokens,
      savedViewBusy: busy,
      closeSavedViewDialog: close,
      readInlineSearchField: read,
      updateSavedViewQuery: (value: string) => {
        query.value = value;
        return false;
      },
      focusSavedViewQuery: focusQuery,
    } as unknown as ViewAndSavedViewInteractionsDependencies);
    const inputQuery = handler('input', '[data-token-search-editor="saved-view-query"]');
    const inputName = handler('input', '[name="saved-view-name"]');
    const hide = handler('wa-hide', '[data-component="saved-view-dialog"]');
    inputQuery(new Event('input'), target({}));
    expect(query.value).toBe('query first');
    expect(name.value).toBe('');
    expect(focusQuery).not.toHaveBeenCalled();
    inputName(new Event('input'), Object.assign(target({}), { value: 'Named later' }));
    expect(name.value).toBe('Named later');
    expect(query.value).toBe('query first');
    expect(error.value).toBe('');
    busy.value = true;
    const prevented = new Event('wa-hide', { cancelable: true });
    hide(prevented, target({}));
    expect(prevented.defaultPrevented).toBe(true);
    expect(close).not.toHaveBeenCalled();
    expect(opened.value).toBe(true);
    busy.value = false;
    hide(new Event('wa-hide'), target({}));
    expect(opened.value).toBe(false);
    opened.value = true;
    name.value = '';
    query.value = '';
    read.mockReturnValue({ text: 'refilled query', tokens: [] });
    inputQuery(new Event('input'), target({}));
    expect(query.value).toBe('refilled query');
    expect(name.value).toBe('');
    handler('click', '[data-action="cancel-saved-view"]')(new Event('click'), target({}));
    expect(opened.value).toBe(false);
    expect(close).toHaveBeenCalledTimes(2);
  });
});

describe('shortcut capture ownership transitions (HS2-835BZD)', () => {
  function setup(apple: boolean) {
    const capturingShortcutId = signal<string | undefined>(undefined);
    const keyboardShortcutOverrides = signal<Record<string, ShortcutChord>>({});
    const storage = new Map<string, string>();
    const frames: FrameRequestCallback[] = [];
    const focus = vi.fn();
    vi.stubGlobal('localStorage', {
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    });
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
    vi.stubGlobal('CSS', { escape: (value: string) => value });
    Object.assign(document, { querySelector: vi.fn(() => ({ focus })) });
    wireCommandAndAiInteractions({
      capturingShortcutId,
      keyboardShortcutOverrides,
      appleShortcutPlatform: apple,
      project: () => ({ id: 'project' }),
      setSettingsCategory: vi.fn(),
    } as unknown as CommandAndAiInteractionsDependencies);
    const click = (action: string, dataset: Record<string, string> = {}) =>
      handler('click', `[data-action="${action}"]`)(new Event('click'), target(dataset));
    const press = (id: string, key: string, flags: Partial<KeyboardEvent> = {}) => {
      const event = Object.assign(
        new Event('keydown', { cancelable: true }),
        { key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false },
        flags,
      );
      handler('keydown', '[data-shortcut-capture]')(event, target({ shortcutCapture: id }));
      return event;
    };
    return { capturingShortcutId, keyboardShortcutOverrides, storage, frames, focus, click, press };
  }

  for (const apple of [true, false]) {
    it(`waits for a real key, commits once, cancels, resets and refills on ${apple ? 'Apple' : 'non-Apple'}`, () => {
      const state = setup(apple),
        { click, press } = state;
      click('edit-shortcut', { shortcutId: 'open-search' });
      expect(press('open-search', 'Control', { ctrlKey: true }).defaultPrevented).toBe(true);
      expect(state.capturingShortcutId.value).toBe('open-search');
      expect(state.storage.size).toBe(0);
      press('open-search', 'k', { ctrlKey: true });
      const committed = { key: 'k', mod: !apple, shift: false, alt: false, ...(apple ? { ctrl: true } : {}) };
      expect(state.keyboardShortcutOverrides.value).toEqual({ 'open-search': committed });
      expect(state.capturingShortcutId.value).toBeUndefined();
      expect(JSON.parse(state.storage.get(KEYBOARD_SHORTCUT_STORAGE_KEY)!)).toEqual({ 'open-search': committed });
      // A stale key repeat cannot overwrite the committed binding after recording ended.
      expect(press('open-search', 'x', { repeat: true }).defaultPrevented).toBe(false);
      click('edit-shortcut', { shortcutId: 'open-search' });
      press('open-search', 'Escape', { ctrlKey: true });
      expect(state.keyboardShortcutOverrides.value).toEqual({ 'open-search': committed });
      click('edit-shortcut', { shortcutId: 'undo' });
      click('cancel-shortcut-capture');
      expect(press('undo', 'z', { ctrlKey: true }).defaultPrevented).toBe(false);
      click('edit-shortcut', { shortcutId: 'undo' });
      press('undo', 'j', { ctrlKey: true, metaKey: true, shiftKey: true, altKey: true });
      expect(Object.keys(state.keyboardShortcutOverrides.value)).toEqual(['open-search', 'undo']);
      click('reset-shortcut', { shortcutId: 'undo' });
      expect(state.keyboardShortcutOverrides.value).toEqual({ 'open-search': committed });
      click('edit-shortcut', { shortcutId: 'open-search' });
      click('reset-all-shortcuts');
      expect(state.capturingShortcutId.value).toBeUndefined();
      expect(state.storage.size).toBe(0);
      expect(state.keyboardShortcutOverrides.value).toEqual({});
      click('edit-shortcut', { shortcutId: 'open-search' });
      press('open-search', 'q', { ctrlKey: true });
      expect(state.keyboardShortcutOverrides.value['open-search'].key).toBe('q');
    });
  }

  it('rejects superseded, cancelled and non-editable capture owners', () => {
    const state = setup(true),
      { click, press } = state;
    click('edit-shortcut', { shortcutId: 'open-search' });
    click('edit-shortcut', { shortcutId: 'undo' });
    expect(press('open-search', 'k', { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(state.capturingShortcutId.value).toBe('undo');
    click('select-settings-category', { itemId: 'keyboard' });
    expect(state.capturingShortcutId.value).toBe('undo');
    click('select-settings-category', { itemId: 'appearance' });
    expect(press('undo', 'k', { ctrlKey: true }).defaultPrevented).toBe(false);
    click('edit-shortcut', { shortcutId: 'activate' });
    click('edit-shortcut', { shortcutId: 'unknown' });
    expect(state.capturingShortcutId.value).toBeUndefined();
    expect(state.storage.size).toBe(0);
  });
});
