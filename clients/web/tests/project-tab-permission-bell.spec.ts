import { expect, test } from '@playwright/test';

import { desktopApp, dialog, openNewTicket } from './desktop-draft-files-fixture';

// The real server lists every pending ask machine-wide; each project tab must badge only the asks whose
// checkout permission identity names that project (HS2-H8TDMP).
test('shows the project tab bell only on projects with their own pending permissions (HS2-H8TDMP)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  const pending = new Set<AbortController>();
  try {
    const page = await app.launch();
    await openNewTicket(page, true);
    await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
    const first = app.server.checkoutId,
      other = await app.addProject(page);
    // A hook's ask stays open until it is answered; aborting it withdraws the prompt like a closed hook.
    const ask = (checkout: string, action: string) => {
      const controller = new AbortController();
      pending.add(controller);
      const answer = fetch(`${app.server.url}/permissions/ask`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': app.server.secret },
        body: JSON.stringify({
          project: `checkout:${checkout}`,
          connection: `synthetic-${checkout}`,
          tool: 'Bash',
          action,
        }),
        signal: controller.signal,
      })
        .then((response) => response.json() as Promise<{ decision: string }>)
        .catch(() => ({ decision: 'withdrawn' }))
        .finally(() => {
          pending.delete(controller);
        });
      return {
        answer,
        withdraw: () => {
          controller.abort();
        },
      };
    };
    const projectTab = (name: string) =>
      page
        .locator('[data-tab-kind="project"]')
        .filter({ has: page.locator('.kui-tab-bar-item__name', { hasText: new RegExp(`^${name}$`, 'u') }) });
    const bell = (name: string) => projectTab(name).locator('.project-tab__notification');
    // Absence is only meaningful once both projects have read the feed that holds the latest change.
    const refreshed = () =>
      Promise.all(
        [first, other].map((checkout) =>
          page.waitForResponse(
            (response) =>
              response.url().includes(`/__hotsheet/project-api/${checkout}/permissions`) &&
              response.request().method() === 'GET',
          ),
        ),
      );
    const expectBells = async (firstLabel: string | undefined, otherLabel: string | undefined) => {
      for (const [name, label] of [
        ['drafts', firstLabel],
        ['other-project', otherLabel],
      ] as const)
        if (label) await expect(bell(name)).toHaveAttribute('aria-label', label);
        else await expect(bell(name)).toHaveCount(0);
    };
    const popup = page.locator('[data-component="permission-request-popup"]');

    // One ask in the unselected first project badges only that tab.
    let read = refreshed();
    const firstAsk = ask(first, 'echo synthetic first project');
    await read;
    await expectBells('1 pending notification', undefined);
    await expect(popup).toContainText('synthetic first project');

    // Asks in different projects count toward their own project only.
    const otherAsk = ask(other, 'echo synthetic other project one');
    const otherSecond = ask(other, 'echo synthetic other project two');
    await expectBells('1 pending notification', '2 pending notifications');

    // Answering the first project's ask clears its bell and leaves the other project's count.
    await expect(popup).toContainText('synthetic first project');
    read = refreshed();
    await popup.getByRole('button', { name: 'Deny', exact: true }).click();
    await read;
    expect(await firstAsk.answer).toEqual({ decision: 'deny' });
    await expectBells(undefined, '2 pending notifications');

    // A withdrawn ask clears from its project, and a new ask badges the first project again.
    read = refreshed();
    otherSecond.withdraw();
    await read;
    await expectBells(undefined, '1 pending notification');
    read = refreshed();
    const again = ask(first, 'echo synthetic first project again');
    await read;
    await expectBells('1 pending notification', '1 pending notification');

    // Switching projects keeps each tab's own count.
    await projectTab('drafts').getByRole('tab').click();
    await expectBells('1 pending notification', '1 pending notification');
    otherAsk.withdraw();
    again.withdraw();
    await expectBells(undefined, undefined);
  } finally {
    for (const controller of pending) controller.abort();
    await app.stop();
  }
});
