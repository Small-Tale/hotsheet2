import { appendFile, mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { expect, type Locator, test } from '@playwright/test';

import type { TerminalInfo } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';
import { openMockedProject } from './open-mocked-project';

// HS2-D0QTGB: a question a Codex session asks through `request_user_input_async` reaches the
// in-app prompt from its own rollout, across resize, and clears when the terminal answers it.
// Synthetic rollout records and a synthetic session only; no Codex binary is run.
const SESSION = '00000000-0000-7000-8000-0000000000d2';
const TERMINAL = 'codex-question-synthetic';

test('shows a Codex rollout question in the app and clears it on terminal input (HS2-D0QTGB)', async ({
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  try {
    const home = join(app.server.root, 'synthetic-codex-home');
    const day = join(home, 'sessions/2026/11/12');
    await mkdir(day, { recursive: true });
    const rollout = join(day, `rollout-2026-11-12T00-00-00-${SESSION}.jsonl`);
    const line = (value: unknown) => `${JSON.stringify(value)}\n`;
    const now = () => new Date().toISOString();
    await writeFile(rollout, line({ timestamp: now(), type: 'session_meta', payload: { id: SESSION } }));
    await app.server.request('/terminals', 'POST', {
      id: TERMINAL,
      connect: 'codex',
      command: '/bin/sh',
      args: ['-c', 'exec cat'],
      cwd: app.server.root,
    });
    const report = await fetch(`${app.server.url}/terminals/${TERMINAL}/ai-connection`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': app.server.secret },
      body: JSON.stringify({ agent: 'codex', session_id: SESSION, codex_home: home }),
    });
    expect(report.status).toBe(204);
    const page = await app.launch();
    await openMockedProject(page);

    await appendFile(
      rollout,
      line({
        timestamp: now(),
        type: 'event_msg',
        payload: {
          type: 'item_completed',
          item: {
            type: 'AgentMessage',
            id: 'item_synthetic_q',
            delivery: 'async',
            questions: [{ title: 'Which synthetic option?', options: ['Alpha', 'Beta'] }],
          },
        },
      }),
    );
    const popup = page.getByRole('dialog', { name: 'AI waiting for your answer' });
    await expect(popup).toBeVisible({ timeout: 30_000 });
    await expect(popup).toContainText('Which synthetic option?');
    await expect(popup.getByRole('radio', { name: 'Alpha' })).toBeVisible();
    await expect(popup.getByRole('button', { name: 'Answer in terminal' })).toBeVisible();
    const question = (await app.server.request<TerminalInfo[]>('/terminals')).find(
      (row) => row.id === TERMINAL,
    )?.question;
    expect(question).toMatchObject({ tool_use_id: 'item_synthetic_q', agent: 'codex' });
    // HS2-7VRFX0: a resize, even across the phone/desktop boundary, keeps the one presented popup node:
    // nothing removes it from the DOM and it never leaves the top layer.
    // The popup first settles as a plain popover once the Open Project dialog beneath it has gone.
    await expect.poll(() => popup.evaluate((dialog) => dialog.matches(':popover-open'))).toBe(true);
    type ResizeWatch = { hs27vrfx0: { dialog: Element; events: string[] } };
    await popup.evaluate((dialog) => {
      const events: string[] = [];
      // Only a close counts: Chromium may still deliver the toggle event of the first presentation.
      dialog.addEventListener('toggle', (event) => {
        if ((event as ToggleEvent).newState === 'closed') events.push('closed');
      });
      new MutationObserver((records) => {
        if (records.some((record) => [...record.removedNodes].some((node) => node.contains(dialog))))
          events.push('removed');
      }).observe(document.body, { childList: true, subtree: true });
      (window as unknown as ResizeWatch).hs27vrfx0 = { dialog, events };
    });
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 390],
      ['wide-return', 1280],
    ] as const) {
      await page.setViewportSize({ width, height: 844 });
      await expect(popup).toBeVisible();
      const watch = await popup.evaluate((dialog) => {
        const { hs27vrfx0 } = window as unknown as ResizeWatch;
        return {
          same: dialog === hs27vrfx0.dialog,
          presented: dialog.matches(':popover-open'),
          events: hs27vrfx0.events,
        };
      });
      expect(watch, `${label}: the same presented popup`).toEqual({ same: true, presented: true, events: [] });
      await expectFooterActionsApart(popup, width);
      await page.screenshot({ path: test.info().outputPath(`hs2-d0qtgb-codex-question-${label}.png`) });
      await page.screenshot({
        path: `target/visual-captures/hs2-5ev4n9-question-footer-${label}-${browserName}.png`,
      });
    }

    // The user answers in the terminal: Codex records the user input and the prompt goes away.
    await appendFile(
      rollout,
      line({
        timestamp: now(),
        type: 'event_msg',
        payload: { type: 'item_completed', item: { type: 'UserMessage', id: 'synthetic-user-input' } },
      }),
    );
    await expect(popup).toHaveCount(0, { timeout: 30_000 });
    await expect
      .poll(
        async () =>
          (await app.server.request<TerminalInfo[]>('/terminals')).find((row) => row.id === TERMINAL)?.question,
      )
      .toBeUndefined();
  } finally {
    await app.stop();
  }
});

// HS2-5EV4N9: the question notice's footer actions wrap instead of overlapping, and stay on screen.
async function expectFooterActionsApart(popup: Locator, width: number) {
  const names = ['Dismiss', 'Pause Notifications', 'Answer in terminal'] as const;
  for (const name of names) await expect(popup.getByRole('button', { name, exact: true })).toBeVisible();
  // The popup keeps its node and presentation across the resize (HS2-7VRFX0), so it measures at once.
  const boxes: { name: string; x: number; y: number; width: number; height: number }[] = [];
  for (const name of names) {
    const box = await popup.getByRole('button', { name, exact: true }).boundingBox();
    expect(box, `${name} is on screen`).not.toBeNull();
    if (box) boxes.push({ name, ...box });
  }
  for (const box of boxes) {
    expect(box.x, box.name).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width, box.name).toBeLessThanOrEqual(width);
  }
  for (const [index, a] of boxes.entries())
    for (const b of boxes.slice(index + 1)) {
      const apart = a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y;
      expect(apart, `${a.name} overlaps ${b.name}`).toBe(true);
    }
}
