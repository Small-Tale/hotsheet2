import { signal } from 'kerfjs';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { Project } from '../interactions/types';
import { mountTerminalViewport } from '../terminal-viewport';
import { createTerminalViewportsController } from './terminal-viewports';

vi.mock('../terminal-viewport', async (original) => ({
  ...(await original<typeof import('../terminal-viewport')>()),
  mountTerminalViewport: vi.fn(() => vi.fn()),
}));

afterEach(() => {
  document.body.replaceChildren();
  vi.mocked(mountTerminalViewport).mockClear();
});

const project = {
  id: 'phone-project',
  name: 'Phone project',
  root: '/synthetic/project',
  apiPath: '/api/phone-project',
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
} as Project;

function magnifiedViewport(): HTMLElement {
  const viewport = document.createElement('div');
  viewport.dataset.component = 'terminal-viewport';
  viewport.dataset.projectId = project.id;
  viewport.dataset.terminalId = 'live';
  viewport.dataset.displayMode = 'interactive';
  viewport.dataset.mountPolicy = 'immediate';
  return viewport;
}

function setup() {
  const owner = createTerminalViewportsController({
    projects: signal([project]),
    pendingTerminalFocus: undefined,
    openTicketReference: vi.fn(),
  });
  const oldParent = document.createElement('section');
  const viewport = magnifiedViewport();
  const input = document.createElement('textarea');
  input.className = 'xterm-helper-textarea';
  viewport.append(input);
  oldParent.append(viewport);
  document.body.append(oldParent);
  owner.syncTerminalViewportMounts();
  return { owner, oldParent, viewport, input };
}

describe('HS2-PSV7BG magnified terminal focus continuity', () => {
  it('restores the same live input after a rendered parent is replaced', async () => {
    const { owner, oldParent, viewport, input } = setup();
    input.focus();
    expect(document.activeElement).toBe(input);
    const focusedInput = document.activeElement as HTMLElement;
    const placeholder = magnifiedViewport();
    oldParent.remove();
    document.body.append(placeholder);
    expect(document.activeElement).toBe(document.body);

    owner.syncTerminalViewportMounts(focusedInput);
    await Promise.resolve();

    expect(viewport.isConnected).toBe(true);
    expect(placeholder.isConnected).toBe(false);
    expect(mountTerminalViewport).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(input);
  });

  it('restores focus when a live magnified popover is re-presented without remounting', async () => {
    const { owner, viewport, input } = setup();
    const overlay = document.createElement('div');
    overlay.dataset.topLayerOverlay = '';
    overlay.tabIndex = -1;
    viewport.replaceWith(overlay);
    overlay.append(viewport);
    input.focus();
    const focusedInput = document.activeElement as HTMLElement;
    overlay.focus();
    expect(document.activeElement).toBe(overlay);

    owner.syncTerminalViewportMounts(focusedInput);
    await Promise.resolve();

    expect(mountTerminalViewport).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(input);
  });

  it('respects focus moved to another control before the queued restoration', async () => {
    const { owner, oldParent, input } = setup();
    const button = document.createElement('button');
    document.body.append(button);
    input.focus();
    const focusedInput = document.activeElement as HTMLElement;
    oldParent.remove();
    document.body.append(magnifiedViewport());

    owner.syncTerminalViewportMounts(focusedInput);
    button.focus();
    await Promise.resolve();

    expect(document.activeElement).toBe(button);
  });
});
