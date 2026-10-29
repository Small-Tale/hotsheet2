import { expect, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;

/** Real local stores and authority; only the browser-to-owned-server routing is adapted. */
async function openFeedbackProjects(
  page: Page,
  source: Server,
  target: Server,
  beforePatch: () => Promise<void>,
  afterPatch?: (body: Record<string, unknown>) => Promise<void>,
) {
  // Hold genuine background event delivery until the normal400 error is observed (HS2-4Q44N2).
  // Then release it to exercise the authoritative refresh and retained response purpose.
  let releaseRefresh = () => {};
  const refreshGate = new Promise<void>((resolve) => {
    releaseRefresh = resolve;
  });
  const servers = source === target ? [source] : [source, target],
    writes: { project: string; path: string; status: number; body: Record<string, unknown>; code?: string }[] = [];
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route('**/__hotsheet/projects/open', (route) => {
    const server = route.request().postDataJSON().root === target.root ? target : source;
    return route.fulfill({
      status: 201,
      json: {
        id: server.checkoutId,
        root: server.root,
        name: server === source ? 'source' : 'target',
        stores: [server.store],
        apiPath: `/__hotsheet/project-api/${server.checkoutId}`,
      },
    });
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: target.root } }));
  for (const server of servers) {
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.route(`**${apiPath}/**`, async (route) => {
      const request = route.request(),
        url = new URL(request.url()),
        path = url.pathname.slice(apiPath.length),
        patch = request.method() === 'PATCH' && /\/checkouts\/[^/]+\/tickets\/[^/]+$/u.test(path);
      if (patch && server === target) await beforePatch();
      const response = await route.fetch({
        url: `${server.url}${path}${url.search}`,
        headers: { ...request.headers(), 'X-Hotsheet-Secret': server.secret },
      });
      if (patch) {
        const payload = (await response.json()) as { code?: string };
        writes.push({
          project: server.checkoutId,
          path: decodeURIComponent(path),
          status: response.status(),
          body: request.postDataJSON() as Record<string, unknown>,
          ...(payload.code ? { code: payload.code } : {}),
        });
      }
      if (patch && server === target) await afterPatch?.(request.postDataJSON() as Record<string, unknown>);
      if (path === '/ws/poll') await refreshGate;
      await route.fulfill({ response });
    });
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  if (source !== target) {
    await page.getByRole('button', { name: 'Add Project', exact: true }).click();
    await expect(page.getByRole('tab', { name: 'target', exact: true })).toBeVisible();
    await page.getByRole('tab', { name: 'source', exact: true }).click();
  }
  return { writes, releaseRefresh };
}

for (const surface of ['workspace', 'linked reader'] as const)
  test(`HS2-VBFSZJ preserves a typed dismissal-like reply and normal400 authority in the ${surface}`, async ({
    page,
  }) => {
    const source = await realTicketServer(),
      target = surface === 'workspace' ? source : await realTicketServer();
    let releaseRefresh = () => {};
    try {
      const ticket = await target.request<FullTicket>('/tickets', 'POST', { title: 'Synthetic reply owner' }),
        path = `/tickets/${ticket.id}`,
        asked = await target.request<FullTicket>(path, 'PATCH', {
          note: 'FEEDBACK NEEDED: choose a synthetic option.',
        }),
        request = asked.notes.find((note) => note.kind === 'feedback_needed')!,
        origin =
          surface === 'workspace'
            ? ticket
            : await source.request<FullTicket>('/tickets', 'POST', {
                title: 'Synthetic source selection',
                details: `Open @${target.checkoutId}/${ticket.slug} without changing this owner.`,
              });
      let corrected = false;
      const { writes, releaseRefresh: resume } = await openFeedbackProjects(page, source, target, async () => {
        if (corrected) return;
        corrected = true;
        await target.request(path, 'PATCH', { note_id: request.id, note_kind: 'regular' });
      });
      releaseRefresh = resume;
      const openReader = async () => {
        await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${origin.slug}"]`).first().click();
        if (surface === 'workspace')
          await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
        else
          await page
            .locator('#app-right-rail')
            .getByRole('link', { name: `@${target.checkoutId}/${ticket.slug}` })
            .click();
      };
      await openReader();
      const reader = page.locator('[data-reader-active="true"]'),
        card = reader.locator(`article[data-note-id="${request.id}"]`),
        response = card.getByRole('textbox', { name: 'Feedback response' });
      await expect(response).toBeVisible();
      await response.fill('No response needed');
      await card.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => writes.map((write) => write.status)).toEqual([409, 400]);
      expect(writes[0]?.code).toBe('ticket_changed');
      expect(writes[1]?.code).toBeUndefined();
      for (const write of writes) {
        expect(write.project).toBe(target.checkoutId);
        expect(write.path).toBe(`/checkouts/${target.checkoutId}/tickets/${ticket.qualified_id}`);
        expect(write.body.note).toBe('No response needed');
        expect(write.body.response_to).toBe(request.id);
      }
      await expect(page.locator('[data-component="app-error"]')).toContainText(
        'response_to must name a feedback request on this ticket',
      );
      if (process.env.HOTSHEET_VBFSZJ_VISUAL_REVIEW) {
        await expect(response).toHaveValue('No response needed');
        await page.screenshot({
          path: test.info().outputPath(`hs2-vbfszj-${surface.replace(' ', '-')}-typed-refusal.png`),
        });
      }
      releaseRefresh();
      await expect(response).toHaveValue('No response needed');
      await expect(reader.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
      expect((await target.request<FullTicket>(path)).notes.some((note) => note.response_to)).toBe(false);
      if (source !== target)
        await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
          'data-ticket-slug',
          origin.slug,
        );
      // Wait for the real refreshed kind, then prove later input stays an append draft.
      await expect(card.locator('.note-card__kind')).toContainText('Note');
      await response.fill('');
      await response.pressSequentially('A further local edit');
      await response.blur();
      await expect(response).toHaveValue('A further local edit');
      await response.fill('No response needed');
      expect(writes).toHaveLength(2);
      // An explicit dismissal now knows the refreshed target is Regular (HS2-VBFSZJ).
      await card.getByRole('button', { name: 'No Response Needed', exact: true }).click();
      await expect(response).toHaveCount(0);
      await expect(card.getByRole('textbox', { name: 'Note body', exact: true })).toHaveCount(0);
      await expect(
        page.getByText('This feedback request was already resolved, so no response was sent.').first(),
      ).toBeVisible();
      expect(writes).toHaveLength(2);
      expect((await target.request<FullTicket>(path)).notes.find((note) => note.id === request.id)?.text).toBe(
        request.text,
      );
      await reader.getByRole('button', { name: 'Close Ticket Reader' }).click();
      // A current request can still store exactly those words as an ordinary typed reply.
      await target.request(path, 'PATCH', { note_id: request.id, note_kind: 'feedback_needed' });
      await page.reload();
      await openReader();
      await expect(response).toBeVisible();
      await response.fill('No response needed');
      await card.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => writes.map((write) => write.status)).toEqual([409, 400, 200]);
      const stored = await target.request<FullTicket>(path);
      expect(stored.notes.filter((note) => note.response_to === request.id).map((note) => note.text)).toEqual([
        'No response needed',
      ]);
      await expect(response).toHaveCount(0);
      expect(stored.feedback_needed).toBe(false);
    } finally {
      releaseRefresh();
      try {
        await page.unrouteAll({ behavior: 'ignoreErrors' });
      } finally {
        if (target !== source) await target.stop();
        await source.stop();
      }
    }
  });

for (const surface of ['workspace', 'linked reader'] as const)
  test(`HS2-VBFSZJ retains a removed-request response through refresh, refusal and reset in the ${surface}`, async ({
    page,
  }) => {
    const source = await realTicketServer(),
      target = surface === 'workspace' ? source : await realTicketServer();
    let releaseRefresh = () => {};
    try {
      const ticket = await target.request<FullTicket>('/tickets', 'POST', {
          title: 'Synthetic removed response owner',
        }),
        path = `/tickets/${ticket.id}`,
        asked = await target.request<FullTicket>(path, 'PATCH', {
          note: 'FEEDBACK NEEDED: a synthetic removed request.',
        }),
        request = asked.notes.find((note) => note.kind === 'feedback_needed')!,
        origin =
          surface === 'workspace'
            ? ticket
            : await source.request<FullTicket>('/tickets', 'POST', {
                title: 'Synthetic separate source owner',
                details: `Open @${target.checkoutId}/${ticket.slug}.`,
              });
      let removed = false;
      const { writes, releaseRefresh: resume } = await openFeedbackProjects(page, source, target, async () => {
        if (removed) return;
        removed = true;
        await target.request(`${path}/notes/${request.id}`, 'DELETE');
      });
      releaseRefresh = resume;
      const openReader = async () => {
        await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${origin.slug}"]`).first().click();
        if (surface === 'workspace')
          await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
        else
          await page
            .locator('#app-right-rail')
            .getByRole('link', { name: `@${target.checkoutId}/${ticket.slug}` })
            .click();
      };
      await openReader();
      const reader = page.locator('[data-reader-active="true"]'),
        card = reader.locator(`article[data-note-id="${request.id}"]`),
        response = card.getByRole('textbox', { name: 'Feedback response' });
      await response.fill('An unsent reply to the original request');
      await card.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => writes.map((write) => write.status)).toEqual([409, 400]);
      await expect(page.locator('[data-component="app-error"]')).toContainText(
        'response_to must name a feedback request on this ticket',
      );
      releaseRefresh();
      await expect(card).toContainText('Unsent response to removed request');
      await expect(card.getByRole('button', { name: 'No Response Needed' })).toHaveCount(0);
      await response.fill('');
      await response.pressSequentially('A further unsent reply');
      await response.blur();
      await expect(response).toHaveValue('A further unsent reply');
      expect(writes).toHaveLength(2);
      if (process.env.HOTSHEET_VBFSZJ_VISUAL_REVIEW) {
        for (const [label, width] of [
          ['wide', 1440],
          ['medium', 900],
          ['narrow', 390],
          ['wide-again', 1440],
        ] as const) {
          await page.setViewportSize({ width, height: 900 });
          await expect(response).toBeInViewport();
          await page.screenshot({
            path: test.info().outputPath(`hs2-vbfszj-${surface.replace(' ', '-')}-removed-${label}.png`),
          });
        }
      }
      await card.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => writes.map((write) => write.status)).toEqual([409, 400, 400]);
      await expect(response).toHaveValue('A further unsent reply');
      for (const write of writes) {
        expect(write.project).toBe(target.checkoutId);
        expect(write.path).toBe(`/checkouts/${target.checkoutId}/tickets/${ticket.qualified_id}`);
        expect(write.body.response_to).toBe(request.id);
        expect(write.body).not.toHaveProperty('note_id');
      }
      const stored = await target.request<FullTicket>(path);
      expect(stored.notes.some((note) => note.id === request.id || note.response_to)).toBe(false);
      await reader.getByRole('button', { name: 'Close Ticket Reader' }).click();
      await openReader();
      await expect(reader.getByText('Unsent response to removed request', { exact: true })).toHaveCount(0);
      await expect(reader.getByRole('textbox', { name: 'Feedback response' })).toHaveCount(0);
      expect(writes).toHaveLength(3);
      if (source !== target)
        await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
          'data-ticket-slug',
          origin.slug,
        );
    } finally {
      releaseRefresh();
      try {
        await page.unrouteAll({ behavior: 'ignoreErrors' });
      } finally {
        if (target !== source) await target.stop();
        await source.stop();
      }
    }
  });

for (const surface of ['workspace', 'linked reader'] as const)
  test(`HS2-VBFSZJ fences a held reply success from a newer response owner in the ${surface}`, async ({ page }) => {
    const source = await realTicketServer(),
      target = surface === 'workspace' ? source : await realTicketServer();
    let releaseRefresh = () => {},
      releaseReply = () => {};
    const heldReply = new Promise<void>((resolve) => {
      releaseReply = resolve;
    });
    try {
      const seed = async (title: string) => {
        const ticket = await target.request<FullTicket>('/tickets', 'POST', { title });
        const asked = await target.request<FullTicket>(`/tickets/${ticket.id}`, 'PATCH', {
          note: 'FEEDBACK NEEDED: choose a synthetic option.',
        });
        return { ticket, request: asked.notes.find((note) => note.kind === 'feedback_needed')! };
      };
      const first = await seed('Synthetic first response owner'),
        second = await seed('Synthetic newer response owner'),
        origin =
          surface === 'workspace'
            ? undefined
            : await source.request<FullTicket>('/tickets', 'POST', {
                title: 'Synthetic independent source',
                details: `First @${target.checkoutId}/${first.ticket.slug}; second @${target.checkoutId}/${second.ticket.slug}.`,
              });
      const { writes, releaseRefresh: resume } = await openFeedbackProjects(
        page,
        source,
        target,
        async () => {},
        async (body) => {
          if (body.response_to === first.request.id) await heldReply;
        },
      );
      releaseRefresh = resume;
      const open = async (ticket: FullTicket) => {
        if (surface === 'workspace') {
          await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${ticket.slug}"]`).first().click();
          await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
        } else {
          await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${origin!.slug}"]`).first().click();
          await page
            .locator('#app-right-rail')
            .getByRole('link', { name: `@${target.checkoutId}/${ticket.slug}` })
            .click();
        }
      };
      const reader = page.locator('[data-reader-active="true"]');
      await open(first.ticket);
      const firstDraft = reader.getByRole('textbox', { name: 'Feedback response' });
      await firstDraft.fill('First owner submitted reply');
      await reader.getByRole('button', { name: 'Use large reader text size' }).click();
      await expect(firstDraft).toHaveValue('First owner submitted reply');
      await firstDraft.press('End');
      await firstDraft.pressSequentially(' after another input');
      await reader.getByRole('button', { name: 'Use standard reader text size' }).click();
      await expect(firstDraft).toHaveValue('First owner submitted reply after another input');
      await reader.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => writes.map((write) => write.status)).toEqual([200]);
      await test.step('finish closing the old native reader before opening a new owner', async () => {
        await reader.getByRole('button', { name: 'Close Ticket Reader' }).click();
        await expect(reader).toHaveCount(0);
      });
      await test.step('open the newer response owner', () => open(second.ticket));
      const secondDraft = reader.getByRole('textbox', { name: 'Feedback response' });
      await expect(secondDraft).toBeVisible();
      await secondDraft.fill('New owner unsent reply');
      const completed = page.waitForResponse(
        (response) =>
          response.request().method() === 'PATCH' && response.request().postDataJSON().response_to === first.request.id,
      );
      releaseReply();
      await completed;
      releaseRefresh();
      await secondDraft.pressSequentially(' remains editable');
      await secondDraft.blur();
      await expect(secondDraft).toHaveValue('New owner unsent reply remains editable');
      expect(
        (await target.request<FullTicket>(`/tickets/${second.ticket.id}`)).notes.some((note) => note.response_to),
      ).toBe(false);
      await reader.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => writes.map((write) => write.status)).toEqual([200, 200]);
      expect(writes.map((write) => [write.project, write.body.response_to, write.body.note])).toEqual([
        [target.checkoutId, first.request.id, 'First owner submitted reply after another input'],
        [target.checkoutId, second.request.id, 'New owner unsent reply remains editable'],
      ]);
      await expect(secondDraft).toHaveCount(0);
    } finally {
      releaseReply();
      releaseRefresh();
      try {
        await page.unrouteAll({ behavior: 'ignoreErrors' });
      } finally {
        if (target !== source) await target.stop();
        await source.stop();
      }
    }
  });

test('HS2-VBFSZJ preserves the real description response append and a further reply after reset', async ({ page }) => {
  const server = await realTicketServer();
  let releaseRefresh = () => {};
  try {
    const originalDetails = 'FEEDBACK NEEDED: respond to this synthetic description.',
      ticket = await server.request<FullTicket>('/tickets', 'POST', {
        title: 'Synthetic description reply',
        details: originalDetails,
      });
    const opened = await openFeedbackProjects(page, server, server, async () => {});
    releaseRefresh = opened.releaseRefresh;
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${ticket.slug}"]`).first().click();
    await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
    const reader = page.locator('[data-reader-active="true"]'),
      details = reader.locator('[data-details-feedback="true"]'),
      response = details.getByRole('textbox', { name: 'Feedback response' });
    for (const [index, text] of ['A description response', 'A further description response'].entries()) {
      await response.fill(text);
      await expect(response).toHaveValue(text);
      await details.getByRole('button', { name: 'Respond', exact: true }).click();
      await expect.poll(() => opened.writes.length).toBe(index + 1);
      const write = opened.writes[index];
      expect(write.status).toBe(200);
      expect(write.project).toBe(server.checkoutId);
      expect(write.path).toBe(`/checkouts/${server.checkoutId}/tickets/${ticket.qualified_id}`);
      expect(write.body).toMatchObject({ note: text, note_kind: 'regular' });
      expect(write.body).not.toHaveProperty('response_to');
      expect(write.body).not.toHaveProperty('note_id');
      await expect(response).toHaveCount(0);
      const saved = await server.request<FullTicket>(`/tickets/${ticket.id}`);
      const reply = saved.notes.find((note) => note.text === text);
      expect(reply?.kind).toBe('regular');
      expect(saved.feedback_needed).toBe(false);
      if (index === 0) {
        // A regular response resolves description feedback. Reset the synthetic request through
        // real authority before exercising a further reply; a resolved editor stays closed.
        await server.request(`/tickets/${ticket.id}/notes/${reply!.id}`, 'DELETE');
        releaseRefresh();
        await page.reload();
        await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${ticket.slug}"]`).first().click();
        await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
        await expect(response).toHaveValue('');
      }
    }
    const stored = await server.request<FullTicket>(`/tickets/${ticket.id}`);
    expect(stored.details).toBe(originalDetails);
    expect(stored.notes.filter((note) => note.kind === 'regular').map((note) => note.text)).toEqual([
      'A further description response',
    ]);
  } finally {
    releaseRefresh();
    try {
      await page.unrouteAll({ behavior: 'ignoreErrors' });
    } finally {
      await server.stop();
    }
  }
});
