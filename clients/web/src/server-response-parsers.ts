import type { Checkout, CheckoutSource } from './api';
import type { CompatibilityRange, ServerCompatibility } from './compatibility';
import { isRecord } from './json-value';

/**
 * Response parsers for successful Hot Sheet server bodies (HS2-34P6XY). `Api` and the
 * project bridge's `serverRequest` read a 2xx body as `unknown` and pass it through one of
 * these before callers see it, so a body in an unexpected shape fails at the boundary with
 * a {@link ServerResponseShapeError} instead of at a later property access. Parsers check
 * every field callers rely on and keep the rest of the object as the server sent it.
 */

export type ResponseParser<T> = (value: unknown) => T;

/** A 2xx body that did not have the documented shape for its endpoint. */
export class ServerResponseShapeError extends Error {
  constructor(readonly what: string) {
    super(`The server returned an invalid ${what} response.`);
    this.name = 'ServerResponseShapeError';
  }
}

function invalid(what: string): never {
  throw new ServerResponseShapeError(what);
}

function record(value: unknown, what: string): Record<string, unknown> {
  return isRecord(value) ? value : invalid(what);
}

function requiredString(value: Record<string, unknown>, key: string, what: string): string {
  const field = value[key];
  return typeof field === 'string' ? field : invalid(what);
}

function optionalOf(
  value: Record<string, unknown>,
  key: string,
  what: string,
  check: (field: unknown) => boolean,
  nullable = false,
) {
  const field = value[key];
  if (field === undefined || (nullable && field === null)) return;
  if (!check(field)) invalid(what);
}

const isString = (field: unknown) => typeof field === 'string';
const isBoolean = (field: unknown) => typeof field === 'boolean';
const isStringArray = (field: unknown) => Array.isArray(field) && field.every(isString);

function list<T>(value: unknown, what: string, item: (entry: unknown) => T): T[] {
  return Array.isArray(value) ? value.map(item) : invalid(what);
}

/** A body the caller ignores (`DELETE`, `POST` acknowledgements): any value is accepted. */
export const ignoreBody: ResponseParser<undefined> = () => undefined;

export const parseStringList: ResponseParser<string[]> = (value) =>
  isStringArray(value) ? value : invalid('string list');

function isRange(field: unknown): field is CompatibilityRange {
  return isRecord(field) && typeof field.min === 'number' && typeof field.max === 'number';
}

/** `GET /compatibility`. */
export const parseServerCompatibility: ResponseParser<ServerCompatibility> = (value) => {
  const what = 'compatibility',
    body = record(value, what);
  requiredString(body, 'generation', what);
  optionalOf(body, 'application_version', what, isString);
  optionalOf(body, 'build_revision', what, isString, true);
  optionalOf(body, 'source_revision', what, isString, true);
  optionalOf(body, 'source_stale', what, isBoolean);
  optionalOf(body, 'protocol', what, isRange);
  optionalOf(body, 'store_schema', what, isRange);
  optionalOf(body, 'started_at', what, isString, true);
  optionalOf(
    body,
    'capabilities',
    what,
    (field) =>
      isRecord(field) &&
      [field.lifecycle_restart, field.lifecycle_quiescence].every((flag) => flag === undefined || isBoolean(flag)),
  );
  return body as unknown as ServerCompatibility;
};

function parseCheckoutSource(value: unknown): CheckoutSource {
  const what = 'checkout source',
    body = record(value, what);
  for (const key of ['connection_id', 'provider', 'locator']) requiredString(body, key, what);
  return body as unknown as CheckoutSource;
}

/** A checkout (`GET /checkouts` entries, source and default-source updates). */
export const parseCheckout: ResponseParser<Checkout> = (value) => {
  const what = 'checkout',
    body = record(value, what);
  for (const key of ['id', 'root', 'alias']) requiredString(body, key, what);
  if (!isStringArray(body.stores)) invalid(what);
  optionalOf(body, 'repository', what, isString);
  optionalOf(body, 'default_source', what, isString);
  optionalOf(
    body,
    'unverified_store_sources',
    what,
    (field) => isRecord(field) && Object.values(field).every(isString),
  );
  if (body.sources !== undefined) list(body.sources, what, parseCheckoutSource);
  return body as unknown as Checkout;
};

export const parseCheckouts: ResponseParser<Checkout[]> = (value) => list(value, 'checkout list', parseCheckout);

/** `POST /projects/open`: the opened checkout with its (possibly empty) ticket sources. */
export interface OpenedCheckout {
  checkout: { id: string; root: string; alias: string; stores: string[]; sources: unknown[] };
}
export const parseOpenedCheckout: ResponseParser<OpenedCheckout> = (value) => {
  const what = 'project open',
    checkout = record(record(value, what).checkout, what);
  for (const key of ['id', 'root', 'alias']) requiredString(checkout, key, what);
  if (!isStringArray(checkout.stores) || !Array.isArray(checkout.sources)) invalid(what);
  return value as OpenedCheckout;
};

/** `GET /checkouts/{id}/corrupt-tickets` reduced to the paths the bridge authorizes reveals for. */
export const parseCorruptTicketPaths: ResponseParser<Array<{ path: string }>> = (value) =>
  list(value, 'corrupt ticket list', (entry) => {
    const body = record(entry, 'corrupt ticket');
    requiredString(body, 'path', 'corrupt ticket');
    return body as { path: string };
  });

/** `POST /accounts/{id}/identity`. */
export const parseAccountIdentity: ResponseParser<{ identity: string }> = (value) => {
  const body = record(value, 'account identity');
  requiredString(body, 'identity', 'account identity');
  return body as { identity: string };
};

/** `POST /checkouts/{id}/trash/empty`. */
export const parseTrashPurge: ResponseParser<{ purged: number; tickets: string[] }> = (value) => {
  const body = record(value, 'trash purge');
  if (typeof body.purged !== 'number' || !isStringArray(body.tickets)) invalid('trash purge');
  return body as { purged: number; tickets: string[] };
};
