import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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
  const dialog = {
    open: false,
    showModal: vi.fn(() => {
      dialog.open = true;
    }),
    close: vi.fn(() => {
      dialog.open = false;
    }),
  };
  const control = { value: 'stop', checked: false };
  const root = {
    querySelector: (selector: string) =>
      selector === 'dialog.command-run-dialog' ? dialog : selector.includes('[name="presentation"]') ? control : null,
  } as unknown as ParentNode;
  return { root, dialog, control };
}

describe('CommandRunDialog demo (HS2-CWWX7S)', () => {
  beforeEach(() => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(0);
      return 0;
    });
    commandRunDialogSettings.presentation.value = 'output';
    commandRunDialogSettings.event.value = '';
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders the run-output presentation by default and offers both presentations', () => {
    const markup = String(CommandRunDialogDemo());
    expect(markup).toContain('data-component="command-run-dialog"');
    expect(markup).not.toContain('command-cancellation-dialog');
    expect(markup).toContain('All checks passed in 4.2s');
    const settings = String(CommandRunDialogSettings());
    expect(settings).toContain('data-settings="command-run-dialog"');
    expect(settings).toContain('Run output');
    expect(settings).toContain('Stop confirmation');
  });

  it('swaps to the stop confirmation, reopens modally, and reports dismiss and stop', () => {
    const { root, dialog } = fakeRoot();
    setCommandRunDialogPresentation(root, 'stop');
    expect(dialog.showModal).toHaveBeenCalledTimes(1);
    const markup = String(CommandRunDialogDemo());
    expect(markup).toContain('data-component="command-cancellation-dialog"');
    expect(markup).toContain('data-run-id="run-43"');

    dismissCommandRunDialogDemo(root);
    expect(dialog.open).toBe(false);
    expect(commandRunDialogSettings.event.value).toBe('Kept Run checks running');

    setCommandRunDialogPresentation(root, 'stop');
    expect(dialog.showModal).toHaveBeenCalledTimes(2);
    expect(commandRunDialogSettings.event.value).toBe('');
    confirmStopCommandRunDemo(root, 'run-43');
    expect(dialog.open).toBe(false);
    expect(commandRunDialogSettings.event.value).toBe('Stop requested for run-43');
  });

  it('does not re-call showModal on an already open dialog', () => {
    const { root, dialog } = fakeRoot();
    dialog.open = true;
    setCommandRunDialogPresentation(root, 'stop');
    expect(dialog.showModal).not.toHaveBeenCalled();
  });

  it('resets the presentation and the live control, then accepts another edit', () => {
    const { root, control, dialog } = fakeRoot();
    setCommandRunDialogPresentation(root, 'stop');
    resetCommandRunDialogDemo(root);
    expect(commandRunDialogSettings.presentation.value).toBe('output');
    expect(control.value).toBe('output');
    dismissCommandRunDialogDemo(root);
    expect(commandRunDialogSettings.event.value).toBe('Closed Run checks output');
    setCommandRunDialogPresentation(root, 'stop');
    expect(commandRunDialogSettings.presentation.value).toBe('stop');
    expect(dialog.open).toBe(true);
  });
});
