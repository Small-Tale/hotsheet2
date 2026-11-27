import { describe, expect, it } from 'vitest';

import {
  accessText,
  applyBanner,
  applyProblems,
  GOAL_LABEL_TITLE_CHARS,
  initialInputs,
  inputsToSend,
  localInputProblems,
  newApplyOperationId,
  parsePlanTemplate,
  parseTemplateApply,
  parseTemplateList,
  parseTemplatePreview,
  permissionRows,
  planningRows,
  problemText,
  savableGoalLabel,
  savableGoals,
  saveProblem,
  stepNumbers,
  stepText,
} from './orchestration-templates';
import { demoTemplateApply, demoTemplatePreview, demoTemplates } from './ux-demo/orchestration-fixtures';

/** The wire shape the server answers (`templates/list`), synthetic. */
const wire = {
  revision: 'R1',
  templates: [
    {
      template_id: 'T1',
      name: 'Feature',
      version: 2,
      state: 'active',
      inputs: [
        { key: 'feature', label: 'Feature', kind: 'text', required: true },
        { key: 'count', label: 'Count', kind: 'number' },
        { key: 'flag', label: 'Flag', kind: 'boolean', default: true },
        { key: 'odd', label: 'Odd', kind: 'colour' },
      ],
      goal: { title: '{{feature}}' },
      steps: [
        { key: 'a', target: '{{api}}', title: 'A', priority: 'high' },
        { key: 'b', target: 'T', title: 'B', ticket: 'create', after: [{ step: 'a', gates: 'review' }, { nope: 1 }] },
        { key: 'broken' },
      ],
    },
    { template_id: 'T2', name: 'Old', state: 'retired', goal: { title: 'x' }, steps: [] },
    { name: 'no id' },
  ],
  applies: [
    { operation_id: 'op-1', template_id: 'T1', template_name: 'Feature', template_version: 1, state: 'planning' },
    { operation_id: 'op-2', template_id: 'T1', state: 'unknown' },
  ],
};

describe('plan templates in the Orchestration view (HS2-A4RVYA)', () => {
  it('parses the list defensively: unknown kinds, broken steps and unknown states are left out', () => {
    const list = parseTemplateList(wire);
    expect(list.revision).toBe('R1');
    expect(list.templates.map((template) => template.template_id)).toEqual(['T1', 'T2']);
    const [feature, retired] = list.templates;
    expect(feature.inputs.map((input) => input.key)).toEqual(['feature', 'count', 'flag']);
    expect(feature.inputs[2].default).toBe(true);
    expect(feature.steps.map((step) => [step.key, step.ticket])).toEqual([
      ['a', 'search'],
      ['b', 'create'],
    ]);
    expect(feature.steps[1].after).toEqual([{ step: 'a', gates: 'review' }]);
    expect(feature.goal_title).toBe('{{feature}}');
    expect(retired.state).toBe('retired');
    expect(list.applies.map((apply) => apply.operation_id)).toEqual(['op-1']);
    expect(parseTemplateList(undefined)).toEqual({ templates: [], applies: [] });
    expect(parsePlanTemplate({ template_id: 'X' })).toBeUndefined();
  });

  it('starts inputs at their defaults and sends only what is set, numbers as numbers', () => {
    const template = parseTemplateList(wire).templates[0];
    const values = initialInputs(template);
    expect(values).toEqual({ feature: '', count: '', flag: true });
    expect(inputsToSend(template, values)).toEqual({ flag: true });
    expect(inputsToSend(template, { feature: '  Rate limits ', count: '3', flag: false })).toEqual({
      feature: 'Rate limits',
      count: 3,
      flag: false,
    });
    // A number field that is not a number is sent as typed, so the server names it.
    expect(inputsToSend(template, { count: 'three' })).toEqual({ count: 'three' });
  });

  it('keeps Apply disabled while a required input is empty or the preview names a problem', () => {
    const template = parseTemplateList(wire).templates[0];
    expect(localInputProblems(template, { feature: ' ' })).toEqual([{ code: 'input_missing', at: 'inputs.feature' }]);
    expect(localInputProblems(template, { feature: 'X' })).toEqual([]);
    const preview = {
      ...demoTemplatePreview(),
      template_id: 'T1',
      problems: [
        { code: 'input_invalid', at: 'inputs.count' },
        { code: 'input_missing', at: 'inputs.feature' },
        { code: 'target_unknown', at: 'steps[1].target' },
      ],
    };
    // The preview's missing-input problem is judged locally; an edited input waits for the next preview.
    expect(applyProblems(template, { feature: 'X', count: 'x' }, preview, new Set())).toEqual([
      { code: 'input_invalid', at: 'inputs.count' },
      { code: 'target_unknown', at: 'steps[1].target' },
    ]);
    expect(applyProblems(template, { feature: 'X', count: '4' }, preview, new Set(['count']))).toEqual([
      { code: 'target_unknown', at: 'steps[1].target' },
    ]);
    // A preview of another template counts for nothing.
    expect(applyProblems(template, { feature: 'X' }, { ...preview, template_id: 'T9' }, new Set())).toEqual([]);
    expect(applyProblems(template, { feature: '' }, undefined, new Set())).toEqual([
      { code: 'input_missing', at: 'inputs.feature' },
    ]);
  });

  it('words problems, steps, access and the planner flow', () => {
    const template = parseTemplateList(wire).templates[0];
    expect(problemText(template, { code: 'input_missing', at: 'inputs.feature' })).toBe('Feature needs a value.');
    expect(problemText(template, { code: 'target_unknown', at: 'steps[1].target' })).toBe(
      'Step 2 names a project this orchestration project does not include.',
    );
    expect(problemText(undefined, { code: 'choice_unknown', at: 'choices.zzz' })).toBe(
      'zzz names a step this template does not have.',
    );
    expect(problemText(undefined, { code: 'brand_new', at: '' })).toBe('Brand new.');
    const preview = demoTemplatePreview();
    expect(stepText(preview.steps[0])).toBe(
      'Find or create “Add rate-limit headers” · high priority · prefers claude · validated by contract-check',
    );
    expect(stepText(preview.steps[1])).toBe('Find or create “Show rate-limit errors” · after api once its CI passes');
    expect(stepText(preview.steps[1], stepNumbers(preview.steps))).toBe(
      'Find or create “Show rate-limit errors” · after step 1 once its CI passes',
    );
    expect(stepText({ ...preview.steps[2], after: [{ step: 'web', gates: 'none' }] })).toBe(
      'Create “Document rate limits” · after web once it is finished',
    );
    expect(accessText('asks')).toBe('Asks through the permission prompt');
    expect(permissionRows(preview)).toEqual([
      { label: 'Create Tickets in API', value: 'Allowed by a remembered rule' },
      { label: 'Create Tickets in Web', value: 'Asks through the permission prompt' },
    ]);
    // An AI session's apply also asks for each project.
    expect(
      permissionRows({ ...preview, targets: [{ ...preview.targets[0], apply: 'asks' }] }).map((row) => row.label),
    ).toEqual(['Create Tickets in API', 'Apply for API']);
    expect(planningRows(preview).map((row) => row.value)).toEqual([
      'Proposes a plan for the new goal (Propose Plan)',
      'Within the Orchestration settings once it is planned',
    ]);
    expect(
      planningRows({ ...preview, planning: 'update', placement_automatic: false }).map((row) => row.value),
    ).toEqual([
      'Drafts the new goal beside the running plan (Update Plan)',
      'After you review the plan and start orchestration',
    ]);
  });

  it('says what an apply did in every state', () => {
    expect(applyBanner(demoTemplateApply('planning'))).toMatchObject({
      tone: 'success',
      title: 'The Planner Is Adapting the Template',
    });
    expect(applyBanner({ ...demoTemplateApply('planning'), outcome: 'existing' }).title).toBe('Already Applied');
    expect(applyBanner({ ...demoTemplateApply('planning'), planning: 'update' }).detail).toContain('plan update');
    expect(applyBanner(demoTemplateApply('waiting_planner')).detail).toContain('is busy with another proposal');
    expect(
      applyBanner({ ...demoTemplateApply('waiting_planner'), reason: 'session_host_unavailable' }).detail,
    ).toContain('could not take it (session host unavailable)');
    expect(applyBanner(demoTemplateApply('waiting_permission')).tone).toBe('info');
    expect(applyBanner(demoTemplateApply('denied')).tone).toBe('danger');
    const parsed = parseTemplateApply({
      operation_id: 'op',
      template_id: 'T1',
      snapshot: { name: 'Feature', version: 4 },
      state: 'waiting_planner',
      goal: { connection_id: 'c', native_id: 'n', slug: 'LCH-9', created: true },
      planning: 'update',
      permission_requests: ['r1', 7],
      outcome: 'applied',
    });
    expect(parsed).toEqual({
      operation_id: 'op',
      template_id: 'T1',
      template_name: 'Feature',
      template_version: 4,
      state: 'waiting_planner',
      goal: { connection_id: 'c', native_id: 'n', slug: 'LCH-9', created: true },
      planning: 'update',
      permission_requests: ['r1'],
      outcome: 'applied',
    });
  });

  it('parses the pre-run summary and keeps fresh operation ids within bounds', () => {
    const preview = parseTemplatePreview({
      template: { template_id: 'T1' },
      problems: [{ code: 'input_missing', at: 'inputs.feature' }, { at: 'x' }],
      goal_title: 'G',
      steps: [{ key: 'a', target_id: 'api', title: 'A', ticket: 'create' }, { key: 'b' }],
      skipped: ['c'],
      targets: [{ target_id: 'api', declared: true, steps: ['a'], apply: 'person', ticket_writes: 'odd' }],
      planning: 'update',
      placement_automatic: true,
    });
    expect(preview.problems).toEqual([{ code: 'input_missing', at: 'inputs.feature' }]);
    expect(preview.steps.map((step) => step.key)).toEqual(['a']);
    expect(preview.targets[0]).toMatchObject({ apply: 'person', ticket_writes: 'asks' });
    expect(preview.planning).toBe('update');
    expect(newApplyOperationId(() => 'abc')).toBe('tpl-abc');
    expect(newApplyOperationId(() => 'x'.repeat(400))).toHaveLength(140);
    expect(newApplyOperationId()).toMatch(/^tpl-[0-9a-f]{32}$/);
  });

  it('offers only open goals with steps to Save a Goal as Template…, and says why Save is disabled', () => {
    const goals = savableGoals({
      goals: [
        { connection_id: 'c', native_id: 'g1', slug: 'LCH-1', steps: 2 },
        { connection_id: 'c', native_id: 'g2', slug: 'LCH-2', steps: 0 },
        { native_id: 'g3', steps: 3 },
      ],
    });
    expect(goals).toEqual([{ connection_id: 'c', native_id: 'g1', slug: 'LCH-1', title: '', steps: 2 }]);
    const form = { goal: 'g1', name: 'Again', description: '', targetsAsInputs: false };
    expect(saveProblem(form, goals)).toBeUndefined();
    expect(saveProblem(form, [])).toBe('No open goal has plan steps to save yet');
    expect(saveProblem({ ...form, goal: 'g2' }, goals)).toBe('Choose a goal');
    expect(saveProblem({ ...form, name: '  ' }, goals)).toBe('Name the template');
    expect(saveProblem({ ...form, name: 'x'.repeat(201) }, goals)).toBe('Use a name of at most 200 characters');
    expect(savableGoals(undefined)).toEqual([]);
  });

  it('HS2-M7RXCB names goals by title in the Save a Goal as Template… select', () => {
    const [titled, untitled, long, one] = savableGoals({
      goals: [
        { connection_id: 'c', native_id: 'g1', slug: 'LCH-1', title: '  Rate limits for the API ', steps: 2 },
        { connection_id: 'c', native_id: 'g2', slug: 'LCH-2', steps: 3 },
        { connection_id: 'c', native_id: 'g3', slug: 'LCH-3', title: 'x'.repeat(200), steps: 4 },
        { connection_id: 'c', native_id: 'g4', slug: 'LCH-4', title: 'One step', steps: 1 },
      ],
    });
    expect(titled.title).toBe('Rate limits for the API');
    expect(savableGoalLabel(titled)).toBe('Rate limits for the API · LCH-1 · 2 steps');
    // An older server sends no title: the slug alone, as before.
    expect(savableGoalLabel(untitled)).toBe('LCH-2 · 3 steps');
    const label = savableGoalLabel(long);
    expect(label).toBe(`${'x'.repeat(GOAL_LABEL_TITLE_CHARS - 1)}… · LCH-3 · 4 steps`);
    expect(savableGoalLabel(one)).toBe('One step · LCH-4 · 1 step');
    // A title of exactly the bound is not shortened, and characters are counted, not code units.
    expect(savableGoalLabel({ ...long, title: '😀'.repeat(GOAL_LABEL_TITLE_CHARS) })).toBe(
      `${'😀'.repeat(GOAL_LABEL_TITLE_CHARS)} · LCH-3 · 4 steps`,
    );
    const family = '\u{1F469}\u200D\u{1F469}\u200D\u{1F467}';
    expect(savableGoalLabel({ ...long, title: family.repeat(GOAL_LABEL_TITLE_CHARS + 1) })).toBe(
      `${family.repeat(GOAL_LABEL_TITLE_CHARS - 1)}… · LCH-3 · 4 steps`,
    );
  });

  it('demo templates parse back through the wire parser', () => {
    for (const template of demoTemplates())
      expect(
        parsePlanTemplate({
          ...template,
          goal: { title: template.goal_title },
        }),
      ).toEqual(template);
  });
});
