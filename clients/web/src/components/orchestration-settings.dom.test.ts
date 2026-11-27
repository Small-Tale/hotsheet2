// HS2-Y2H7BP: the project rows keep their nodes while a background read's message comes and goes.
import { morph } from 'kerfjs';
import { afterEach, describe, expect, it } from 'vitest';

import { DEMO_SITE_ROWS } from '../ux-demo/orchestration-settings-fixtures';
import { OrchestrationSettings, type OrchestrationSettingsProps } from './orchestration-settings';

afterEach(() => {
  document.body.replaceChildren();
});

const render = (change: Partial<OrchestrationSettingsProps> = {}) =>
  String(OrchestrationSettings({ rows: DEMO_SITE_ROWS, ownerId: 'demo-sites', ...change }));

describe('HS2-Y2H7BP project settings messages', () => {
  it('keeps the project rows in place while a read failure message appears and goes, repeatedly', () => {
    const host = document.createElement('main');
    host.innerHTML = render();
    document.body.append(host);
    const row = () => host.querySelector('[data-orchestration-project="demo-docs"]');
    const first = row();
    expect(first).not.toBeNull();
    const slot = (name: string) => host.querySelector(`[data-orchestration-${name}]`);
    expect(slot('message')?.hasAttribute('hidden')).toBe(true);
    expect(slot('unavailable')?.hasAttribute('hidden')).toBe(true);
    for (const change of [
      { message: 'Could not read the orchestration settings. offline' },
      {},
      { unavailableReason: 'The remote server’s open projects could not be read.' },
      { message: 'Could not read the orchestration settings. offline', unavailableReason: 'Unavailable.' },
      {},
    ]) {
      // A move disconnects a node for a moment (outside Chromium's moveBefore()), which closes an open menu:
      // record every node taken out while the morph runs.
      const removed: Node[] = [];
      const observer = new MutationObserver((records) => {
        for (const record of records) removed.push(...record.removedNodes);
      });
      observer.observe(host, { childList: true, subtree: true });
      morph(host, render(change));
      for (const record of observer.takeRecords()) removed.push(...record.removedNodes);
      observer.disconnect();
      // The same row node, never taken out: nothing below the messages was rebuilt or moved.
      expect(row()).toBe(first);
      expect(removed.filter((node) => node.contains(first))).toEqual([]);
      expect(slot('message')?.hasAttribute('hidden')).toBe(!('message' in change));
      expect(slot('unavailable')?.hasAttribute('hidden')).toBe(!('unavailableReason' in change));
      if ('message' in change) expect(slot('message')?.textContent).toContain('Could not read');
    }
  });
});
