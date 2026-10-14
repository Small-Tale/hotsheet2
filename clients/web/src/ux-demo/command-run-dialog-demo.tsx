import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CommandDefinition, CommandRun } from '../api';
import { CommandRunDialog } from '../components/command-run-dialog';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { DemoEvent } from './components/demo-event';
import { onDemoSelected } from './components/demo-state';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

/** The two public presentations of CommandRunDialog: run output, or the `confirmStop` confirmation. */
export type CommandRunDialogPresentation = 'output' | 'stop';

const COMMAND_RUN_DIALOG_DEMO_SELECTOR =
  '[data-component="command-run-dialog"], [data-component="command-cancellation-dialog"]';

export const commandRunDialogSettings = {
  presentation: signal<CommandRunDialogPresentation>('output'),
  open: signal(true),
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
 * CommandRunDialog renders an open `wa-dialog` whenever it is mounted, the same way the app shows it
 * while a command dialog is selected; the demo mounts it on selection, presentation swap, or Open
 * dialog, and unmounts it on dismiss or Web Awesome's own `wa-hide` (HS2-Z7C27E).
 */
export function showCommandRunDialogDemo(): void {
  commandRunDialogSettings.open.value = true;
}

function closeCommandRunDialogDemo(): void {
  commandRunDialogSettings.open.value = false;
}

export function setCommandRunDialogPresentation(presentation: CommandRunDialogPresentation): void {
  commandRunDialogSettings.presentation.value = presentation;
  commandRunDialogSettings.event.value = '';
  showCommandRunDialogDemo();
}

export function resetCommandRunDialogDemo(root: ParentNode): void {
  setCommandRunDialogPresentation('output');
  syncSettingsControls(root, 'command-run-dialog', { values: { presentation: 'output' } });
}

/** Demo stand-ins for the app's dismiss and stop side effects: close the dialog and report the request. */
export function dismissCommandRunDialogDemo(): void {
  if (!commandRunDialogSettings.open.value) return;
  closeCommandRunDialogDemo();
  commandRunDialogSettings.event.value =
    commandRunDialogSettings.presentation.value === 'stop' ? 'Kept Run checks running' : 'Closed Run checks output';
}

export function confirmStopCommandRunDemo(runId: string | undefined): void {
  closeCommandRunDialogDemo();
  commandRunDialogSettings.event.value = `Stop requested for ${runId ?? 'the run'}`;
}

export function CommandRunDialogDemo() {
  const stop = commandRunDialogSettings.presentation.value === 'stop';
  return (
    <>
      <CatalogExample align="glyph" label="Interactive" rootAttributes={{ 'data-command-run-dialog-demo': '' }}>
        {commandRunDialogSettings.open.value && (
          <CommandRunDialog command={command} run={stop ? runningRun : completedRun} confirmStop={stop} />
        )}
        <wa-button {...DEMO_ACTIONS.openCommandRunDialogDemo.attrs}>Open Dialog</wa-button>
      </CatalogExample>
      <DemoEvent>{commandRunDialogSettings.event.value}</DemoEvent>
    </>
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
          { value: 'output', label: 'Run Output' },
          { value: 'stop', label: 'Stop Confirmation' },
        ]}
      />
      <wa-button type="button" {...DEMO_ACTIONS.resetSettings.attrs}>
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireCommandRunDialogDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    onDemoSelected((id) => {
      if (id === 'command-run-dialog') showCommandRunDialogDemo();
    }),
  );
  // CommandRunDialog demo: swap between its run-output and stop-confirmation presentations and stand in
  // for the app's dismiss/stop side effects (HS2-CWWX7S).
  demoListeners.add(
    delegate(root, 'change', '[data-settings="command-run-dialog"] [name="presentation"]', (_event, target) => {
      setCommandRunDialogPresentation((target as FormControl).value as CommandRunDialogPresentation);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openCommandRunDialogDemo.selector, () => {
      showCommandRunDialogDemo();
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'wa-hide', COMMAND_RUN_DIALOG_DEMO_SELECTOR, () => {
      dismissCommandRunDialogDemo();
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-command-run-dialog-demo] ${COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.selector}`,
      () => {
        dismissCommandRunDialogDemo();
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-command-run-dialog-demo] ${COMMANDS_AND_AI_ACTIONS.confirmStopCommand.selector}`,
      (_event, target) => {
        confirmStopCommandRunDemo((target as HTMLElement).dataset.runId);
      },
    ),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'command-run-dialog': {
      catalogExamples: true,
      Demo: CommandRunDialogDemo,
      Settings: CommandRunDialogSettings,
      reset: resetCommandRunDialogDemo,
    },
  },
  wire: wireCommandRunDialogDemo,
};
