import { createServer, request as requestHttp } from 'node:http';
import { connect } from 'node:net';

import { installProjectWebSocketBridge } from '../src/terminal-ws-bridge';
import type { realTicketServer } from './real-ticket-server';

export const remoteOrigin = 'http://hotsheet-remote.test';

export async function insecureOriginProxy(
  baseURL: string,
  terminalServer?: Awaited<ReturnType<typeof realTicketServer>>,
) {
  // A real HTTP proxy preserves WebKit's non-loopback origin security rules. It
  // streams Vite's module graph without routing every asset through APIRequest.
  const server = createServer((incoming, outgoing) => {
    const path = new URL(incoming.url!, remoteOrigin),
      upstream = requestHttp(
        `${baseURL}${path.pathname}${path.search}`,
        {
          method: incoming.method,
          headers: { ...incoming.headers, host: new URL(baseURL).host },
        },
        (response) => {
          outgoing.writeHead(response.statusCode!, response.headers);
          response.pipe(outgoing);
        },
      );
    upstream.on('error', () => {
      outgoing.writeHead(502);
      outgoing.end();
    });
    incoming.pipe(upstream);
  });
  server.on('connect', (incoming, socket, head) => {
    const target = new URL(`http://${incoming.url}`);
    const remote = target.hostname === new URL(remoteOrigin).hostname;
    if (!remote && target.hostname !== '127.0.0.1') {
      socket.destroy();
      return;
    }
    const upstream = connect(
      remote ? (server.address() as { port: number }).port : Number(target.port),
      '127.0.0.1',
      () => {
        socket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
        if (head.length) upstream.write(head);
        socket.pipe(upstream).pipe(socket);
      },
    );
    upstream.on('error', () => socket.destroy());
    socket.on('error', () => upstream.destroy());
    socket.on('close', () => upstream.destroy());
  });
  if (terminalServer) {
    const socketBase = terminalServer.url.replace('http:', 'ws:');
    installProjectWebSocketBridge(
      { httpServer: server },
      (_project, terminal) => `${socketBase}/terminals/${terminal}/attach?secret=${terminalServer.secret}`,
      () => `${socketBase}/ws/sync?secret=${terminalServer.secret}`,
    );
  }
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address() as { port: number };
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => {
          if (error) reject(error);
          else resolve();
        });
        server.closeAllConnections();
      }),
  };
}
