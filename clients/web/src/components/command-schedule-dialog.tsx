import { Button } from '@kerfjs/ui/button';
import { px } from '@kerfjs/ui/css-values';
import { Form } from '@kerfjs/ui/form';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { RadioGroup } from '@kerfjs/ui/radio-group';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import type { ScheduleFormDraft } from '../command-schedule-form';
import type { ScheduleView } from '../command-schedules';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

export const COMMAND_SCHEDULE_DIALOG_ID = 'command-schedule-dialog';
export const COMMAND_SCHEDULE_FORM_ID = 'command-schedule-form';

const REPEATS = [
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
  { value: 'yearly', label: 'Yearly' },
  { value: 'selected_dates', label: 'Selected Dates' },
] as const;
const ORDINALS = [
  { value: '1', label: 'First' },
  { value: '2', label: 'Second' },
  { value: '3', label: 'Third' },
  { value: '4', label: 'Fourth' },
  { value: '5', label: 'Fifth' },
  { value: '-1', label: 'Last' },
];
const WEEKDAYS = [
  { value: '1', label: 'Monday' },
  { value: '2', label: 'Tuesday' },
  { value: '3', label: 'Wednesday' },
  { value: '4', label: 'Thursday' },
  { value: '5', label: 'Friday' },
  { value: '6', label: 'Saturday' },
  { value: '7', label: 'Sunday' },
];
const MONTHS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
].map((label, index) => ({ value: String(index + 1), label }));
const ACTIONS = [
  { value: 'create_ticket', label: 'Create Ticket Only' },
  { value: 'new_ai_session', label: 'Auto-Start in a New AI Session' },
  { value: 'reuse_or_new_ai_session', label: 'Reuse or Start an AI Session' },
] as const;

function timeZones(current: string) {
  const zones = new Set(['UTC', current, ...Intl.supportedValuesOf('timeZone')]);
  return [...zones].sort().map((value) => ({ value, label: value }));
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
  draft,
  saved,
  preview = [],
  previewBusy = false,
  busy = false,
  error = '',
}: CommandScheduleDialogProps) {
  const repeating = draft?.scheduleType === 'repeating';
  const calendar = draft?.repeats === 'monthly' || draft?.repeats === 'yearly';
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
            <RadioGroup
              label="Schedule Type"
              name="scheduleType"
              value={draft.scheduleType}
              orientation="horizontal"
              action="change-command-schedule-field"
              choices={[
                { value: 'once', label: 'One Time' },
                { value: 'repeating', label: 'Repeating' },
              ]}
            />
            {!repeating ? (
              <TextField
                label="Date"
                name="date"
                type="date"
                required
                value={draft.date}
                rootAttributes={{ 'data-schedule-field': 'true' }}
              />
            ) : (
              <>
                <Grid minColumnWidth={px(144)} gap="m">
                  <Select name="repeats" label="Repeats" value={draft.repeats} choices={REPEATS} />
                  {draft.repeats === 'yearly' && (
                    <Select name="month" label="Month" value={draft.month} choices={MONTHS} />
                  )}
                </Grid>
                {draft.repeats === 'weekly' && (
                  <Select
                    name="weekdays"
                    label="Weekdays"
                    multiple
                    value={draft.weekdays.map(String)}
                    choices={WEEKDAYS}
                  />
                )}
                {calendar && (
                  <>
                    <Select
                      name="on"
                      label="On"
                      value={draft.on}
                      choices={[
                        { value: 'day_of_month', label: 'Day of Month' },
                        { value: 'last_day', label: 'Last Day' },
                        { value: 'weekday_ordinal', label: 'Weekday Occurrence' },
                      ]}
                    />
                    {draft.on === 'day_of_month' && (
                      <TextField
                        label="Day"
                        name="day"
                        type="number"
                        min={1}
                        max={31}
                        required
                        value={draft.day}
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                    )}
                    {draft.on === 'weekday_ordinal' && (
                      <Grid minColumnWidth={px(144)} gap="m">
                        <Select name="ordinal" label="Occurrence" value={draft.ordinal} choices={ORDINALS} />
                        <Select name="weekday" label="Weekday" value={draft.weekday} choices={WEEKDAYS} />
                      </Grid>
                    )}
                  </>
                )}
                {draft.repeats === 'selected_dates' ? (
                  <TextField
                    label="Dates"
                    name="selectedDates"
                    hint="Separate dates with commas (YYYY-MM-DD)."
                    value={draft.selectedDates}
                    rootAttributes={{ 'data-schedule-field': 'true' }}
                  />
                ) : (
                  <>
                    <Grid minColumnWidth={px(144)} gap="m">
                      <TextField
                        label="Starts"
                        name="startDate"
                        type="date"
                        required
                        value={draft.startDate}
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                      <TextField
                        label="Every"
                        name="every"
                        type="number"
                        min={1}
                        max={365}
                        required
                        value={draft.every}
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                    </Grid>
                    <Grid minColumnWidth={px(144)} gap="m">
                      <TextField
                        label="Ends On"
                        name="endDate"
                        type="date"
                        value={draft.endDate}
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                      <TextField
                        label="After Runs"
                        name="count"
                        type="number"
                        min={1}
                        max={1000}
                        value={draft.count}
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                    </Grid>
                    <Grid minColumnWidth={px(144)} gap="m">
                      <TextField
                        label="Include Dates"
                        name="includeDates"
                        value={draft.includeDates}
                        hint="Optional YYYY-MM-DD dates, comma separated."
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                      <TextField
                        label="Exclude Dates"
                        name="excludeDates"
                        value={draft.excludeDates}
                        hint="Optional YYYY-MM-DD dates, comma separated."
                        rootAttributes={{ 'data-schedule-field': 'true' }}
                      />
                    </Grid>
                  </>
                )}
              </>
            )}
            <Grid minColumnWidth={px(144)} gap="m">
              <TextField
                label="Time (24-hour)"
                name="time"
                required
                value={draft.time}
                hint="HH:mm"
                rootAttributes={{ 'data-schedule-field': 'true' }}
              />
              <Select name="timeZone" label="Time Zone" value={draft.timeZone} choices={timeZones(draft.timeZone)} />
            </Grid>
            <Select name="action" label="Action" value={draft.action} choices={ACTIONS} />
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
                Skipped {latestSkip.occurrence_date}: an open ticket from this command already exists.{' '}
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
