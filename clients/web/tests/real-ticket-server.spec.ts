import { expect, test } from '@playwright/test';

import { openUpstreamSocket, realTicketServer } from './real-ticket-server';

test('HS2-N8FTCK closes routed WebSocket upstreams on stop and refuses a reconnect without dialing', async ({
  page,
}) => {
  // Playwright cannot remove a WebSocket route, so after `stop(page)` a reconnecting page still
  // reaches the test's handler. The shared upstream helper must refuse it instead of dialing the
  // stopped port, where an upstream without an `error` listener threw an unhandled ECONNREFUSED.
  const server = await realTicketServer(),
    target = `${server.url.replace(/^http/, 'ws')}/ws/sync?secret=${server.secret}&checkout=${server.checkoutId}`,
    dialed: boolean[] = [];
  let upstreamOpen: Promise<void> | undefined;
  try {
    await page.route('http://hs2-n8ftck.test/', (route) => route.fulfill({ contentType: 'text/html', body: '' }));
    await page.routeWebSocket('ws://hs2-n8ftck.test/proxy', (route) => {
      const upstream = openUpstreamSocket(route, target);
      dialed.push(upstream !== undefined);
      if (upstream)
        upstreamOpen ??= new Promise((resolve) => {
          upstream.once('open', () => {
            resolve();
          });
        });
    });
    await page.goto('http://hs2-n8ftck.test/');
    const connect = (name: string) =>
      page.evaluate((key) => {
        const sockets = ((window as unknown as { sockets?: Record<string, Promise<string>> }).sockets ??= {});
        sockets[key] = new Promise<string>((resolve) => {
          new WebSocket('ws://hs2-n8ftck.test/proxy').addEventListener('close', () => {
            resolve('closed');
          });
        });
      }, name);
    const closed = (name: string) =>
      page.evaluate((key) => (window as unknown as { sockets: Record<string, Promise<string>> }).sockets[key], name);

    await connect('live');
    await expect.poll(() => dialed).toEqual([true]);
    await upstreamOpen;
    await server.stop(page);
    // The live upstream closes with the server, and its route closes the page's socket.
    expect(await closed('live')).toBe('closed');

    await connect('reconnect');
    expect(await closed('reconnect')).toBe('closed');
    expect(dialed).toEqual([true, false]);
  } finally {
    await server.stop(page);
  }
});
