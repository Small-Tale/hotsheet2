import type { PendingOrchestratorAsk } from './orchestration-validation-steps';

/**
 * HS2-BPTQ5X: a drawer hand-back (HS2-KR28FS) waiting on its `session_start` permission request. It is
 * sent again under the same `handBackOperationId` once the request is answered, so an Allow finishes it
 * and a Deny is reported, without polling.
 */
export interface PendingHandBack {
  /** The hosted session: the checkout this machine hosts it for and its hosted operation id. */
  checkout_id: string;
  operation_id: string;
  /** The permission request's operation id, sent with every try of this hand-back. */
  handBackOperationId: string;
  /** The permission request it waits on, once the server named one. */
  requestId?: string;
}

/** The key of one hosted session's hand-back. */
export function handBackKey(checkout: string, operation: string): string {
  return `${checkout}/${operation}`;
}

/** A short, stable digest of a hosted session, so its permission request can be matched after a reload. */
export function handBackDigest(checkout: string, operation: string): string {
  // FNV-1a, 32 bits: a lookup tag, not a secret.
  let hash = 0x811c9dc5;
  for (const char of handBackKey(checkout, operation)) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

/** A new permission request id for a hand-back of the hosted session, naming it by its digest. */
export function handBackOperationId(checkout: string, operation: string, random: string): string {
  return `hand-back-${handBackDigest(checkout, operation)}-${random.replace(/[^A-Za-z0-9]/gu, '').slice(0, 32)}`;
}

/**
 * Hand-backs whose permission request still waits, recovered from the pending requests the app reads
 * from the server, for the hosted sessions this drawer shows (after a reload, when the drawer remembers
 * nothing). A request whose digest names none of `sessions` is left alone.
 */
export function recoveredHandBacks(
  asks: readonly PendingOrchestratorAsk[],
  sessions: readonly { checkout_id: string; operation_id: string }[],
): PendingHandBack[] {
  const byDigest = new Map(
    sessions.map((session) => [handBackDigest(session.checkout_id, session.operation_id), session]),
  );
  return asks.flatMap((ask) => {
    const digest = /^hand-back-([0-9a-f]{8})-/u.exec(ask.operation_id)?.[1];
    const session = digest === undefined ? undefined : byDigest.get(digest);
    return session
      ? [
          {
            checkout_id: session.checkout_id,
            operation_id: session.operation_id,
            handBackOperationId: ask.operation_id,
            requestId: ask.request_id,
          },
        ]
      : [];
  });
}

/**
 * The waiting hand-backs whose request was seen pending and is pending no longer: answered. `seen`
 * remembers the requests seen pending across calls; an answered one is forgotten.
 */
export function answeredHandBacks(
  waiting: readonly PendingHandBack[],
  pendingRequestIds: ReadonlySet<string>,
  seen: Set<string>,
): PendingHandBack[] {
  return waiting.filter((entry) => {
    if (entry.requestId === undefined) return false;
    if (pendingRequestIds.has(entry.requestId)) {
      seen.add(entry.requestId);
      return false;
    }
    if (!seen.has(entry.requestId)) return false;
    seen.delete(entry.requestId);
    return true;
  });
}
