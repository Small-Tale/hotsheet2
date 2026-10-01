import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import { ConfidenceBadge } from '../components/confidence-badge';
import { syncSettingsControls } from './settings-controls';

/** Every public ConfidenceBadge variant: both appearances across all four rubric bands. */
const CONFIDENCE_CHOICES: SelectChoice[] = [
  { value: '96', label: '96% (fully verified)' },
  { value: '82', label: '82% (minor assumptions)' },
  { value: '55', label: '55% (partially verified)' },
  { value: '20', label: '20% (largely unverified)' },
];

export const confidenceBadgeSettings = {
  value: signal<string>('82'),
  appearance: signal<'compact' | 'labeled'>('compact'),
};

export function resetConfidenceBadgeDemo(root?: ParentNode): void {
  confidenceBadgeSettings.value.value = '82';
  confidenceBadgeSettings.appearance.value = 'compact';
  if (root)
    syncSettingsControls(root, 'confidence-badge', {
      values: { value: confidenceBadgeSettings.value.value, appearance: confidenceBadgeSettings.appearance.value },
    });
}

export function ConfidenceBadgeDemo() {
  return (
    <section class="component-stage" aria-label="ConfidenceBadge demo">
      <div class="component-stage__canvas">
        {ConfidenceBadge({
          value: Number(confidenceBadgeSettings.value.value),
          appearance: confidenceBadgeSettings.appearance.value,
        })}
      </div>
      <p class="component-stage__guidance">
        Compact is the pill on note cards and ticket list/board summaries; labeled is reserved for the inspector and
        reader header. The band tint follows the confidence rubric.
      </p>
    </section>
  );
}

export function ConfidenceBadgeSettings() {
  return (
    <form class="settings-form" data-settings="confidence-badge">
      <Select
        name="value"
        label="Confidence"
        value={confidenceBadgeSettings.value.value}
        choices={CONFIDENCE_CHOICES}
      />
      <Select
        name="appearance"
        label="Appearance"
        value={confidenceBadgeSettings.appearance.value}
        choices={[
          { value: 'compact', label: 'Compact (pill)' },
          { value: 'labeled', label: 'Labeled (inspector header)' },
        ]}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}
