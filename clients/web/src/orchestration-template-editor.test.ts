import { describe, expect, it } from 'vitest';

import {
  addAfter,
  addInput,
  addStep,
  draftChanged,
  draftFromTemplate,
  editDraft,
  fieldError,
  INPUT_KINDS,
  inputReferences,
  kindLabel,
  localProblems,
  movePart,
  newTemplateDraft,
  partExists,
  partLabel,
  partProblemText,
  placeProblems,
  problemWords,
  removeAfter,
  removeInput,
  removeStep,
  shownFields,
  templateFromDraft,
  templateSummary,
} from './orchestration-template-editor';

/** A checked-in template with every field the editor edits (synthetic). */
const CHECKED_IN = {
  template_id: '01JTEMPLATE00000000000FEAT',
  name: 'Feature across API and Web',
  description: 'Build it, then show it.',
  state: 'active',
  version: 3,
  inputs: [
    { key: 'feature', label: 'Feature', kind: 'text', required: true, max_len: 80, hint: 'One line' },
    { key: 'count', label: 'Count', kind: 'number', required: false, default: 2 },
    { key: 'contract', label: 'Contract Check', kind: 'boolean', required: false, default: true },
    { key: 'size', label: 'Size', kind: 'choice', required: true, choices: ['small', 'large'], default: 'small' },
    { key: 'api', label: 'API Project', kind: 'target', required: true, default: 'api' },
  ],
  goal: { title: 'Rate limits: {{feature}}', details: 'Why it matters', tags: ['limits', 'api'] },
  steps: [
    {
      key: 'api',
      target: '{{api}}',
      title: 'Add {{feature}} headers',
      details: 'Headers first',
      category: 'feature',
      priority: 'high',
      validation: ['contract-check', 'smoke'],
      tool: 'claude',
    },
    {
      key: 'web',
      target: 'web',
      title: 'Show {{feature}} errors',
      ticket: 'create',
      after: [{ step: 'api', gates: 'ci' }],
      when: '{{contract}}',
    },
  ],
  instructions: 'Keep the change small.',
  created_at_ms: 1,
  updated_at_ms: 2,
};

describe('HS2-1XCW6N template editor model', () => {
  it('reads a checked-in template into a draft and writes it back unchanged', () => {
    const draft = draftFromTemplate(CHECKED_IN);
    expect(draft.inputs.map((input) => input.id)).toEqual(['i1', 'i2', 'i3', 'i4', 'i5']);
    expect(draft.steps.map((step) => step.id)).toEqual(['s6', 's7']);
    expect(draft.next).toBe(8);
    expect(draft.inputs[2].default).toBe('true');
    expect(draft.inputs[1].default).toBe('2');
    expect(draft.inputs[3].choices).toBe('small\nlarge');
    expect(draft.steps[0].validation).toBe('contract-check, smoke');
    const wire = templateFromDraft(draft);
    const definition: Record<string, unknown> = { ...CHECKED_IN };
    delete definition.created_at_ms;
    delete definition.updated_at_ms;
    expect(wire).toEqual({
      ...definition,
      inputs: [
        { key: 'feature', label: 'Feature', kind: 'text', required: true, max_len: 80, hint: 'One line' },
        { key: 'count', label: 'Count', kind: 'number', required: false, default: 2 },
        { key: 'contract', label: 'Contract Check', kind: 'boolean', required: false, default: true },
        { key: 'size', label: 'Size', kind: 'choice', required: true, default: 'small', choices: ['small', 'large'] },
        { key: 'api', label: 'API Project', kind: 'target', required: true, default: 'api' },
      ],
      version: undefined,
    });
    expect(draftChanged(draft, draftFromTemplate(CHECKED_IN))).toBe(false);
    // Unknown or missing shapes read as empty, never throw.
    const empty = draftFromTemplate(null);
    expect(empty).toMatchObject({ template_id: '', name: '', inputs: [], steps: [], state: 'active', version: 0 });
    expect(draftFromTemplate({ inputs: [{ kind: 'mystery' }], state: 'retired' })).toMatchObject({
      state: 'retired',
      inputs: [{ kind: 'text', key: '' }],
    });
  });

  it('leaves out empty optional fields and keeps a typed number default that does not parse as typed', () => {
    let draft = newTemplateDraft('api');
    expect(templateFromDraft(draft)).toEqual({
      name: '',
      state: 'active',
      inputs: [],
      goal: { title: '' },
      steps: [{ key: 'step-1', target: 'api', title: '' }],
    });
    draft = addInput(draft).draft;
    draft = editDraft(draft, `input.${draft.inputs[0].id}.kind`, 'number');
    draft = editDraft(draft, `input.${draft.inputs[0].id}.default`, 'twelve');
    draft = editDraft(draft, `input.${draft.inputs[0].id}.maxLen`, '40');
    const input = (templateFromDraft(draft).inputs as Array<Record<string, unknown>>)[0];
    // A longest value belongs to a text input only; a number that does not parse is left for the validator.
    expect(input).toEqual({ key: 'input-1', label: '', kind: 'number', required: true, default: 'twelve' });
    expect(localProblems(draft)).toEqual([{ code: 'input_max_len_invalid', at: 'inputs[0].max_len' }]);
    draft = editDraft(draft, `input.${draft.inputs[0].id}.kind`, 'text');
    expect(localProblems(draft)).toEqual([]);
    draft = editDraft(draft, `input.${draft.inputs[0].id}.maxLen`, '0');
    expect(localProblems(draft)).toHaveLength(1);
    draft = editDraft(draft, `input.${draft.inputs[0].id}.maxLen`, '');
    expect(localProblems(draft)).toEqual([]);
  });

  it('edits each field by its control name and ignores unknown ones', () => {
    const start = draftFromTemplate(CHECKED_IN);
    let draft = start;
    for (const [field, value] of [
      ['name', 'Renamed'],
      ['description', 'New description'],
      ['instructions', 'New instructions'],
      ['goal.title', 'New goal'],
      ['goal.details', 'New details'],
      ['goal.tags', 'a, b c'],
    ] as const)
      draft = editDraft(draft, field, value);
    expect(templateFromDraft(draft)).toMatchObject({
      name: 'Renamed',
      description: 'New description',
      instructions: 'New instructions',
      goal: { title: 'New goal', details: 'New details', tags: ['a', 'b', 'c'] },
    });
    draft = editDraft(draft, 'input.i1.required', false);
    draft = editDraft(draft, 'input.i1.hint', '');
    draft = editDraft(draft, 'step.s7.ticket', 'search');
    draft = editDraft(draft, 'step.s7.after.0.gates', 'deployment');
    draft = editDraft(draft, 'step.s6.validation', '');
    const wire = templateFromDraft(draft) as {
      inputs: Array<Record<string, unknown>>;
      steps: Array<Record<string, unknown>>;
    };
    expect(wire.inputs[0]).toMatchObject({ required: false });
    expect(wire.inputs[0]).not.toHaveProperty('hint');
    expect(wire.steps[1]).not.toHaveProperty('ticket');
    expect(wire.steps[1].after).toEqual([{ step: 'api', gates: 'deployment' }]);
    expect(wire.steps[0]).not.toHaveProperty('validation');
    // A changed kind starts its default over; the same kind keeps it.
    expect(editDraft(start, 'input.i4.kind', 'choice').inputs[3].default).toBe('small');
    expect(editDraft(start, 'input.i4.kind', 'text').inputs[3].default).toBe('');
    // Unknown fields, parts and indexes change nothing.
    for (const field of [
      'nope',
      'input.zz.label',
      'input.i1.nope',
      'step.zz.title',
      'step.s6.nope',
      'step.s7.after.5.step',
      'step.s7.after.0.nope',
      'other.x.y',
    ])
      expect(editDraft(start, field, 'x')).toBe(start);
    expect(draftChanged(draft, start)).toBe(true);
  });

  it('follows a renamed input key and step key into whole references and prerequisites', () => {
    let draft = draftFromTemplate(CHECKED_IN);
    draft = editDraft(draft, 'input.i5.key', 'backend');
    expect(draft.steps[0].target).toBe('{{backend}}');
    draft = editDraft(draft, 'input.i3.key', 'contract_check');
    expect(draft.steps[1].when).toBe('{{contract_check}}');
    draft = editDraft(draft, 'step.s6.key', 'headers');
    expect(draft.steps[1].after).toEqual([{ step: 'headers', gates: 'ci' }]);
    // A placeholder inside text is left for the validator to name.
    expect(draft.steps[0].title).toBe('Add {{feature}} headers');
    // Renaming a key that was empty references nothing.
    const fresh = addInput(newTemplateDraft()).draft;
    const emptied = editDraft(fresh, `input.${fresh.inputs[0].id}.key`, '');
    expect(editDraft(emptied, `input.${fresh.inputs[0].id}.key`, 'x').steps).toEqual(emptied.steps);
  });

  it('adds, removes and moves inputs and steps, keeping references sound (empty, then refilled)', () => {
    let draft = newTemplateDraft('api');
    const first = addInput(draft);
    expect(first.part).toBe(`input:${first.draft.inputs[0].id}`);
    draft = addInput(first.draft).draft;
    expect(draft.inputs.map((input) => input.key)).toEqual(['input-1', 'input-2']);
    draft = editDraft(draft, `input.${draft.inputs[0].id}.kind`, 'target');
    draft = editDraft(draft, `step.${draft.steps[0].id}.target`, '{{input-1}}');
    const second = addStep(draft);
    draft = second.draft;
    expect(draft.steps[1]).toMatchObject({
      key: 'step-2',
      target: '{{input-1}}',
      after: [{ step: 'step-1', gates: '' }],
    });
    expect(second.part).toBe(`step:${draft.steps[1].id}`);
    // Removing the input clears what named it; the neighbour is shown next.
    const removed = removeInput(draft, draft.inputs[0].id);
    expect(removed.draft.steps.map((step) => step.target)).toEqual(['', '']);
    expect(removed.part).toBe(`input:${draft.inputs[1].id}`);
    expect(removeInput(removed.draft, draft.inputs[1].id).part).toBe('template');
    expect(removeInput(draft, 'missing')).toEqual({ draft, part: 'template' });
    // Moving: up and down, nothing at the ends.
    const moved = movePart(draft, `step:${draft.steps[1].id}`, -1);
    expect(moved.steps.map((step) => step.key)).toEqual(['step-2', 'step-1']);
    expect(movePart(moved, `step:${draft.steps[1].id}`, -1)).toEqual(moved);
    expect(movePart(draft, `input:${draft.inputs[0].id}`, 1).inputs.map((input) => input.key)).toEqual([
      'input-2',
      'input-1',
    ]);
    expect(movePart(draft, 'goal', 1)).toBe(draft);
    // Prerequisites: add the nearest earlier step, then any other, then none is left but an empty one.
    let steps = addStep(addStep(newTemplateDraft()).draft).draft;
    const third = steps.steps[2].id;
    steps = removeAfter(steps, third, 0);
    steps = addAfter(steps, third);
    expect(steps.steps[2].after).toEqual([{ step: 'step-2', gates: '' }]);
    steps = addAfter(steps, third);
    expect(steps.steps[2].after.map((after) => after.step)).toEqual(['step-2', 'step-1']);
    steps = addAfter(steps, third);
    expect(steps.steps[2].after.at(-1)).toEqual({ step: '', gates: '' });
    expect(addAfter(steps, 'missing')).toBe(steps);
    // Removing a step drops the prerequisites on it; the last step removed shows the goal.
    const without = removeStep(steps, steps.steps[1].id);
    expect(without.draft.steps[1].after.map((after) => after.step)).toEqual(['step-1', '']);
    expect(without.part).toBe(`step:${steps.steps[2].id}`);
    const only = newTemplateDraft();
    expect(removeStep(only, only.steps[0].id)).toMatchObject({ part: 'goal', draft: { steps: [] } });
    expect(removeStep(only, 'missing').part).toBe('template');
    // A free key skips one already taken.
    const taken = editDraft(newTemplateDraft(), 'step.s1.key', 'step-2');
    expect(addStep(taken).draft.steps[1].key).toBe('step-3');
  });

  it('places the validator problems on parts and fields by stable id, and words them', () => {
    const draft = draftFromTemplate(CHECKED_IN);
    const placed = placeProblems(draft, [
      { code: 'name_invalid', at: 'name' },
      { code: 'placeholder_unknown', at: 'goal.title' },
      { code: 'input_duplicate', at: 'inputs[1].key' },
      { code: 'input_max_len_invalid', at: 'inputs[0].max_len' },
      { code: 'input_choices_missing', at: 'inputs[3]' },
      { code: 'step_after_unknown', at: 'steps[1].after[0]' },
      { code: 'step_gates_invalid', at: 'steps[1].after[0].gates' },
      { code: 'target_unknown', at: 'steps[1].target' },
      { code: 'step_cycle', at: 'steps.web' },
      { code: 'step_cycle', at: 'steps.gone' },
      { code: 'too_many_steps', at: 'steps' },
      { code: 'input_key_invalid', at: 'inputs[9].key' },
      { code: 'mystery_code', at: 'somewhere' },
    ]);
    expect(placed.map((problem) => [problem.part, problem.field])).toEqual([
      ['template', 'name'],
      ['goal', 'goal.title'],
      ['input:i2', 'input.i2.key'],
      ['input:i1', 'input.i1.maxLen'],
      ['input:i4', undefined],
      ['step:s7', 'step.s7.after.0.step'],
      ['step:s7', 'step.s7.after.0.gates'],
      ['step:s7', 'step.s7.target'],
      ['step:s7', 'step.s7.after'],
      ['template', undefined],
      ['template', undefined],
      ['template', undefined],
      ['template', undefined],
    ]);
    // Moving a step keeps its problems with it: the draft's order decides, the id stays.
    const moved = movePart(draft, 'step:s7', -1);
    expect(placeProblems(moved, [{ code: 'step_duplicate', at: 'steps[0].key' }])[0].part).toBe('step:s7');
    expect(fieldError(placed, 'input.i2.key')).toBe('Another input already uses this key.');
    expect(fieldError(placed, 'input.i3.key')).toBeUndefined();
    const stepProblems = placed.filter((problem) => problem.part === 'step:s7');
    expect(partProblemText(stepProblems, new Set(['step.s7.title']))).toEqual([
      'Choose a step of this template.',
      'Choose what to wait for.',
      'This project is not one this orchestration project includes.',
      'The steps wait for each other in a circle.',
    ]);
    // Shown at a control, a problem is not repeated in the banner; duplicates collapse.
    expect(
      partProblemText(
        stepProblems,
        new Set(['step.s7.after.0.step', 'step.s7.after.0.gates', 'step.s7.target', 'step.s7.after']),
      ),
    ).toEqual([]);
    expect(problemWords('mystery_code')).toBe('mystery code.');
  });

  it('HS2-324VN2 shows a select problem under its select and keeps only unshown problems in the banner', () => {
    const draft = draftFromTemplate(CHECKED_IN);
    const step = draft.steps[1];
    expect(step.after.length).toBe(1);
    const placed = placeProblems(draft, [
      { code: 'step_target_invalid', at: 'steps[1].target' },
      { code: 'step_priority_invalid', at: 'steps[1].priority' },
      { code: 'step_tool_invalid', at: 'steps[1].tool' },
      { code: 'step_when_invalid', at: 'steps[1].when' },
      { code: 'step_after_unknown', at: 'steps[1].after[0]' },
      { code: 'step_gates_invalid', at: 'steps[1].after[0].gates' },
      { code: 'step_after_unknown', at: 'steps[1].after[3].step' },
      { code: 'step_key_invalid', at: 'steps[1].key' },
      { code: 'step_cycle', at: 'steps.web' },
      { code: 'input_choices_missing', at: 'steps[1]' },
    ]);
    const shown = new Set(shownFields(`step:${step.id}`, draft));
    for (const name of ['target', 'priority', 'tool', 'when', 'after.0.step', 'after.0.gates'])
      expect(shown.has(`step.${step.id}.${name}`)).toBe(true);
    expect(fieldError(placed, `step.${step.id}.target`)).toBe('Choose a project, or a project input.');
    expect(fieldError(placed, `step.${step.id}.priority`)).toBe('Choose a priority, or a priority input.');
    expect(fieldError(placed, `step.${step.id}.tool`)).toBe('Choose an AI tool, or an AI tool input.');
    expect(fieldError(placed, `step.${step.id}.when`)).toBe('Choose a yes-or-no input.');
    expect(fieldError(placed, `step.${step.id}.after.0.step`)).toBe('Choose a step of this template.');
    expect(fieldError(placed, `step.${step.id}.after.0.gates`)).toBe('Choose what to wait for.');
    // The banner keeps what no control shows: a wait the step no longer has, the key (no control), the
    // circular wait and a whole-step problem.
    expect(partProblemText(placed, shown)).toEqual([
      'Choose a step of this template.',
      'Use a key of letters, digits, “-”, “_” or “.”, at most 64 characters.',
      'The steps wait for each other in a circle.',
      'A choice input needs at least one choice.',
    ]);
    // Removing the wait takes its selects with it: its problems move to the banner.
    const unwaited = removeAfter(draft, step.id, 0);
    expect(shownFields(`step:${step.id}`, unwaited)).not.toContain(`step.${step.id}.after.0.gates`);
    expect(
      partProblemText(
        placeProblems(unwaited, [{ code: 'step_gates_invalid', at: 'steps[1].after[0].gates' }]),
        new Set(shownFields(`step:${step.id}`, unwaited)),
      ),
    ).toEqual(['Choose what to wait for.']);

    // An input's kind and default show their own problems whatever the kind; choices and longest value only
    // for the kinds that show them.
    const byKind = (kind: string) => {
      const input = editDraft(draft, `input.${draft.inputs[0].id}.kind`, kind);
      return new Set(shownFields(`input:${draft.inputs[0].id}`, input));
    };
    const at = (name: string) => `input.${draft.inputs[0].id}.${name}`;
    for (const kind of INPUT_KINDS) {
      const fields = byKind(kind);
      expect([at('kind'), at('default'), at('label'), at('key'), at('hint')].every((name) => fields.has(name))).toBe(
        true,
      );
      expect(fields.has(at('choices'))).toBe(kind === 'choice');
      expect(fields.has(at('maxLen'))).toBe(kind === 'text');
    }
    const inputPlaced = placeProblems(editDraft(draft, at('kind'), 'priority'), [
      { code: 'input_default_invalid', at: 'inputs[0].default' },
      { code: 'input_choices_unexpected', at: 'inputs[0].choices' },
    ]);
    expect(fieldError(inputPlaced, at('default'))).toBe('This default is not a value the input accepts.');
    expect(partProblemText(inputPlaced, byKind('priority'))).toEqual(['Only a choice input has choices.']);

    // A part the draft no longer has shows nothing, so every problem stays in its banner.
    expect(shownFields('step:gone', draft)).toEqual([]);
    expect(shownFields('input:gone', draft)).toEqual([]);
    expect(shownFields('template', draft)).toEqual(['name', 'description', 'instructions']);
    expect(shownFields('goal', draft)).toEqual(['goal.title', 'goal.details', 'goal.tags']);
  });

  it('labels parts, kinds, references and summaries', () => {
    const draft = draftFromTemplate(CHECKED_IN);
    expect(partLabel(draft, 'template')).toBe('Template');
    expect(partLabel(draft, 'goal')).toBe('Goal');
    expect(partLabel(draft, 'input:i1')).toBe('Feature');
    expect(partLabel(editDraft(draft, 'input.i1.label', ' '), 'input:i1')).toBe('feature');
    expect(partLabel(addInput(newTemplateDraft()).draft, 'input:i2')).toBe('input-1');
    expect(partLabel(draft, 'input:none')).toBe('Input');
    expect(partLabel(draft, 'step:s7')).toBe('2 · Show {{feature}} errors');
    expect(partLabel(editDraft(editDraft(draft, 'step.s7.title', ''), 'step.s7.key', ''), 'step:s7')).toBe(
      '2 · New Step',
    );
    expect(partLabel(draft, 'step:none')).toBe('Step');
    expect(kindLabel('boolean')).toBe('Yes or No');
    expect(kindLabel('target')).toBe('Project');
    expect(partExists(draft, 'goal') && partExists(draft, 'input:i1') && partExists(draft, 'step:s6')).toBe(true);
    expect(partExists(draft, 'input:i9') || partExists(draft, 'step:s9')).toBe(false);
    expect(inputReferences(draft, 'target')).toEqual([{ value: '{{api}}', label: 'Input: API Project' }]);
    expect(inputReferences(draft, 'tool')).toEqual([]);
    expect(templateSummary(CHECKED_IN)).toBe('5 inputs · 2 steps · version 3');
    expect(templateSummary({ inputs: [{}], steps: [{}] })).toBe('1 input · 1 step');
  });
});

describe('HS2-36XDDX Wait for Accept in the template editor', () => {
  it('is on for a new template and for one written before the setting, and round-trips both ways', () => {
    const fresh = newTemplateDraft('api');
    expect(fresh.waitForAccept).toBe(true);
    expect(templateFromDraft(fresh)).not.toHaveProperty('wait_for_accept');
    const old = draftFromTemplate({ template_id: 'T', name: 'Old', goal: { title: 'G' }, steps: [] });
    expect(old.waitForAccept).toBe(true);
    // The control turns it off, and the server gets `false`; on again leaves the field out.
    const off = editDraft(fresh, 'wait_for_accept', false);
    expect(off.waitForAccept).toBe(false);
    expect(templateFromDraft(off).wait_for_accept).toBe(false);
    expect(draftChanged(off, fresh)).toBe(true);
    const reopened = draftFromTemplate(templateFromDraft(off));
    expect(reopened.waitForAccept).toBe(false);
    const on = editDraft(reopened, 'wait_for_accept', true);
    expect(templateFromDraft(on)).not.toHaveProperty('wait_for_accept');
    expect(draftChanged(on, fresh)).toBe(false);
    // Repeated edits settle; other fields leave it alone.
    expect(editDraft(editDraft(off, 'wait_for_accept', false), 'name', 'X').waitForAccept).toBe(false);
  });
});
