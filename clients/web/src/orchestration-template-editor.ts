/**
 * HS2-1XCW6N: the visual template editor of Settings › Orchestration › Templates (docs/26 §26.32). A
 * template is edited as a draft of strings, one part at a time (the template, its goal, each input and
 * each step), and saved whole through the existing `templates/set` route with the list's revision, so a
 * template changed elsewhere meanwhile is refused (`template_conflict`) rather than overwritten. The
 * server's validator is the authority: its coded problems (`steps[1].after[0]`) are mapped onto the
 * draft's parts and fields, which keep stable local ids, so a renamed or reordered step keeps its
 * problems. Pure: it converts, edits and words, so the dialog and its controller stay thin.
 */
import type { TemplateInputKind, TemplateProblem } from './orchestration-templates';

export const INPUT_KINDS: readonly TemplateInputKind[] = [
  'text',
  'number',
  'boolean',
  'choice',
  'priority',
  'target',
  'tool',
];
export const PRIORITIES = ['highest', 'high', 'default', 'low', 'lowest'] as const;
export const STEP_GATES = ['none', 'ci', 'deployment', 'review'] as const;
/** The validator's bounds (`hotsheet-ticketing` templates.rs). */
export const MAX_INPUTS = 16;
export const MAX_STEPS = 64;
export const MAX_LINE = 200;
export const MAX_TEXT = 4000;
export const MAX_INPUT_TEXT = 500;

/** One input being edited. `id` is local and stable; `key` is what placeholders name. */
export interface InputDraft {
  id: string;
  key: string;
  label: string;
  kind: TemplateInputKind;
  required: boolean;
  /** Text of a text, number, choice, priority, target or tool default; `'true'`/`''` for a boolean. */
  default: string;
  /** One choice per line. */
  choices: string;
  /** A text input's longest value, as typed. */
  maxLen: string;
  hint: string;
}

export interface AfterDraft {
  step: string;
  /** `''` is the default gate (CI). */
  gates: string;
}

/** One step being edited. `target`, `priority`, `tool` and `when` may be `{{key}}` of an input. */
export interface StepDraft {
  id: string;
  key: string;
  target: string;
  title: string;
  details: string;
  category: string;
  priority: string;
  ticket: 'search' | 'create';
  after: AfterDraft[];
  /** Test profile names, separated by commas. */
  validation: string;
  tool: string;
  when: string;
}

export interface TemplateDraft {
  /** Empty for a template not saved yet. */
  template_id: string;
  name: string;
  description: string;
  state: 'active' | 'retired';
  version: number;
  inputs: InputDraft[];
  goalTitle: string;
  goalDetails: string;
  /** Goal tags, separated by commas. */
  goalTags: string;
  steps: StepDraft[];
  instructions: string;
  /** HS2-36XDDX: Wait for Accept, on by default. */
  waitForAccept: boolean;
  /** The next local id. */
  next: number;
}

/** A part of the editor's list: the template itself, its goal, one input or one step. */
export type EditorPart = 'template' | 'goal' | `input:${string}` | `step:${string}`;

type Raw = Record<string, unknown>;
const record = (value: unknown): Raw =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Raw) : {};
const records = (value: unknown): Raw[] => (Array.isArray(value) ? value.map(record) : []);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');
const texts = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
const kindOf = (value: unknown): TemplateInputKind => INPUT_KINDS.find((kind) => kind === value) ?? 'text';

function defaultText(kind: TemplateInputKind, value: unknown): string {
  if (kind === 'boolean') return value === true ? 'true' : '';
  if (typeof value === 'number') return String(value);
  return text(value);
}

/** A checked-in template (the server's JSON) as a draft. */
export function draftFromTemplate(value: unknown): TemplateDraft {
  const raw = record(value),
    goal = record(raw.goal);
  let next = 1;
  const inputs = records(raw.inputs).map((input): InputDraft => {
    const kind = kindOf(input.kind);
    return {
      id: `i${next++}`,
      key: text(input.key),
      label: text(input.label),
      kind,
      required: input.required === true,
      default: defaultText(kind, input.default),
      choices: texts(input.choices).join('\n'),
      maxLen: typeof input.max_len === 'number' ? String(input.max_len) : '',
      hint: text(input.hint),
    };
  });
  const steps = records(raw.steps).map((step): StepDraft => ({
    id: `s${next++}`,
    key: text(step.key),
    target: text(step.target),
    title: text(step.title),
    details: text(step.details),
    category: text(step.category),
    priority: text(step.priority),
    ticket: step.ticket === 'create' ? 'create' : 'search',
    after: records(step.after).map((after) => ({ step: text(after.step), gates: text(after.gates) })),
    validation: texts(step.validation).join(', '),
    tool: text(step.tool),
    when: text(step.when),
  }));
  return {
    template_id: text(raw.template_id),
    name: text(raw.name),
    description: text(raw.description),
    state: raw.state === 'retired' ? 'retired' : 'active',
    version: typeof raw.version === 'number' ? raw.version : 0,
    inputs,
    goalTitle: text(goal.title),
    goalDetails: text(goal.details),
    goalTags: texts(goal.tags).join(', '),
    steps,
    instructions: text(raw.instructions),
    // HS2-36XDDX: absent is on (the default).
    waitForAccept: raw.wait_for_accept !== false,
    next,
  };
}

/** A new template: one search-first step in the first project, if any. */
export function newTemplateDraft(firstProject = ''): TemplateDraft {
  return {
    template_id: '',
    name: '',
    description: '',
    state: 'active',
    version: 0,
    inputs: [],
    goalTitle: '',
    goalDetails: '',
    goalTags: '',
    steps: [blankStep('s1', 'step-1', firstProject)],
    instructions: '',
    waitForAccept: true,
    next: 2,
  };
}

function blankStep(id: string, key: string, target: string): StepDraft {
  return {
    id,
    key,
    target,
    title: '',
    details: '',
    category: '',
    priority: '',
    ticket: 'search',
    after: [],
    validation: '',
    tool: '',
    when: '',
  };
}

const list = (value: string, separator: RegExp) =>
  value
    .split(separator)
    .map((item) => item.trim())
    .filter((item) => item !== '');

/** A default as the server takes it, or undefined for none. A number that does not parse is sent as typed. */
function defaultValue(input: InputDraft): unknown {
  if (input.kind === 'boolean') return input.default === 'true' ? true : undefined;
  const value = input.default.trim();
  if (value === '') return undefined;
  if (input.kind === 'number') {
    const number = Number(value);
    return Number.isFinite(number) ? number : value;
  }
  return value;
}

/** A typed longest value, or undefined when blank or not a positive whole number (a local problem). */
function maxLenValue(input: InputDraft): number | undefined {
  const value = input.maxLen.trim();
  if (input.kind !== 'text' || !/^\d+$/.test(value)) return undefined;
  return Number(value);
}

/** The draft as the server's template JSON; empty optional fields are left out. */
export function templateFromDraft(draft: TemplateDraft): Raw {
  const optional = (key: string, value: string): Raw => (value.trim() === '' ? {} : { [key]: value });
  return {
    ...(draft.template_id ? { template_id: draft.template_id } : {}),
    name: draft.name,
    ...optional('description', draft.description),
    state: draft.state,
    inputs: draft.inputs.map((input) => {
      const fallback = defaultValue(input),
        choices = input.kind === 'choice' ? list(input.choices, /\n/) : [],
        maxLen = maxLenValue(input);
      return {
        key: input.key,
        label: input.label,
        kind: input.kind,
        required: input.required,
        ...(fallback === undefined ? {} : { default: fallback }),
        ...(choices.length > 0 ? { choices } : {}),
        ...(maxLen === undefined ? {} : { max_len: maxLen }),
        ...optional('hint', input.hint),
      };
    }),
    goal: {
      title: draft.goalTitle,
      ...optional('details', draft.goalDetails),
      ...(list(draft.goalTags, /[,\s]+/).length > 0 ? { tags: list(draft.goalTags, /[,\s]+/) } : {}),
    },
    steps: draft.steps.map((step) => ({
      key: step.key,
      target: step.target,
      title: step.title,
      ...optional('details', step.details),
      ...optional('category', step.category),
      ...optional('priority', step.priority),
      ...(step.ticket === 'create' ? { ticket: 'create' } : {}),
      ...(step.after.length > 0
        ? { after: step.after.map((after) => ({ step: after.step, ...optional('gates', after.gates) })) }
        : {}),
      ...(list(step.validation, /[,\s]+/).length > 0 ? { validation: list(step.validation, /[,\s]+/) } : {}),
      ...optional('tool', step.tool),
      ...optional('when', step.when),
    })),
    ...optional('instructions', draft.instructions),
    // HS2-36XDDX: written only when off, as the server writes it.
    ...(draft.waitForAccept ? {} : { wait_for_accept: false }),
  };
}

/** Whether the draft differs from the template it was opened from (or from a new blank one). */
export function draftChanged(draft: TemplateDraft, original: TemplateDraft): boolean {
  return JSON.stringify(templateFromDraft(draft)) !== JSON.stringify(templateFromDraft(original));
}

const reference = (key: string) => `{{${key}}}`;

/** Rewrite whole-value references (`{{old}}`) of steps to an input renamed from `from` to `to`. */
function renameReferences(steps: StepDraft[], from: string, to: string): StepDraft[] {
  if (!from) return steps;
  const swap = (value: string) => (value === reference(from) ? (to ? reference(to) : '') : value);
  return steps.map((step) => ({
    ...step,
    target: swap(step.target),
    priority: swap(step.priority),
    tool: swap(step.tool),
    when: swap(step.when),
  }));
}

/**
 * Set one field, named as the editor's controls name it: `name`, `description`, `instructions`, `wait_for_accept`,
 * `goal.title`, `goal.details`, `goal.tags`, `input.<id>.<field>`, `step.<id>.<field>` or
 * `step.<id>.after.<n>.step|gates`. Renaming an input's key or a step's key follows the references that
 * name it whole; an unknown field changes nothing.
 */
export function editDraft(draft: TemplateDraft, field: string, value: string | boolean): TemplateDraft {
  const textValue = typeof value === 'string' ? value : value ? 'true' : '';
  switch (field) {
    case 'name':
      return { ...draft, name: textValue };
    case 'description':
      return { ...draft, description: textValue };
    case 'instructions':
      return { ...draft, instructions: textValue };
    case 'wait_for_accept':
      return { ...draft, waitForAccept: value === true || value === 'true' };
    case 'goal.title':
      return { ...draft, goalTitle: textValue };
    case 'goal.details':
      return { ...draft, goalDetails: textValue };
    case 'goal.tags':
      return { ...draft, goalTags: textValue };
    default:
      break;
  }
  const [part, id, name, index, sub] = field.split('.');
  if (part === 'input' && id && name) {
    const input = draft.inputs.find((candidate) => candidate.id === id);
    if (!input) return draft;
    let changed: InputDraft;
    switch (name) {
      case 'key':
      case 'label':
      case 'choices':
      case 'hint':
      case 'maxLen':
      case 'default':
        changed = { ...input, [name]: textValue };
        break;
      case 'required':
        changed = { ...input, required: value === true || value === 'true' };
        break;
      case 'kind': {
        const kind = kindOf(textValue);
        // A default of another kind would not fit; a kind change starts it over.
        changed = { ...input, kind, default: kind === input.kind ? input.default : '' };
        break;
      }
      default:
        return draft;
    }
    return {
      ...draft,
      inputs: draft.inputs.map((candidate) => (candidate.id === id ? changed : candidate)),
      steps: name === 'key' ? renameReferences(draft.steps, input.key, changed.key) : draft.steps,
    };
  }
  if (part === 'step' && id && name) {
    const step = draft.steps.find((candidate) => candidate.id === id);
    if (!step) return draft;
    let changed: StepDraft;
    if (name === 'after') {
      const n = Number(index);
      if (!Number.isInteger(n) || n < 0 || n >= step.after.length || (sub !== 'step' && sub !== 'gates')) return draft;
      changed = {
        ...step,
        after: step.after.map((after, at) => (at === n ? { ...after, [sub]: textValue } : after)),
      };
    } else {
      switch (name) {
        case 'key':
        case 'target':
        case 'title':
        case 'details':
        case 'category':
        case 'priority':
        case 'validation':
        case 'tool':
        case 'when':
          changed = { ...step, [name]: textValue };
          break;
        case 'ticket':
          changed = { ...step, ticket: textValue === 'create' ? 'create' : 'search' };
          break;
        default:
          return draft;
      }
    }
    return {
      ...draft,
      steps: draft.steps.map((candidate) => {
        if (candidate.id === id) return changed;
        // A renamed step key follows into the steps that wait for it.
        if (name !== 'key' || !step.key) return candidate;
        return {
          ...candidate,
          after: candidate.after.map((after) => (after.step === step.key ? { ...after, step: changed.key } : after)),
        };
      }),
    };
  }
  return draft;
}

function freeKey(prefix: string, taken: readonly string[]): string {
  for (let n = taken.length + 1; ; n += 1) if (!taken.includes(`${prefix}-${n}`)) return `${prefix}-${n}`;
}

/** Add an input (a required text input with a free key) and answer its part. */
export function addInput(draft: TemplateDraft): { draft: TemplateDraft; part: EditorPart } {
  const id = `i${draft.next}`,
    key = freeKey(
      'input',
      draft.inputs.map((input) => input.key),
    );
  return {
    draft: {
      ...draft,
      next: draft.next + 1,
      inputs: [
        ...draft.inputs,
        { id, key, label: '', kind: 'text', required: true, default: '', choices: '', maxLen: '', hint: '' },
      ],
    },
    part: `input:${id}`,
  };
}

/** Add a step after the last one, waiting for it, in the same project; answer its part. */
export function addStep(draft: TemplateDraft): { draft: TemplateDraft; part: EditorPart } {
  const id = `s${draft.next}`,
    last = draft.steps.at(-1),
    step = blankStep(
      id,
      freeKey(
        'step',
        draft.steps.map((candidate) => candidate.key),
      ),
      last?.target ?? '',
    );
  return {
    draft: {
      ...draft,
      next: draft.next + 1,
      steps: [...draft.steps, last?.key ? { ...step, after: [{ step: last.key, gates: '' }] } : step],
    },
    part: `step:${id}`,
  };
}

/** Remove an input; references to it are cleared. The part to show next is answered. */
export function removeInput(draft: TemplateDraft, id: string): { draft: TemplateDraft; part: EditorPart } {
  const input = draft.inputs.find((candidate) => candidate.id === id);
  if (!input) return { draft, part: 'template' };
  const index = draft.inputs.indexOf(input),
    inputs = draft.inputs.filter((candidate) => candidate.id !== id),
    neighbour = inputs.at(Math.min(index, inputs.length - 1));
  return {
    draft: { ...draft, inputs, steps: renameReferences(draft.steps, input.key, '') },
    part: neighbour ? `input:${neighbour.id}` : 'template',
  };
}

/** Remove a step; the steps that waited for it no longer do. The part to show next is answered. */
export function removeStep(draft: TemplateDraft, id: string): { draft: TemplateDraft; part: EditorPart } {
  const step = draft.steps.find((candidate) => candidate.id === id);
  if (!step) return { draft, part: 'template' };
  const index = draft.steps.indexOf(step),
    steps = draft.steps
      .filter((candidate) => candidate.id !== id)
      .map((candidate) => ({ ...candidate, after: candidate.after.filter((after) => after.step !== step.key) })),
    neighbour = steps.at(Math.min(index, steps.length - 1));
  return { draft: { ...draft, steps }, part: neighbour ? `step:${neighbour.id}` : 'goal' };
}

/** Move an input or a step one place up (`-1`) or down (`1`); at an end nothing changes. */
export function movePart(draft: TemplateDraft, part: EditorPart, by: -1 | 1): TemplateDraft {
  const move = <T extends { id: string }>(items: T[], id: string): T[] => {
    const from = items.findIndex((item) => item.id === id),
      to = from + by;
    if (from < 0 || to < 0 || to >= items.length) return items;
    const copy = [...items];
    [copy[from], copy[to]] = [copy[to], copy[from]];
    return copy;
  };
  if (part.startsWith('input:')) return { ...draft, inputs: move(draft.inputs, part.slice('input:'.length)) };
  if (part.startsWith('step:')) return { ...draft, steps: move(draft.steps, part.slice('step:'.length)) };
  return draft;
}

/** Add a prerequisite to a step: the nearest earlier step it does not wait for yet, else any other. */
export function addAfter(draft: TemplateDraft, id: string): TemplateDraft {
  const step = draft.steps.find((candidate) => candidate.id === id);
  if (!step) return draft;
  const waited = new Set(step.after.map((after) => after.step)),
    index = draft.steps.indexOf(step),
    others = [...draft.steps.slice(0, index).reverse(), ...draft.steps.slice(index + 1)],
    choice = others.find((other) => other.key && !waited.has(other.key));
  return {
    ...draft,
    steps: draft.steps.map((candidate) =>
      candidate.id === id
        ? { ...candidate, after: [...candidate.after, { step: choice?.key ?? '', gates: '' }] }
        : candidate,
    ),
  };
}

export function removeAfter(draft: TemplateDraft, id: string, index: number): TemplateDraft {
  return {
    ...draft,
    steps: draft.steps.map((step) =>
      step.id === id ? { ...step, after: step.after.filter((_after, at) => at !== index) } : step,
    ),
  };
}

/** One problem placed in the editor: its part, the field it is about (a control name suffix), and words. */
export interface PlacedProblem {
  code: string;
  at: string;
  part: EditorPart;
  /** The control it is about, or undefined for a problem of the whole part. */
  field?: string;
}

/**
 * Place the validator's problems (paths by index) on the draft's parts (by stable id). A path the draft
 * cannot place is a problem of the template part, so nothing is dropped.
 */
export function placeProblems(draft: TemplateDraft, problems: readonly TemplateProblem[]): PlacedProblem[] {
  return problems.map((problem): PlacedProblem => {
    const base = { code: problem.code, at: problem.at };
    const indexed = /^(inputs|steps)\[(\d+)\](?:\.(\w+)(?:\[(\d+)\](?:\.(\w+))?)?)?$/.exec(problem.at);
    if (indexed) {
      const [collection, index, name, subIndex, sub] = [1, 2, 3, 4, 5].map((group) => indexed.at(group));
      const items: ReadonlyArray<{ id: string }> = collection === 'inputs' ? draft.inputs : draft.steps,
        item = index === undefined ? undefined : items[Number(index)];
      if (!item) return { ...base, part: 'template' };
      const kind = collection === 'inputs' ? 'input' : 'step';
      const part: EditorPart = `${kind}:${item.id}`;
      if (!name) return { ...base, part };
      const field =
        name === 'after' && subIndex !== undefined
          ? `${kind}.${item.id}.after.${subIndex}.${sub ?? 'step'}`
          : `${kind}.${item.id}.${name === 'max_len' ? 'maxLen' : name}`;
      return { ...base, part, field };
    }
    const cycle = /^steps\.(.+)$/.exec(problem.at);
    if (cycle) {
      const step = draft.steps.find((candidate) => candidate.key === cycle[1]);
      return step
        ? { ...base, part: `step:${step.id}`, field: `step.${step.id}.after` }
        : { ...base, part: 'template' };
    }
    if (problem.at.startsWith('goal.')) return { ...base, part: 'goal', field: problem.at };
    if (problem.at === 'name' || problem.at === 'description' || problem.at === 'instructions')
      return { ...base, part: 'template', field: problem.at };
    return { ...base, part: 'template' };
  });
}

/** Problems the server cannot report because the draft would not reach it in shape (a longest value). */
export function localProblems(draft: TemplateDraft): TemplateProblem[] {
  return draft.inputs.flatMap((input, n) => {
    const value = input.maxLen.trim();
    return value !== '' && (input.kind !== 'text' || !/^\d+$/.test(value) || Number(value) < 1)
      ? [{ code: 'input_max_len_invalid', at: `inputs[${n}].max_len` }]
      : [];
  });
}

const WORDS: Readonly<Record<string, string>> = {
  name_invalid: 'Name the template in one line of at most 200 characters.',
  text_invalid: 'Fill this in, in one line of at most 200 characters.',
  text_too_long: 'Keep this to 4,000 characters.',
  placeholder_unknown: 'A {{…}} placeholder here names no input of this template.',
  too_many_inputs: 'A template has at most 16 inputs.',
  input_key_invalid: 'Use a key of letters, digits, “-”, “_” or “.”, at most 64 characters.',
  input_duplicate: 'Another input already uses this key.',
  input_label_invalid: 'Label the input in one line of at most 200 characters.',
  input_hint_invalid: 'Keep the hint to one line of at most 200 characters.',
  input_choices_missing: 'A choice input needs at least one choice.',
  input_choices_unexpected: 'Only a choice input has choices.',
  input_choices_invalid: 'Use at most 32 different choices, each one line of at most 200 characters.',
  input_max_len_invalid: 'Use a whole number from 1 to 500, for a text input only.',
  input_default_invalid: 'This default is not a value the input accepts.',
  goal_tags_invalid: 'Use at most 8 tags of letters, digits, “-”, “_” or “.”.',
  steps_missing: 'A template needs at least one step.',
  too_many_steps: 'A template has at most 64 steps.',
  step_key_invalid: 'Use a key of letters, digits, “-”, “_” or “.”, at most 64 characters.',
  step_duplicate: 'Another step already uses this key.',
  step_target_invalid: 'Choose a project, or a project input.',
  target_unknown: 'This project is not one this orchestration project includes.',
  step_category_invalid: 'Use a category of letters, digits, “-”, “_” or “.”.',
  step_priority_invalid: 'Choose a priority, or a priority input.',
  step_tool_invalid: 'Choose an AI tool, or an AI tool input.',
  step_when_invalid: 'Choose a yes-or-no input.',
  step_validation_invalid: 'Name at most 4 test profiles of letters, digits, “-”, “_” or “.”.',
  step_after_self: 'A step cannot wait for itself.',
  step_after_unknown: 'Choose a step of this template.',
  step_gates_invalid: 'Choose what to wait for.',
  step_cycle: 'The steps wait for each other in a circle.',
};

/** A problem in words: what to change. An unknown code reads as its words. */
export function problemWords(code: string): string {
  return WORDS[code] ?? `${code.replaceAll('_', ' ')}.`;
}

/**
 * The fields of a part that the editor shows as a control with its own `error` (text fields, selects and
 * checkboxes; HS2-324VN2), so the part's banner lists only the rest: whole-part problems, a step's
 * circular wait, a step's key (it has no control), and a field the input's kind hides (a non-choice
 * input's choices, a non-text input's longest value).
 */
export function shownFields(part: EditorPart, draft: TemplateDraft): string[] {
  if (part === 'template') return ['name', 'description', 'instructions'];
  if (part === 'goal') return ['goal.title', 'goal.details', 'goal.tags'];
  if (part.startsWith('input:')) {
    const id = part.slice('input:'.length),
      input = draft.inputs.find((candidate) => candidate.id === id);
    if (!input) return [];
    const fields = ['label', 'key', 'kind', 'required', ...(input.kind === 'choice' ? ['choices'] : []), 'default'];
    if (input.kind === 'text') fields.push('maxLen');
    fields.push('hint');
    return fields.map((field) => `input.${id}.${field}`);
  }
  const id = part.slice('step:'.length),
    step = draft.steps.find((candidate) => candidate.id === id);
  if (!step) return [];
  return [
    'title',
    'target',
    'ticket',
    'details',
    'priority',
    'category',
    'tool',
    'when',
    'validation',
    ...step.after.flatMap((_after, index) => [`after.${index}.step`, `after.${index}.gates`]),
  ].map((field) => `step.${id}.${field}`);
}

/** The words of a part's problems that no control of it shows (whole-part problems, hidden fields). */
export function partProblemText(placed: readonly PlacedProblem[], shownFields: ReadonlySet<string>): string[] {
  return [
    ...new Set(
      placed.filter((problem) => !problem.field || !shownFields.has(problem.field)).map((p) => problemWords(p.code)),
    ),
  ];
}

/** The first problem of a field, in words, for its control's `error`. */
export function fieldError(placed: readonly PlacedProblem[], field: string): string | undefined {
  const found = placed.find((problem) => problem.field === field);
  return found ? problemWords(found.code) : undefined;
}

/** How a part is named in the editor's list. */
export function partLabel(draft: TemplateDraft, part: EditorPart): string {
  if (part === 'template') return 'Template';
  if (part === 'goal') return 'Goal';
  if (part.startsWith('input:')) {
    const input = draft.inputs.find((candidate) => `input:${candidate.id}` === part);
    return input ? input.label.trim() || input.key || 'New Input' : 'Input';
  }
  const index = draft.steps.findIndex((candidate) => `step:${candidate.id}` === part),
    step = index < 0 ? undefined : draft.steps.at(index);
  return step ? `${index + 1} · ${step.title.trim() || step.key || 'New Step'}` : 'Step';
}

const KIND_LABELS: Readonly<Record<TemplateInputKind, string>> = {
  text: 'Text',
  number: 'Number',
  boolean: 'Yes or No',
  choice: 'Choice',
  priority: 'Priority',
  target: 'Project',
  tool: 'AI Tool',
};

export function kindLabel(kind: TemplateInputKind): string {
  return KIND_LABELS[kind];
}

/** Whether a part still exists in the draft (a removed part falls back to the template). */
export function partExists(draft: TemplateDraft, part: EditorPart): boolean {
  if (part === 'template' || part === 'goal') return true;
  if (part.startsWith('input:')) return draft.inputs.some((input) => `input:${input.id}` === part);
  return draft.steps.some((step) => `step:${step.id}` === part);
}

/** The `{{key}}` references of a kind's inputs, as select choices labeled by the input. */
export function inputReferences(
  draft: TemplateDraft,
  kind: TemplateInputKind,
): Array<{ value: string; label: string }> {
  return draft.inputs
    .filter((input) => input.kind === kind && input.key)
    .map((input) => ({ value: reference(input.key), label: `Input: ${input.label.trim() || input.key}` }));
}

/** A one-line summary of a checked-in template for the settings list. */
export function templateSummary(value: unknown): string {
  const raw = record(value),
    inputs = records(raw.inputs).length,
    steps = records(raw.steps).length,
    version = typeof raw.version === 'number' ? raw.version : 0;
  return [
    inputs === 1 ? '1 input' : `${inputs} inputs`,
    steps === 1 ? '1 step' : `${steps} steps`,
    ...(version > 0 ? [`version ${version}`] : []),
  ].join(' · ');
}
