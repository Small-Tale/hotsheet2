import { px } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { RadioGroup } from '@kerfjs/ui/radio-group';
import { Select } from '@kerfjs/ui/select';
import { TextField } from '@kerfjs/ui/text-field';

import { everyBounds, isSubDay, type ScheduleField, type ScheduleFormDraft } from '../command-schedule-form';

const REPEATS = [
  { value: 'minutely', label: 'Every Few Minutes' },
  { value: 'hourly', label: 'Hourly' },
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
const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'].map((label, index) => ({
  value: String(index + 1),
  label,
}));
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

function timeZones(current: string) {
  const zones = new Set(['UTC', current, ...Intl.supportedValuesOf('timeZone')]);
  return [...zones].sort().map((value) => ({ value, label: value }));
}

export interface ScheduleRecurrenceFieldsProps {
  draft: ScheduleFormDraft;
  /** Put before each field's draft key, so a form with other fields can tell these apart. */
  namePrefix?: string;
  /** The Schedule Type radio group's action. */
  typeAction?: string;
  /** Extra root attributes for each text field, by its draft key (a per-open `data-key`, a test hook). */
  fieldAttributes?: (field: ScheduleField) => Record<string, string>;
}

/**
 * The full recurrence of a schedule: one time or repeating; every few minutes, hourly, daily, weekly on several
 * weekdays, monthly and yearly by day rule, or selected dates; start, end, run count, included and excluded dates;
 * time and time zone. The command-schedule dialog and the Triggers… setup share it (HS2-SEEMXR); each field is
 * named by its draft key, and `withScheduleField` applies a change.
 */
export function ScheduleRecurrenceFields({
  draft,
  namePrefix = '',
  typeAction,
  fieldAttributes = () => ({}),
}: ScheduleRecurrenceFieldsProps) {
  const name = (field: ScheduleField) => `${namePrefix}${field}`;
  const root = (field: ScheduleField) => ({ 'data-schedule-field': 'true', ...fieldAttributes(field) });
  const repeating = draft.scheduleType === 'repeating';
  const calendar = draft.repeats === 'monthly' || draft.repeats === 'yearly';
  // Sub-day repeats start at the time on the start date (HS2-29V4ZG).
  const subDay = repeating && isSubDay(draft.repeats);
  const every = everyBounds(draft.repeats);
  return (
    <>
      <RadioGroup
        label="Schedule Type"
        name={name('scheduleType')}
        value={draft.scheduleType}
        orientation="horizontal"
        {...(typeAction ? { action: typeAction } : {})}
        choices={[
          { value: 'once', label: 'One Time' },
          { value: 'repeating', label: 'Repeating' },
        ]}
      />
      {!repeating ? (
        <TextField
          label="Date"
          name={name('date')}
          type="date"
          required
          value={draft.date}
          rootAttributes={root('date')}
        />
      ) : (
        <>
          <Grid minColumnWidth={px(144)} gap="m">
            <Select name={name('repeats')} label="Repeats" value={draft.repeats} choices={REPEATS} />
            {draft.repeats === 'yearly' && (
              <Select name={name('month')} label="Month" value={draft.month} choices={MONTHS} />
            )}
          </Grid>
          {draft.repeats === 'weekly' && (
            <Select
              name={name('weekdays')}
              label="Weekdays"
              multiple
              value={draft.weekdays.map(String)}
              choices={WEEKDAYS}
            />
          )}
          {calendar && (
            <>
              <Select
                name={name('on')}
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
                  name={name('day')}
                  type="number"
                  min={1}
                  max={31}
                  required
                  value={draft.day}
                  rootAttributes={root('day')}
                />
              )}
              {draft.on === 'weekday_ordinal' && (
                <Grid minColumnWidth={px(144)} gap="m">
                  <Select name={name('ordinal')} label="Occurrence" value={draft.ordinal} choices={ORDINALS} />
                  <Select name={name('weekday')} label="Weekday" value={draft.weekday} choices={WEEKDAYS} />
                </Grid>
              )}
            </>
          )}
          {draft.repeats === 'selected_dates' ? (
            <TextField
              label="Dates"
              name={name('selectedDates')}
              hint="Separate dates with commas (YYYY-MM-DD)."
              value={draft.selectedDates}
              rootAttributes={root('selectedDates')}
            />
          ) : (
            <>
              <Grid minColumnWidth={px(144)} gap="m">
                <TextField
                  label="Starts"
                  name={name('startDate')}
                  type="date"
                  required
                  value={draft.startDate}
                  rootAttributes={root('startDate')}
                />
                <TextField
                  label={every.label}
                  name={name('every')}
                  type="number"
                  min={every.min}
                  max={every.max}
                  required
                  value={draft.every}
                  {...(every.hint ? { hint: every.hint } : {})}
                  rootAttributes={root('every')}
                />
              </Grid>
              <Grid minColumnWidth={px(144)} gap="m">
                <TextField
                  label="Ends On"
                  name={name('endDate')}
                  type="date"
                  value={draft.endDate}
                  rootAttributes={root('endDate')}
                />
                <TextField
                  label="After Runs"
                  name={name('count')}
                  type="number"
                  min={1}
                  max={1000}
                  value={draft.count}
                  rootAttributes={root('count')}
                />
              </Grid>
              <Grid minColumnWidth={px(144)} gap="m">
                <TextField
                  label="Include Dates"
                  name={name('includeDates')}
                  value={draft.includeDates}
                  hint="Optional YYYY-MM-DD dates, comma separated."
                  rootAttributes={root('includeDates')}
                />
                <TextField
                  label="Exclude Dates"
                  name={name('excludeDates')}
                  value={draft.excludeDates}
                  hint="Optional YYYY-MM-DD dates, comma separated."
                  rootAttributes={root('excludeDates')}
                />
              </Grid>
            </>
          )}
        </>
      )}
      <Grid minColumnWidth={px(144)} gap="m">
        <TextField
          label={subDay ? 'First Run' : 'Time (24-hour)'}
          name={name('time')}
          required
          value={draft.time}
          hint={subDay ? '24-hour HH:mm on the start date; repeats from this time.' : 'HH:mm'}
          rootAttributes={root('time')}
        />
        <Select name={name('timeZone')} label="Time Zone" value={draft.timeZone} choices={timeZones(draft.timeZone)} />
      </Grid>
    </>
  );
}
