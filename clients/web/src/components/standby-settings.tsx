import { Checkbox } from '@kerfjs/ui/checkbox';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';

import { SHELL_AND_GLOBAL_ACTIONS } from '../interaction-attrs/shell-and-global';
import { STANDBY_IDLE_CHOICES, type StandbySettingsView } from '../standby';

/** The control names the save handler reads (HS2-EQVZTZ). */
export const STANDBY_SETTING_FIELDS = {
  enabled: 'standby-enabled',
  idleMinutes: 'standby-idle-minutes',
} as const;

export interface StandbySettingsProps {
  /** The server's answer; absent while loading or after a failure. */
  settings?: StandbySettingsView;
  message?: string;
}

const minutes = (value: number) =>
  value % 60 === 0 ? `${value / 60} hour${value === 60 ? '' : 's'}` : `${value} minutes`;

/** The idle choices, plus a configured value set elsewhere (the CLI) that is not a preset. */
export function standbyIdleChoices(configured: number | undefined): SelectChoice[] {
  const values: number[] =
    configured === undefined || (STANDBY_IDLE_CHOICES as readonly number[]).includes(configured)
      ? [...STANDBY_IDLE_CHOICES]
      : [...STANDBY_IDLE_CHOICES, configured].sort((a, b) => a - b);
  return values.map((value) => ({ value: String(value), label: minutes(value) }));
}

/**
 * App Settings → General → Standby (HS2-EQVZTZ): the machine-wide idle entry switch and threshold.
 * Both save immediately to `PUT /standby-settings`; `hotsheet-cli settings set standby.*` writes the
 * same values.
 */
export function StandbySettings({ settings, message = '' }: StandbySettingsProps) {
  const loading = !settings,
    effective = settings?.effective;
  return (
    <div data-component="standby-settings" {...SHELL_AND_GLOBAL_ACTIONS.saveStandbySettings.attrs}>
      <List gap="xs">
        <Text variant="h2">Standby</Text>
        <Grid minColumnWidth={rem(15)} gap="m">
          <Checkbox
            name={STANDBY_SETTING_FIELDS.enabled}
            checked={effective?.enabled ?? false}
            disabled={loading}
            label="Enter standby when idle"
            hint="Shows the night display after no direct interaction. The moon button enters it at any time. This applies to every client of this machine."
          />
          <Select
            name={STANDBY_SETTING_FIELDS.idleMinutes}
            label="Standby After"
            value={effective ? String(effective.idle_minutes) : ''}
            disabled={loading || effective?.enabled === false}
            placeholder={loading}
            choices={standbyIdleChoices(effective?.idle_minutes)}
          />
        </Grid>
        <Text variant="span" tone="danger" size="compact" role="status">
          {message}
        </Text>
      </List>
    </div>
  );
}
