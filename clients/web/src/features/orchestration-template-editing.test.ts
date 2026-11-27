import { describe, expect, it } from 'vitest';

import { ApiHttpError } from '../api';
import { createOrchestrationTemplateEditing, type TemplateEditingApi } from './orchestration-template-editing';

type Raw = Record<string, unknown>;

const TEMPLATE: Raw = {
  template_id: '01JTEMPLATE00000000000FEAT',
  name: 'Feature in Self',
  state: 'active',
  version: 1,
  inputs: [{ key: 'feature', label: 'Feature', kind: 'text', required: true }],
  goal: { title: '{{feature}} in Self' },
  steps: [{ key: 'build', target: 'self', title: 'Build {{feature}}' }],
};

const refusal = (code: string, status = 409) => new ApiHttpError(`refused: ${code}`, status, code);

/**
 * A fake of the template routes with the server's shapes: list answers templates and revision, validate
 * answers the validator's problems, set compares the revision, retire marks the template retired.
 */
function setup(options: { confirm?: boolean[]; problems?: Raw[] } = {}) {
  const calls: Array<[string, Raw, string]> = [],
    toasts: string[] = [],
    confirms: string[] = [],
    answers = [...(options.confirm ?? [])];
  let templates: Raw[] = [TEMPLATE],
    revision = 'rev-1',
    problems: Raw[] = options.problems ?? [],
    failNext: { error: Error; operation?: string } | undefined;
  const api: TemplateEditingApi = {
    orchestrationTemplates: (_checkout, operation, body, actor) => {
      calls.push([operation, body, actor]);
      if (failNext && (failNext.operation === undefined || failNext.operation === operation)) {
        const { error } = failNext;
        failNext = undefined;
        return Promise.reject(error);
      }
      switch (operation) {
        case 'list':
          return Promise.resolve({ revision, templates, applies: [] });
        case 'validate':
          return Promise.resolve({ valid: problems.length === 0, problems });
        case 'set': {
          if (body.expected_revision !== revision) return Promise.reject(refusal('template_conflict'));
          const incoming = (body.templates as Raw[])[0];
          const id = typeof incoming.template_id === 'string' ? incoming.template_id : '01JNEWTEMPLATE000000000000';
          const known = templates.find((template) => template.template_id === id);
          const saved = { ...incoming, template_id: id, version: Number(known?.version ?? 0) + 1 };
          templates = [...templates.filter((template) => template.template_id !== id), saved];
          revision = `rev-${String(Number(revision.slice(4)) + 1)}`;
          return Promise.resolve({ revision, templates });
        }
        case 'retire': {
          templates = templates.map((template) =>
            template.template_id === body.template_id ? { ...template, state: 'retired' } : template,
          );
          revision = `rev-${String(Number(revision.slice(4)) + 1)}`;
          return Promise.resolve({ ...templates[0] });
        }
        default:
          return Promise.reject(new Error(operation));
      }
    },
  };
  const pauses: Array<{ run: () => void; cancelled: boolean }> = [];
  const editing = createOrchestrationTemplateEditing({
    afterPause: (run) => {
      const pause = { run, cancelled: false };
      pauses.push(pause);
      return () => {
        pause.cancelled = true;
      };
    },
    api: (projectId) => (projectId === 'orch' ? api : undefined),
    personId: () => 'person-1',
    confirm: (request) => {
      confirms.push(request.title);
      return Promise.resolve(answers.shift() ?? true);
    },
    notify: (message) => {
      toasts.push(message);
    },
  });
  return {
    editing,
    pauses,
    calls,
    toasts,
    confirms,
    setProblems: (next: Raw[]) => {
      problems = next;
    },
    changeElsewhere: () => {
      templates = templates.map((template) => ({ ...template, description: 'Changed elsewhere.', version: 2 }));
      revision = 'rev-9';
    },
    failNext: (error: Error, operation?: string) => {
      failNext = { error, operation };
    },
    templates: () => templates,
  };
}

describe('HS2-1XCW6N template editing controller', () => {
  it('reads the list as the person, and a failed read stays inline', async () => {
    const f = setup();
    await f.editing.load('orch');
    expect(f.editing.lists.value.orch).toMatchObject({ loading: false, revision: 'rev-1', templates: [TEMPLATE] });
    expect(f.calls[0]).toEqual(['list', {}, 'person-1']);
    f.failNext(refusal('template_conflict'));
    await f.editing.load('orch');
    expect(f.editing.lists.value.orch?.error).toContain('Could not read the templates');
    // The templates read before stay listed; a project that is not open is a failure too.
    expect(f.editing.lists.value.orch?.templates).toHaveLength(1);
    await f.editing.load('other');
    expect(f.editing.lists.value.other?.error).toContain('This project is not open.');
  });

  it('builds a new template, checks each commit, refuses to save with problems, then saves with the revision', async () => {
    const f = setup({ problems: [{ code: 'name_invalid', at: 'name' }] });
    await f.editing.load('orch');
    f.editing.openNew('orch', 'self');
    const opened = f.editing.editor.value;
    expect(opened).toMatchObject({ part: 'template', revision: 'rev-1', busy: false, conflict: false });
    expect(opened?.draft.steps[0].target).toBe('self');
    // A keystroke edits the draft without a request; the commit checks it.
    f.editing.edit('name', 'Rate', false);
    expect(f.calls.map(([operation]) => operation)).toEqual(['list']);
    f.editing.edit('name', 'Rate limits', true);
    await Promise.resolve();
    await Promise.resolve();
    expect(f.calls.at(-1)?.[0]).toBe('validate');
    expect((f.calls.at(-1)?.[1].template as Raw).name).toBe('Rate limits');
    expect(f.editing.editor.value?.problems).toEqual([{ code: 'name_invalid', at: 'name' }]);
    await f.editing.save();
    expect(f.toasts).toEqual(['Fix 1 problem before saving.']);
    expect(f.editing.editor.value?.failure).toBe('Fix 1 problem before saving.');
    expect(f.calls.some(([operation]) => operation === 'set')).toBe(false);
    // Fixed: the save sends the whole template with the revision it was opened from, then lists again.
    f.setProblems([]);
    f.editing.edit('goal.title', 'Rate limits', false);
    expect(f.editing.editor.value?.failure).toBeUndefined();
    await f.editing.save();
    const set = f.calls.find(([operation]) => operation === 'set');
    expect(set?.[1]).toMatchObject({ expected_revision: 'rev-1', templates: [{ name: 'Rate limits' }] });
    expect(f.toasts.at(-1)).toBe('Saved Rate limits.');
    expect(f.editing.editor.value).toBeUndefined();
    expect(f.editing.lists.value.orch?.revision).toBe('rev-2');
    // Saving again with nothing open does nothing.
    await f.editing.save();
    expect(f.calls.filter(([operation]) => operation === 'set')).toHaveLength(1);
  });

  it('refuses a save over a change elsewhere and reloads the template as it now is', async () => {
    const f = setup();
    await f.editing.load('orch');
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    f.editing.select('step:s2');
    expect(f.editing.editor.value).toMatchObject({ part: 'step:s2', detailActive: true });
    f.editing.back();
    expect(f.editing.editor.value?.detailActive).toBe(false);
    f.editing.edit('name', 'Mine', true);
    f.changeElsewhere();
    await f.editing.save();
    expect(f.editing.editor.value).toMatchObject({ conflict: true, busy: false });
    expect(f.editing.editor.value?.failure).toContain('The templates changed meanwhile.');
    expect(f.toasts.at(-1)).toContain('Could not save Mine');
    await f.editing.reload();
    expect(f.editing.editor.value).toMatchObject({ conflict: false, revision: 'rev-9', part: 'step:s2' });
    expect(f.editing.editor.value?.draft).toMatchObject({ name: 'Feature in Self', description: 'Changed elsewhere.' });
    // Edited again over the new revision, it saves.
    f.editing.edit('name', 'Mine', true);
    await f.editing.save();
    expect(f.calls.filter(([operation]) => operation === 'set').at(-1)?.[1].expected_revision).toBe('rev-9');
    expect(f.templates()[0]).toMatchObject({ name: 'Mine', version: 3 });
  });

  it('keeps a new draft after a conflict, and says when the edited template is gone', async () => {
    const f = setup();
    await f.editing.load('orch');
    f.editing.openNew('orch');
    f.editing.edit('name', 'Fresh', false);
    f.changeElsewhere();
    await f.editing.save();
    expect(f.editing.editor.value?.conflict).toBe(true);
    await f.editing.reload();
    expect(f.editing.editor.value).toMatchObject({ conflict: false, revision: 'rev-9', draft: { name: 'Fresh' } });
    // An edited template removed meanwhile cannot be reloaded.
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    f.editing.editor.value = {
      ...f.editing.editor.value!,
      draft: { ...f.editing.editor.value!.draft, template_id: 'GONE' },
    };
    await f.editing.reload();
    const now = () => f.editing.editor.value;
    expect(now()?.failure).toBe('That template no longer exists.');
    // Reloading with nothing open does nothing.
    f.editing.editor.value = undefined;
    await f.editing.reload();
    expect(f.editing.editor.value).toBeUndefined();
  });

  it('shows the problems of an invalid save and a request the server cannot read', async () => {
    const f = setup();
    await f.editing.load('orch');
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    f.editing.edit('name', 'Changed', false);
    // The check itself fails: the save stops before writing, with the reason in place.
    f.failNext(new Error('network down'), 'validate');
    await f.editing.save();
    expect(f.editing.editor.value).toMatchObject({ busy: false });
    expect(f.editing.editor.value?.failure).toContain('Could not check the template: network down');
    expect(f.calls.some(([operation]) => operation === 'set')).toBe(false);
    // The store refuses the definition (template_invalid): the refusal is shown and the problems read again.
    f.failNext(refusal('template_invalid', 400), 'set');
    f.setProblems([{ code: 'step_cycle', at: 'steps.build' }]);
    const checks = f.calls.filter(([operation]) => operation === 'validate').length;
    // The validator answers no problems for the save's own check, then the cycle once refused.
    f.setProblems([]);
    const saving = f.editing.save();
    await saving;
    expect(f.editing.editor.value?.failure).toContain('The template definition is not valid.');
    expect(f.toasts.at(-1)).toContain('Could not save Changed');
    expect(f.calls.filter(([operation]) => operation === 'validate').length).toBe(checks + 2);
  });

  it('adds, moves and removes parts through the draft and checks after each', async () => {
    const f = setup();
    await f.editing.load('orch');
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    f.editing.addInput();
    expect(f.editing.editor.value?.part).toMatch(/^input:/);
    f.editing.addStep();
    const step = f.editing.editor.value?.draft.steps[1];
    expect(step?.after).toEqual([{ step: 'build', gates: '' }]);
    f.editing.addPrerequisite(step!.id);
    f.editing.removePrerequisite(step!.id, 0);
    f.editing.move(`step:${step!.id}`, -1);
    expect(f.editing.editor.value?.draft.steps.map((item) => item.key)).toEqual(['step-2', 'build']);
    f.editing.removePart(`step:${step!.id}`);
    expect(f.editing.editor.value?.draft.steps.map((item) => item.key)).toEqual(['build']);
    f.editing.removePart('input:i3');
    f.editing.removePart('goal');
    expect(f.editing.editor.value?.draft.inputs.map((input) => input.key)).toEqual(['feature']);
    // Selecting a part that does not exist changes nothing; a removed part falls back to the template.
    f.editing.select('step:zz');
    expect(f.editing.editor.value?.part).toBe('input:i1');
    await Promise.resolve();
    expect(f.calls.filter(([operation]) => operation === 'validate').length).toBeGreaterThanOrEqual(7);
  });

  it('asks before discarding changes, and a retired template is read-only', async () => {
    const f = setup({ confirm: [false, true] });
    await f.editing.load('orch');
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    // Unchanged: closes without asking.
    await f.editing.close();
    expect(f.editing.editor.value).toBeUndefined();
    expect(f.confirms).toEqual([]);
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    const session = f.editing.editor.value?.session ?? 0;
    f.editing.edit('name', 'Changed', false);
    await f.editing.close();
    expect(f.confirms).toEqual(['Discard your changes?']);
    expect(f.editing.editor.value?.session).toBe(session);
    await f.editing.close();
    expect(f.editing.editor.value).toBeUndefined();
    // Retire asks; declined, nothing is sent.
    const retiring = setup({ confirm: [false, true] });
    await retiring.editing.load('orch');
    await retiring.editing.retire('orch', String(TEMPLATE.template_id));
    expect(retiring.calls.some(([operation]) => operation === 'retire')).toBe(false);
    retiring.editing.openEdit('orch', String(TEMPLATE.template_id));
    await retiring.editing.retire('orch', String(TEMPLATE.template_id));
    expect(retiring.confirms).toEqual(['Retire Feature in Self?', 'Retire Feature in Self?']);
    expect(retiring.toasts).toEqual(['Retired Feature in Self.']);
    // The editor that showed it closes; the list shows it retired and never busy.
    expect(retiring.editing.editor.value).toBeUndefined();
    expect(retiring.editing.lists.value.orch).toMatchObject({ busy: [], templates: [{ state: 'retired' }] });
    // Retiring again, or an unknown template, does nothing.
    await retiring.editing.retire('orch', String(TEMPLATE.template_id));
    await retiring.editing.retire('orch', 'unknown');
    expect(retiring.confirms).toHaveLength(2);
    // Opened again it is read-only: edits, structure changes and save do nothing, and close never asks.
    retiring.editing.openEdit('orch', String(TEMPLATE.template_id));
    const before = retiring.editing.editor.value?.draft;
    retiring.editing.edit('name', 'Nope', true);
    retiring.editing.addStep();
    await retiring.editing.save();
    expect(retiring.editing.editor.value?.draft).toBe(before);
    await retiring.editing.close();
    expect(retiring.editing.editor.value).toBeUndefined();
    expect(retiring.confirms).toHaveLength(2);
  });

  it('checks the draft after a typing pause; a later keystroke, a commit or a new session cancels it', async () => {
    const f = setup();
    await f.editing.load('orch');
    f.editing.openEdit('orch', String(TEMPLATE.template_id));
    const checks = () => f.calls.filter(([operation]) => operation === 'validate').length;
    f.editing.edit('name', 'A', false);
    f.editing.edit('name', 'AB', false);
    expect(f.pauses.map((pause) => pause.cancelled)).toEqual([true, false]);
    expect(checks()).toBe(0);
    f.pauses[1].run();
    await Promise.resolve();
    expect(checks()).toBe(1);
    expect((f.calls.at(-1)?.[1].template as Raw).name).toBe('AB');
    // A commit replaces the pending pause with an immediate check.
    f.editing.edit('name', 'ABC', false);
    f.editing.edit('name', 'ABC', true);
    expect(f.pauses[2].cancelled).toBe(true);
    await Promise.resolve();
    expect(checks()).toBe(2);
    // An unknown field changes nothing, so nothing is scheduled.
    f.editing.edit('nope', 'x', false);
    expect(f.pauses).toHaveLength(3);
    // A pause that outlives its session checks nothing.
    f.editing.edit('name', 'ABCD', false);
    f.editing.openNew('orch');
    expect(f.pauses[3].cancelled).toBe(true);
    f.pauses[3].run();
    await Promise.resolve();
    expect(checks()).toBe(2);
  });

  it('toasts a refused retire and keeps the row usable', async () => {
    const f = setup();
    await f.editing.load('orch');
    f.failNext(refusal('template_not_found', 404));
    await f.editing.retire('orch', String(TEMPLATE.template_id));
    expect(f.toasts.at(-1)).toContain('Could not retire Feature in Self');
    expect(f.editing.lists.value.orch?.busy).toEqual([]);
  });
});
