import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { ChoiceCard, type ChoiceCardTone } from '../components/choice-card';
import { DemoEvent } from './components/demo-event';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

type ChoiceCardDemoType = 'radio' | 'checkbox';

export const choiceCardSettings = {
  type: signal<ChoiceCardDemoType>('radio'),
  tone: signal<ChoiceCardTone>('brand'),
  description: signal(true),
  disabled: signal(false),
  /** Values of the checked demo cards (one for radio, any for checkbox). */
  checked: signal<readonly string[]>(['entire']),
};

const DEMO_OPTIONS = [
  { value: 'entire', title: 'Entire conversation', description: 'All 12 messages, in their original order.' },
  { value: 'selected', title: 'Selected messages', description: '3 messages selected in the chat.' },
  { value: 'overwrite', title: 'Overwrite the existing bundle', description: 'Replace its files in this destination.' },
] as const;

export function syncChoiceCardSettingsControls(root: ParentNode): void {
  syncSettingsControls(root, 'choice-card', {
    values: { type: choiceCardSettings.type.value, tone: choiceCardSettings.tone.value },
    checked: { description: choiceCardSettings.description.value, disabled: choiceCardSettings.disabled.value },
  });
}

export function resetChoiceCardDemo(controlRoot?: ParentNode): void {
  choiceCardSettings.type.value = 'radio';
  choiceCardSettings.tone.value = 'brand';
  choiceCardSettings.description.value = true;
  choiceCardSettings.disabled.value = false;
  choiceCardSettings.checked.value = ['entire'];
  if (controlRoot) syncChoiceCardSettingsControls(controlRoot);
}

export function ChoiceCardDemo() {
  const type = choiceCardSettings.type.value,
    checked = choiceCardSettings.checked.value;
  return (
    <>
      <CatalogExample
        label="Interactive"
        note="The input stays native, so its owning feature reads it by name. The last card always uses the danger tone, the presentation for destructive choices; the Tone setting styles the others."
        viewport={{ width: 'medium' }}
      >
        <fieldset data-choice-card-demo>
          <legend>Messages</legend>
          <List gap="2xs">
            {DEMO_OPTIONS.map((option) => (
              <ChoiceCard
                type={type}
                name="choice-card-demo"
                value={option.value}
                checked={checked.includes(option.value)}
                disabled={choiceCardSettings.disabled.value}
                tone={option.value === 'overwrite' ? 'danger' : choiceCardSettings.tone.value}
                title={option.title}
                description={choiceCardSettings.description.value ? option.description : undefined}
              />
            ))}
          </List>
        </fieldset>
      </CatalogExample>
      <DemoEvent>Checked: {checked.length ? checked.join(', ') : 'none'}</DemoEvent>
    </>
  );
}

export function ChoiceCardSettings() {
  return (
    <form class="settings-form" data-settings="choice-card">
      <Select
        name="type"
        label="Input type"
        value={choiceCardSettings.type.value}
        choices={(['radio', 'checkbox'] as const).map((value) => ({ value, label: value }))}
      />
      <Select
        name="tone"
        label="Tone"
        value={choiceCardSettings.tone.value}
        choices={(['brand', 'danger'] as const).map((value) => ({ value, label: value }))}
      />
      <wa-checkbox name="description" checked={choiceCardSettings.description.value}>
        Description
      </wa-checkbox>
      <wa-checkbox name="disabled" checked={choiceCardSettings.disabled.value}>
        Disabled
      </wa-checkbox>
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireChoiceCardDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="choice-card"] [name]', (_event, target) => {
      const control = target as FormControl;
      switch (control.getAttribute('name')) {
        case 'type':
          choiceCardSettings.type.value = control.value as ChoiceCardDemoType;
          choiceCardSettings.checked.value = choiceCardSettings.checked.value.slice(0, 1);
          break;
        case 'tone':
          choiceCardSettings.tone.value = control.value as ChoiceCardTone;
          break;
        case 'description':
          choiceCardSettings.description.value = control.checked;
          break;
        case 'disabled':
          choiceCardSettings.disabled.value = control.checked;
          break;
        case null:
          break;
      }
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-choice-card-demo] input[name="choice-card-demo"]', (_event, target) => {
      const input = target as HTMLInputElement,
        current = choiceCardSettings.checked.value;
      choiceCardSettings.checked.value =
        input.type === 'radio'
          ? [input.value]
          : input.checked
            ? [...current, input.value]
            : current.filter((value) => value !== input.value);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'choice-card': {
      catalogExamples: true,
      Demo: ChoiceCardDemo,
      Settings: ChoiceCardSettings,
      reset: resetChoiceCardDemo,
    },
  },
  wire: wireChoiceCardDemo,
};
