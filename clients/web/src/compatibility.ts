export interface CompatibilityRange { min: number; max: number }
export interface ServerCompatibility {
  generation: string;
  application_version?: string;
  build_revision?: string | null;
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
  canRestartServer: boolean;
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
  const unknown = (detail: string): CompatibilityAssessment => ({ kind: 'unknown', detail, revisionMismatch: false, canRestartServer: false });
  if (!server) return unknown('The server did not provide compatibility metadata.');
  if (server.generation !== 'hs2') return unknown(`Expected an HS2 server, received ${server.generation || 'an unknown generation'}.`);
  if (!validRange(server.protocol) || !validRange(client)) return unknown('The server or client reported an invalid protocol range.');
  const revisionMismatch = Boolean(clientRevision && server.build_revision && clientRevision !== server.build_revision);
  if (server.protocol.max < client.min) return {
    kind: 'server_too_old',
    detail: `Server protocol ${server.protocol.min}–${server.protocol.max} is older than client protocol ${client.min}–${client.max}.`,
    revisionMismatch,
    canRestartServer: server.capabilities?.lifecycle_restart === true && server.capabilities.lifecycle_quiescence === true,
  };
  if (client.max < server.protocol.min) return {
    kind: 'client_too_old',
    detail: `Client protocol ${client.min}–${client.max} is older than server protocol ${server.protocol.min}–${server.protocol.max}.`,
    revisionMismatch,
    canRestartServer: false,
  };
  return { kind: 'compatible', revisionMismatch, canRestartServer: false };
}
