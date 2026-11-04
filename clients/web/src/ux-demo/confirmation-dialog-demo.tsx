import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { action } from 'kerfjs/actions';
import { createScope } from 'kerfjs/scope';

import { ConfirmationDialog } from '../components/confirmation-dialog';
import { type ConfirmationOptions, createConfirmation } from '../confirmation';
import { wireConfirmation } from '../confirmation-interactions';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

const ask = action('demo-open-confirmation');
const askTwice = action('demo-open-two-confirmations');

/** The production workflow, its dialog, and its wiring over demo-only questions (HS2-6S1346). */
const confirmation = createConfirmation();

export const confirmationDialogSettings = {
  tone: signal<'danger' | 'brand'>('danger'),
  message: signal(true),
  cancelLabel: signal(false),
  event: signal(''),
};

function question(index?: number): ConfirmationOptions {
  const danger = confirmationDialogSettings.tone.value === 'danger';
  return {
    title: danger ? 'Delete the old Hot Sheet 1 files?' : 'Run checks?',
    ...(confirmationDialogSettings.message.value
      ? {
          message: danger
            ? 'This deletes the old Hot Sheet 1 files from this project. Backups are kept.'
            : 'This runs the project checks in a new terminal.',
        }
      : {}),
    confirmLabel: `${danger ? 'Delete Old Files' : 'Run Command'}${index ? ` (${String(index)} of 2)` : ''}`,
    ...(confirmationDialogSettings.cancelLabel.value ? { cancelLabel: 'Keep Files' } : {}),
    tone: confirmationDialogSettings.tone.value,
  };
}

function report(label: string, confirmed: boolean) {
  confirmationDialogSettings.event.value = `${label}: ${confirmed ? 'confirmed' : 'cancelled'}`;
}

export function resetConfirmationDialogDemo(root: ParentNode): void {
  confirmation.cancelAll();
  confirmationDialogSettings.tone.value = 'danger';
  confirmationDialogSettings.message.value = true;
  confirmationDialogSettings.cancelLabel.value = false;
  confirmationDialogSettings.event.value = '';
  syncSettingsControls(root, 'confirmation-dialog', {
    values: { tone: 'danger' },
    checked: { message: true, 'cancel-label': false },
  });
}

export function ConfirmationDialogDemo() {
  return (
    <>
      <CatalogExample label="Interactive">
        <Row gap="xs" wrap>
          <Button action={ask.value}>Ask…</Button>
          <Button emphasis="secondary" action={askTwice.value}>
            Ask Twice…
          </Button>
        </Row>
        <ConfirmationDialog request={confirmation.current.value} />
      </CatalogExample>
      <DemoEvent>{confirmationDialogSettings.event.value}</DemoEvent>
    </>
  );
}

export function ConfirmationDialogSettings() {
  return (
    <DemoSettingsForm settings="confirmation-dialog">
      <Select
        name="tone"
        label="Tone"
        value={confirmationDialogSettings.tone.value}
        choices={[
          { value: 'danger', label: 'Danger' },
          { value: 'brand', label: 'Brand' },
        ]}
      />
      <Checkbox name="message" checked={confirmationDialogSettings.message.value} label="Message" />
      <Checkbox
        name="cancel-label"
        checked={confirmationDialogSettings.cancelLabel.value}
        label="Custom cancel label"
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireConfirmationDialogDemo(root: HTMLElement): () => void {
  const scope = createScope();
  scope.add(wireConfirmation(root, confirmation));
  scope.add(
    delegate(root, 'click', ask.selector, () => {
      void confirmation.request(question()).then((confirmed) => {
        report('Question', confirmed);
      });
    }),
  );
  // Two concurrent requests show one after the other and settle independently.
  scope.add(
    delegate(root, 'click', askTwice.selector, () => {
      const answers: string[] = [];
      for (const index of [1, 2])
        void confirmation.request(question(index)).then((confirmed) => {
          answers.push(`${String(index)} ${confirmed ? 'confirmed' : 'cancelled'}`);
          confirmationDialogSettings.event.value = `Questions: ${answers.join(', ')}`;
        });
    }),
  );
  scope.add(
    delegate(root, 'change', '[data-settings="confirmation-dialog"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'tone') confirmationDialogSettings.tone.value = control.value === 'brand' ? 'brand' : 'danger';
      else if (name === 'message') confirmationDialogSettings.message.value = control.checked;
      else if (name === 'cancel-label') confirmationDialogSettings.cancelLabel.value = control.checked;
    }),
  );
  return () => {
    confirmation.cancelAll();
    scope.dispose();
  };
}

export const demoModule: DemoModule = {
  entries: {
    'confirmation-dialog': {
      catalogExamples: true,
      Demo: ConfirmationDialogDemo,
      Settings: ConfirmationDialogSettings,
      reset: resetConfirmationDialogDemo,
    },
  },
  wire: wireConfirmationDialogDemo,
};
