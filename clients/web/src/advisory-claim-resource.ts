import { signal } from 'kerfjs';

import type { AdvisoryClaimEventInput, AdvisoryClaimStatus } from './api';

export interface AdvisoryClaimTarget {
  checkout: string;
  connection: string;
  nativeId: string;
  apiPath?: string;
}

export interface AdvisoryClaimResourceState {
  target?: AdvisoryClaimTarget;
  status?: AdvisoryClaimStatus;
  loading: boolean;
  busy: boolean;
  error?: string;
}

type Transport = {
  advisoryClaimStatus: (checkout: string, connection: string, nativeId: string) => Promise<AdvisoryClaimStatus>;
  appendAdvisoryClaimEvent: (
    checkout: string,
    connection: string,
    nativeId: string,
    event: AdvisoryClaimEventInput,
  ) => Promise<AdvisoryClaimStatus>;
};

const targetKey = (target: AdvisoryClaimTarget) =>
  JSON.stringify([target.checkout, target.connection, target.nativeId, target.apiPath]);
const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/** A live marker wins over a stale session retry id after reload. */
export function activeAdvisoryOperationId(status: AdvisoryClaimStatus | undefined, worker: string, saved?: string) {
  return status?.projection.claims.find((claim) => claim.worker === worker)?.operation_id ?? saved;
}

/** Renew only within the v2 operation's 24-hour horizon; old live v1 markers remain renewable. */
export function advisoryRenewExpiry(operationId: string, now = Date.now()): number | undefined {
  const issued = /^v2:(\d{13}):/.exec(operationId)?.[1];
  const expiry = issued ? Math.min(now + 30 * 60_000, Number(issued) + 24 * 60 * 60_000) : now + 30 * 60_000;
  return expiry > now ? expiry : undefined;
}

/** Selected-ticket-only resource; every request is scoped and stale responses are discarded. */
export function createAdvisoryClaimResource(transport: () => Transport) {
  const state = signal<AdvisoryClaimResourceState>({ loading: false, busy: false });
  let generation = 0;

  function select(target?: AdvisoryClaimTarget) {
    const previous = state.peek().target;
    if (target && previous && targetKey(target) === targetKey(previous)) return;
    generation += 1;
    state.value = { target, loading: Boolean(target), busy: false };
    if (target) void refresh();
  }

  async function refresh() {
    const target = state.peek().target;
    if (!target) return;
    // A collection event during a write must not re-enable a second write or hide its outcome.
    if (state.peek().busy) return;
    const read = ++generation;
    state.value = { target, loading: true, busy: false };
    try {
      const status = await transport().advisoryClaimStatus(target.checkout, target.connection, target.nativeId);
      if (read === generation) state.value = { target, status, loading: false, busy: false };
    } catch (reason) {
      // A failed remote read cannot be represented by a stale or empty projection.
      if (read === generation) state.value = { target, loading: false, busy: false, error: message(reason) };
    }
  }

  async function submit(event: AdvisoryClaimEventInput): Promise<AdvisoryClaimStatus | undefined> {
    const target = state.peek().target;
    if (!target || state.peek().busy) return;
    const write = ++generation;
    state.value = { ...state.peek(), busy: true, error: undefined };
    try {
      const status = await transport().appendAdvisoryClaimEvent(
        target.checkout,
        target.connection,
        target.nativeId,
        event,
      );
      if (write === generation) {
        state.value = { target, status, loading: false, busy: false };
        return status;
      }
    } catch (reason) {
      if (write === generation) state.value = { target, loading: false, busy: false, error: message(reason) };
    }
  }

  return { state, select, refresh, submit };
}
