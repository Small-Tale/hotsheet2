import { Button } from '@kerfjs/ui/button';
import { Form } from '@kerfjs/ui/form';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import { COMMAND_DEFAULT_ACTION, occurrenceLabel, type ScheduleFormDraft } from '../command-schedule-form';
import {
  SCHEDULE_ACTION_LABELS,
  SCHEDULE_ACTIONS,
  type ScheduleAction,
  scheduleInheritedAction,
  type ScheduleView,
} from '../command-schedules';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { ScheduleRecurrenceFields } from './schedule-recurrence-fields';

export const COMMAND_SCHEDULE_DIALOG_ID = 'command-schedule-dialog';
export const COMMAND_SCHEDULE_FORM_ID = 'command-schedule-form';

/** "Use Command Default" names what the command's action resolves to, then the explicit overrides (HS2-17Y2EC). */
function actionChoices(commandAction: ScheduleAction | undefined) {
  return [
    {
      value: COMMAND_DEFAULT_ACTION,
      label: `Use Command Default (${SCHEDULE_ACTION_LABELS[scheduleInheritedAction(commandAction)]})`,
    },
    ...SCHEDULE_ACTIONS.map((value) => ({ value, label: SCHEDULE_ACTION_LABELS[value] })),
  ];
}

function displayRun(instant: string, zone: string) {
  try {
    return new Intl.DateTimeFormat('en', {
      timeZone: zone,
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
    }).format(new Date(instant));
  } catch {
    return instant;
  }
}

export interface CommandScheduleDialogProps {
  commandTitle?: string;
  /** The command's own run action, which "Use Command Default" inherits (HS2-17Y2EC). */
  commandAction?: ScheduleAction;
  draft?: ScheduleFormDraft;
  saved?: ScheduleView;
  preview?: readonly string[];
  previewBusy?: boolean;
  busy?: boolean;
  error?: string;
}

/** The approved schedule form; the controller owns its draft and API requests (HS2-17663F). */
export function CommandScheduleDialog({
  commandTitle,
  commandAction,
  draft,
  saved,
  preview = [],
  previewBusy = false,
  busy = false,
  error = '',
}: CommandScheduleDialogProps) {
  const upcoming = preview.map((run) => displayRun(run, draft?.timeZone ?? 'UTC'));
  const latestSkip = saved?.history
    .slice()
    .reverse()
    .find((item) => item.outcome === 'skipped_open_ticket' && item.ticket_slug);
  return (
    <DialogSurface
      id={COMMAND_SCHEDULE_DIALOG_ID}
      label={saved ? 'Manage Schedule' : 'Schedule Command'}
      size="medium"
      lightDismiss={false}
      rootAttributes={{ 'data-component': 'command-schedule-dialog' }}
      footer={
        <>
          {saved && (
            <Button
              action={COMMANDS_AND_AI_ACTIONS.deleteCommandSchedule.value}
              tone="danger"
              emphasis="plain"
              disabled={busy}
            >
              Delete
            </Button>
          )}
          <Button action={COMMANDS_AND_AI_ACTIONS.cancelCommandSchedule.value} emphasis="plain">
            Cancel
          </Button>
          <Button type="submit" form={COMMAND_SCHEDULE_FORM_ID} tone="brand" loading={busy} disabled={!draft}>
            {saved ? 'Save Schedule' : 'Create Schedule'}
          </Button>
        </>
      }
    >
      {draft && (
        <Form id={COMMAND_SCHEDULE_FORM_ID} action={COMMANDS_AND_AI_ACTIONS.saveCommandSchedule.value}>
          <List gap="m">
            {commandTitle ? (
              <Text tone="quiet" size="compact">
                {commandTitle}
              </Text>
            ) : null}
            <ScheduleRecurrenceFields draft={draft} typeAction="change-command-schedule-field" />
            <Select name="action" label="Action" value={draft.action} choices={actionChoices(commandAction)} />
            {saved && (
              <Button
                action={COMMANDS_AND_AI_ACTIONS.toggleCommandSchedule.value}
                emphasis="secondary"
                pressed={draft.paused}
              >
                {draft.paused ? 'Resume Schedule' : 'Pause Schedule'}
              </Button>
            )}
            {error ? (
              <StateBanner
                title="Schedule Could Not Be Saved"
                detail={error}
                copyLayout="stacked"
                tone="danger"
                urgency="alert"
              />
            ) : null}
            {previewBusy ? (
              <StateBanner title="Calculating Next Runs" tone="info" />
            ) : upcoming.length ? (
              <StateBanner
                title={`Next Run: ${upcoming[0] ?? ''}`}
                tone="info"
                copyLayout="stacked"
                detail={
                  upcoming.length > 1
                    ? `Then ${upcoming.slice(1).join(' and ')}. Creates an Up Next ticket first.`
                    : 'Creates an Up Next ticket first.'
                }
              />
            ) : (
              <StateBanner title={draft.paused ? 'Schedule Is Paused' : 'No Upcoming Runs'} tone="neutral" />
            )}
            {latestSkip?.ticket_slug ? (
              <Text tone="quiet" size="compact">
                Skipped {occurrenceLabel(latestSkip)}: an open ticket from this command already exists.{' '}
                <a
                  href={`#ticket-${latestSkip.ticket_slug}`}
                  data-action="open-linked-ticket"
                  data-ticket-slug={latestSkip.ticket_slug}
                >
                  {latestSkip.ticket_slug}
                </a>
              </Text>
            ) : null}
          </List>
        </Form>
      )}
    </DialogSurface>
  );
}
