import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { type CommandSettingsEditorBindings, wireCommandSettingsEditor } from './command-settings-editor-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => {
  const register = (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  };
  return { ...(await original<typeof import('kerfjs')>()), delegate: register, delegateCapture: register };
});
function handler(event: string, selector: string) {
  const found = registrations.find((item) => item.event === event && item.selector === selector);
  expect(found, `${event} ${selector}`).toBeDefined();
  return found!.handle;
}
const rowSelector = '.command-settings-editor__row';
/** A command row: answers `closest` for itself and nothing else. */
function row(id: string, top = 0) {
  const element = {
    dataset: { commandId: id } as Record<string, string>,
    closest: (selector: string) => (selector === '[data-command-id]' ? element : null),
    getBoundingClientRect: () => ({ top, height: 20 }),
  };
  return element as unknown as HTMLElement;
}
const rows = ['a', 'b', 'c'].map((id, index) => row(id, index * 20));
const root = { querySelectorAll: () => rows } as unknown as HTMLElement;
const plainEvent = (init: Record<string, unknown> = {}) =>
  ({ preventDefault: vi.fn(), target: { closest: () => null }, ...init }) as unknown as Event;

function bindings(selected: string[] = []): CommandSettingsEditorBindings & Record<string, ReturnType<typeof vi.fn>> {
  return {
    ownsCommand: (id: string) => id !== 'stale',
    openEditor: vi.fn(),
    closeEditor: vi.fn(),
    onEditorClosed: vi.fn(),
    addCommand: vi.fn(),
    deleteCommand: vi.fn(),
    addGroup: vi.fn(),
    deleteGroup: vi.fn(),
    selection: () => selected,
    orderedIds: () => ['a', 'b', 'c'],
    selectRow: vi.fn(),
    selectOnly: vi.fn(),
    reorder: vi.fn(),
    updateField: vi.fn(),
    setIconSearch: vi.fn(),
    chooseAi: vi.fn(),
    openManualModel: vi.fn(),
  } as unknown as CommandSettingsEditorBindings & Record<string, ReturnType<typeof vi.fn>>;
}

beforeEach(() => {
  registrations.length = 0;
  for (const item of rows) delete item.dataset.dragging;
});
afterEach(() => vi.unstubAllGlobals());

describe('wireCommandSettingsEditor (HS2-RWY78R)', () => {
  it('ignores an Edit click for a command the current list does not own', () => {
    const state = bindings();
    wireCommandSettingsEditor(root, state);
    const edit = handler('click', '[data-action="edit-command-setting"]');
    edit(plainEvent(), row('stale'));
    expect(state.openEditor).not.toHaveBeenCalled();
    edit(plainEvent(), row('b'));
    expect(state.openEditor).toHaveBeenCalledWith('b');
  });

  it('drags the ordered multi-selection onto a row, then forgets it after the drop', () => {
    const state = bindings(['c', 'a']);
    wireCommandSettingsEditor(root, state);
    const transfer = { setData: vi.fn(), effectAllowed: '' };
    handler('dragstart', rowSelector)(plainEvent({ dataTransfer: transfer }), rows[0]);
    expect(transfer.setData).toHaveBeenCalledWith('text/plain', 'a,c');
    expect(rows[0].dataset.dragging).toBe('true');
    expect(state.selectOnly).not.toHaveBeenCalled();
    const drop = handler('drop', '.command-settings-editor__list');
    drop(plainEvent({ clientY: 25, target: rows[1] }), rows[1]);
    expect(state.reorder).toHaveBeenCalledWith(['a', 'c'], { kind: 'row', id: 'b', position: 'before' });
    drop(plainEvent({ clientY: 25, target: rows[1] }), rows[1]);
    expect(state.reorder).toHaveBeenCalledOnce();
  });

  it('selects a lone dragged row and ignores a drop on a dragged row', () => {
    const state = bindings();
    wireCommandSettingsEditor(root, state);
    handler('dragstart', rowSelector)(plainEvent(), rows[2]);
    expect(state.selectOnly).toHaveBeenCalledWith('c');
    handler('drop', '.command-settings-editor__list')(plainEvent({ clientY: 45, target: rows[2] }), rows[2]);
    expect(state.reorder).not.toHaveBeenCalled();
  });

  it('routes AI menu choices with their values', () => {
    const state = bindings();
    wireCommandSettingsEditor(root, state);
    const choice = (value?: string) =>
      ({ dataset: value ? { value } : {}, closest: () => ({ dataset: { commandId: 'b' } }) }) as unknown as Element;
    handler('click', '[data-action="select-command-ai-default"]')(plainEvent(), choice());
    handler('click', '[data-action="select-command-ai-model"]')(plainEvent(), choice('gpt-5.6'));
    handler('click', '[data-action="select-command-ai-effort"]')(plainEvent(), choice());
    expect(vi.mocked(state.chooseAi).mock.calls).toEqual([
      ['b', { kind: 'default' }],
      ['b', { kind: 'model', value: 'gpt-5.6' }],
    ]);
  });
});
