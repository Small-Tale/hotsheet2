/**
 * Close codes a WebSocket relay passes from its upstream to the browser (HS2-QWT94V). Only the
 * application range (4000-4999) carries meaning a viewer acts on, such as a terminal that ended; the
 * reserved codes a dropped connection reports (1005, 1006) cannot be sent, so the browser gets a plain
 * close for those, and reconnects as before.
 */
export function relayableCloseCode(code: unknown): number | undefined {
  return typeof code === 'number' && Number.isInteger(code) && code >= 4000 && code <= 4999 ? code : undefined;
}

/** Close `target` the way its upstream closed: with the upstream's application code when it sent one. */
export function closeLikeUpstream(
  target: { close(code?: number, reason?: string): void },
  code: unknown,
  reason: unknown,
): void {
  const relayed = relayableCloseCode(code);
  if (relayed === undefined) {
    target.close();
    return;
  }
  const text =
    typeof reason === 'string' ? reason : reason instanceof Uint8Array ? new TextDecoder().decode(reason) : '';
  target.close(relayed, text);
}
