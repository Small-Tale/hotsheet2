import { once } from 'node:events';
import { createServer } from 'node:http';

import { expect, type Page, test } from '@playwright/test';
import WebSocket, { WebSocketServer } from 'ws';

test('two tabs share one bridge socket but keep independent project streams (HS2-NP7R42)', async ({
  page,
  context,
}) => {
  const upstreamHttp = createServer(),
    upstream = new WebSocketServer({ server: upstreamHttp });
  upstreamHttp.listen(0, '127.0.0.1');
  await once(upstreamHttp, 'listening');
  const address = upstreamHttp.address();
  if (!address || typeof address === 'string') throw new Error('Missing socket address.');
  const endpoint = `127.0.0.1:${address.port}`;
  const commands: Array<{ type: string; client: string; project: string; subscription: number }> = [];
  const connected = new Set<WebSocket>();
  upstream.on('connection', (socket) => {
    connected.add(socket);
    socket.on('close', () => connected.delete(socket));
    socket.on('message', (raw) => {
      const text = Array.isArray(raw)
        ? Buffer.concat(raw).toString('utf8')
        : Buffer.isBuffer(raw)
          ? raw.toString('utf8')
          : Buffer.from(raw).toString('utf8');
      const command = JSON.parse(text) as (typeof commands)[number];
      commands.push(command);
      if (command.type === 'subscribe')
        socket.send(
          JSON.stringify({
            version: 2,
            type: 'subscribed',
            client: command.client,
            project: command.project,
            subscription: command.subscription,
          }),
        );
    });
  });
  const second = await context.newPage();
  const connect = async (tab: Page, clientId: string) => {
    await tab.goto('/');
    await tab.evaluate(
      async ({ endpoint: target, clientId: client }) => {
        const worker = new SharedWorker(new URL('/tests/fixtures/change-stream-shared-harness.ts', location.href), {
          type: 'module',
          name: 'hotsheet-shared-harness',
        });
        const state = window as Window & { __sharedPort?: MessagePort; __sharedFrames?: unknown[] };
        state.__sharedPort = worker.port;
        state.__sharedFrames = [];
        worker.port.addEventListener('message', (event) => state.__sharedFrames!.push(event.data));
        worker.port.start();
        worker.port.postMessage({ type: 'hello', protocol: 1 });
        await new Promise<void>((resolve) => {
          const ready = (event: MessageEvent) => {
            if (event.data?.type !== 'ready') return;
            worker.port.removeEventListener('message', ready);
            resolve();
          };
          worker.port.addEventListener('message', ready);
        });
        worker.port.postMessage({
          type: 'open',
          project: 'same',
          generation: 1,
          apiUrl: `http://${target}/__hotsheet/project-api/same`,
          webSocketUrl: `ws://${target}/__hotsheet/project-api/same/ws/sync?client=${client}`,
          changeMuxUrl: `http://${target}/__hotsheet/change-mux/poll`,
          clientId: client,
        });
      },
      { endpoint, clientId },
    );
  };
  try {
    await connect(page, 'tab-a');
    await connect(second, 'tab-b');
    await expect.poll(() => commands.filter((command) => command.type === 'subscribe').length).toBe(2);
    expect(connected.size).toBe(1);
    const subscriptions = commands.filter((command) => command.type === 'subscribe');
    const send = (client: string, cursor: number) => {
      const subscription = subscriptions.find((item) => item.client === client)!;
      [...connected][0].send(
        JSON.stringify({
          version: 2,
          type: 'event',
          client,
          project: 'same',
          subscription: subscription.subscription,
          data: JSON.stringify({ cursor, store: 'same', kind: 'changed', id: String(cursor), slug: `HS2-${cursor}` }),
        }),
      );
    };
    send('tab-b', 1);
    await expect
      .poll(() =>
        second.evaluate(
          () =>
            (window as Window & { __sharedFrames?: Array<{ type?: string }> }).__sharedFrames?.filter(
              (frame) => frame.type === 'batch',
            ).length ?? 0,
        ),
      )
      .toBe(1);
    expect(
      await page.evaluate(
        () =>
          (window as Window & { __sharedFrames?: Array<{ type?: string }> }).__sharedFrames?.filter(
            (frame) => frame.type === 'batch',
          ).length ?? 0,
      ),
    ).toBe(0);
    await page.evaluate(() =>
      (window as Window & { __sharedPort?: MessagePort }).__sharedPort?.postMessage({ type: 'dispose' }),
    );
    await expect
      .poll(() => commands.some((command) => command.type === 'unsubscribe' && command.client === 'tab-a'))
      .toBe(true);
    expect(connected.size).toBe(1);
    await second.evaluate(() =>
      (window as Window & { __sharedPort?: MessagePort }).__sharedPort?.postMessage({
        type: 'ack',
        project: 'same',
        generation: 1,
        seq: 1,
      }),
    );
    send('tab-b', 2);
    await expect
      .poll(() =>
        second.evaluate(
          () =>
            (window as Window & { __sharedFrames?: Array<{ type?: string }> }).__sharedFrames?.filter(
              (frame) => frame.type === 'batch',
            ).length ?? 0,
        ),
      )
      .toBe(2);
    await second.evaluate(() =>
      (window as Window & { __sharedPort?: MessagePort }).__sharedPort?.postMessage({ type: 'dispose' }),
    );
    await expect.poll(() => connected.size).toBe(0);
  } finally {
    await second.close();
    for (const socket of connected) socket.terminate();
    await new Promise<void>((resolve) => {
      upstream.close(() => {
        resolve();
      });
    });
    await new Promise<void>((resolve) => {
      upstreamHttp.close(() => {
        resolve();
      });
    });
  }
});

test('two tabs share one long poll for the same project with client-scoped answers (HS2-NP7R42)', async ({
  page,
  context,
}) => {
  const requests: Array<{ version: number; polls: Array<{ project: string; client: string }> }> = [];
  const server = createServer((request, response) => {
    response.setHeader('Access-Control-Allow-Origin', '*');
    response.setHeader('Access-Control-Allow-Methods', 'POST, OPTIONS');
    response.setHeader('Access-Control-Allow-Headers', 'Content-Type');
    if (request.method === 'OPTIONS') {
      response.writeHead(204).end();
      return;
    }
    void (async () => {
      let body = '';
      for await (const chunk of request) body += String(chunk);
      const parsed = JSON.parse(body) as (typeof requests)[number];
      requests.push(parsed);
      if (parsed.polls.length !== 2) return;
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(
        JSON.stringify({
          version: 2,
          results: parsed.polls.map((poll) => ({
            project: poll.project,
            client: poll.client,
            status: 200,
            body: { cursor: poll.client === 'tab-a' ? 1 : 2, overflow: false, events: [] },
          })),
        }),
      );
    })();
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing poll address.');
  const endpoint = `http://127.0.0.1:${address.port}`;
  const second = await context.newPage();
  const connect = async (tab: Page, client: string) => {
    await tab.goto('/');
    await tab.evaluate(
      async ({ endpoint: target, client }) => {
        const worker = new SharedWorker(
          new URL('/tests/fixtures/change-stream-shared-poll-harness.ts', location.href),
          {
            type: 'module',
            name: 'hotsheet-shared-poll-harness',
          },
        );
        const state = window as Window & { __sharedPort?: MessagePort; __sharedFrames?: unknown[] };
        state.__sharedPort = worker.port;
        state.__sharedFrames = [];
        worker.port.addEventListener('message', (event) => state.__sharedFrames!.push(event.data));
        worker.port.start();
        worker.port.postMessage({ type: 'hello', protocol: 1 });
        await new Promise<void>((resolve) => {
          const ready = (event: MessageEvent) => {
            if (event.data?.type !== 'ready') return;
            worker.port.removeEventListener('message', ready);
            resolve();
          };
          worker.port.addEventListener('message', ready);
        });
        worker.port.postMessage({
          type: 'open',
          project: 'same',
          generation: 1,
          apiUrl: `${target}/__hotsheet/project-api/same`,
          changeMuxUrl: `${target}/__hotsheet/change-mux/poll`,
          clientId: client,
        });
      },
      { endpoint, client },
    );
  };
  try {
    await connect(page, 'tab-a');
    await connect(second, 'tab-b');
    await expect.poll(() => requests.some((request) => request.polls.length === 2)).toBe(true);
    const shared = requests.find((request) => request.polls.length === 2)!;
    expect(shared.version).toBe(2);
    expect(shared.polls).toEqual([
      { project: 'same', client: 'tab-a', timeout_ms: expect.any(Number) },
      { project: 'same', client: 'tab-b', timeout_ms: expect.any(Number) },
    ]);
    const cursor = (tab: Page) =>
      tab.evaluate(
        () =>
          (
            window as Window & { __sharedFrames?: Array<{ type?: string; response?: { cursor: number } }> }
          ).__sharedFrames?.find((frame) => frame.type === 'batch')?.response?.cursor,
      );
    await expect.poll(() => cursor(page)).toBe(1);
    await expect.poll(() => cursor(second)).toBe(2);
  } finally {
    await page.evaluate(() =>
      (window as Window & { __sharedPort?: MessagePort }).__sharedPort?.postMessage({ type: 'dispose' }),
    );
    await second.evaluate(() =>
      (window as Window & { __sharedPort?: MessagePort }).__sharedPort?.postMessage({ type: 'dispose' }),
    );
    await second.close();
    server.closeAllConnections();
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve();
      });
    });
  }
});
