import { once } from 'node:events';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer, type Server } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, it } from 'vitest';

import { projectSessionRegistry, proxyProjectRequest } from '../project-bridge';
import { remoteProjectId } from './identity';

const first = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const second = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
const originalHome = process.env.HOTSHEET_HOME;
const servers: Server[] = [];
const homes: string[] = [];

afterEach(async () => {
  projectSessionRegistry().clear();
  if (originalHome === undefined) delete process.env.HOTSHEET_HOME;
  else process.env.HOTSHEET_HOME = originalHome;
  await Promise.all(
    servers.splice(0).map(
      (server) =>
        new Promise<void>((resolve) =>
          server.close(() => {
            resolve();
          }),
        ),
    ),
  );
  await Promise.all(homes.splice(0).map((home) => rm(home, { recursive: true, force: true })));
});

it('HS2-TCJQHX sends native ids and scopes AI records through two real remote hosts', async () => {
  const seen: Array<{ host: string; path: string; body?: unknown }> = [];
  const start = async (host: string) => {
    const server = createServer((request, response) => {
      void (async () => {
        const chunks: Buffer[] = [];
        for await (const chunk of request) chunks.push(Buffer.from(chunk));
        const body = chunks.length ? (JSON.parse(Buffer.concat(chunks).toString('utf8')) as unknown) : undefined;
        seen.push({ host, path: request.url ?? '', ...(body === undefined ? {} : { body }) });
        response.setHeader('content-type', 'application/json');
        if (request.url === '/connections') {
          response.end(
            JSON.stringify([
              { id: `${host}-app`, project: '/srv/app', tool: 'claude' },
              { id: `${host}-api`, project: '/srv/api', tool: 'claude' },
            ]),
          );
        } else if (request.url === '/drive/sessions') {
          response.end(
            JSON.stringify([
              { connection_id: `${host}-app`, project: '/srv/app', session_id: 'thread' },
              { connection_id: `${host}-api`, project: '/srv/api', session_id: 'other' },
            ]),
          );
        } else if (request.url === '/drive/connections') {
          response.end(JSON.stringify({ id: `${host}-new`, project: '/srv/app', tool: 'claude' }));
        } else {
          response.statusCode = 404;
          response.end('{}');
        }
      })().catch(() => {
        response.statusCode = 500;
        response.end('{}');
      });
    });
    servers.push(server);
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('fixture did not bind');
    return `http://127.0.0.1:${address.port}`;
  };
  const leftOrigin = await start('left');
  const rightOrigin = await start('right');
  const home = await mkdtemp(join(tmpdir(), 'hs2-tcjqhx-'));
  homes.push(home);
  process.env.HOTSHEET_HOME = home;
  await writeFile(
    join(home, 'remotes.json'),
    JSON.stringify({
      servers: [
        { id: first, revision: 1, revoked: false, origin: leftOrigin, identity: { server_id: first } },
        { id: second, revision: 1, revoked: false, origin: rightOrigin, identity: { server_id: second } },
      ],
    }),
  );
  const register = (bindingId: string, checkoutId: string, root: string, origin: string) => {
    const id = remoteProjectId(bindingId, checkoutId);
    projectSessionRegistry().set(id, {
      url: origin,
      secret: '',
      remote: {
        bindingId,
        bindingRevision: 1,
        checkoutId,
        root,
        stores: [],
        connection: {
          binding: {
            id: bindingId,
            label: bindingId,
            revision: 1,
            revoked: false,
            origin,
            identity: { server_id: bindingId, ca_fingerprint: null },
            last_failure: null,
            last_confirmed_checkouts: [],
            stale: false,
          },
          credentials: {
            bearer_secret: 'session:approved',
            session_origin: 'http://localhost:4175',
            session_csrf: 'csrf',
          },
        },
      },
    });
    return id;
  };
  const leftApp = register(first, 'same', '/srv/app', leftOrigin);
  const leftApi = register(first, 'api', '/srv/api', leftOrigin);
  const rightApp = register(second, 'same', '/srv/app', rightOrigin);
  const send = (id: string, path: string, body?: object) =>
    proxyProjectRequest(
      id,
      path,
      new Request(`http://bridge.invalid${path}`, {
        method: body === undefined ? 'GET' : 'POST',
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }),
    );

  expect(await (await send(leftApp, '/connections')).json()).toEqual([
    { id: 'left-app', project: leftApp, tool: 'claude' },
  ]);
  expect(await (await send(leftApi, '/connections')).json()).toEqual([
    { id: 'left-api', project: leftApi, tool: 'claude' },
  ]);
  expect(await (await send(rightApp, '/connections')).json()).toEqual([
    { id: 'right-app', project: rightApp, tool: 'claude' },
  ]);
  expect(await (await send(leftApp, '/drive/sessions')).json()).toEqual([
    { connection_id: 'left-app', project: leftApp, session_id: 'thread' },
  ]);
  expect(await (await send(leftApp, '/drive/connections', { checkout: leftApp, tool: 'claude' })).json()).toEqual({
    id: 'left-new',
    project: leftApp,
    tool: 'claude',
  });
  expect(seen.at(-1)).toEqual({ host: 'left', path: '/drive/connections', body: { checkout: 'same', tool: 'claude' } });
});
