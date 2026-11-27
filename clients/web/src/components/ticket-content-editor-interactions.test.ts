import { describe, expect, it, vi } from 'vitest';

import { wireMarkdownDetailsEditor, wireNoteComposer, wireNoteEditor } from './ticket-content-editor-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
interface Entry {
  click: boolean;
  key: boolean;
  repeat: boolean;
  press?: unknown;
  shifted?: unknown;
  presses: unknown[];
}
const entry = vi.hoisted((): Entry => ({ click: true, key: true, repeat: false, presses: [] }));
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
  pressOnEditSurface: () => entry.press,
  clickCompletingShiftedPress: (_event: unknown, press: unknown) => {
    entry.presses.push(press);
    return entry.shifted;
  },
}));
const handler = (event: string, selector: string) =>
  [...registrations].reverse().find((item) => item.event === event && item.selector.includes(selector))!.handle;
const cancelable = (extra: Record<string, unknown> = {}) => ({ preventDefault: vi.fn(), ...extra });

describe('ticket content editor wiring (HS2-KM3DNM)', () => {
  it('routes the note composer open, draft, cancel, and submit', () => {
    const bindings = { open: vi.fn(), setDraft: vi.fn(), cancel: vi.fn(), submit: vi.fn() };
    wireNoteComposer({} as HTMLElement, bindings);
    const button = { id: 'button' },
      field = { value: 'Draft' },
      cancel = { id: 'cancel' },
      form = { id: 'form' };
    handler('click', 'add-ticket-note')({}, button);
    handler('input', 'new-note-body')({}, field);
    handler('click', 'cancel-new-note')({}, cancel);
    const submit = cancelable();
    handler('submit', 'create-note-form')(submit, form);
    // Every gesture names its composer element so a linked reader frame owns its own composer (HS2-ANRXAE).
    expect(bindings.open).toHaveBeenCalledWith(button);
    expect(bindings.setDraft).toHaveBeenCalledWith('Draft', field);
    expect(bindings.cancel).toHaveBeenCalledWith(cancel);
    expect(submit.preventDefault).toHaveBeenCalled();
    expect(bindings.submit).toHaveBeenCalledWith(form);
  });

  it('finishes a note composer only when focus leaves the composer', () => {
    vi.stubGlobal('Node', function Node() {});
    const finish = vi.fn();
    wireNoteComposer({} as HTMLElement, { open: vi.fn(), setDraft: vi.fn(), cancel: vi.fn(), submit: vi.fn(), finish });
    const inside = new (globalThis as unknown as { Node: new () => object }).Node(),
      outside = new (globalThis as unknown as { Node: new () => object }).Node(),
      field = { closest: () => ({ contains: (node: unknown) => node === inside }) };
    handler('focusout', 'new-note-body')({ relatedTarget: inside }, field);
    handler('focusout', 'new-note-body')({ relatedTarget: outside }, field);
    expect(finish).toHaveBeenCalledExactlyOnceWith(field);
    vi.unstubAllGlobals();
  });

  it('keeps an unsent note as a draft when focus moves to Minimize (HS2-Q8WF64)', () => {
    vi.stubGlobal('Node', function Node() {});
    const finish = vi.fn();
    wireNoteComposer({} as HTMLElement, { open: vi.fn(), setDraft: vi.fn(), cancel: vi.fn(), submit: vi.fn(), finish });
    const NodeType = (globalThis as unknown as { Node: new () => object }).Node,
      minimize = Object.assign(new NodeType(), {
        closest: (selector: string) => (selector === '[data-action="minimize-ticket-reader"]' ? {} : null),
      }),
      elsewhere = Object.assign(new NodeType(), { closest: () => null }),
      field = { closest: () => ({ contains: () => false }) };
    handler('focusout', 'new-note-body')({ relatedTarget: minimize }, field);
    expect(finish).not.toHaveBeenCalled();
    handler('focusout', 'new-note-body')({ relatedTarget: elsewhere }, field);
    expect(finish).toHaveBeenCalledExactlyOnceWith(field);
    vi.unstubAllGlobals();
  });

  it('begins note edits only when the click or key qualifies and the note has an id', () => {
    const bindings = { begin: vi.fn(), beginPrompt: vi.fn(), input: vi.fn(), finish: vi.fn(), save: vi.fn() };
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

  it('opens a FEEDBACK NEEDED prompt only from its Edit Note action or a body double-click (HS2-ZXGEFE)', () => {
    const bindings = { begin: vi.fn(), beginPrompt: vi.fn(), input: vi.fn(), finish: vi.fn(), save: vi.fn() };
    wireNoteEditor({} as HTMLElement, bindings);
    const button = { closest: () => ({ dataset: { noteId: 'p1' } }) },
      body = { closest: () => ({ dataset: { noteId: 'p1' } }) },
      orphan = { closest: () => null },
      onLink = { target: { closest: (selector: string) => (selector.includes('a') ? {} : null) } },
      onText = { target: { closest: () => null } };
    handler('click', 'edit-feedback-prompt')({}, button);
    handler('click', 'edit-feedback-prompt')({}, orphan);
    handler('dblclick', 'data-feedback-prompt-body')(onText, body);
    // A double-click on a link inside the prompt keeps the link's own action.
    handler('dblclick', 'data-feedback-prompt-body')(onLink, body);
    expect(bindings.beginPrompt.mock.calls).toEqual([
      ['p1', button],
      ['p1', body],
    ]);
    expect(bindings.begin).not.toHaveBeenCalled();
  });

  it('drafts notes and replies, finishes only note edits on focus-leave, and saves', () => {
    const bindings = { begin: vi.fn(), beginPrompt: vi.fn(), input: vi.fn(), finish: vi.fn(), save: vi.fn() };
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
    const toggle = cancelable();
    handler('mousedown', 'toggle-markdown-expanded')(toggle, {});
    // WebKit never focuses a pressed button: the press keeps the source focused (HS2-628K80).
    expect(toggle.preventDefault).toHaveBeenCalledTimes(1);
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

  it('completes a press whose click a re-flow retargeted, for notes and Details (HS2-628K80)', () => {
    const note = { begin: vi.fn(), beginPrompt: vi.fn(), input: vi.fn(), finish: vi.fn(), save: vi.fn() };
    wireNoteEditor({} as HTMLElement, note);
    const noteBody = { closest: () => ({ dataset: { noteId: 'n1' } }) },
      pressNote = handler('capture:mousedown', '*'),
      clickNote = handler('click', '*');
    entry.presses.length = 0;
    // The click consumes the recorded press, so a later click never reuses it.
    entry.press = { surface: noteBody };
    pressNote({}, {});
    entry.shifted = noteBody;
    clickNote({}, {});
    entry.shifted = undefined;
    clickNote({}, {});
    expect(entry.presses).toEqual([{ surface: noteBody }, undefined]);
    expect(note.begin.mock.calls).toEqual([['n1', noteBody]]);

    const details = { begin: vi.fn(), input: vi.fn(), focusLeave: vi.fn() };
    wireMarkdownDetailsEditor({} as HTMLElement, details);
    const shiftedRegistrations = registrations.filter((item) => item.selector === '*'),
      [pressDetails, clickDetails] = shiftedRegistrations.slice(-3, -1).map((item) => item.handle),
      preview = { id: 'preview' };
    entry.press = { surface: preview };
    pressDetails({}, {});
    entry.shifted = preview;
    clickDetails({}, {});
    entry.shifted = undefined;
    entry.press = undefined;
    expect(details.begin).toHaveBeenCalledExactlyOnceWith(preview);
  });

  it('leaves the expand toggle unwired when the host hides it', () => {
    const before = registrations.filter((item) => item.selector.includes('toggle-markdown-expanded')).length;
    wireMarkdownDetailsEditor({} as HTMLElement, { begin: vi.fn(), input: vi.fn(), focusLeave: vi.fn() });
    expect(registrations.filter((item) => item.selector.includes('toggle-markdown-expanded'))).toHaveLength(before);
  });
});
