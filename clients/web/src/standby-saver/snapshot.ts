import type { LiveStateList } from '../api';
import { isRecord } from '../json-value';
import { parseLiveStateList, type ResponseParser, ServerResponseShapeError } from '../server-response-parsers';
import {
  formatStandbyClock,
  type StandbyFreshness,
  standbyFreshnessLabel,
  type StandbyModel,
  standbyModel,
} from '../standby';

/**
 * The credential-free snapshot the Hot Sheet server leaves for the macOS screen saver (HS2-MZM988,
 * docs/34-glance-surfaces.md §34.12). The saver host is sandboxed and holds no server secret, so it reads
 * this one file; the page derives the same standby display the app shows from the same live state list.
 */
export interface StandbySnapshot {
  version: 1;
  /** When the server wrote it, Unix milliseconds. */
  written_at_ms: number;
  /**
   * Until when the server vouches for it. A running server rewrites the file before this passes; once it
   * does, the server stopped or lost the change stream and the freshness line says so.
   */
  valid_until_ms: number;
  /** The same list `GET /live-state` returns. */
  live: LiveStateList;
}

export const parseStandbySnapshot: ResponseParser<StandbySnapshot> = (value) => {
  const what = 'standby snapshot';
  if (
    !isRecord(value) ||
    value.version !== 1 ||
    typeof value.written_at_ms !== 'number' ||
    typeof value.valid_until_ms !== 'number'
  )
    throw new ServerResponseShapeError(what);
  return {
    version: 1,
    written_at_ms: value.written_at_ms,
    valid_until_ms: value.valid_until_ms,
    live: parseLiveStateList(value.live),
  };
};

/** Everything `StandbyScreen` needs at `nowMs`; with no snapshot yet it shows the empty, not-live display. */
export interface SaverView {
  clock: string;
  model: StandbyModel;
  freshness: { live: boolean; label: string };
}

export function saverView(snapshot: StandbySnapshot | undefined, nowMs: number, locale?: string): SaverView {
  const freshness: StandbyFreshness = {
    live: snapshot !== undefined && nowMs < snapshot.valid_until_ms,
    ...(snapshot ? { updatedAtMs: snapshot.written_at_ms } : {}),
  };
  return {
    clock: formatStandbyClock(nowMs, locale),
    model: standbyModel(snapshot?.live.stores ?? [], nowMs),
    freshness: { live: freshness.live, label: standbyFreshnessLabel(freshness, locale) },
  };
}
