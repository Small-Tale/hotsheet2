import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { List } from '@kerfjs/ui/list';
import { ListDetailDialog } from '@kerfjs/ui/list-detail-dialog';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ArrowDown, ArrowUp, Flag, LayoutTemplate, ListChecks, Plus, TextCursorInput, Trash2 } from 'lucide';

import type { TemplateEditorState } from '../features/orchestration-template-editing';
import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import {
  draftChanged,
  type EditorPart,
  fieldError,
  INPUT_KINDS,
  type InputDraft,
  inputReferences,
  kindLabel,
  MAX_INPUT_TEXT,
  MAX_INPUTS,
  MAX_LINE,
  MAX_STEPS,
  MAX_TEXT,
  partLabel,
  partProblemText,
  type PlacedProblem,
  placeProblems,
  PRIORITIES,
  STEP_GATES,
  type StepDraft,
  type TemplateDraft,
} from '../orchestration-template-editor';

/** The prefix of every editor control's `name`; the rest is the draft field it edits. */
export const TEMPLATE_FIELD_PREFIX = 'tpl-edit:';

export interface OrchestrationTemplateEditorProps {
  state?: TemplateEditorState;
  /** The declared projects a step may run in. */
  projects?: ReadonlyArray<{ target_id: string; label: string }>;
  /** The AI tools a step may prefer. */
  tools?: ReadonlyArray<{ id: string; label: string }>;
  /** On a phone the dialog fills the screen and drills from the parts into one part. */
  compact?: boolean;
  /** The UX catalog shows the dialog in place; the app opens it over Settings. */
  embedded?: boolean;
}

const capitalized = (value: string) => value.charAt(0).toUpperCase() + value.slice(1);
const GATE_LABELS: Readonly<Record<string, string>> = {
  none: 'Finished',
  ci: 'CI Passed',
  deployment: 'Deployed',
  review: 'Review Approved',
};

interface Context {
  draft: TemplateDraft;
  placed: readonly PlacedProblem[];
  disabled: boolean;
  session: number;
  projects: ReadonlyArray<{ target_id: string; label: string }>;
  tools: ReadonlyArray<{ id: string; label: string }>;
}

/** The fields a part shows in a TextField (with its own error), so its banner lists only the others. */
const TEXT_FIELDS = (part: EditorPart, draft: TemplateDraft): string[] => {
  if (part === 'template') return ['name', 'description', 'instructions'];
  if (part === 'goal') return ['goal.title', 'goal.details', 'goal.tags'];
  if (part.startsWith('input:')) {
    const id = part.slice('input:'.length),
      input = draft.inputs.find((candidate) => candidate.id === id);
    const fields = ['key', 'label', 'hint', ...(input?.kind === 'choice' ? ['choices'] : [])];
    if (input?.kind === 'text') fields.push('maxLen');
    if (input && ['text', 'number', 'tool'].includes(input.kind)) fields.push('default');
    return fields.map((field) => `input.${id}.${field}`);
  }
  const id = part.slice('step:'.length);
  return ['key', 'title', 'details', 'category', 'validation'].map((field) => `step.${id}.${field}`);
};

function field(context: Context, name: string) {
  return {
    name: `${TEMPLATE_FIELD_PREFIX}${name}`,
    disabled: context.disabled,
    rootAttributes: { 'data-key': `tpl-edit-${name}-${context.session}`, 'data-template-field': name },
  };
}

function TemplatePart({ context }: { context: Context }) {
  const { draft } = context;
  return (
    <List gap="m" controlInsets="rl">
      <TextField
        {...field(context, 'name')}
        label="Name"
        value={draft.name}
        maxLength={MAX_LINE}
        required
        error={fieldError(context.placed, 'name')}
      />
      <TextField
        {...field(context, 'description')}
        label="Description"
        value={draft.description}
        multiline
        rows={2}
        maxLength={MAX_TEXT}
        hint="What the template is for, as Use Template… lists it."
        error={fieldError(context.placed, 'description')}
      />
      <TextField
        {...field(context, 'instructions')}
        label="Instructions for the Planner"
        value={draft.instructions}
        multiline
        rows={4}
        maxLength={MAX_TEXT}
        hint="Guidance the planner reads with the steps. Use {{key}} to insert an input."
        error={fieldError(context.placed, 'instructions')}
      />
    </List>
  );
}

function GoalPart({ context }: { context: Context }) {
  const { draft } = context;
  return (
    <List gap="m" controlInsets="rl">
      <TextField
        {...field(context, 'goal.title')}
        label="Goal Title"
        value={draft.goalTitle}
        maxLength={MAX_LINE}
        required
        hint="Each apply opens a new goal ticket with this title. Use {{key}} to insert an input."
        error={fieldError(context.placed, 'goal.title')}
      />
      <TextField
        {...field(context, 'goal.details')}
        label="Goal Details"
        value={draft.goalDetails}
        multiline
        rows={3}
        maxLength={MAX_TEXT}
        error={fieldError(context.placed, 'goal.details')}
      />
      <TextField
        {...field(context, 'goal.tags')}
        label="Goal Tags"
        value={draft.goalTags}
        hint="Separated by commas, at most 8."
        error={fieldError(context.placed, 'goal.tags')}
      />
    </List>
  );
}

function DefaultField({ context, input }: { context: Context; input: InputDraft }) {
  const name = `input.${input.id}.default`;
  switch (input.kind) {
    case 'boolean':
      return <Checkbox {...field(context, name)} label="On by default" checked={input.default === 'true'} />;
    case 'choice':
    case 'priority':
    case 'target': {
      const choices =
        input.kind === 'choice'
          ? input.choices
              .split('\n')
              .map((choice) => choice.trim())
              .filter(Boolean)
              .map((choice) => ({ value: choice, label: choice }))
          : input.kind === 'priority'
            ? PRIORITIES.map((value) => ({ value, label: capitalized(value) }))
            : context.projects.map((project) => ({ value: project.target_id, label: project.label }));
      return (
        <Select
          {...field(context, name)}
          label="Default"
          value={input.default}
          placeholderText="No default"
          choices={[{ value: '', label: 'No Default' }, ...choices]}
        />
      );
    }
    case 'text':
    case 'number':
    case 'tool':
      return (
        <TextField
          {...field(context, name)}
          label="Default"
          type={input.kind === 'number' ? 'number' : 'text'}
          value={input.default}
          error={fieldError(context.placed, name)}
        />
      );
    default:
      return input.kind satisfies never;
  }
}

function InputPart({ context, input }: { context: Context; input: InputDraft }) {
  const at = (name: string) => `input.${input.id}.${name}`;
  return (
    <List gap="m" controlInsets="rl">
      <TextField
        {...field(context, at('label'))}
        label="Label"
        value={input.label}
        maxLength={MAX_LINE}
        required
        error={fieldError(context.placed, at('label'))}
      />
      <TextField
        {...field(context, at('key'))}
        label="Key"
        value={input.key}
        maxLength={64}
        required
        hint={`Placeholders insert it as {{${input.key || 'key'}}}.`}
        error={fieldError(context.placed, at('key'))}
      />
      <Select
        {...field(context, at('kind'))}
        label="Kind"
        value={input.kind}
        choices={INPUT_KINDS.map((kind) => ({ value: kind, label: kindLabel(kind) }))}
      />
      <Checkbox {...field(context, at('required'))} label="Required" checked={input.required} />
      {input.kind === 'choice' ? (
        <TextField
          {...field(context, at('choices'))}
          label="Choices"
          value={input.choices}
          multiline
          rows={3}
          hint="One per line."
          error={fieldError(context.placed, at('choices'))}
        />
      ) : undefined}
      <DefaultField context={context} input={input} />
      {input.kind === 'text' ? (
        <TextField
          {...field(context, at('maxLen'))}
          label="Longest Value"
          type="number"
          min={1}
          max={MAX_INPUT_TEXT}
          value={input.maxLen}
          hint={`Characters; ${MAX_INPUT_TEXT} when empty.`}
          error={fieldError(context.placed, at('maxLen'))}
        />
      ) : undefined}
      <TextField
        {...field(context, at('hint'))}
        label="Hint"
        value={input.hint}
        maxLength={MAX_LINE}
        hint="Shown under the input in Use Template…."
        error={fieldError(context.placed, at('hint'))}
      />
    </List>
  );
}

function StepPart({ context, step }: { context: Context; step: StepDraft }) {
  const at = (name: string) => `step.${step.id}.${name}`,
    { draft } = context,
    others = draft.steps.filter((other) => other.id !== step.id && other.key);
  const withCurrent = (choices: Array<{ value: string; label: string }>, value: string) =>
    value && !choices.some((choice) => choice.value === value) ? [...choices, { value, label: value }] : choices;
  return (
    <>
      <List gap="m" controlInsets="rl">
        <TextField
          {...field(context, at('title'))}
          label="Ticket Title"
          value={step.title}
          maxLength={MAX_LINE}
          required
          hint="The step's ticket in its project. Use {{key}} to insert an input."
          error={fieldError(context.placed, at('title'))}
        />
        <Select
          {...field(context, at('target'))}
          label="Project"
          value={step.target}
          placeholderText="Choose a project"
          choices={withCurrent(
            [
              ...context.projects.map((project) => ({ value: project.target_id, label: project.label })),
              ...inputReferences(draft, 'target'),
            ],
            step.target,
          )}
        />
        <Select
          {...field(context, at('ticket'))}
          label="Ticket"
          value={step.ticket}
          choices={[
            { value: 'search', label: 'Find an Open Ticket First' },
            { value: 'create', label: 'Always Create a New Ticket' },
          ]}
        />
        <TextField
          {...field(context, at('details'))}
          label="Ticket Details"
          value={step.details}
          multiline
          rows={3}
          maxLength={MAX_TEXT}
          error={fieldError(context.placed, at('details'))}
        />
        <Select
          {...field(context, at('priority'))}
          label="Priority"
          value={step.priority}
          choices={withCurrent(
            [
              { value: '', label: 'Project Default' },
              ...PRIORITIES.map((value) => ({ value, label: capitalized(value) })),
              ...inputReferences(draft, 'priority'),
            ],
            step.priority,
          )}
        />
        <TextField
          {...field(context, at('category'))}
          label="Category"
          value={step.category}
          maxLength={64}
          hint="Such as bug, feature or task; empty for the project's default."
          error={fieldError(context.placed, at('category'))}
        />
        <Select
          {...field(context, at('tool'))}
          label="Preferred AI Tool"
          value={step.tool}
          choices={withCurrent(
            [
              { value: '', label: 'No Preference' },
              ...context.tools.map((tool) => ({ value: tool.id, label: tool.label })),
              ...inputReferences(draft, 'tool'),
            ],
            step.tool,
          )}
        />
        <Select
          {...field(context, at('when'))}
          label="Include"
          value={step.when}
          choices={withCurrent(
            [
              { value: '', label: 'Always' },
              ...inputReferences(draft, 'boolean').map((choice) => ({
                value: choice.value,
                label: `When ${choice.label.slice('Input: '.length)} Is On`,
              })),
            ],
            step.when,
          )}
        />
        <TextField
          {...field(context, at('validation'))}
          label="Test Profiles"
          value={step.validation}
          hint="Names of test profiles the project defines, separated by commas, at most 4."
          error={fieldError(context.placed, at('validation'))}
        />
      </List>
      <List gap="xs" rootAttributes={{ 'data-template-waits': step.id }}>
        <ListHeader label="Waits For" />
        {step.after.length === 0 ? (
          <List textInsets="rl">
            <Text tone="quiet" size="compact" flush>
              Starts without waiting for another step.
            </Text>
          </List>
        ) : undefined}
        <List gap="xs" controlInsets="rl">
          {step.after.map((after, index) => (
            <Row gap="xs" vAlign="bottom" wrap rootAttributes={{ 'data-template-after': String(index) }}>
              <Select
                {...field(context, at(`after.${index}.step`))}
                label="Step"
                value={after.step}
                placeholderText="Choose a step"
                choices={withCurrent(
                  others.map((other) => ({
                    value: other.key,
                    label: `${draft.steps.indexOf(other) + 1} · ${other.title.trim() || other.key}`,
                  })),
                  after.step,
                )}
              />
              <Select
                {...field(context, at(`after.${index}.gates`))}
                label="Once"
                value={after.gates}
                choices={withCurrent(
                  [
                    { value: '', label: 'CI Passed (Default)' },
                    ...STEP_GATES.map((gate): { value: string; label: string } => ({
                      value: gate,
                      label: GATE_LABELS[gate] ?? gate,
                    })),
                  ],
                  after.gates,
                )}
              />
              <Button
                ariaLabel={`Stop Waiting for Step ${index + 1}`}
                emphasis="plain"
                icon={<LucideIcon size="s" icon={Trash2} name="trash-2" />}
                action={ACTIONS.removeTemplateAfter.value}
                disabled={context.disabled}
                rootAttributes={{ 'data-step-id': step.id, 'data-index': String(index) }}
              />
            </Row>
          ))}
          <Row>
            <Button
              emphasis="secondary"
              size="compact"
              icon={<LucideIcon size="s" icon={Plus} name="plus" />}
              action={ACTIONS.addTemplateAfter.value}
              disabled={context.disabled || others.length === 0}
              rootAttributes={{ 'data-step-id': step.id }}
            >
              Wait for a Step
            </Button>
          </Row>
        </List>
      </List>
    </>
  );
}

/** Move Up, Move Down and Remove for an input or a step. */
function PartActions({ part, context }: { part: EditorPart; context: Context }) {
  const input = part.startsWith('input:'),
    items: ReadonlyArray<{ id: string }> = input ? context.draft.inputs : context.draft.steps,
    index = items.findIndex((item) => part.endsWith(`:${item.id}`)),
    noun = input ? 'Input' : 'Step',
    attributes = { 'data-template-part': part };
  return (
    <Row gap="xs" wrap rootAttributes={{ 'data-template-part-actions': part }}>
      <Button
        emphasis="secondary"
        size="compact"
        icon={<LucideIcon size="s" icon={ArrowUp} name="arrow-up" />}
        action={ACTIONS.moveTemplatePartUp.value}
        disabled={context.disabled || index <= 0}
        rootAttributes={attributes}
      >
        Move Up
      </Button>
      <Button
        emphasis="secondary"
        size="compact"
        icon={<LucideIcon size="s" icon={ArrowDown} name="arrow-down" />}
        action={ACTIONS.moveTemplatePartDown.value}
        disabled={context.disabled || index < 0 || index >= items.length - 1}
        rootAttributes={attributes}
      >
        Move Down
      </Button>
      <Button
        emphasis="secondary"
        tone="danger"
        size="compact"
        icon={<LucideIcon size="s" icon={Trash2} name="trash-2" />}
        action={ACTIONS.removeTemplatePart.value}
        disabled={context.disabled || (!input && items.length <= 1)}
        rootAttributes={attributes}
      >
        {`Remove ${noun}`}
      </Button>
    </Row>
  );
}

function partIcon(part: EditorPart) {
  if (part === 'template') return <LucideIcon icon={LayoutTemplate} name="layout-template" />;
  if (part === 'goal') return <LucideIcon icon={Flag} name="flag" />;
  if (part.startsWith('input:')) return <LucideIcon icon={TextCursorInput} name="text-cursor-input" />;
  return <LucideIcon icon={ListChecks} name="list-checks" />;
}

function PartItem({ part, state, placed }: { part: EditorPart; state: TemplateEditorState; placed: PlacedProblem[] }) {
  const count = placed.filter((problem) => problem.part === part).length,
    { draft } = state;
  const description = part.startsWith('input:')
    ? (() => {
        const input = draft.inputs.find((candidate) => `input:${candidate.id}` === part);
        return input ? [kindLabel(input.kind), input.required ? 'required' : 'optional'].join(' · ') : undefined;
      })()
    : part.startsWith('step:')
      ? draft.steps.find((candidate) => `step:${candidate.id}` === part)?.target || undefined
      : undefined;
  return (
    <ListItem
      label={partLabel(draft, part)}
      description={description}
      icon={partIcon(part)}
      selected={state.part === part}
      action={ACTIONS.selectTemplatePart.value}
      status={
        count > 0 ? (
          <Badge tone="danger" appearance="quiet" size="compact">
            {count === 1 ? '1 problem' : `${count} problems`}
          </Badge>
        ) : undefined
      }
      rootAttributes={{ 'data-template-part': part, 'data-key': `tpl-part-${part}` }}
    />
  );
}

/**
 * HS2-1XCW6N: the visual template editor, opened from Settings › Orchestration › Templates. A Kerf
 * `ListDetailDialog` like Use Template…: the template's parts on the left (the template, its goal, each
 * input and each step, with Add Input and Add Step), the selected part's fields on the right with the
 * validator's problems in place, and Save Template in the footer. On a phone it is the compact drill-down.
 */
export function OrchestrationTemplateEditor({
  state,
  projects = [],
  tools = [],
  compact = false,
  embedded = false,
}: OrchestrationTemplateEditorProps) {
  if (!state) return <></>;
  const { draft } = state,
    retired = draft.state === 'retired',
    placed = placeProblems(draft, state.problems),
    context: Context = {
      draft,
      placed,
      disabled: retired || state.busy,
      session: state.session,
      projects,
      tools,
    },
    part = state.part,
    shown = new Set(TEXT_FIELDS(part, draft)),
    partProblems = partProblemText(
      placed.filter((problem) => problem.part === part),
      shown,
    ),
    changed = draftChanged(draft, state.original),
    labelOf = (target: string) => projects.find((project) => project.target_id === target)?.label ?? target;
  const listState: TemplateEditorState = {
    ...state,
    draft: {
      ...draft,
      // The list names a step's project by its label.
      steps: draft.steps.map((step) => ({ ...step, target: step.target ? labelOf(step.target) : '' })),
    },
  };
  const listContent = (
    <List gap="xs">
      <PartItem part="template" state={listState} placed={placed} />
      <PartItem part="goal" state={listState} placed={placed} />
      <ListHeader label="Inputs" />
      {draft.inputs.length === 0 ? (
        <List textInsets="rl">
          <Text tone="quiet" size="compact" flush>
            No inputs: every apply is the same.
          </Text>
        </List>
      ) : undefined}
      {draft.inputs.map((input) => (
        <PartItem part={`input:${input.id}`} state={listState} placed={placed} />
      ))}
      <ListHeader label="Steps" />
      {draft.steps.map((step) => (
        <PartItem part={`step:${step.id}`} state={listState} placed={placed} />
      ))}
    </List>
  );
  const listFooter = (
    <Toolbar
      label="Add Parts"
      position="footer"
      leading={
        <Row gap="xs" wrap>
          <Button
            emphasis="plain"
            action={ACTIONS.addTemplateInput.value}
            disabled={context.disabled || draft.inputs.length >= MAX_INPUTS}
            icon={<LucideIcon size="s" icon={Plus} name="plus" />}
          >
            Add Input
          </Button>
          <Button
            emphasis="plain"
            action={ACTIONS.addTemplateStep.value}
            disabled={context.disabled || draft.steps.length >= MAX_STEPS}
            icon={<LucideIcon size="s" icon={Plus} name="plus" />}
          >
            Add Step
          </Button>
        </Row>
      }
    />
  );
  const input = part.startsWith('input:') ? draft.inputs.find((item) => `input:${item.id}` === part) : undefined,
    step = part.startsWith('step:') ? draft.steps.find((item) => `step:${item.id}` === part) : undefined;
  const content = (
    <List gap="m" rootAttributes={{ 'data-template-editor-part': part }}>
      {retired ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="info"
            title="Retired"
            detail="A retired template can no longer be applied, and it is kept as it was."
          />
        </List>
      ) : undefined}
      {state.failure ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone={state.conflict ? 'warning' : 'danger'}
            urgency="alert"
            title={state.conflict ? 'Changed Elsewhere' : 'Not Saved'}
            detail={
              state.conflict
                ? `The templates changed elsewhere since you opened this. Reload Templates shows them as they are now; ${
                    draft.template_id ? 'your changes to this template are discarded.' : 'your new template is kept.'
                  }`
                : state.failure
            }
            action={
              state.conflict ? (
                <Button emphasis="secondary" size="compact" action={ACTIONS.reloadTemplate.value}>
                  Reload Templates
                </Button>
              ) : undefined
            }
          />
        </List>
      ) : undefined}
      {partProblems.length > 0 ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="danger"
            title={partProblems.length === 1 ? '1 Problem' : `${partProblems.length} Problems`}
            detail={partProblems.join(' ')}
            rootAttributes={{ 'data-template-part-problems': part }}
          />
        </List>
      ) : undefined}
      {part === 'template' ? (
        <TemplatePart context={context} />
      ) : part === 'goal' ? (
        <GoalPart context={context} />
      ) : input ? (
        <InputPart context={context} input={input} />
      ) : step ? (
        <StepPart context={context} step={step} />
      ) : undefined}
      {input || step ? (
        <List controlInsets="rl">
          <PartActions part={part} context={context} />
        </List>
      ) : undefined}
    </List>
  );
  const total = placed.length;
  const footer = (
    <Toolbar
      label="Template Actions"
      position="footer"
      leading={
        state.validating ? (
          <LoadingSpinner size="s" label="Checking the template" />
        ) : total > 0 ? (
          <Text tone="danger" size="compact" flush>
            {total === 1 ? '1 problem' : `${total} problems`}
          </Text>
        ) : undefined
      }
      trailing={
        retired ? (
          <Button emphasis="primary" rootAttributes={{ 'data-dialog-close': 'true' }}>
            Close
          </Button>
        ) : (
          <>
            <Button emphasis="secondary" rootAttributes={{ 'data-dialog-close': 'true' }}>
              Cancel
            </Button>
            <Button
              emphasis="primary"
              action={ACTIONS.saveTemplate.value}
              disabled={state.busy || (!changed && Boolean(draft.template_id))}
              icon={state.busy ? <LoadingSpinner size="s" /> : undefined}
            >
              Save Template
            </Button>
          </>
        )
      }
    />
  );
  const title = partLabel(draft, part);
  const heading = draft.template_id ? draft.name.trim() || 'Template' : 'New Template';
  return (
    <div data-key="orchestration-template-editor">
      <ListDetailDialog
        id="orchestration-template-editor"
        label={retired ? `Template: ${heading}` : `Edit Template: ${heading}`}
        compact={compact}
        embedded={embedded}
        detailActive={state.detailActive}
        backLabel="Back to Parts"
        height="viewport"
        list={{ title: heading, content: listContent, footer: listFooter }}
        detail={{
          title,
          description:
            part === 'template'
              ? draft.template_id
                ? `Version ${draft.version}. Saving checks it in as the next version.`
                : 'Saving checks it in at .hotsheet2/orchestration-templates.json.'
              : part === 'goal'
                ? 'The goal ticket each apply opens for the planner.'
                : input
                  ? 'A value the person fills in when applying.'
                  : 'One step the planner plans from the template.',
          content,
          footer,
        }}
        rootAttributes={{
          'data-key': 'orchestration-template-editor',
          'data-dialog-component': 'orchestration-template-editor',
          ...(embedded ? {} : { 'data-controlled-open': 'true' }),
        }}
      />
    </div>
  );
}
