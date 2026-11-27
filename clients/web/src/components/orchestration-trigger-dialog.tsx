import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { px, remify } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListDetailDialog } from '@kerfjs/ui/list-detail-dialog';
import { ListItem } from '@kerfjs/ui/list-item';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { RadioGroup } from '@kerfjs/ui/radio-group';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { CalendarClock, GitCommitHorizontal, Pause, Play, Plus, Zap } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import type { PlanTemplate, TemplateInput } from '../orchestration-templates';
import {
  EVENT_FIELD_LABELS,
  EVENT_LABELS,
  eventFields,
  formProblem,
  historyRows,
  nextRows,
  overlapBanner,
  permissionRows,
  scopeRows,
  sourceText,
  stateBadge,
  stateBanner,
  statusRows,
  type TriggerEvent,
  type TriggerForm,
  triggerProblemText,
  type TriggerReview,
  type TriggerView,
  warningText,
} from '../orchestration-triggers';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';
import { ScheduleRecurrenceFields } from './schedule-recurrence-fields';
import { TemplateInputField } from './template-input-field';

/** The open Triggers… dialog (HS2-KXKA7X). */
export interface OrchestrationTriggerDialogState {
  /** The triggers are being read. */
  loading: boolean;
  /** Reading the triggers failed. */
  error?: string;
  triggers: readonly TriggerView[];
  /** The selected trigger's id. */
  selected?: string;
  /** `detail` shows the selected trigger; `setup` is New Trigger… or Edit…. */
  mode: 'detail' | 'setup';
  /** The trigger Edit… changes; absent for New Trigger…. */
  editing?: string;
  /** On a phone: the detail is pushed over the list. */
  detailActive: boolean;
  form: TriggerForm;
  /** The template inputs given a fixed value, by key. */
  fixedInputs: Readonly<Partial<Record<string, unknown>>>;
  /** The checked-in templates the setup offers. */
  templates: readonly PlanTemplate[];
  /** The declared projects, for names and the project choice. */
  projects: ReadonlyArray<{ target_id: string; label: string }>;
  /** The orchestration project's own name (where goal tickets open). */
  ownerName: string;
  /** The last review of the selected trigger or of the setup. */
  review?: TriggerReview;
  reviewing: boolean;
  /** A save or state change is on its way. */
  busy: boolean;
  /** A refused request, in words. */
  failure?: string;
  /** When the dialog last read the triggers, for "Today" and "Yesterday". */
  now: number;
  /** Increments per open, so fields never show a previous session's text. */
  session: number;
}

export interface OrchestrationTriggerDialogProps {
  state?: OrchestrationTriggerDialogState;
  compact?: boolean;
  embedded?: boolean;
}

const EVENTS = (Object.keys(EVENT_LABELS) as TriggerEvent[]).map((value) => ({ value, label: EVENT_LABELS[value] }));

function sourceIcon(view: Pick<TriggerView, 'source'>) {
  switch (view.source.kind) {
    case 'schedule':
      return <LucideIcon icon={CalendarClock} name="calendar-clock" />;
    case 'commit_landed':
      return <LucideIcon icon={GitCommitHorizontal} name="git-commit-horizontal" />;
    case 'ticket_opened':
    case 'gate_failed':
    case 'branch_ci_failed':
      return <LucideIcon icon={Zap} name="zap" />;
    default:
      return view.source satisfies never;
  }
}

const labelsOf = (state: OrchestrationTriggerDialogState) =>
  Object.fromEntries(state.projects.map((project) => [project.target_id, project.label]));

function Rows({ label, rows }: { label: string; rows: ReadonlyArray<{ label: string; value: string }> }) {
  return (
    <ValueTable label={label} density="compact" stackBelow="narrow">
      {rows.map((row) => (
        <ValueTableRow label={row.label} value={row.value} />
      ))}
    </ValueTable>
  );
}

/** Scope, Permissions and Next Actions: what the review answers, side-effect free. */
function ReviewColumn({ state }: { state: OrchestrationTriggerDialogState }) {
  const review = state.review,
    labels = labelsOf(state);
  if (!review)
    return (
      <List textInsets="rl">
        <Text tone="quiet" flush>
          {state.reviewing ? 'Reading what this trigger may do…' : 'Fill in the trigger to see what it may do.'}
        </Text>
      </List>
    );
  return (
    <List gap="m" rootAttributes={{ 'data-trigger-review': review.digest }}>
      <OrchestrationInspectorSection label="Scope">
        <Rows label="Scope" rows={scopeRows(review, labels, state.ownerName)} />
      </OrchestrationInspectorSection>
      <OrchestrationInspectorSection
        label="Permissions"
        note="Runs ask like any orchestrator action. Turning a trigger on never creates remembered rules."
      >
        <Rows label="Permissions" rows={permissionRows(review, labels)} />
      </OrchestrationInspectorSection>
      <OrchestrationInspectorSection label="Next Actions">
        <Rows label="Next Actions" rows={nextRows(review, state.now)} />
      </OrchestrationInspectorSection>
    </List>
  );
}

function TriggerDetail({ state, view }: { state: OrchestrationTriggerDialogState; view: TriggerView }) {
  const labels = labelsOf(state),
    overlap = overlapBanner(view),
    banner = stateBanner(view),
    warning = warningText(view),
    runs = historyRows(view, state.now, labels);
  const dry = view.state === 'dry_run';
  return (
    <List gap="m" rootAttributes={{ 'data-trigger-detail': view.trigger_id, 'data-trigger-state': view.state }}>
      {overlap ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="warning"
            title={overlap.title}
            detail={overlap.detail}
            rootAttributes={{ 'data-trigger-overlap': 'true' }}
            action={
              overlap.goal ? (
                <Button
                  size="compact"
                  shape="pill"
                  action={ORCHESTRATION_ACTIONS.openOrchestrationTriggerGoal.value}
                  rootAttributes={{ 'data-goal': overlap.goal }}
                >
                  {`Open ${overlap.goal}`}
                </Button>
              ) : undefined
            }
          />
        </List>
      ) : banner ? (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone={banner.tone}
            title={banner.title}
            detail={banner.detail}
            rootAttributes={{ 'data-trigger-banner': view.state }}
            action={
              banner.turnOn ? (
                <Button
                  size="compact"
                  shape="pill"
                  action={ORCHESTRATION_ACTIONS.turnOnOrchestrationTrigger.value}
                  disabled={state.busy || (state.review?.problems.length ?? 0) > 0}
                  icon={<LucideIcon size="s" icon={Play} name="play" />}
                >
                  Turn On…
                </Button>
              ) : undefined
            }
          />
        </List>
      ) : undefined}
      {warning ? (
        <List controlInsets="trl">
          <StateBanner copyLayout="stacked" tone="warning" title="Cannot Watch Its Source" detail={warning} />
        </List>
      ) : undefined}
      {state.failure ? (
        <List controlInsets="trl">
          <StateBanner copyLayout="stacked" tone="danger" urgency="alert" title="Not Changed" detail={state.failure} />
        </List>
      ) : undefined}
      <Grid minColumnWidth={px(320)} gap="m">
        <List gap="m">
          <OrchestrationInspectorSection label="Status">
            <Rows label="Status" rows={statusRows(view, labels, state.now)} />
          </OrchestrationInspectorSection>
          <OrchestrationInspectorSection
            label={dry ? 'Would Have Run' : 'Recent Runs'}
            note={
              runs.length === 0
                ? dry
                  ? 'Nothing yet. Each event records what it would have done.'
                  : 'Nothing has run yet.'
                : undefined
            }
          >
            {runs.length > 0 ? (
              <ValueTable label={dry ? 'Would Have Run' : 'Recent Runs'} density="compact" stackBelow="narrow">
                {runs.map((row) => (
                  <ValueTableRow
                    label={row.label}
                    value={row.value}
                    rootAttributes={{ 'data-trigger-run': row.outcome }}
                  />
                ))}
              </ValueTable>
            ) : undefined}
          </OrchestrationInspectorSection>
        </List>
        <ReviewColumn state={state} />
      </Grid>
    </List>
  );
}

/** HS2-SEEMXR: the command-schedule dialog's own recurrence fields, named `trigger-schedule-<draft key>`. */
function ScheduleFields({ state }: { state: OrchestrationTriggerDialogState }) {
  return (
    <List gap="m">
      <ScheduleRecurrenceFields
        draft={state.form.schedule}
        namePrefix="trigger-schedule-"
        fieldAttributes={(field) => ({ 'data-key': `trigger-schedule-${field}-${state.session}` })}
      />
    </List>
  );
}

function EventFields({ state }: { state: OrchestrationTriggerDialogState }) {
  const form = state.form,
    key = (name: string) => ({ 'data-key': `trigger-form-${name}-${state.session}` });
  const branch = form.event === 'branch_ci_failed' || form.event === 'commit_landed';
  return (
    <List gap="m">
      <Grid minColumnWidth={px(180)} gap="m">
        <Select
          name="trigger-form-target"
          label="Project"
          value={form.target}
          placeholderText={form.event === 'gate_failed' ? 'Any project' : 'Choose a project'}
          choices={[
            ...(form.event === 'gate_failed' ? [{ value: '', label: 'Any Project' }] : []),
            ...state.projects.map((project) => ({ value: project.target_id, label: project.label })),
          ]}
        />
        <Select name="trigger-form-event" label="Event" value={form.event} choices={EVENTS} />
      </Grid>
      {branch ? (
        <Grid minColumnWidth={px(180)} gap="m">
          <TextField name="trigger-form-branch" label="Branch" value={form.branch} rootAttributes={key('branch')} />
          <TextField
            name="trigger-form-workflow"
            label="Workflow"
            value={form.workflow}
            placeholder="ci.yml"
            rootAttributes={key('workflow')}
          />
          <TextField
            name="trigger-form-repository"
            label="Repository"
            value={form.repository}
            placeholder="owner/name"
            hint="As the project’s Git remote names it"
            rootAttributes={key('repository')}
          />
          <TextField
            name="trigger-form-provider"
            label="CI Provider"
            value={form.provider}
            placeholder="github-actions"
            rootAttributes={key('provider')}
          />
        </Grid>
      ) : form.event === 'ticket_opened' ? (
        <Grid minColumnWidth={px(180)} gap="m">
          <TextField name="trigger-form-tag" label="With Tag" value={form.tag} rootAttributes={key('tag')} />
          <TextField
            name="trigger-form-category"
            label="Of Category"
            value={form.category}
            rootAttributes={key('category')}
          />
        </Grid>
      ) : undefined}
    </List>
  );
}

/** The template's default for an input, in words ("Default: Self"), or nothing when it has none. */
function defaultText(state: OrchestrationTriggerDialogState, input: TemplateInput): string | undefined {
  if (input.default === undefined) return undefined;
  const value =
    typeof input.default === 'string'
      ? input.kind === 'target'
        ? (labelsOf(state)[input.default] ?? input.default)
        : input.default
      : JSON.stringify(input.default);
  return `Default: ${value}`;
}

function WhatRuns({ state }: { state: OrchestrationTriggerDialogState }) {
  const form = state.form,
    template = state.templates.find((candidate) => candidate.template_id === form.template),
    fields = eventFields(form.when === 'schedule' ? 'schedule' : form.event);
  return (
    <List gap="m">
      <Select
        name="trigger-form-template"
        label="Template"
        value={form.template}
        placeholderText="No template"
        choices={[
          { value: '', label: 'No Template' },
          ...state.templates
            .filter((candidate) => candidate.state === 'active')
            .map((candidate) => ({ value: candidate.template_id, label: candidate.name })),
        ]}
      />
      {template
        ? template.inputs.map((input) => (
            <Grid minColumnWidth={px(180)} gap="m" rootAttributes={{ 'data-trigger-input': input.key }}>
              <Select
                name={`trigger-event-input-${input.key}`}
                label={input.label}
                value={form.eventInputs[input.key] ?? ''}
                choices={[
                  { value: '', label: 'Fixed Value' },
                  ...fields.map((field) => ({ value: field, label: `From the Event: ${EVENT_FIELD_LABELS[field]}` })),
                ]}
              />
              {form.eventInputs[input.key] ? undefined : (
                // HS2-40GPZK: Use Template…'s typed control, so a fixed value cannot be mistyped.
                <TemplateInputField
                  input={input}
                  value={
                    input.kind === 'boolean'
                      ? (state.fixedInputs[input.key] ?? input.default)
                      : state.fixedInputs[input.key]
                  }
                  projects={state.projects}
                  name={`trigger-input-${input.key}`}
                  label="Value"
                  emptyChoice={defaultText(state, input) ?? 'Template Default'}
                  {...(defaultText(state, input) === undefined ? {} : { placeholder: defaultText(state, input) })}
                  rootAttributes={{ 'data-key': `trigger-input-${input.key}-${state.session}` }}
                />
              )}
            </Grid>
          ))
        : undefined}
      {template ? (
        <TextField
          name="trigger-form-tool"
          label="Planner AI Tool"
          value={form.tool}
          rootAttributes={{ 'data-key': `trigger-form-tool-${state.session}` }}
        />
      ) : undefined}
      <RadioGroup
        name="trigger-form-placement"
        label="Orchestration"
        value={form.placement === '' ? 'none' : form.placement}
        choices={[
          { value: 'none', label: 'Leave as it is' },
          { value: 'start', label: 'Start orchestration', hint: 'Turns automatic placement on' },
          { value: 'pause', label: 'Pause orchestration', hint: 'Turns automatic placement off' },
        ]}
      />
    </List>
  );
}

function SetupDetail({ state }: { state: OrchestrationTriggerDialogState }) {
  const form = state.form,
    problems = state.review?.problems ?? [],
    local = formProblem(form);
  return (
    <List gap="m" rootAttributes={{ 'data-trigger-setup': state.editing ?? 'new' }}>
      {/* HS2-CJYD5A: both banner slots are always present (hidden when empty), so a banner a review answer adds or
          removes never shifts the columns below. A shift moved or rebuilt them, which closed a select a person had
          open (WebKit has no state-preserving moveBefore()). */}
      <List controlInsets="trl" hidden={!state.failure}>
        {state.failure ? (
          <StateBanner copyLayout="stacked" tone="danger" urgency="alert" title="Save Failed" detail={state.failure} />
        ) : undefined}
      </List>
      <List controlInsets="trl" hidden={problems.length === 0 || Boolean(local)}>
        {problems.length > 0 && !local ? (
          <StateBanner
            copyLayout="stacked"
            tone="danger"
            title={
              problems.length === 1 ? '1 Problem Keeps Save Disabled' : `${problems.length} Problems Keep Save Disabled`
            }
            detail={problems.map(triggerProblemText).join(' ')}
          />
        ) : undefined}
      </List>
      <Grid minColumnWidth={px(320)} gap="m">
        <List gap="m">
          <OrchestrationInspectorSection label="When">
            <List gap="m" controlInsets="rl">
              <TextField
                name="trigger-form-name"
                label="Name"
                value={form.name}
                maxLength={200}
                required
                rootAttributes={{ 'data-key': `trigger-form-name-${state.session}` }}
              />
              <RadioGroup
                name="trigger-form-when"
                label="Runs"
                orientation="horizontal"
                value={form.when}
                choices={[
                  { value: 'schedule', label: 'On a schedule' },
                  { value: 'event', label: 'On a project event' },
                ]}
              />
              {form.when === 'schedule' ? <ScheduleFields state={state} /> : <EventFields state={state} />}
            </List>
          </OrchestrationInspectorSection>
          <OrchestrationInspectorSection label="What Runs">
            <List controlInsets="rl">
              <WhatRuns state={state} />
            </List>
          </OrchestrationInspectorSection>
          <OrchestrationInspectorSection label="Mode">
            <List gap="m" controlInsets="rl">
              {state.editing ? undefined : (
                <RadioGroup
                  name="trigger-form-start"
                  label="Start As"
                  value={form.start}
                  choices={[
                    {
                      value: 'dry_run',
                      label: 'Dry run',
                      hint: 'Records what each event would do. Creates nothing and asks nobody.',
                    },
                    { value: 'paused', label: 'Paused', hint: 'Ignores events until you resume it.' },
                  ]}
                />
              )}
              <RadioGroup
                name="trigger-form-overlap"
                label="While a Run Is Working"
                value={form.overlap}
                choices={[
                  { value: 'skip', label: 'Skip and record it' },
                  { value: 'queue_latest', label: 'Run the latest once the current run finishes' },
                ]}
              />
            </List>
          </OrchestrationInspectorSection>
        </List>
        <ReviewColumn state={state} />
      </Grid>
    </List>
  );
}

/**
 * HS2-KXKA7X: Triggers… (design v1). A Kerf `ListDetailDialog`: this machine's triggers with an On, Dry Run,
 * Paused or Review badge and New Trigger… in the list footer; the selected trigger's Status, Recent Runs (or
 * Would Have Run) and its review (Scope, Permissions, Next Actions), with Edit… and Pause or Resume. New
 * Trigger… and Edit… show the setup with the live review beside it, saved as Dry Run; Turn On… is the reviewed
 * act. On a phone it is the compact drill-down.
 */
export function OrchestrationTriggerDialog({
  state,
  compact = false,
  embedded = false,
}: OrchestrationTriggerDialogProps) {
  if (!state) return <></>;
  const labels = labelsOf(state);
  const view = state.triggers.find((candidate) => candidate.trigger_id === state.selected);
  const newRow =
    state.mode === 'setup' && !state.editing ? (
      <ListItem
        label="New Trigger"
        description="Not saved"
        icon={<LucideIcon icon={Plus} name="plus" />}
        selected
        action={ORCHESTRATION_ACTIONS.newOrchestrationTrigger.value}
        rootAttributes={{ 'data-key': 'trigger-new-row' }}
      />
    ) : undefined;
  const listContent = state.loading ? (
    <List fill hAlign="center" vAlign="middle">
      <LoadingSpinner label="Reading the triggers" />
    </List>
  ) : state.triggers.length === 0 && !newRow ? (
    <List fill hAlign="center" vAlign="middle">
      <EmptyState
        icon={<LucideIcon icon={Zap} name="zap" />}
        title="No Triggers Yet"
        detail={state.error ?? 'A trigger applies a template or starts orchestration on a schedule or a project event.'}
      />
    </List>
  ) : (
    <List gap="xs">
      {newRow}
      {state.triggers.map((candidate) => {
        const badge = stateBadge(candidate.state);
        return (
          <ListItem
            label={candidate.name}
            description={sourceText(candidate.source, labels)}
            icon={sourceIcon(candidate)}
            multiline
            selected={candidate.trigger_id === state.selected && !(state.mode === 'setup' && !state.editing)}
            action={ORCHESTRATION_ACTIONS.selectOrchestrationTrigger.value}
            trailing={
              <Badge tone={badge.tone} size="compact" rootAttributes={{ 'data-trigger-badge': candidate.state }}>
                {badge.label}
              </Badge>
            }
            rootAttributes={{ 'data-trigger-id': candidate.trigger_id, 'data-key': candidate.trigger_id }}
          />
        );
      })}
    </List>
  );
  const listFooter = (
    <Toolbar
      label="Trigger Actions"
      position="footer"
      leading={
        <Button
          emphasis="plain"
          action={ORCHESTRATION_ACTIONS.newOrchestrationTrigger.value}
          icon={<LucideIcon size="s" icon={Plus} name="plus" />}
        >
          New Trigger…
        </Button>
      }
    />
  );
  const setupProblem =
    formProblem(state.form) ?? (state.review?.problems.length ? 'Correct the problems first' : undefined);
  const footer =
    state.mode === 'setup' ? (
      <Toolbar
        label="Setup Actions"
        position="footer"
        trailing={
          <>
            <Button emphasis="secondary" action={ORCHESTRATION_ACTIONS.cancelOrchestrationTrigger.value}>
              Cancel
            </Button>
            <Button
              emphasis="primary"
              action={ORCHESTRATION_ACTIONS.saveOrchestrationTrigger.value}
              disabled={state.busy || setupProblem !== undefined}
              title={setupProblem}
              icon={state.busy ? <LoadingSpinner size="s" /> : undefined}
            >
              {state.editing ? 'Save Changes' : state.form.start === 'paused' ? 'Save as Paused' : 'Save as Dry Run'}
            </Button>
          </>
        }
      />
    ) : view ? (
      <Toolbar
        label="Trigger Actions"
        position="footer"
        trailing={
          <>
            <Button emphasis="secondary" action={ORCHESTRATION_ACTIONS.editOrchestrationTrigger.value}>
              Edit…
            </Button>
            {view.state === 'paused' ? (
              <Button
                emphasis="secondary"
                action={ORCHESTRATION_ACTIONS.resumeOrchestrationTrigger.value}
                disabled={state.busy}
                icon={<LucideIcon size="s" icon={Play} name="play" />}
              >
                Resume
              </Button>
            ) : (
              <Button
                emphasis="secondary"
                action={ORCHESTRATION_ACTIONS.pauseOrchestrationTrigger.value}
                disabled={state.busy}
                icon={<LucideIcon size="s" icon={Pause} name="pause" />}
              >
                Pause
              </Button>
            )}
          </>
        }
      />
    ) : undefined;
  const title =
    state.mode === 'setup'
      ? state.editing
        ? `Edit ${view?.name ?? 'Trigger'}`
        : 'New Trigger'
      : (view?.name ?? 'Triggers');
  const description =
    state.mode === 'setup'
      ? 'Review exactly what it may do before it runs. Saving changes nothing in any project.'
      : view
        ? sourceText(view.source, labels)
        : undefined;
  const content =
    state.mode === 'setup' ? (
      <SetupDetail state={state} />
    ) : view ? (
      <TriggerDetail state={state} view={view} />
    ) : (
      <List fill hAlign="center" vAlign="middle">
        <EmptyState
          icon={<LucideIcon icon={Zap} name="zap" />}
          title="Choose a Trigger"
          detail="Triggers live on this computer. New ones start as a dry run that records what would have happened."
        />
      </List>
    );
  return (
    <div data-key="orchestration-trigger-dialog">
      <ListDetailDialog
        id="orchestration-trigger-dialog"
        label="Triggers"
        compact={compact}
        embedded={embedded}
        detailActive={state.detailActive}
        backLabel="Back to Triggers"
        height="viewport"
        preferredWidth={remify(1240)}
        list={{ title: 'Triggers', content: listContent, footer: listFooter }}
        detail={{ title, description, content, footer }}
        rootAttributes={{
          'data-key': 'orchestration-trigger-dialog',
          'data-dialog-component': 'orchestration-trigger-dialog',
          ...(embedded ? {} : { 'data-controlled-open': 'true' }),
        }}
      />
    </div>
  );
}
