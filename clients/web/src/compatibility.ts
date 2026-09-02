export interface CompatibilityRange { min: number; max: number }
export interface ServerCompatibility {
  generation: string;
  application_version?: string;
  build_revision?: string | null;
  source_revision?: string | null;
  source_stale?: boolean;
  protocol?: CompatibilityRange;
  store_schema?: CompatibilityRange;
  capabilities?: { lifecycle_restart?: boolean; lifecycle_quiescence?: boolean };
  started_at?: string | null;
}

export type CompatibilityKind = 'compatible' | 'client_too_old' | 'server_too_old' | 'unknown';
export interface CompatibilityAssessment {
  kind: CompatibilityKind;
  detail?: string;
  revisionMismatch: boolean;
  sourceStale: boolean;
  canRestartServer: boolean;
  server?: ServerCompatibility;
  clientProtocol?: CompatibilityRange;
  clientRevision?: string;
}

export const WEB_PROTOCOL_RANGE: CompatibilityRange = { min: 1, max: 1 };

function validRange(value: CompatibilityRange | undefined): value is CompatibilityRange {
  return Boolean(value && Number.isInteger(value.min) && Number.isInteger(value.max) && value.min >= 0 && value.min <= value.max);
}

/** Evaluate hard compatibility by protocol-range intersection, never exact build equality. */
export function assessCompatibility(
  server: ServerCompatibility | undefined,
  client: CompatibilityRange = WEB_PROTOCOL_RANGE,
  clientRevision?: string,
): CompatibilityAssessment {
  const context = { server, clientProtocol: client, clientRevision };
  const unknown = (detail: string): CompatibilityAssessment => ({ kind: 'unknown', detail, revisionMismatch: false, sourceStale: false, canRestartServer: false, ...context });
  if (!server) return unknown('The server did not provide compatibility metadata.');
  if (server.generation !== 'hs2') return unknown(`Expected an HS2 server, received ${server.generation || 'an unknown generation'}.`);
  if (!validRange(server.protocol) || !validRange(client)) return unknown('The server or client reported an invalid protocol range.');
  const sourceStale = server.source_stale === true || Boolean(server.build_revision && server.source_revision && server.build_revision !== server.source_revision);
  const revisionMismatch = sourceStale || Boolean(clientRevision && server.build_revision && clientRevision !== server.build_revision);
  if (server.protocol.max < client.min) return {
    kind: 'server_too_old',
    detail: `Server protocol ${server.protocol.min}–${server.protocol.max} is older than client protocol ${client.min}–${client.max}.`,
    revisionMismatch,
    sourceStale,
    canRestartServer: server.capabilities?.lifecycle_restart === true && server.capabilities.lifecycle_quiescence === true,
    ...context,
  };
  if (client.max < server.protocol.min) return {
    kind: 'client_too_old',
    detail: `Client protocol ${client.min}–${client.max} is older than server protocol ${server.protocol.min}–${server.protocol.max}.`,
    revisionMismatch,
    sourceStale,
    canRestartServer: false,
    ...context,
  };
  return { kind: 'compatible', revisionMismatch, sourceStale, canRestartServer: false, ...context };
}
