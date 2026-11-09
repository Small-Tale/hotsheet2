import { appendFile, mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { expect, test } from '@playwright/test';

import type { TerminalInfo } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';

// HS2-D0QTGB: a question a Codex session asks through `request_user_input_async` reaches the
// in-app prompt from its own rollout, across resize, and clears when the terminal answers it.
// Synthetic rollout records and a synthetic session only; no Codex binary is run.
const SESSION = '00000000-0000-7000-8000-0000000000d2';
const TERMINAL = 'codex-question-synthetic';

test('shows a Codex rollout question in the app and clears it on terminal input (HS2-D0QTGB)', async ({
  context,
  baseURL,
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
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();

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
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 390],
      ['wide-return', 1280],
    ] as const) {
      await page.setViewportSize({ width, height: 844 });
      await expect(popup).toBeVisible();
      await page.screenshot({ path: test.info().outputPath(`hs2-d0qtgb-codex-question-${label}.png`) });
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
