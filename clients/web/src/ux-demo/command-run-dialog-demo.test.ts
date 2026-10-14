import { beforeEach, describe, expect, it } from 'vitest';

import {
  CommandRunDialogDemo,
  CommandRunDialogSettings,
  commandRunDialogSettings,
  confirmStopCommandRunDemo,
  dismissCommandRunDialogDemo,
  resetCommandRunDialogDemo,
  setCommandRunDialogPresentation,
} from './command-run-dialog-demo';

function fakeRoot() {
  const control = { value: 'stop', checked: false };
  const root = {
    querySelector: (selector: string) => (selector.includes('[name="presentation"]') ? control : null),
  } as unknown as ParentNode;
  return { root, control };
}

describe('CommandRunDialog demo (HS2-CWWX7S, HS2-Z7C27E)', () => {
  beforeEach(() => {
    commandRunDialogSettings.presentation.value = 'output';
    commandRunDialogSettings.open.value = true;
    commandRunDialogSettings.event.value = '';
  });

  it('renders the run-output presentation by default and offers both presentations', () => {
    const markup = String(CommandRunDialogDemo());
    expect(markup).toContain('data-component="command-run-dialog"');
    expect(markup).toMatch(/<wa-dialog[^>]* open/);
    expect(markup).not.toContain('command-cancellation-dialog');
    expect(markup).toContain('All checks passed in 4.2s');
    const settings = String(CommandRunDialogSettings());
    expect(settings).toContain('data-settings="command-run-dialog"');
    expect(settings).toContain('Run Output');
    expect(settings).toContain('Stop Confirmation');
  });

  it('swaps to the stop confirmation, reopens on swap, and reports dismiss and stop', () => {
    setCommandRunDialogPresentation('stop');
    expect(commandRunDialogSettings.open.value).toBe(true);
    const markup = String(CommandRunDialogDemo());
    expect(markup).toContain('data-component="command-cancellation-dialog"');
    expect(markup).toContain('data-run-id="run-43"');

    dismissCommandRunDialogDemo();
    expect(commandRunDialogSettings.open.value).toBe(false);
    expect(String(CommandRunDialogDemo())).not.toContain('<wa-dialog');
    expect(commandRunDialogSettings.event.value).toBe('Kept Run checks running');

    setCommandRunDialogPresentation('stop');
    expect(commandRunDialogSettings.open.value).toBe(true);
    expect(commandRunDialogSettings.event.value).toBe('');
    confirmStopCommandRunDemo('run-43');
    expect(commandRunDialogSettings.open.value).toBe(false);
    expect(commandRunDialogSettings.event.value).toBe('Stop requested for run-43');
  });

  it('ignores a repeated dismiss (a wa-hide after a button close) instead of rewriting the event', () => {
    confirmStopCommandRunDemo('run-43');
    dismissCommandRunDialogDemo();
    expect(commandRunDialogSettings.event.value).toBe('Stop requested for run-43');
  });

  it('resets the presentation and the live control, then accepts another edit', () => {
    const { root, control } = fakeRoot();
    setCommandRunDialogPresentation('stop');
    resetCommandRunDialogDemo(root);
    expect(commandRunDialogSettings.presentation.value).toBe('output');
    expect(control.value).toBe('output');
    dismissCommandRunDialogDemo();
    expect(commandRunDialogSettings.event.value).toBe('Closed Run checks output');
    setCommandRunDialogPresentation('stop');
    expect(commandRunDialogSettings.presentation.value).toBe('stop');
    expect(commandRunDialogSettings.open.value).toBe(true);
  });
});
