import { describe, expect, it, vi } from 'vitest';

import { wireMarkdownDetailsEditor, wireNoteComposer, wireNoteEditor } from './ticket-content-editor-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
const entry = vi.hoisted(() => ({ click: true, key: true, repeat: false }));
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
  delegateCapture: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event: `capture:${event}`, selector, handle });
    return () => undefined;
  },
}));
vi.mock('../markdown-click-edit', () => ({
  clickBeginsMarkdownEdit: () => entry.click,
  keyBeginsMarkdownEdit: () => entry.key,
  repeatPressWouldLeaveNewEditor: () => entry.repeat,
}));
const handler = (event: string, selector: string) =>
  [...registrations].reverse().find((item) => item.event === event && item.selector.includes(selector))!.handle;
const cancelable = (extra: Record<string, unknown> = {}) => ({ preventDefault: vi.fn(), ...extra });

describe('ticket content editor wiring (HS2-KM3DNM)', () => {
  it('routes the note composer open, draft, cancel, and submit', () => {
    const bindings = { open: vi.fn(), setDraft: vi.fn(), cancel: vi.fn(), submit: vi.fn() };
    wireNoteComposer({} as HTMLElement, bindings);
    handler('click', 'add-ticket-note')({}, {});
    handler('input', 'new-note-body')({}, { value: 'Draft' });
    handler('click', 'cancel-new-note')({}, {});
    const submit = cancelable();
    handler('submit', 'create-note-form')(submit, {});
    expect(bindings.open).toHaveBeenCalledTimes(1);
    expect(bindings.setDraft).toHaveBeenCalledWith('Draft');
    expect(bindings.cancel).toHaveBeenCalledTimes(1);
    expect(submit.preventDefault).toHaveBeenCalled();
    expect(bindings.submit).toHaveBeenCalledTimes(1);
  });

  it('begins note edits only when the click or key qualifies and the note has an id', () => {
    const bindings = { begin: vi.fn(), input: vi.fn(), finish: vi.fn(), save: vi.fn() };
    wireNoteEditor({} as HTMLElement, bindings);
    const body = { closest: () => ({ dataset: { noteId: 'n1' } }) },
      orphan = { closest: () => null };
    handler('click', 'data-edit-on-click')({}, body);
    handler('click', 'data-edit-on-click')({}, orphan);
    entry.click = false;
    handler('click', 'data-edit-on-click')({}, body);
    entry.click = true;
    const key = cancelable();
    handler('keydown', 'data-edit-on-click')(key, body);
    entry.key = false;
    const ignored = cancelable();
    handler('keydown', 'data-edit-on-click')(ignored, body);
    entry.key = true;
    expect(bindings.begin.mock.calls).toEqual([
      ['n1', body],
      ['n1', body],
    ]);
    expect(key.preventDefault).toHaveBeenCalled();
    expect(ignored.preventDefault).not.toHaveBeenCalled();
  });

  it('drafts notes and replies, finishes only note edits on focus-leave, and saves', () => {
    const bindings = { begin: vi.fn(), input: vi.fn(), finish: vi.fn(), save: vi.fn() };
    wireNoteEditor({} as HTMLElement, bindings);
    const edit = { dataset: { noteId: 'n1' } },
      reply = { dataset: { noteId: 'n2', noteResponse: 'true' } };
    handler('input', 'note-body')({}, edit);
    handler('input', 'note-body')({}, reply);
    handler('focusout', 'note-body')({}, edit);
    handler('focusout', 'note-body')({}, reply);
    handler('click', 'save-note-edit')({}, reply);
    expect(bindings.input.mock.calls).toEqual([
      [edit, 'n1', false],
      [reply, 'n2', true],
    ]);
    expect(bindings.finish.mock.calls).toEqual([[edit]]);
    expect(bindings.save).toHaveBeenCalledWith(reply);
  });

  it('enters, drafts, and leaves the Markdown editor, distinguishing moves inside the editor', () => {
    vi.stubGlobal('document', { activeElement: null });
    vi.stubGlobal('Node', function Node() {});
    const bindings = { begin: vi.fn(), input: vi.fn(), focusLeave: vi.fn(), toggleExpanded: vi.fn() };
    wireMarkdownDetailsEditor({} as HTMLElement, bindings);
    const press = cancelable();
    handler('capture:mousedown', '*')(press, {});
    entry.repeat = true;
    handler('capture:mousedown', '*')(press, {});
    entry.repeat = false;
    expect(press.preventDefault).toHaveBeenCalledTimes(1);
    const preview = {};
    handler('click', 'edit-markdown')({}, preview);
    handler('keydown', 'edit-markdown')(cancelable(), preview);
    expect(bindings.begin.mock.calls).toEqual([[preview], [preview]]);
    const field = {
      closest: () => ({ contains: (node: unknown) => node === inside }),
    };
    const inside = new (globalThis as unknown as { Node: new () => object }).Node(),
      outside = new (globalThis as unknown as { Node: new () => object }).Node();
    handler('input', 'markdown-source')({}, field);
    handler('focusout', 'markdown-source')({ relatedTarget: inside }, field);
    handler('focusout', 'markdown-source')({ relatedTarget: outside }, field);
    handler('focusout', 'markdown-source')({ relatedTarget: null }, field);
    handler('click', 'toggle-markdown-expanded')({}, {});
    expect(bindings.input).toHaveBeenCalledWith(field);
    expect(bindings.focusLeave.mock.calls).toEqual([
      [field, inside, true],
      [field, outside, false],
      [field, null, false],
    ]);
    expect(bindings.toggleExpanded).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });

  it('leaves the expand toggle unwired when the host hides it', () => {
    const before = registrations.filter((item) => item.selector.includes('toggle-markdown-expanded')).length;
    wireMarkdownDetailsEditor({} as HTMLElement, { begin: vi.fn(), input: vi.fn(), focusLeave: vi.fn() });
    expect(registrations.filter((item) => item.selector.includes('toggle-markdown-expanded'))).toHaveLength(before);
  });
});
