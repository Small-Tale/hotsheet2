import { expect, test } from '@playwright/test';

import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

// HS2-HSFVHC: the shipped app answers an AI question asked in a terminal of a remote project. The
// Rust fixture owns the terminal, its question and the hook's answer poll; the page reaches it only
// through the remote tab's project API, as the bridge forwards it (the bridge's own forwarding is
// covered against a real paired server by src/remote-project-parity.test.ts).
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';

for (const width of [1440, 900, 390])
  test(`answers a remote project's AI question from the in-app notice at ${width}px (HS2-HSFVHC)`, async ({ page }) => {
    test.setTimeout(120_000);
    const server = await realTicketServer();
    try {
      const remoteId = `remote:${BINDING}:${server.checkoutId}`,
        encoded = encodeURIComponent(remoteId),
        apiPath = `/__hotsheet/project-api/${encoded}`;
      await page.addInitScript((root) => {
        localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
      }, remoteId);
      await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
        route.request().postDataJSON().root === remoteId
          ? route.fulfill({
              status: 201,
              json: {
                id: remoteId,
                root: remoteId,
                name: 'Build host · Launch',
                remoteHost: 'Build host',
                stores: [],
                apiPath,
              },
            })
          : route.fulfill({ status: 404, json: { error: 'Unknown synthetic checkout' } }),
      );
      await page.route('**/__hotsheet/remotes', (route) => route.fulfill({ json: [] }));
      await page.routeWebSocket(/\/terminals\/[^/?]+\/attach(?:\?.*)?$/, (route) => {
        void route.close();
      });
      await routeProjectChangeSocket(page, server, encoded);
      const answers: unknown[] = [];
      await page.context().route(`**${apiPath}/**`, async (route) => {
        const request = route.request(),
          incoming = new URL(request.url());
        // The bridge addresses the host's checkout and presents host terminals in tab terms.
        const path = incoming.pathname
          .slice(apiPath.length)
          .replace(`/checkouts/${encoded}`, `/checkouts/${server.checkoutId}`);
        if (path.endsWith('/question/answer')) answers.push(request.postDataJSON());
        try {
          const response = await proxyFetch(route, {
            url: `${server.url}${path}${incoming.search}`,
            headers: { ...request.headers(), 'X-Hotsheet-Secret': server.secret },
            timeout: 60_000,
          });
          if (path === '/terminals' && request.method() === 'GET' && response.ok()) {
            const terminals = (await response.json()) as Array<Record<string, unknown>>;
            return await route.fulfill({
              response,
              json: terminals.map((terminal) => ({ ...terminal, launch_cwd: remoteId })),
            });
          }
          await route.fulfill({ response });
        } catch {
          await route.abort().catch(() => undefined);
        }
      });

      // A Claude session already runs in a terminal on the hosting machine.
      await server.request('/terminals', 'POST', { id: 'remote-claude', command: 'cat', cwd: server.root });
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/');
      await expect(page.getByText('Build host · Launch').first()).toBeVisible();

      // Its AskUserQuestion hook reports the question to its own server and waits for the answer.
      const questions = [
        { question: 'Which branch should I use?', options: [{ label: 'main' }, { label: 'release' }] },
        { question: 'Anything else?' },
      ];
      const asked = await fetch(`${server.url}/terminals/remote-claude/question`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: JSON.stringify({
          question: questions[0].question,
          tool_use_id: 'tool-remote',
          session_id: 'session-remote',
          questions,
        }),
      });
      expect(asked.status).toBe(204);
      const popup = page.getByRole('dialog', { name: 'AI waiting for your answer' });
      await expect(popup).toBeVisible();
      await expect(popup).toContainText('Which branch should I use?');
      await expect(popup).toContainText('Build host · Launch');
      await popup.getByLabel('release').check();
      await popup.getByRole('textbox', { name: 'Other answer for Anything else?' }).fill('Run the tests');
      await page.screenshot({ path: test.info().outputPath(`hs2-hsfvhc-remote-question-${width}.png`) });
      await popup.getByRole('button', { name: 'Send Answer' }).click();
      await expect(popup).toHaveCount(0);
      expect(answers).toHaveLength(1);

      // The hook on the hosting machine receives exactly what the remote tab sent.
      const polled = await fetch(
        `${server.url}/terminals/remote-claude/question?tool_use_id=tool-remote&session_id=session-remote`,
        { headers: { 'X-Hotsheet-Secret': server.secret } },
      );
      expect(polled.status).toBe(200);
      expect(await polled.json()).toEqual({
        answers: { 'Which branch should I use?': 'release', 'Anything else?': 'Run the tests' },
      });
    } finally {
      await server.stop(page.context(), page);
    }
  });
