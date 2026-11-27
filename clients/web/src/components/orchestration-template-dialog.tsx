import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListDetailDialog } from '@kerfjs/ui/list-detail-dialog';
import { ListItem } from '@kerfjs/ui/list-item';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { LayoutTemplate, Plus } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import {
  applyBanner,
  applyProblems,
  permissionRows,
  planningRows,
  type PlanTemplate,
  problemText,
  type SavableGoal,
  savableGoalLabel,
  type SaveGoalForm,
  saveProblem,
  stepNumbers,
  stepText,
  type StepTicketChoice,
  type TemplateApply,
  type TemplateInput,
  type TemplatePreview,
  unverifiedProfilesText,
} from '../orchestration-templates';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

/** The open Use Template… dialog (HS2-A4RVYA). */
export interface OrchestrationTemplateDialogState {
  /** The templates are being read. */
  loading: boolean;
  /** Reading the templates failed. */
  error?: string;
  templates: readonly PlanTemplate[];
  /** The selected template's id. */
  selected?: string;
  /** `template` fills a template's inputs, `save` is Save a Goal as Template…, `result` is an apply's outcome. */
  mode: 'template' | 'save' | 'result';
  /** On a phone: the detail is pushed over the list. */
  detailActive: boolean;
  /** The typed input values of the selected template, by key. */
  inputs: Readonly<Record<string, unknown>>;
  /** Inputs changed since the last preview. */
  changed: readonly string[];
  /** Steps the person switched between search-first and create. */
  choices: Readonly<Record<string, StepTicketChoice>>;
  /** The last pre-run summary of the selected template. */
  preview?: TemplatePreview;
  previewing: boolean;
  /** An apply or save is on its way. */
  busy: boolean;
  /** The last apply's outcome. */
  result?: TemplateApply;
  /** A refused apply or save, in words. */
  failure?: string;
  /** The declared projects a project input offers. */
  projects: ReadonlyArray<{ target_id: string; label: string }>;
  save: SaveGoalForm;
  goals: readonly SavableGoal[];
  /** Increments per open, so fields never show a previous session's text. */
  session: number;
}

export interface OrchestrationTemplateDialogProps {
  state?: OrchestrationTemplateDialogState;
  /** On a phone the dialog fills the screen and drills from the list into the detail. */
  compact?: boolean;
  /** The UX catalog shows the dialog in place; the app opens it over the view. */
  embedded?: boolean;
}

const PRIORITY_CHOICES = ['highest', 'high', 'default', 'low', 'lowest'].map((value) => ({
  value,
  label: value.charAt(0).toUpperCase() + value.slice(1),
}));

function InputField({
  input,
  value,
  projects,
  session,
}: {
  input: TemplateInput;
  value: unknown;
  projects: OrchestrationTemplateDialogState['projects'];
  session: number;
}) {
  const name = `template-input-${input.key}`,
    label = input.label,
    rootAttributes = { 'data-key': `${name}-${session}`, 'data-template-input': input.key };
  switch (input.kind) {
    case 'boolean':
      return (
        <Checkbox
          name={name}
          label={input.label}
          checked={value === true}
          hint={input.hint}
          rootAttributes={rootAttributes}
        />
      );
    case 'choice':
    case 'priority':
    case 'target': {
      const choices =
        input.kind === 'choice'
          ? input.choices.map((choice) => ({ value: choice, label: choice }))
          : input.kind === 'priority'
            ? PRIORITY_CHOICES
            : projects.map((project) => ({ value: project.target_id, label: project.label }));
      return (
        <Select
          name={name}
          label={label}
          value={typeof value === 'string' ? value : ''}
          choices={choices}
          placeholderText={input.kind === 'target' ? 'Choose a project' : 'Choose'}
          hint={input.hint}
        />
      );
    }
    case 'text':
    case 'number':
    case 'tool':
      return (
        <TextField
          name={name}
          label={label}
          value={typeof value === 'string' || typeof value === 'number' ? String(value) : ''}
          type={input.kind === 'number' ? 'number' : 'text'}
          hint={input.hint}
          required={input.required}
          rootAttributes={rootAttributes}
        />
      );
    default:
      return input.kind satisfies never;
  }
}

function TemplateDetail({ state, template }: { state: OrchestrationTemplateDialogState; template: PlanTemplate }) {
  const preview = state.preview?.template_id === template.template_id ? state.preview : undefined;
  const problems = applyProblems(template, state.inputs, preview, new Set(state.changed));
  // Steps are numbered as the planner gets them; before a preview, as the template lists them.
  const numbers = stepNumbers(preview?.steps ?? template.steps);
  return (
    <List gap="m" rootAttributes={{ 'data-template-detail': template.template_id }}>
      {problems.length > 0 ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="danger"
            title={
              problems.length === 1
                ? '1 Problem Keeps Apply Disabled'
                : `${problems.length} Problems Keep Apply Disabled`
            }
            detail={problems.map((problem) => problemText(template, problem, preview)).join(' ')}
          />
        </List>
      ) : undefined}
      {state.failure ? (
        <List controlInsets="trl">
          <StateBanner copyLayout="stacked" tone="danger" urgency="alert" title="Apply Failed" detail={state.failure} />
        </List>
      ) : undefined}
      {template.inputs.length > 0 ? (
        <OrchestrationInspectorSection label="Inputs">
          <List gap="m" controlInsets="rl">
            {template.inputs.map((input) => (
              <InputField
                input={input}
                value={state.inputs[input.key]}
                projects={state.projects}
                session={state.session}
              />
            ))}
          </List>
        </OrchestrationInspectorSection>
      ) : undefined}
      <OrchestrationInspectorSection
        label="What the Planner Gets"
        note="The planner adapts these steps to the projects as they are now and proposes a plan for you to review. A step that finds an existing ticket asks you whether to keep it."
      >
        {preview ? (
          <ValueTable label="Steps" density="compact" stackBelow="narrow">
            <ValueTableRow label="Goal" value={`Opens “${preview.goal_title}”`} />
            {preview.steps.map((step, index) => (
              <ValueTableRow
                label={`${index + 1} · ${
                  preview.targets.find((target) => target.target_id === step.target_id)?.label ?? step.target_id
                }`}
                value={stepText(step, numbers)}
                rootAttributes={{ 'data-template-step': step.key }}
              />
            ))}
          </ValueTable>
        ) : (
          <List textInsets="rl">
            <Text tone="quiet" flush>
              {state.previewing ? 'Reading what this template would do…' : 'Fill in the inputs to see the steps.'}
            </Text>
          </List>
        )}
        {preview && unverifiedProfilesText(preview) ? (
          <List textInsets="rl" rootAttributes={{ 'data-template-profiles-unverified': 'true' }}>
            <Text tone="quiet" size="compact" flush>
              {unverifiedProfilesText(preview)}
            </Text>
          </List>
        ) : undefined}
        {preview && preview.skipped.length > 0 ? (
          <List textInsets="rl">
            <Text tone="quiet" size="compact" flush>
              {`Left out by the inputs: ${preview.skipped.join(', ')}.`}
            </Text>
          </List>
        ) : undefined}
        <List gap="xs" controlInsets="rl">
          {template.steps
            .filter((step) => step.ticket === 'search' && numbers[step.key] !== undefined)
            .map((step) => (
              <Checkbox
                name={`template-step-create-${step.key}`}
                size="compact"
                label={`Create a New Ticket for Step ${numbers[step.key] ?? ''}`}
                hint="Instead of reusing a ticket the planner finds"
                checked={state.choices[step.key] === 'create'}
                rootAttributes={{ 'data-key': `template-step-${step.key}-${state.session}` }}
              />
            ))}
        </List>
      </OrchestrationInspectorSection>
      {preview ? (
        <OrchestrationInspectorSection
          label="Permissions"
          note="Applying is your own act. The planner is an AI session, so its ticket writes ask unless a remembered rule allows them."
        >
          <ValueTable label="Permissions" density="compact" stackBelow="narrow">
            {[...permissionRows(preview), ...planningRows(preview)].map((row) => (
              <ValueTableRow label={row.label} value={row.value} />
            ))}
          </ValueTable>
        </OrchestrationInspectorSection>
      ) : undefined}
    </List>
  );
}

function ResultDetail({ result }: { result: TemplateApply }) {
  const banner = applyBanner(result);
  return (
    <List gap="m" rootAttributes={{ 'data-template-result': result.state }}>
      <List controlInsets="trl">
        <StateBanner copyLayout="stacked" tone={banner.tone} title={banner.title} detail={banner.detail} />
      </List>
      <OrchestrationInspectorSection
        label="Applied"
        note="The result is ordinary orchestration work. Later changes to the template do not change it."
      >
        <ValueTable label="Applied" density="compact" stackBelow="narrow">
          <ValueTableRow label="Template" value={`${result.template_name} · version ${result.template_version}`} />
          <ValueTableRow label="Goal" value={result.goal?.slug ?? 'Not opened yet'} />
          <ValueTableRow
            label="Planner"
            value={
              result.planning === 'update'
                ? 'Update Plan'
                : result.planning === 'propose'
                  ? 'Propose Plan'
                  : 'Not asked yet'
            }
          />
          <ValueTableRow label="Operation" value={result.operation_id} />
        </ValueTable>
      </OrchestrationInspectorSection>
    </List>
  );
}

function SaveDetail({ state }: { state: OrchestrationTemplateDialogState }) {
  const problem = saveProblem(state.save, state.goals);
  return (
    <List gap="m" rootAttributes={{ 'data-template-save': 'true' }}>
      {state.failure ? (
        <List controlInsets="trl">
          <StateBanner copyLayout="stacked" tone="danger" urgency="alert" title="Save Failed" detail={state.failure} />
        </List>
      ) : undefined}
      {state.goals.length === 0 ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="info"
            title="No Goal Has Plan Steps Yet"
            detail="Propose a plan for a goal ticket first. Its steps are what the template saves."
          />
        </List>
      ) : undefined}
      <List gap="m" controlInsets="rl">
        <Select
          name="template-save-goal"
          label="Goal"
          value={state.save.goal}
          placeholderText="Choose a goal"
          disabled={state.goals.length === 0}
          choices={state.goals.map((goal) => ({
            value: goal.native_id,
            label: savableGoalLabel(goal),
          }))}
        />
        <TextField
          name="template-save-name"
          label="Template Name"
          value={state.save.name}
          maxLength={200}
          required
          rootAttributes={{ 'data-key': `template-save-name-${state.session}` }}
        />
        <TextField
          name="template-save-description"
          label="Description"
          value={state.save.description}
          multiline
          rows={2}
          maxLength={4000}
          rootAttributes={{ 'data-key': `template-save-description-${state.session}` }}
        />
        <Checkbox
          name="template-save-inputs"
          label="Make Each Project an Input"
          hint="Apply the template to other projects later, defaulting to these"
          checked={state.save.targetsAsInputs}
        />
      </List>
      <List textInsets="rl">
        <Text tone="quiet" size="compact" flush>
          {problem
            ? `${problem}.`
            : 'Each step of the goal becomes a step that searches its project first. Templates are checked in at .hotsheet2/orchestration-templates.json.'}
        </Text>
      </List>
    </List>
  );
}

/**
 * HS2-A4RVYA: Use Template… (design v1, adapted to the planner flow the maintainer chose). A Kerf
 * `ListDetailDialog`: the templates on the left with Save a Goal as Template…, and the selected template's
 * typed inputs, the steps its brief gives the planner, and what asks, with Apply Template in the footer.
 * Applying opens a new goal ticket and hands it to the planner; its proposed plan is the preview, reviewed
 * in the timeline. On a phone it is the compact drill-down.
 */
export function OrchestrationTemplateDialog({
  state,
  compact = false,
  embedded = false,
}: OrchestrationTemplateDialogProps) {
  if (!state) return <></>;
  const template = state.templates.find((candidate) => candidate.template_id === state.selected);
  const active = state.templates.filter((candidate) => candidate.state === 'active');
  const preview = template && state.preview?.template_id === template.template_id ? state.preview : undefined;
  const problems = template ? applyProblems(template, state.inputs, preview, new Set(state.changed)) : [];
  const listContent = state.loading ? (
    <List fill hAlign="center" vAlign="middle">
      <LoadingSpinner label="Reading the templates" />
    </List>
  ) : active.length === 0 ? (
    <List fill hAlign="center" vAlign="middle">
      <EmptyState
        icon={<LucideIcon icon={LayoutTemplate} name="layout-template" />}
        title="No Templates Yet"
        detail={
          state.error ?? 'Save a goal’s plan as a template, or check one in at .hotsheet2/orchestration-templates.json.'
        }
      />
    </List>
  ) : (
    <List gap="xs">
      {active.map((candidate) => (
        <ListItem
          label={candidate.name}
          description={candidate.description || undefined}
          icon={<LucideIcon icon={LayoutTemplate} name="layout-template" />}
          multiline
          selected={state.mode !== 'save' && candidate.template_id === state.selected}
          action={ORCHESTRATION_ACTIONS.selectOrchestrationTemplate.value}
          rootAttributes={{ 'data-template-id': candidate.template_id, 'data-key': candidate.template_id }}
        />
      ))}
    </List>
  );
  const saveButton = (
    <Toolbar
      label="Template Actions"
      position="footer"
      leading={
        <Button
          emphasis="plain"
          action={ORCHESTRATION_ACTIONS.saveOrchestrationGoalTemplate.value}
          icon={<LucideIcon size="s" icon={Plus} name="plus" />}
        >
          Save a Goal as Template…
        </Button>
      }
    />
  );
  const title =
    state.mode === 'save'
      ? 'Save a Goal as Template'
      : state.mode === 'result' && state.result
        ? `Applied ${state.result.template_name}`
        : (template?.name ?? 'Use Template');
  const description =
    state.mode === 'save'
      ? 'Saves the goal’s current steps as a template others can apply.'
      : state.mode === 'result'
        ? undefined
        : template
          ? 'Nothing changes until you apply. The planner then proposes a plan from it.'
          : undefined;
  const footer =
    state.mode === 'save' ? (
      <Toolbar
        label="Save Actions"
        position="footer"
        trailing={
          <>
            <Button emphasis="secondary" action={ORCHESTRATION_ACTIONS.cancelOrchestrationGoalTemplate.value}>
              Cancel
            </Button>
            <Button
              emphasis="primary"
              action={ORCHESTRATION_ACTIONS.submitOrchestrationGoalTemplate.value}
              disabled={state.busy || saveProblem(state.save, state.goals) !== undefined}
              icon={state.busy ? <LoadingSpinner size="s" /> : undefined}
            >
              Save Template
            </Button>
          </>
        }
      />
    ) : state.mode === 'result' && state.result ? (
      <Toolbar
        label="Result Actions"
        position="footer"
        trailing={
          <>
            {state.result.state === 'waiting_planner' ? (
              <Button
                emphasis="secondary"
                action={ORCHESTRATION_ACTIONS.applyOrchestrationTemplate.value}
                disabled={state.busy}
              >
                Apply Again
              </Button>
            ) : undefined}
            <Button emphasis="secondary" rootAttributes={{ 'data-dialog-close': 'true' }}>
              Close
            </Button>
            {state.result.goal?.created ? (
              <Button emphasis="primary" action={ORCHESTRATION_ACTIONS.openOrchestrationTemplateGoal.value}>
                {`Open ${state.result.goal.slug ?? 'Goal'}`}
              </Button>
            ) : undefined}
          </>
        }
      />
    ) : template ? (
      <Toolbar
        label="Apply Actions"
        position="footer"
        trailing={
          <>
            <Button emphasis="secondary" rootAttributes={{ 'data-dialog-close': 'true' }}>
              Cancel
            </Button>
            <Button
              emphasis="primary"
              action={ORCHESTRATION_ACTIONS.applyOrchestrationTemplate.value}
              disabled={state.busy || problems.length > 0}
              icon={state.busy ? <LoadingSpinner size="s" /> : undefined}
              title={
                problems.length > 0
                  ? problemText(template, problems[0], preview)
                  : 'Open a new goal and hand it to the planner'
              }
            >
              Apply Template
            </Button>
          </>
        }
      />
    ) : undefined;
  const content =
    state.mode === 'save' ? (
      <SaveDetail state={state} />
    ) : state.mode === 'result' && state.result ? (
      <ResultDetail result={state.result} />
    ) : template ? (
      <TemplateDetail state={state} template={template} />
    ) : (
      <List fill hAlign="center" vAlign="middle">
        <EmptyState
          icon={<LucideIcon icon={LayoutTemplate} name="layout-template" />}
          title="Choose a Template"
          detail="A template opens a new goal ticket and hands it to the planner, which proposes a plan for you to review."
        />
      </List>
    );
  return (
    <div data-key="orchestration-template-dialog">
      <ListDetailDialog
        id="orchestration-template-dialog"
        label="Use Template"
        compact={compact}
        embedded={embedded}
        detailActive={state.detailActive}
        backLabel="Back to Templates"
        height="viewport"
        list={{ title: 'Templates', content: listContent, footer: saveButton }}
        detail={{ title, description, content, footer }}
        rootAttributes={{
          'data-key': 'orchestration-template-dialog',
          'data-dialog-component': 'orchestration-template-dialog',
          ...(embedded ? {} : { 'data-controlled-open': 'true' }),
        }}
      />
    </div>
  );
}
