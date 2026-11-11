import './theme-picker.css';

import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { FieldLabel, Text } from '@kerfjs/ui/text';
import { Check } from 'lucide';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { THEME_PREFERENCES, type ThemePreference } from '../theme-preference';
import { themeTileArt } from '../theme-tile-art';

export interface ThemePickerProps {
  value: ThemePreference;
  /** The radio group's name, which also keys its label ids; only a page showing several pickers sets it. */
  name?: string;
}

const LABELS: Record<ThemePreference, string> = { auto: 'Auto', light: 'Light', dark: 'Dark' };

/**
 * App Settings → General → Theme (HS2-EFTHS6): Auto, Light, or Dark, each shown as a stylized Hot Sheet
 * window in that scheme so the choice is visible before it is made. The tiles are native radios inside
 * labels (one Tab stop, arrow keys, a check beside the selected label so the choice does not rely on the
 * ring's color). Kerf has no preview-tile choice yet (KF-C29WJ9), so the tile frame is app CSS.
 */
export function ThemePicker({ value, name = 'hotsheet-theme' }: ThemePickerProps) {
  return (
    <div class="theme-picker" data-component="theme-picker">
      <List gap="xs">
        <FieldLabel id={`${name}-label`}>Theme</FieldLabel>
        <div role="radiogroup" aria-labelledby={`${name}-label`} aria-describedby={`${name}-hint`}>
          <Grid columns={3} gap="xs">
            {THEME_PREFERENCES.map((preference) => (
              <label class="theme-picker__tile" data-theme-choice={preference}>
                <input
                  class="theme-picker__input"
                  type="radio"
                  name={name}
                  value={preference}
                  checked={preference === value}
                  {...COMMANDS_AND_AI_ACTIONS.chooseTheme.attrs}
                />
                <span class="theme-picker__frame">{themeTileArt(preference)}</span>
                <span class="theme-picker__label">
                  <span class="theme-picker__check">
                    <LucideIcon icon={Check} name="check" />
                  </span>
                  {LABELS[preference]}
                </span>
              </label>
            ))}
          </Grid>
        </div>
        <Text tone="quiet" size="compact" id={`${name}-hint`}>
          Auto follows this device's light or dark appearance. This applies only in this browser.
        </Text>
      </List>
    </div>
  );
}
