import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { type CommandAndAiInteractionsDependencies, wireCommandAndAiInteractions } from './commands-and-ai';
import { type ProjectLifecycleInteractionsDependencies, wireProjectLifecycleInteractions } from './project-lifecycle';
import { type RepositoryInteractionsDependencies, wireRepositoryInteractions } from './repository';

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
});
