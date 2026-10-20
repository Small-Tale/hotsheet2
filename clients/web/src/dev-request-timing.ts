import { randomUUID } from 'node:crypto';

/** Timings added only by the development bridge; the production host never calls this wrapper. */
export async function traceDevBridgeRequest(
  forward: (mark: (phase: 'dispatch' | 'headers') => void) => Promise<Response>,
  now: () => number = () => performance.now(),
  traceId: () => string = randomUUID,
): Promise<Response> {
  const received = now();
  let dispatched: number | undefined, headers: number | undefined;
  const response = await forward((phase) => {
    if (phase === 'dispatch') dispatched = now();
    else headers = now();
  });
  const returned = now(),
    metrics = [
      `bridge;dur=${Math.max(0, returned - received).toFixed(2)}`,
      ...(dispatched === undefined ? [] : [`bridge_queue;dur=${Math.max(0, dispatched - received).toFixed(2)}`]),
      ...(dispatched === undefined || headers === undefined
        ? []
        : [`upstream;dur=${Math.max(0, headers - dispatched).toFixed(2)}`]),
    ],
    resultHeaders = new Headers(response.headers),
    previous = resultHeaders.get('server-timing');
  resultHeaders.set('server-timing', [...(previous ? [previous] : []), ...metrics].join(', '));
  resultHeaders.set('x-hotsheet-trace-id', traceId());
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers: resultHeaders,
  });
}
