import type { Checkout } from './api';
import type {
  ConversationExportDestination,
  ConversationExportManifest,
  ConversationExportOpenResult,
  ConversationExportWriteResult,
} from './conversation-export';
import type { Project, UnhealthyServerRecovery } from './interactions/types';
import { isRecord } from './json-value';

/**
 * Typed adapter for the same-origin `/__hotsheet/*` local bridge (HS2-313JET).
 *
 * Every call reads the body as text, parses it as `unknown`, surfaces a server `error`
 * string (or the caller's fallback) for a non-2xx status, and validates a 2xx body with
 * an explicit response parser before callers see it. A body that is not JSON, or JSON in
 * an unexpected shape, becomes a {@link LocalBridgeResponseError} instead of an unchecked
 * cast that fails later at a property access.
 */

/** A 2xx or error response whose body did not have the documented shape. */
export class LocalBridgeResponseError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = 'LocalBridgeResponseError';
  }
}

/** A non-2xx response; `message` is the server's `error` string or the caller's fallback. */
export class LocalBridgeHttpError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly body: unknown,
  ) {
    super(message);
    this.name = 'LocalBridgeHttpError';
  }
}

type Parser<T> = (value: unknown) => T;

export interface LocalBridgeRequest<T> {
  readonly method?: 'GET' | 'POST' | 'DELETE';
  readonly body?: unknown;
  readonly parse: Parser<T>;
  /** Message used when a failed response carries no `error` string. */
  readonly fallbackError: string;
  readonly request?: typeof fetch;
  /** Names the responder in an invalid-JSON message, e.g. "folder chooser". */
  readonly responder?: string;
}

export { isRecord };

function invalid(what: string): never {
  throw new LocalBridgeResponseError(`The local bridge returned an invalid ${what} response.`, 200);
}

function optionalString(value: Record<string, unknown>, key: string, what: string) {
  const field = value[key];
  if (field !== undefined && field !== null && typeof field !== 'string') invalid(what);
  return typeof field === 'string' ? field : undefined;
}

/** Error text from any failed bridge body, or undefined. */
export function bridgeErrorMessage(body: unknown): string | undefined {
  return isRecord(body) && typeof body.error === 'string' && body.error ? body.error : undefined;
}

export async function localBridgeRequest<T>(path: string, options: LocalBridgeRequest<T>): Promise<T> {
  const request = options.request ?? fetch,
    init: RequestInit = { method: options.method ?? 'GET' };
  if (options.body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(options.body);
  }
  const response = await request(path, init),
    text = await response.text();
  let body: unknown;
  try {
    body = text ? (JSON.parse(text) as unknown) : {};
  } catch {
    throw new LocalBridgeResponseError(
      `The ${options.responder ?? 'local bridge'} returned an invalid response (${response.status}).`,
      response.status,
    );
  }
  if (!response.ok)
    throw new LocalBridgeHttpError(bridgeErrorMessage(body) ?? options.fallbackError, response.status, body);
  try {
    return options.parse(body);
  } catch (reason) {
    if (reason instanceof LocalBridgeResponseError) throw new LocalBridgeResponseError(reason.message, response.status);
    throw reason;
  }
}

// ---- Response parsers ----

export function parseFolderChoice(value: unknown): { path?: string } {
  if (!isRecord(value)) invalid('folder chooser');
  return { path: optionalString(value, 'path', 'folder chooser') || undefined };
}

function isCheckout(value: unknown): value is Checkout {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.root === 'string' &&
    typeof value.alias === 'string' &&
    Array.isArray(value.stores) &&
    value.stores.every((store) => typeof store === 'string')
  );
}

export function parseCheckouts(value: unknown): Checkout[] {
  if (!Array.isArray(value) || !value.every(isCheckout)) invalid('checkout list');
  return value;
}

export function parseOpenedProject(value: unknown): Project {
  if (
    !isRecord(value) ||
    typeof value.id !== 'string' ||
    typeof value.root !== 'string' ||
    typeof value.name !== 'string' ||
    typeof value.apiPath !== 'string' ||
    !Array.isArray(value.stores)
  )
    invalid('project');
  return value as unknown as Project;
}

/** The unhealthy-server recovery identity a failed project open may carry (HS2-3DA0FQ). */
export function parseUnhealthyServerRecovery(value: unknown): UnhealthyServerRecovery | undefined {
  if (!isRecord(value) || typeof value.store !== 'string' || !isRecord(value.expected)) return undefined;
  const { pid, url, started_at: startedAt } = value.expected;
  if (typeof pid !== 'number' || typeof url !== 'string' || typeof startedAt !== 'string') return undefined;
  return { store: value.store, expected: { pid, url, started_at: startedAt } };
}

export function parseRecoveryResult(value: unknown): { recovered: boolean; error?: string } {
  if (!isRecord(value)) invalid('server recovery');
  return { recovered: value.recovered === true, error: bridgeErrorMessage(value) };
}

export function parseRemovedHs1Data(value: unknown): { removed: string[] } {
  if (!isRecord(value)) invalid('Hot Sheet 1 cleanup');
  const removed = value.removed ?? [];
  if (!Array.isArray(removed) || !removed.every((item) => typeof item === 'string')) invalid('Hot Sheet 1 cleanup');
  return { removed };
}

export function parseGitSetup(value: unknown): { ticketStore?: string; connectionId?: string; error?: string } {
  if (!isRecord(value)) invalid('git setup');
  return {
    error: bridgeErrorMessage(value),
    ticketStore: optionalString(value, 'ticketStore', 'git setup') || undefined,
    connectionId: optionalString(value, 'connectionId', 'git setup') || undefined,
  };
}

export function parseGitRemoteSetup(value: unknown): { connected: boolean; error?: string } {
  if (!isRecord(value)) invalid('git remote setup');
  return { connected: value.connected === true, error: bridgeErrorMessage(value) };
}

function isManifest(value: unknown): value is ConversationExportManifest {
  return (
    isRecord(value) &&
    value.format === 'hotsheet-conversation-export' &&
    typeof value.exportId === 'string' &&
    typeof value.revision === 'number' &&
    isRecord(value.source) &&
    isRecord(value.reopen) &&
    Array.isArray(value.selectedMessageIds) &&
    Array.isArray(value.entries) &&
    Array.isArray(value.assets)
  );
}

function isDestination(value: unknown): value is ConversationExportDestination {
  return (
    isRecord(value) &&
    typeof value.selectionToken === 'string' &&
    typeof value.displayPath === 'string' &&
    (value.kind === 'directory' || value.kind === 'archive') &&
    (value.existing === undefined || (isRecord(value.existing) && typeof value.existing.revision === 'number'))
  );
}

export function parseConversationExportDestination(value: unknown): { destination?: ConversationExportDestination } {
  if (!isRecord(value)) invalid('conversation destination');
  if (value.destination === undefined || value.destination === null) return {};
  if (!isDestination(value.destination)) invalid('conversation destination');
  return { destination: value.destination };
}

export function parseConversationExportWrite(value: unknown): ConversationExportWriteResult {
  if (!isRecord(value) || typeof value.displayPath !== 'string' || !isManifest(value.manifest))
    invalid('conversation save');
  return { displayPath: value.displayPath, manifest: value.manifest };
}

export function parseConversationExportOpen(value: unknown): { conversation?: ConversationExportOpenResult } {
  if (!isRecord(value)) invalid('saved conversation');
  const conversation = value.conversation;
  if (conversation === undefined || conversation === null) return {};
  if (
    !isRecord(conversation) ||
    typeof conversation.displayPath !== 'string' ||
    !isManifest(conversation.manifest) ||
    !Array.isArray(conversation.messages) ||
    !Array.isArray(conversation.activity)
  )
    invalid('saved conversation');
  return { conversation: conversation as unknown as ConversationExportOpenResult };
}

// ---- Endpoints ----

export const localBridge = {
  chooseFolder: (fallbackError = 'Could not open the folder chooser.', request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/folders/choose', {
      method: 'POST',
      parse: parseFolderChoice,
      fallbackError,
      request,
      responder: 'folder chooser',
    }),
  checkouts: (fallbackError = 'Could not find registered Hot Sheet projects.', request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/checkouts', { parse: parseCheckouts, fallbackError, request }),
  openProject: (
    root: string,
    fallbackError = 'Could not open project.',
    request?: typeof fetch,
    ticketStore?: string,
  ) =>
    localBridgeRequest('/__hotsheet/projects/open', {
      method: 'POST',
      body: { root, ticketStore },
      parse: parseOpenedProject,
      fallbackError,
      request,
    }),
  recoverUnhealthyServer: (recovery: unknown, request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/server/recover-unhealthy', {
      method: 'POST',
      body: recovery,
      parse: parseRecoveryResult,
      fallbackError: 'Could not recover the unresponsive server.',
      request,
    }),
  removeHs1Data: (projectId: string, request?: typeof fetch) =>
    localBridgeRequest(`/__hotsheet/projects/${encodeURIComponent(projectId)}/hs1-data`, {
      method: 'DELETE',
      parse: parseRemovedHs1Data,
      fallbackError: 'Could not remove the old Hot Sheet 1 files.',
      request,
    }),
  setupGit: (root: string, location: string | undefined, request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/projects/setup-git', {
      method: 'POST',
      body: { root, location },
      parse: parseGitSetup,
      fallbackError: 'Could not create the git ticket store.',
      request,
    }),
  setupGitRemote: (store: string, remote: string, request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/projects/setup-git-remote', {
      method: 'POST',
      body: { store, remote },
      parse: parseGitRemoteSetup,
      fallbackError: 'Could not connect the Git remote.',
      request,
    }),
  chooseConversationExportDestination: (suggestedName: string, request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/conversation-exports/destination', {
      method: 'POST',
      body: { suggestedName },
      parse: parseConversationExportDestination,
      fallbackError: 'Could not choose a conversation export destination.',
      request,
    }),
  writeConversationExport: (payload: unknown, request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/conversation-exports/write', {
      method: 'POST',
      body: payload,
      parse: parseConversationExportWrite,
      fallbackError: 'Could not save the conversation.',
      request,
    }),
  openConversationExport: (request?: typeof fetch) =>
    localBridgeRequest('/__hotsheet/conversation-exports/open', {
      method: 'POST',
      parse: parseConversationExportOpen,
      fallbackError: 'Could not open the saved conversation.',
      request,
    }),
};
