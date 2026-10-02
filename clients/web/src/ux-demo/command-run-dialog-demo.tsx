import { Select } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import type { CommandDefinition, CommandRun } from '../api';
import { CommandRunDialog } from '../components/command-run-dialog';
import { DEMO_ACTIONS } from './demo-actions';
import { syncSettingsControls } from './settings-controls';

/** The two public presentations of CommandRunDialog: run output, or the `confirmStop` confirmation. */
export type CommandRunDialogPresentation = 'output' | 'stop';

export const commandRunDialogSettings = {
  presentation: signal<CommandRunDialogPresentation>('output'),
  event: signal(''),
};

const command: CommandDefinition = {
  id: 'run-checks',
  title: 'Run checks',
  kind: 'program',
  program: 'npm',
  args: ['run', 'check'],
  group: 'Quality',
};
const completedRun: CommandRun = {
  id: 'run-42',
  command_id: 'run-checks',
  state: 'completed',
  exit_code: 0,
  output: [
    { seq: 1, stream: 'stdout', text: '$ npm run check' },
    { seq: 2, stream: 'stdout', text: 'Typecheck: 0 errors' },
    { seq: 3, stream: 'stdout', text: 'Lint: 0 warnings' },
    { seq: 4, stream: 'stderr', text: 'note: 2 files skipped (no changes)' },
    { seq: 5, stream: 'stdout', text: 'All checks passed in 4.2s' },
  ],
};
const runningRun: CommandRun = {
  id: 'run-43',
  command_id: 'run-checks',
  state: 'running',
  output: [
    { seq: 1, stream: 'stdout', text: '$ npm run check' },
    { seq: 2, stream: 'stdout', text: 'Typecheck: 0 errors' },
  ],
};

/**
 * CommandRunDialog is a standalone native `<dialog>` that stays hidden until `showModal`, so the
 * demo opens it after every mount, selection, or presentation swap, the same way the app does
 * (HS2-Z0CTHN, HS2-CWWX7S).
 */
export function showCommandRunDialogDemo(root: ParentNode): void {
  requestAnimationFrame(() => {
    const dialog = root.querySelector<HTMLDialogElement>('dialog.command-run-dialog');
    if (dialog && !dialog.open) dialog.showModal();
  });
}

function closeCommandRunDialogDemo(root: ParentNode): void {
  root.querySelector<HTMLDialogElement>('dialog.command-run-dialog')?.close();
}

export function setCommandRunDialogPresentation(root: ParentNode, presentation: CommandRunDialogPresentation): void {
  commandRunDialogSettings.presentation.value = presentation;
  commandRunDialogSettings.event.value = '';
  showCommandRunDialogDemo(root);
}

export function resetCommandRunDialogDemo(root: ParentNode): void {
  setCommandRunDialogPresentation(root, 'output');
  syncSettingsControls(root, 'command-run-dialog', { values: { presentation: 'output' } });
}

/** Demo stand-ins for the app's dismiss and stop side effects: close the dialog and report the request. */
export function dismissCommandRunDialogDemo(root: ParentNode): void {
  closeCommandRunDialogDemo(root);
  commandRunDialogSettings.event.value =
    commandRunDialogSettings.presentation.value === 'stop' ? 'Kept Run checks running' : 'Closed Run checks output';
}

export function confirmStopCommandRunDemo(root: ParentNode, runId: string | undefined): void {
  closeCommandRunDialogDemo(root);
  commandRunDialogSettings.event.value = `Stop requested for ${runId ?? 'the run'}`;
}

export function CommandRunDialogDemo() {
  const stop = commandRunDialogSettings.presentation.value === 'stop';
  return (
    <section class="component-stage" aria-label="CommandRunDialog demo">
      <CommandRunDialog command={command} run={stop ? runningRun : completedRun} confirmStop={stop} />
      <button type="button" {...DEMO_ACTIONS.openCommandRunDialogDemo.attrs}>
        Open dialog
      </button>
      <p class="component-stage__event" aria-live="polite">
        {commandRunDialogSettings.event.value}
      </p>
    </section>
  );
}

export function CommandRunDialogSettings() {
  return (
    <form class="settings-form" data-settings="command-run-dialog">
      <Select
        name="presentation"
        label="Presentation"
        value={commandRunDialogSettings.presentation.value}
        choices={[
          { value: 'output', label: 'Run output' },
          { value: 'stop', label: 'Stop confirmation' },
        ]}
      />
      <wa-button type="button" {...DEMO_ACTIONS.resetSettings.attrs}>
        Reset
      </wa-button>
    </form>
  );
}
