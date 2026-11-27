/**
 * HS2-A4RVYA: plan templates in the Orchestration view (docs/26 §26.32). A template is a checked-in brief
 * the planner adapts each run (the maintainer's decision); Use Template… shows its typed inputs, the steps
 * the brief describes, the projects with what asks there, and applies it, which opens a new goal ticket
 * and hands it to the planner. Pure: it parses the server's answers and words them, so the dialog and its
 * controller stay thin. Unknown codes read as their words.
 */

export type TemplateInputKind = 'text' | 'number' | 'boolean' | 'choice' | 'priority' | 'target' | 'tool';
export type StepTicketChoice = 'search' | 'create';
export type AccessNeed = 'person' | 'remembered' | 'asks';
export type ApplyState = 'waiting_permission' | 'waiting_planner' | 'planning' | 'denied';

export interface TemplateInput {
  key: string;
  label: string;
  kind: TemplateInputKind;
  required: boolean;
  default?: unknown;
  choices: string[];
  hint?: string;
}

export interface TemplateStepAfter {
  step: string;
  gates?: string;
}

export interface TemplateStep {
  key: string;
  target: string;
  title: string;
  ticket: StepTicketChoice;
  after: TemplateStepAfter[];
  priority?: string;
  tool?: string;
  validation: string[];
  when?: string;
}

export interface PlanTemplate {
  template_id: string;
  name: string;
  description: string;
  state: 'active' | 'retired';
  version: number;
  inputs: TemplateInput[];
  goal_title: string;
  steps: TemplateStep[];
}

export interface TemplateProblem {
  code: string;
  at: string;
}

export interface BriefStep {
  key: string;
  target_id: string;
  title: string;
  ticket: StepTicketChoice;
  after: TemplateStepAfter[];
  priority?: string;
  tool?: string;
  validation: string[];
}

export interface PreviewTarget {
  target_id: string;
  label?: string;
  declared: boolean;
  steps: string[];
  apply: AccessNeed;
  ticket_writes: AccessNeed;
}

export interface TemplatePreview {
  template_id: string;
  problems: TemplateProblem[];
  goal_title: string;
  steps: BriefStep[];
  skipped: string[];
  targets: PreviewTarget[];
  planning: 'propose' | 'update';
  placement_automatic: boolean;
}

export interface ApplyGoal {
  connection_id: string;
  native_id: string;
  slug?: string;
  created: boolean;
}

export interface TemplateApply {
  operation_id: string;
  template_id: string;
  template_name: string;
  template_version: number;
  state: ApplyState;
  goal?: ApplyGoal;
  planning?: 'propose' | 'update';
  reason?: string;
  permission_requests: string[];
  outcome?: 'applied' | 'existing';
}

export interface TemplateList {
  revision?: string;
  templates: PlanTemplate[];
  applies: TemplateApply[];
}

type Raw = Record<string, unknown>;
const record = (value: unknown): Raw =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Raw) : {};
const text = (value: unknown): string | undefined => (typeof value === 'string' ? value : undefined);
const texts = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : [];
const records = (value: unknown): Raw[] => (Array.isArray(value) ? value.map(record) : []);
const INPUT_KINDS: readonly TemplateInputKind[] = ['text', 'number', 'boolean', 'choice', 'priority', 'target', 'tool'];
const NEEDS: readonly AccessNeed[] = ['person', 'remembered', 'asks'];
const STATES: readonly ApplyState[] = ['waiting_permission', 'waiting_planner', 'planning', 'denied'];

const ticketChoice = (value: unknown): StepTicketChoice => (value === 'create' ? 'create' : 'search');
const after = (value: unknown): TemplateStepAfter[] =>
  records(value).flatMap((entry) => {
    const step = text(entry.step);
    return step ? [{ step, ...(text(entry.gates) ? { gates: text(entry.gates) } : {}) }] : [];
  });

export function parsePlanTemplate(value: unknown): PlanTemplate | undefined {
  const raw = record(value);
  const id = text(raw.template_id),
    name = text(raw.name);
  if (!id || !name) return undefined;
  return {
    template_id: id,
    name,
    description: text(raw.description) ?? '',
    state: raw.state === 'retired' ? 'retired' : 'active',
    version: typeof raw.version === 'number' ? raw.version : 1,
    inputs: records(raw.inputs).flatMap((input) => {
      const key = text(input.key),
        kind = INPUT_KINDS.find((candidate) => candidate === input.kind);
      if (!key || !kind) return [];
      return [
        {
          key,
          label: text(input.label) ?? key,
          kind,
          required: input.required === true,
          ...(input.default !== undefined && input.default !== null ? { default: input.default } : {}),
          choices: texts(input.choices),
          ...(text(input.hint) ? { hint: text(input.hint) } : {}),
        },
      ];
    }),
    goal_title: text(record(raw.goal).title) ?? name,
    steps: records(raw.steps).flatMap((step) => {
      const key = text(step.key),
        target = text(step.target),
        title = text(step.title);
      if (!key || !target || !title) return [];
      return [
        {
          key,
          target,
          title,
          ticket: ticketChoice(step.ticket),
          after: after(step.after),
          ...(text(step.priority) ? { priority: text(step.priority) } : {}),
          ...(text(step.tool) ? { tool: text(step.tool) } : {}),
          validation: texts(step.validation),
          ...(text(step.when) ? { when: text(step.when) } : {}),
        },
      ];
    }),
  };
}

function parseApply(value: unknown): TemplateApply | undefined {
  const raw = record(value);
  const operation = text(raw.operation_id),
    template = text(raw.template_id),
    state = STATES.find((candidate) => candidate === raw.state);
  if (!operation || !template || !state) return undefined;
  const snapshot = record(raw.snapshot);
  const goal = record(raw.goal);
  return {
    operation_id: operation,
    template_id: template,
    template_name: text(raw.template_name) ?? text(snapshot.name) ?? template,
    template_version:
      typeof raw.template_version === 'number'
        ? raw.template_version
        : typeof snapshot.version === 'number'
          ? snapshot.version
          : 1,
    state,
    ...(text(goal.native_id) && text(goal.connection_id)
      ? {
          goal: {
            connection_id: text(goal.connection_id)!,
            native_id: text(goal.native_id)!,
            ...(text(goal.slug) ? { slug: text(goal.slug) } : {}),
            created: goal.created === true,
          },
        }
      : {}),
    ...(raw.planning === 'propose' || raw.planning === 'update' ? { planning: raw.planning } : {}),
    ...(text(raw.reason) ? { reason: text(raw.reason) } : {}),
    permission_requests: texts(raw.permission_requests),
    ...(raw.outcome === 'applied' || raw.outcome === 'existing' ? { outcome: raw.outcome } : {}),
  };
}

export const parseTemplateApply = parseApply;

export function parseTemplateList(value: unknown): TemplateList {
  const raw = record(value);
  return {
    ...(text(raw.revision) ? { revision: text(raw.revision) } : {}),
    templates: (Array.isArray(raw.templates) ? raw.templates : []).flatMap((entry) => {
      const parsed = parsePlanTemplate(entry);
      return parsed ? [parsed] : [];
    }),
    applies: (Array.isArray(raw.applies) ? raw.applies : []).flatMap((entry) => {
      const parsed = parseApply(entry);
      return parsed ? [parsed] : [];
    }),
  };
}

export function parseTemplatePreview(value: unknown): TemplatePreview {
  const raw = record(value);
  return {
    template_id: text(record(raw.template).template_id) ?? '',
    problems: records(raw.problems).flatMap((problem) => {
      const code = text(problem.code);
      return code ? [{ code, at: text(problem.at) ?? '' }] : [];
    }),
    goal_title: text(raw.goal_title) ?? '',
    steps: records(raw.steps).flatMap((step) => {
      const key = text(step.key),
        target = text(step.target_id),
        title = text(step.title);
      if (!key || !target || !title) return [];
      return [
        {
          key,
          target_id: target,
          title,
          ticket: ticketChoice(step.ticket),
          after: after(step.after),
          ...(text(step.priority) ? { priority: text(step.priority) } : {}),
          ...(text(step.tool) ? { tool: text(step.tool) } : {}),
          validation: texts(step.validation),
        },
      ];
    }),
    skipped: texts(raw.skipped),
    targets: records(raw.targets).flatMap((target) => {
      const id = text(target.target_id);
      if (!id) return [];
      return [
        {
          target_id: id,
          ...(text(target.label) ? { label: text(target.label) } : {}),
          declared: target.declared === true,
          steps: texts(target.steps),
          apply: NEEDS.find((need) => need === target.apply) ?? 'asks',
          ticket_writes: NEEDS.find((need) => need === target.ticket_writes) ?? 'asks',
        },
      ];
    }),
    planning: raw.planning === 'update' ? 'update' : 'propose',
    placement_automatic: raw.placement_automatic === true,
  };
}

/** An input's starting value: its default, or empty (false for a boolean). */
export function initialInputs(template: PlanTemplate): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const input of template.inputs)
    values[input.key] = input.default ?? (input.kind === 'boolean' ? false : input.kind === 'number' ? '' : '');
  return values;
}

/**
 * The inputs to send: empty text is left out (the server fills defaults), numbers are numbers, and
 * booleans stay booleans. A number field that does not read as a number is sent as typed, so the server
 * names it `input_invalid`.
 */
export function inputsToSend(template: PlanTemplate, values: Readonly<Record<string, unknown>>) {
  const sent: Record<string, unknown> = {};
  for (const input of template.inputs) {
    const value = values[input.key];
    if (value === undefined || value === null || value === '') continue;
    if (input.kind === 'number' && typeof value === 'string') {
      const number = Number(value);
      sent[input.key] = value.trim() !== '' && Number.isFinite(number) ? number : value;
    } else sent[input.key] = typeof value === 'string' ? value.trim() : value;
  }
  return sent;
}

/**
 * The problems a person can see before asking the server: a required input left empty. The server's
 * preview adds the rest (a value of the wrong type, a project that is not declared); Apply stays disabled
 * while either has a problem.
 */
export function localInputProblems(
  template: PlanTemplate,
  values: Readonly<Record<string, unknown>>,
): TemplateProblem[] {
  return template.inputs
    .filter((input) => {
      if (!input.required || input.kind === 'boolean') return false;
      const value = values[input.key];
      return value === undefined || value === null || (typeof value === 'string' && value.trim() === '');
    })
    .map((input) => ({ code: 'input_missing', at: `inputs.${input.key}` }));
}

/**
 * The problems that keep Apply disabled: the local ones, then the last preview's, leaving out a preview's
 * input problem for an input the person changed since (`changed`), which the next preview judges again.
 */
export function applyProblems(
  template: PlanTemplate,
  values: Readonly<Record<string, unknown>>,
  preview: TemplatePreview | undefined,
  changed: ReadonlySet<string>,
): TemplateProblem[] {
  const local = localInputProblems(template, values);
  const seen = new Set(local.map((problem) => `${problem.code}:${problem.at}`));
  const fromServer = (preview?.template_id === template.template_id ? preview.problems : []).filter((problem) => {
    if (problem.at.startsWith('inputs.') && changed.has(problem.at.slice('inputs.'.length))) return false;
    if (problem.code === 'input_missing' && problem.at.startsWith('inputs.')) return false;
    return !seen.has(`${problem.code}:${problem.at}`);
  });
  return [...local, ...fromServer];
}

const PROBLEMS: Readonly<Record<string, string>> = {
  input_missing: 'needs a value',
  input_invalid: 'is not a value it accepts',
  input_unknown: 'is not an input of this template',
  target_unknown: 'names a project this orchestration project does not include',
  choice_unknown: 'names a step this template does not have',
};

/** One problem in words, naming the input or step it is about. */
export function problemText(template: PlanTemplate | undefined, problem: TemplateProblem): string {
  const words = PROBLEMS[problem.code] ?? problem.code.replaceAll('_', ' ');
  if (problem.at.startsWith('inputs.')) {
    const key = problem.at.slice('inputs.'.length);
    const label = template?.inputs.find((input) => input.key === key)?.label ?? key;
    return `${label} ${words}.`;
  }
  const step = /^steps\[(\d+)\]/.exec(problem.at);
  if (step) return `Step ${Number(step[1]) + 1} ${words}.`;
  if (problem.at.startsWith('choices.')) return `${problem.at.slice('choices.'.length)} ${words}.`;
  return `${words.charAt(0).toUpperCase()}${words.slice(1)}.`;
}

const GATES: Readonly<Record<string, string>> = {
  none: 'once it is finished',
  ci: 'once its CI passes',
  deployment: 'once it is deployed',
  review: 'once its review is approved',
};

/**
 * What one step of the brief asks the planner, in words, such as "Find or create “X” · after step 1 once its CI
 * passes". `numbers` names steps by their place in the brief; a step it does not number is named by its key.
 */
export function stepText(step: BriefStep, numbers: Readonly<Partial<Record<string, number>>> = {}): string {
  const parts = [`${step.ticket === 'create' ? 'Create' : 'Find or create'} “${step.title}”`];
  if (step.priority) parts.push(`${step.priority} priority`);
  if (step.tool) parts.push(`prefers ${step.tool}`);
  for (const previous of step.after) {
    const number = numbers[previous.step];
    const name = number === undefined ? previous.step : `step ${number}`;
    parts.push(`after ${name} ${GATES[previous.gates ?? 'ci'] ?? ''}`.trim());
  }
  for (const profile of step.validation) parts.push(`validated by ${profile}`);
  return parts.join(' · ');
}

/** The brief's steps by key, numbered from 1 in the order the planner gets them. */
export function stepNumbers(steps: readonly { key: string }[]): Partial<Record<string, number>> {
  return Object.fromEntries(steps.map((step, index) => [step.key, index + 1]));
}

const NEED_TEXT: Readonly<Record<AccessNeed, string>> = {
  person: 'Your own act; nothing asks',
  remembered: 'Allowed by a remembered rule',
  asks: 'Asks through the permission prompt',
};

export function accessText(need: AccessNeed): string {
  return NEED_TEXT[need];
}

/** The permission rows of the pre-run summary: what applying and the planner's ticket writes need per project. */
export function permissionRows(preview: TemplatePreview): Array<{ label: string; value: string }> {
  return preview.targets.flatMap((target) => {
    const name = target.label ?? target.target_id;
    return [
      { label: `Create Tickets in ${name}`, value: accessText(target.ticket_writes) },
      ...(target.apply === 'person' ? [] : [{ label: `Apply for ${name}`, value: accessText(target.apply) }]),
    ];
  });
}

/** How the planner takes the new goal, and when its plan starts work. */
export function planningRows(preview: TemplatePreview): Array<{ label: string; value: string }> {
  return [
    {
      label: 'Planner',
      value:
        preview.planning === 'update'
          ? 'Drafts the new goal beside the running plan (Update Plan)'
          : 'Proposes a plan for the new goal (Propose Plan)',
    },
    {
      label: 'Start Work',
      value: preview.placement_automatic
        ? 'Within the Orchestration settings once it is planned'
        : 'After you review the plan and start orchestration',
    },
  ];
}

/** The apply's outcome as a banner: what happened and what comes next. */
export function applyBanner(apply: TemplateApply): {
  tone: 'info' | 'warning' | 'danger' | 'success';
  title: string;
  detail: string;
} {
  const goal = apply.goal?.slug ?? 'The new goal';
  switch (apply.state) {
    case 'planning':
      return {
        tone: 'success',
        title: apply.outcome === 'existing' ? 'Already Applied' : 'The Planner Is Adapting the Template',
        detail:
          apply.planning === 'update'
            ? `${goal} was handed to the planner as a plan update. Review its proposed steps in the timeline; the current plan keeps running meanwhile.`
            : `${goal} was handed to the planner. Review its proposed plan in the timeline before work starts.`,
      };
    case 'waiting_planner':
      return {
        tone: 'warning',
        title: 'Waiting for the Planner',
        detail: `${goal} is open, but the planner ${
          apply.reason === 'plan_update_planner_busy' || apply.reason === undefined
            ? 'is busy with another proposal'
            : `could not take it (${apply.reason.replaceAll('_', ' ')})`
        }. Apply again later, or press Propose Plan or Update Plan: either plans every open goal.`,
      };
    case 'waiting_permission':
      return {
        tone: 'info',
        title: 'Waiting for Permission',
        detail: 'A person answers the permission request; the same apply then goes on.',
      };
    case 'denied':
      return {
        tone: 'danger',
        title: 'Permission Denied',
        detail: 'A person denied this apply. Nothing was opened.',
      };
    default:
      return apply.state satisfies never;
  }
}

/** A fresh operation id for one apply; the dialog keeps it while the same apply is retried. */
export function newApplyOperationId(random: () => string = () => crypto.randomUUID().replaceAll('-', '')): string {
  return `tpl-${random()}`.slice(0, 140);
}

/** The Save a Goal as Template… form. */
export interface SaveGoalForm {
  goal: string;
  name: string;
  description: string;
  targetsAsInputs: boolean;
}

/** An open goal that has plan steps, which Save a Goal as Template… can save. */
export interface SavableGoal {
  connection_id: string;
  native_id: string;
  slug: string;
  steps: number;
}

/** The goals Save a Goal as Template… offers, from `plan show`: open goals with plan steps. */
export function savableGoals(view: Record<string, unknown> | undefined): SavableGoal[] {
  return records(view?.goals).flatMap((goal) => {
    const connection = text(goal.connection_id),
      native = text(goal.native_id),
      steps = typeof goal.steps === 'number' ? goal.steps : 0;
    if (!connection || !native || steps < 1) return [];
    return [{ connection_id: connection, native_id: native, slug: text(goal.slug) ?? native, steps }];
  });
}

/** Why Save Template is disabled, or undefined when it may be pressed. */
export function saveProblem(form: SaveGoalForm, goals: readonly SavableGoal[]): string | undefined {
  if (goals.length === 0) return 'No open goal has plan steps to save yet';
  if (!goals.some((goal) => goal.native_id === form.goal)) return 'Choose a goal';
  if (form.name.trim() === '') return 'Name the template';
  if (form.name.trim().length > 200) return 'Use a name of at most 200 characters';
  return undefined;
}
