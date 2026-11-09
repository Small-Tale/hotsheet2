/** Same-machine remote-project workflow. All credential material stays in this Node process or
 * the OS private secret store; browser routes return only reviewed server/project metadata. */
import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { resolve } from 'node:path';

import { spawn } from '../child-process';
import { developmentRepositoryRoot } from '../repository-root';
import { remoteRequest, type RemoteTls } from './transport';

export interface RemoteServerView {
  id: string;
  label: string;
  origin: string;
  identity: { server_id: string; ca_fingerprint: string | null };
  revision: number;
  revoked: boolean;
  last_failure: string | null;
  last_confirmed_checkouts: RemoteCheckoutView[];
  stale: boolean;
}

export interface RemoteCheckoutView {
  id: string;
  name: string;
  state: 'available' | 'inaccessible' | 'identity_changed';
  sources: Array<{ connection_id: string; provider: string }>;
}

export interface RemoteDiscoveryView {
  remote: RemoteServerView;
  checkouts: RemoteCheckoutView[] | null;
}

export interface PrivateRemoteCredential {
  bearer_secret: string;
  session_origin?: string | null;
  session_csrf?: string | null;
  ca_pem?: string | null;
  client_certificate_pem?: string | null;
  client_key_pem?: string | null;
}

export interface PrivateRemoteConnection {
  binding: RemoteServerView;
  credentials: PrivateRemoteCredential;
}

export interface SelectedRemoteProject {
  binding_id: string;
  checkout_id: string;
}

interface PendingPair {
  origin: string;
  browserOrigin: string;
  label: string;
  id: string;
  expectedServerId: string;
  challenge: string;
  expiresAt: number;
  tls: RemoteTls;
  review?: RemoteServerView;
}

export interface RemoteTlsFiles {
  caFile?: string;
  certificateFile?: string;
  keyFile?: string;
}

const pendingPairs = new Map<string, PendingPair>();
const MAX_CLI_OUTPUT = 2 * 1024 * 1024;

function cliBinary(): string {
  return process.env.HOTSHEET_CLI_BIN || `${developmentRepositoryRoot()}/target/debug/hotsheet-cli`;
}

export function normalizeRemoteAddress(value: string): string {
  if (value !== value.trim() || /[\\\s]/.test(value))
    throw new Error('Enter an explicit HTTP or HTTPS server address.');
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error('Enter an explicit HTTP or HTTPS server address.');
  }
  if (
    !/^https?:\/\/(?:\[[^\]]+\]|[^/:?#]+):[0-9]{1,5}$/.test(value) ||
    !['http:', 'https:'].includes(parsed.protocol) ||
    !parsed.hostname ||
    parsed.username ||
    parsed.password ||
    parsed.pathname !== '/' ||
    parsed.search ||
    parsed.hash
  )
    throw new Error('Use an explicit protocol, host, and port without a path or credentials.');
  return parsed.origin;
}

async function readTlsFiles(files: RemoteTlsFiles): Promise<RemoteTls> {
  if (Boolean(files.certificateFile) !== Boolean(files.keyFile))
    throw new Error('A client certificate and key must be selected together.');
  const pem = async (path: string | undefined) => {
    if (!path) return undefined;
    const bytes = await readFile(path);
    if (bytes.length > 128 * 1024) throw new Error('Remote TLS file is too large.');
    const value = bytes.toString('utf8');
    if (!value.includes('-----BEGIN ')) throw new Error('Remote TLS file is not PEM.');
    return value;
  };
  return {
    ca_pem: await pem(files.caFile),
    client_certificate_pem: await pem(files.certificateFile),
    client_key_pem: await pem(files.keyFile),
  };
}

/** CLI output is captured, bounded, and never written to a log. The only command returning a
 * credential is `bridge-connection`, which callers must keep in Node memory. */
export function privateRemoteCli<T>(args: string[], input?: unknown): Promise<T> {
  return new Promise((resolve, reject) => {
    const child = spawn(cliBinary(), ['remote', ...args], { stdio: ['pipe', 'pipe', 'pipe'] });
    const chunks: Buffer[] = [];
    let size = 0;
    child.stdout.on('data', (chunk: Buffer) => {
      size += chunk.length;
      if (size > MAX_CLI_OUTPUT) child.kill();
      else chunks.push(chunk);
    });
    // CLI errors can contain private input paths or peer details. Keep browser errors generic.
    child.stderr.resume();
    child.once('error', () => {
      reject(new Error('Remote registry CLI is unavailable.'));
    });
    child.once('close', (code) => {
      if (size > MAX_CLI_OUTPUT) {
        reject(new Error('Remote registry reply exceeded its limit.'));
        return;
      }
      if (code !== 0) {
        reject(new Error('Remote registry request failed. Check the server identity and pairing.'));
        return;
      }
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')) as T);
      } catch {
        reject(new Error('Remote registry returned an invalid response.'));
      }
    });
    child.stdin.end(input === undefined ? undefined : JSON.stringify(input));
  });
}

export const listRemoteServers = () => privateRemoteCli<RemoteServerView[]>(['list']);
export const selectedRemoteProjects = () => privateRemoteCli<SelectedRemoteProject[]>(['selected-projects']);
export const discoverRemoteProjects = (id: string) => privateRemoteCli<RemoteDiscoveryView>(['checkouts', id]);
export const privateRemoteConnection = (id: string) =>
  privateRemoteCli<PrivateRemoteConnection>(['bridge-connection', id]);
export const selectRemoteProject = (id: string, checkout: string) =>
  privateRemoteCli<{ selected: boolean }>(['select-project', id, checkout]);
export const unselectRemoteProject = (id: string, checkout: string) =>
  privateRemoteCli<{ selected: boolean }>(['unselect-project', id, checkout]);
export const removeRemoteServer = (id: string, revision: number) =>
  privateRemoteCli<RemoteServerView>(['revoke', id, '--expected-revision', String(revision)]);

/** An opened tab must stop forwarding immediately when this machine revokes or reviews the
 * binding. The registry contains no credential; the secret remains in the private store. */
export async function requireCurrentRemoteBinding(connection: PrivateRemoteConnection): Promise<void> {
  const home = process.env.HOTSHEET_HOME || resolve(homedir(), '.hotsheet2');
  const file = JSON.parse(await readFile(resolve(home, 'remotes.json'), 'utf8')) as {
    servers?: Array<{
      id: string;
      revision: number;
      revoked: boolean;
      origin: string;
      identity: { server_id: string };
    }>;
  };
  const current = file.servers?.find((item) => item.id === connection.binding.id);
  if (
    !current ||
    current.revoked ||
    current.revision !== connection.binding.revision ||
    current.origin !== connection.binding.origin ||
    current.identity.server_id !== connection.binding.identity.server_id
  )
    throw new Error('Remote server approval changed. Review and reconnect this server.');
}

async function remoteJson<T>(origin: string, path: string, options: RequestInit = {}, tls: RemoteTls = {}): Promise<T> {
  const response = await remoteRequest(
    new URL(path, origin),
    {
      ...options,
      redirect: 'manual',
      signal: AbortSignal.timeout(10_000),
    },
    tls,
  );
  if (!response.ok) throw new Error(`Remote server returned ${response.status}; check its address and approval.`);
  const size = Number(response.headers.get('content-length'));
  if (Number.isFinite(size) && size > MAX_CLI_OUTPUT) throw new Error('Remote server reply exceeded its limit.');
  const body = await response.text();
  if (body.length > MAX_CLI_OUTPUT) throw new Error('Remote server reply exceeded its limit.');
  return JSON.parse(body) as T;
}

export async function beginRemotePairing(
  address: string,
  label: string,
  browserOrigin: string,
  tlsFiles: RemoteTlsFiles = {},
  reviewId?: string,
) {
  const origin = normalizeRemoteAddress(address);
  const review = reviewId
    ? (await listRemoteServers()).find((server) => server.id === reviewId && !server.revoked)
    : undefined;
  if (reviewId && !review) throw new Error('The server binding is no longer approved.');
  const tls = await readTlsFiles(tlsFiles);
  if (origin.startsWith('https:') && !tls.ca_pem)
    throw new Error('Select the host CA certificate before pairing over HTTPS.');
  const identity = await remoteJson<{ generation: string; protocol: number; server_id: string }>(
    origin,
    '/remote-identity',
    {},
    tls,
  );
  if (identity.generation !== 'hs2' || identity.protocol !== 1 || !/^[0-9A-HJKMNP-TV-Z]{26}$/.test(identity.server_id))
    throw new Error('This server is not a compatible Hot Sheet 2 peer.');
  const pairing = await remoteJson<{ id: string; challenge: string; server_id: string; expires_at: number }>(
    origin,
    '/remote-auth/pair',
    {
      method: 'POST',
      headers: { origin: browserOrigin, 'content-type': 'application/json' },
      body: JSON.stringify({ label }),
    },
    tls,
  );
  if (pairing.server_id !== identity.server_id || !pairing.id || !pairing.challenge)
    throw new Error('The remote server identity changed during pairing.');
  const handle = randomUUID();
  pendingPairs.set(handle, {
    origin,
    browserOrigin,
    label,
    id: pairing.id,
    expectedServerId: identity.server_id,
    challenge: pairing.challenge,
    expiresAt: pairing.expires_at,
    tls,
    review,
  });
  return { handle, approvalId: pairing.id, serverId: identity.server_id, origin, label };
}

export async function finishRemotePairing(handle: string) {
  const pending = pendingPairs.get(handle);
  if (!pending || pending.expiresAt * 1_000 <= Date.now()) throw new Error('Pairing expired. Start again.');
  const response = await remoteRequest(
    new URL('/remote-auth/redeem', pending.origin),
    {
      method: 'POST',
      headers: { origin: pending.browserOrigin, 'content-type': 'application/json' },
      body: JSON.stringify({ id: pending.id, challenge: pending.challenge }),
      redirect: 'manual',
      signal: AbortSignal.timeout(10_000),
    },
    pending.tls,
  );
  if (!response.ok) throw new Error('Approval is pending or expired on the host Mac.');
  const issued = (await response.json()) as { server_id: string };
  if (issued.server_id !== pending.expectedServerId) throw new Error('The remote server identity changed.');
  const cookies = response.headers.getSetCookie();
  const session = cookies
    .find((cookie) => cookie.startsWith('hs2_remote_session='))
    ?.split(';')[0]
    ?.split('=')[1];
  const csrf = cookies
    .find((cookie) => cookie.startsWith('hs2_remote_csrf='))
    ?.split(';')[0]
    ?.split('=')[1];
  if (!session || !csrf) throw new Error('The remote server did not issue a complete session.');
  pendingPairs.delete(handle);
  const args = pending.review
    ? [
        'review',
        pending.review.id,
        '--expected-revision',
        String(pending.review.revision),
        '--previous-server-id',
        pending.review.identity.server_id,
        ...(pending.review.identity.ca_fingerprint
          ? ['--previous-ca-fingerprint', pending.review.identity.ca_fingerprint]
          : []),
        '--label',
        pending.label,
        '--origin',
        pending.origin,
        '--expected-server-id',
        pending.expectedServerId,
        '--credentials-file',
        '-',
      ]
    : [
        'register',
        '--label',
        pending.label,
        '--origin',
        pending.origin,
        '--expected-server-id',
        pending.expectedServerId,
        '--credentials-file',
        '-',
      ];
  const result = await privateRemoteCli<RemoteDiscoveryView>(args, {
    bearer_secret: `session:${session}`,
    session_origin: pending.browserOrigin,
    session_csrf: csrf,
    ...pending.tls,
  });
  return { discovery: result, revisedId: pending.review?.id };
}
