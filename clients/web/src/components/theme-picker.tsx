import { RadioGroup } from '@kerfjs/ui/radio-group';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { THEME_PREFERENCES, type ThemePreference } from '../theme-preference';
import { themeTileArt } from '../theme-tile-art';

export interface ThemePickerProps {
  value: ThemePreference;
  /** The radios' native name; only a page showing several pickers sets it. */
  name?: string;
}

const LABELS: Record<ThemePreference, string> = { auto: 'Auto', light: 'Light', dark: 'Dark' };

/**
 * App Settings → General → Theme (HS2-EFTHS6): Auto, Light, or Dark, each shown as a stylized Hot Sheet
 * window in that scheme so the choice is visible before it is made. Kerf's `RadioGroup` tiles
 * (KF-C29WJ9, HS2-VW7JVV) own the native radios, the frame, the selected ring and check, and the focus
 * ring; the app supplies only the artwork and the `choose-theme` action.
 */
export function ThemePicker({ value, name = 'hotsheet-theme' }: ThemePickerProps) {
  return (
    <RadioGroup
      label="Theme"
      name={name}
      presentation="tiles"
      value={value}
      action={COMMANDS_AND_AI_ACTIONS.chooseTheme.value}
      hint="Auto follows this device's light or dark appearance. This applies only in this browser."
      rootAttributes={{ 'data-theme-picker': '' }}
      choices={THEME_PREFERENCES.map((preference) => ({
        value: preference,
        label: LABELS[preference],
        preview: themeTileArt(preference),
      }))}
    />
  );
}
