import { execFile } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-G3HJZH: ticket milestones reach the shipped announcer from a real server. A headless `hotsheet-cli`
 * write by an AI actor (a `## Preliminary thoughts` note, then a `feedback_needed` question) is seen by
 * the server's store watcher, turned into a milestone with the rule score and template line (installed AI
 * tools are never run in tests), recorded, and pushed over the live stream; live narration says it in the
 * first person after the project's name. A person's note is shown nowhere. The Milestones submenu writes
 * the server's machine-wide switches. Synthetic tickets only.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const run = promisify(execFile);
const captures = join('target', 'visual-captures', 'hs2-g3hjzh');
mkdirSync(captures, { recursive: true });

async function openProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'Launchpad', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.goto('/');
  await openMockedProject(page);
  await expect(page.locator('#app-loading-indicator')).toHaveCount(0);
}

/** Wait until finite layout animations (a rail settling after a resize, a menu opening) end. */
async function settle(page: Page): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(async () => {
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        return document
          .getAnimations()
          .filter(
            (animation) =>
              animation.playState === 'running' && animation.effect?.getComputedTiming().iterations !== Infinity,
          ).length;
      }),
    )
    .toBe(0);
}

/** Append a note as the CLI does for an agent, outside the server. */
async function cliNote(
  server: Server,
  slug: string,
  text: string,
  summary: string,
  options: { kind?: string; role?: string } = {},
) {
  const file = join(server.root, `note-${Date.now()}.md`);
  writeFileSync(file, text);
  await run(
    server.cli,
    [
      '-C',
      server.store,
      'edit',
      slug,
      '--note-file',
      file,
      '--note-summary',
      summary,
      ...(options.kind ? ['--note-kind', options.kind] : []),
      '--actor-role',
      options.role ?? 'ai',
      '--actor-id',
      options.role === 'human' ? 'synthetic-person' : 'claude-e2e',
    ],
    { cwd: server.root, env: server.env },
  );
}

test('HS2-G3HJZH says ticket milestones from a real server in the first person', async ({ page }) => {
  test.setTimeout(90_000);
  await page.addInitScript(() => {
    const spoken: string[] = [];
    Object.defineProperty(window, '__announcerSpoken', { value: spoken });
    Object.defineProperty(window, 'SpeechSynthesisUtterance', {
      configurable: true,
      value: class {
        onend?: () => void;
        constructor(readonly text: string) {}
      },
    });
    Object.defineProperty(window, 'speechSynthesis', {
      configurable: true,
      value: {
        cancel() {},
        getVoices: () => [],
        speak(utterance: { text: string; onend?: () => void }) {
          spoken.push(utterance.text);
          queueMicrotask(() => utterance.onend?.());
        },
      },
    });
  });
  const server = await realTicketServer();
  try {
    const ticket = await server.request<FullTicket>(`/checkouts/${server.checkoutId}/tickets`, 'POST', {
      title: 'Code organization and documentation',
      category: 'task',
      tags: [],
    });
    const short = ticket.slug.split('-').at(-1) ?? ticket.slug;
    await page.setViewportSize({ width: 1280, height: 800 });
    await openProject(page, server);
    await page.getByRole('button', { name: 'Announcer', exact: true }).click();
    const overlay = page.locator('[data-component="announcer-overlay"]');
    await overlay.getByRole('button', { name: 'Live' }).click();
    const spoken = () => page.evaluate(() => (window as unknown as { __announcerSpoken: string[] }).__announcerSpoken);

    await cliNote(
      server,
      ticket.slug,
      '## Preliminary thoughts\n\nA synthetic body that is never shown or said.',
      'Several files are over our 1000-line limit',
    );
    await expect
      .poll(spoken, { timeout: 30_000 })
      .toEqual([
        `For Launchpad, I started investigating a ticket focusing on Code organization and documentation (${short}). My initial thoughts: Several files are over our 1000-line limit.`,
      ]);
    // The shown entry is the structured line, rendered as Markdown.
    await expect(overlay).toContainText(
      `Started ${ticket.slug} Code organization and documentation. Initial thoughts: Several files are over our 1000-line limit`,
    );
    await expect(overlay).not.toContainText('synthetic body');
    await settle(page);
    await page.screenshot({ path: join(captures, 'initial-thoughts-wide-light.png') });

    // A person's note is never a first-person milestone; the agent's question is said next.
    await cliNote(server, ticket.slug, 'A person writes here.', 'A person note', { role: 'human' });
    await cliNote(server, ticket.slug, 'FEEDBACK NEEDED\n\nWhich accent?', 'Which accent color', {
      kind: 'feedback_needed',
    });
    await expect
      .poll(async () => (await spoken()).slice(1), { timeout: 30_000 })
      .toEqual([
        `I have a question about Code organization and documentation (${short}), when you have a moment: Which accent color.`,
      ]);
    await expect(overlay).toContainText(`${ticket.slug} Code organization and documentation needs your answer`);
    await expect(overlay).not.toContainText('A person note');

    // Narrow: the corner card shows the same entry.
    await page.setViewportSize({ width: 390, height: 844 });
    await settle(page);
    await page.screenshot({ path: join(captures, 'question-narrow-light.png') });
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.emulateMedia({ colorScheme: 'dark' });
    await settle(page);
    await page.screenshot({ path: join(captures, 'question-wide-dark.png') });
    await page.emulateMedia({ colorScheme: 'light' });

    // The Milestones switches are the server's machine-wide settings.
    await overlay.getByRole('button', { name: 'Announcer Settings' }).click();
    await page.getByRole('menuitem', { name: 'Milestones', exact: true }).click();
    const wording = page.getByRole('menuitemcheckbox', { name: 'AI wording', exact: true });
    await expect(wording).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByRole('menuitemcheckbox', { name: 'AI importance', exact: true })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    await settle(page);
    await page.screenshot({ path: join(captures, 'milestones-menu-wide-light.png') });
    await wording.click();
    await expect
      .poll(() => server.request<{ ai_importance: boolean; ai_rewrite: boolean }>('/announcer/settings'))
      .toEqual({ ai_importance: true, ai_rewrite: false });
    // Reopened, the menu reads the stored switch back.
    await overlay.getByRole('button', { name: 'Announcer Settings' }).click();
    await page.getByRole('menuitem', { name: 'Milestones', exact: true }).click();
    await expect(page.getByRole('menuitemcheckbox', { name: 'AI wording', exact: true })).toHaveAttribute(
      'aria-checked',
      'false',
    );
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
  } finally {
    await server.stop(page);
  }
});
