import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, readFile, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, resolve } from 'node:path';

import { assessCompatibility, type CompatibilityAssessment, type ServerCompatibility } from './compatibility';

export interface ProjectSession {
  id: string;
  root: string;
  name: string;
  stores: string[];
  apiPath: string;
  compatibility: CompatibilityAssessment;
}

interface InstanceInfo { pid:number; url:string; secret:string }
interface SessionTarget { url:string; secret:string }
interface CorruptDiagnostic { path:string }

export type RevealLauncher = (command: string, args: string[]) => Promise<void>;

const sessions = new Map<string, SessionTarget>();

export function revealCommand(path: string, hostPlatform = process.platform): {command:string;args:string[]} {
  if (hostPlatform === 'darwin') return { command: 'open', args: ['-R', path] };
  if (hostPlatform === 'win32') return { command: 'explorer.exe', args: [`/select,${path}`] };
  return { command: 'xdg-open', args: [dirname(path)] };
}

export function requireReportedCorruptPath(diagnostics: CorruptDiagnostic[], path: string): void {
  if (!diagnostics.some(item => item.path === path)) throw new Error('The corrupt ticket is no longer present in this checkout.');
}

const launchReveal: RevealLauncher = (command, args) => new Promise((resolveLaunch, reject) => {
  const child = spawn(command, args, { detached: true, stdio: 'ignore' });
  child.once('error', reject);
  child.once('spawn', () => { child.unref(); resolveLaunch(); });
});

/** Refuse API use across a negotiated hard boundary. Unknown metadata stays usable
 * for pre-handshake development servers, but an explicit skew result is authoritative. */
export function requireCompatibleServer(assessment: CompatibilityAssessment): void {
  if (assessment.kind === 'client_too_old') {
    throw new Error(`Hot Sheet 2 update required. This project cannot be opened because its server requires a newer HS2 client. ${assessment.detail ?? ''}`.trim());
  }
  if (assessment.kind === 'server_too_old') {
    throw new Error(`Hot Sheet 2 server update required. This client cannot open the project through the older server. ${assessment.detail ?? ''}`.trim());
  }
}

export function developmentRepositoryRoot(cwd = process.cwd(), environment = process.env) {
  return resolve(environment.HOTSHEET_REPO_ROOT ?? resolve(cwd, '../..'));
}

function hotsheetHome() {
  return process.env.HOTSHEET_HOME || resolve(homedir(), '.hotsheet2');
}

async function exists(path: string) {
  try { await access(path); return true; } catch { return false; }
}

export async function suggestedTicketStore(root: string): Promise<string | undefined> {
  const canonical = await realpath(root);
  const candidate = `${canonical}.hs2`;
  return await exists(resolve(candidate, 'hotsheet-store.json')) ? candidate : undefined;
}

async function instanceFor(store: string): Promise<InstanceInfo | undefined> {
  const canonical = await realpath(store);
  const id = createHash('sha256').update(canonical).digest('hex').slice(0, 16);
  try {
    const info = JSON.parse(await readFile(resolve(hotsheetHome(), 'instances', `${id}.json`), 'utf8')) as InstanceInfo;
    process.kill(info.pid, 0);
    return info;
  } catch { return undefined; }
}

async function ensureServer(store: string): Promise<InstanceInfo> {
  const existing = await instanceFor(store);
  if (existing) return existing;
  const repoRoot = developmentRepositoryRoot();
  const binary = process.env.HOTSHEET_SERVER_BIN || resolve(repoRoot, 'target/debug/hotsheet-server');
  if (!await exists(binary)) throw new Error(`Hot Sheet server is not built at ${binary}. Run cargo build -p hotsheet-server.`);
  const child = spawn(binary, ['-C', store, '--bind', '127.0.0.1:0'], { cwd: repoRoot, detached: true, stdio: 'ignore' });
  child.unref();
  for (let attempt = 0; attempt < 80; attempt += 1) {
    await new Promise(resolveWait => setTimeout(resolveWait, 100));
    const info = await instanceFor(store);
    if (info) return info;
  }
  throw new Error(`Timed out starting the Hot Sheet server for ${store}.`);
}

async function serverRequest<T>(target: SessionTarget, path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`${target.url}${path}`, { ...init, headers: { 'content-type': 'application/json', 'x-hotsheet-secret': target.secret, ...init.headers } });
  if (!response.ok) throw new Error((await response.json().catch(() => null) as {error?:string}|null)?.error || `${response.status}`);
  return response.json() as Promise<T>;
}

export async function openLocalProject(rootInput: string, ticketStoreInput?: string): Promise<ProjectSession> {
  const root = await realpath(rootInput.trim());
  const ticketStore = ticketStoreInput?.trim() ? await realpath(ticketStoreInput.trim()) : await suggestedTicketStore(root);
  if (!ticketStore) throw new Error(`No ticket source was found at ${root}.hs2. Choose a git ticket store to continue.`);
  const instance = await ensureServer(ticketStore);
  const target = { url: instance.url, secret: instance.secret };
  const metadata = await serverRequest<ServerCompatibility>(target, '/compatibility').catch(() => undefined);
  const compatibility = assessCompatibility(metadata, undefined, process.env.HOT_SHEET_BUILD_REVISION);
  requireCompatibleServer(compatibility);
  const opened = await serverRequest<{checkout:{id:string;root:string;alias:string;stores:string[]}}>(target, '/projects/open', {
    method: 'POST',
    body: JSON.stringify({ root, ...(ticketStoreInput?.trim() ? { stores: [ticketStore] } : {}) }),
  });
  sessions.set(opened.checkout.id, target);
  return { id: opened.checkout.id, root: opened.checkout.root, name: opened.checkout.alias, stores: opened.checkout.stores, apiPath: `/__hotsheet/project-api/${encodeURIComponent(opened.checkout.id)}`, compatibility };
}

export async function proxyProjectRequest(projectId: string, path: string, request: Request): Promise<Response> {
  const target = sessions.get(projectId);
  if (!target) return Response.json({ error: 'Project session is not open.' }, { status: 404 });
  const headers = new Headers(request.headers);
  headers.set('x-hotsheet-secret', target.secret);
  headers.delete('host');
  const body = request.method === 'GET' || request.method === 'HEAD' ? undefined : await request.arrayBuffer();
  const response = await fetch(authenticatedServerUrl(target.url, path, target.secret), { method: request.method, headers, body, redirect: 'manual' });
  return new Response(response.body, { status: response.status, headers: response.headers });
}

/** Reveal only a path the authenticated server still reports as corrupt for this checkout. */
export async function revealCorruptTicket(projectId: string, path: string, launch: RevealLauncher = launchReveal): Promise<void> {
  const target = sessions.get(projectId);
  if (!target) throw new Error('Project session is not open.');
  const diagnostics = await serverRequest<CorruptDiagnostic[]>(target, `/checkouts/${encodeURIComponent(projectId)}/corrupt-tickets`);
  requireReportedCorruptPath(diagnostics, path);
  const command = revealCommand(path);
  await launch(command.command, command.args);
}

/** Build the loopback-only upstream URL, retaining poll auth for older HS2 servers. */
export function authenticatedServerUrl(origin: string, path: string, secret: string): string {
  const url = new URL(path, origin);
  // Polling originally predated standard API header auth. Keep the legacy query form on
  // this server-side hop so a new client bridge can long-poll an older running server.
  // The browser-facing bridge URL never contains this secret.
  if (url.pathname === '/ws/poll' && !url.searchParams.has('secret')) url.searchParams.set('secret', secret);
  return url.toString();
}
