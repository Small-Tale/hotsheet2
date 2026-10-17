import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { ConfidenceBadge } from '../components/confidence-badge';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Every public ConfidenceBadge variant: both appearances across all four rubric bands. */
const CONFIDENCE_CHOICES: SelectChoice[] = [
  { value: '96', label: '96% (Fully Verified)' },
  { value: '82', label: '82% (Minor Assumptions)' },
  { value: '55', label: '55% (Partially Verified)' },
  { value: '20', label: '20% (Largely Unverified)' },
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
    <CatalogExample
      align="glyph"
      label="Interactive"
      note="Compact is the pill on note cards and ticket list/board summaries; labeled is reserved for the inspector and reader header. The band tint follows the confidence rubric."
    >
      {ConfidenceBadge({
        value: Number(confidenceBadgeSettings.value.value),
        appearance: confidenceBadgeSettings.appearance.value,
      })}
    </CatalogExample>
  );
}

export function ConfidenceBadgeSettings() {
  return (
    <DemoSettingsForm settings="confidence-badge">
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
          { value: 'compact', label: 'Compact (Pill)' },
          { value: 'labeled', label: 'Labeled (Inspector Header)' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireConfidenceBadgeDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="confidence-badge"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'value') confidenceBadgeSettings.value.value = control.value;
      if (control.getAttribute('name') === 'appearance')
        confidenceBadgeSettings.appearance.value = control.value as typeof confidenceBadgeSettings.appearance.value;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'confidence-badge': {
      catalogExamples: true,
      Demo: ConfidenceBadgeDemo,
      Settings: ConfidenceBadgeSettings,
      reset: resetConfidenceBadgeDemo,
    },
  },
  wire: wireConfidenceBadgeDemo,
};
