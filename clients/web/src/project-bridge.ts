import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, readdir, readFile, realpath, rm } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, resolve } from 'node:path';

import type { Checkout } from './api';
import { assessCompatibility, type CompatibilityAssessment, type ServerCompatibility } from './compatibility';
import {
  type BackupGit,
  backupGit,
  hasCompletedHs1Import,
  hasHs1Backup,
  invalidateHs1Backup,
  readImportCompletion,
  recordHs1Backup,
  requireHs1Backup,
} from './hs1-backup';

export interface ProjectSession {
  id: string;
  root: string;
  name: string;
  stores: string[];
  apiPath: string;
  compatibility: CompatibilityAssessment;
  needsTicketSetup: boolean;
  needsHs1Migration: boolean;
  hs1ImportCompleted: boolean;
  hs1CleanupEligible: boolean;
  hs1SourcePath?: string;
  hs1DatabasePath?: string;
  hs1PostgresVersion?: string;
}

export interface InstanceInfo {
  pid: number;
  url: string;
  secret: string;
  started_at?: string;
}
export interface UnhealthyServerRecovery {
  store: string;
  expected: { pid: number; url: string; started_at: string };
}
interface SessionTarget {
  url: string;
  secret: string;
  root?: string;
  serverStore?: string;
  ticketStore?: string;
}
interface CorruptDiagnostic {
  path: string;
}
interface CliCompatibility {
  generation: string;
  setup_assets_fingerprint?: string;
  store_schema: { min: number; max: number; creates: number };
  selected_store_schema?: number | null;
}

export type RevealLauncher = (command: string, args: string[]) => Promise<void>;
export type FolderChooserRunner = (command: string, args: string[]) => Promise<string | undefined>;
export type GitRunner = (command: string, args: string[]) => Promise<void>;

type ProjectBridgeProcess = typeof process & { __hotsheetProjectSessions?: Map<string, SessionTarget> };

/** Vite evaluates config plugins and its SSR dev entry in separate module graphs. Keep the
 * authenticated project-session registry on their shared process object so HTTP opens and
 * terminal WebSocket upgrades resolve the same session without exposing credentials. */
export function projectSessionRegistry(host: ProjectBridgeProcess = process): Map<string, SessionTarget> {
  return (host.__hotsheetProjectSessions ??= new Map<string, SessionTarget>());
}

const sessions = projectSessionRegistry();

export function revealCommand(path: string, hostPlatform = process.platform): { command: string; args: string[] } {
  if (hostPlatform === 'darwin') return { command: 'open', args: ['-R', path] };
  if (hostPlatform === 'win32') return { command: 'explorer.exe', args: [`/select,${path}`] };
  return { command: 'xdg-open', args: [dirname(path)] };
}

export function requireReportedCorruptPath(diagnostics: CorruptDiagnostic[], path: string): void {
  if (!diagnostics.some((item) => item.path === path))
    throw new Error('The corrupt ticket is no longer present in this checkout.');
}

const launchReveal: RevealLauncher = (command, args) =>
  new Promise((resolveLaunch, reject) => {
    const child = spawn(command, args, { detached: true, stdio: 'ignore' });
    child.once('error', reject);
    child.once('spawn', () => {
      child.unref();
      resolveLaunch();
    });
  });

export function folderChooserCommand(hostPlatform = process.platform): { command: string; args: string[] } {
  if (hostPlatform === 'darwin')
    return {
      command: 'osascript',
      args: ['-e', 'POSIX path of (choose folder with prompt "Choose a folder for Hot Sheet")'],
    };
  if (hostPlatform === 'win32')
    return {
      command: 'powershell.exe',
      args: [
        '-NoProfile',
        '-Command',
        "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }",
      ],
    };
  return { command: 'zenity', args: ['--file-selection', '--directory', '--title=Choose a folder for Hot Sheet'] };
}

const runFolderChooser: FolderChooserRunner = (command, args) =>
  new Promise((resolveChoice, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'ignore'] }),
      chunks: Buffer[] = [];
    child.stdout.on('data', (chunk: Buffer) => {
      chunks.push(chunk);
    });
    child.once('error', reject);
    child.once('close', (code) => {
      resolveChoice(code === 0 ? Buffer.concat(chunks).toString('utf8').trim() || undefined : undefined);
    });
  });

export async function chooseLocalFolder(
  runner: FolderChooserRunner = runFolderChooser,
  hostPlatform = process.platform,
): Promise<string | undefined> {
  const command = folderChooserCommand(hostPlatform);
  return runner(command.command, command.args);
}

/** Refuse API use across a negotiated hard boundary. Unknown metadata stays usable
 * for pre-handshake development servers, but an explicit skew result is authoritative. */
export function requireCompatibleServer(assessment: CompatibilityAssessment): void {
  if (assessment.kind === 'client_too_old') {
    throw new Error(
      `Hot Sheet 2 update required. This project cannot be opened because its server requires a newer HS2 client. ${assessment.detail ?? ''}`.trim(),
    );
  }
  if (assessment.kind === 'server_too_old') {
    throw new Error(
      `Hot Sheet 2 server update required. This client cannot open the project through the older server. ${assessment.detail ?? ''}`.trim(),
    );
  }
}

/** Refuse a store/bootstrap operation before it crosses a server format boundary. The
 * application version can remain identical during pre-release schema work, so negotiate
 * the actual store range instead of treating a version label as compatibility. */
export function requireStoreSchemaCompatibility(
  server: ServerCompatibility | undefined,
  cli: CliCompatibility,
  mode: 'open' | 'create',
): void {
  const schema = mode === 'create' ? cli.store_schema.creates : cli.selected_store_schema;
  if (schema === undefined || schema === null || !server?.store_schema || schema <= server.store_schema.max) return;
  const action =
    mode === 'create'
      ? 'No ticket repository was created.'
      : 'This ticket repository cannot be opened through that server.';
  throw new Error(
    `${action} This project is connected to an older Hot Sheet server build that supports ticket-store schema through ${server.store_schema.max}, while the current Hot Sheet CLI ${mode === 'create' ? 'creates' : 'found'} schema ${schema}. Finish any active work in this project, stop or restart its detached Hot Sheet server, then reopen the project.`,
  );
}

export function developmentRepositoryRoot(cwd = process.cwd(), environment = process.env) {
  return resolve(environment.HOTSHEET_REPO_ROOT ?? resolve(cwd, '../..'));
}

function hotsheetHome() {
  return process.env.HOTSHEET_HOME || resolve(homedir(), '.hotsheet2');
}

async function exists(path: string) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

function toolBinary() {
  return process.env.HOTSHEET_CLI_BIN || resolve(developmentRepositoryRoot(), 'target/debug/hotsheet-cli');
}

function migrateBinary() {
  return process.env.HOTSHEET_MIGRATE_BIN || resolve(developmentRepositoryRoot(), 'target/debug/hotsheet-migrate');
}

function migratorScript() {
  return process.env.HOTSHEET_MIGRATOR || resolve(developmentRepositoryRoot(), 'migrator/src/export.mjs');
}

const HS1_MARKER = '.hotsheet/db/PG_VERSION';
const HS1_CLEANUP_ENTRIES = new Set([
  '.db-content-marker.json',
  '.db-created-empty.json',
  'attachments',
  'auth-devices.json',
  'channel-port',
  'channel-ports.d',
  'codex-app-server.json',
  'db',
  'freeze.log',
  'hotsheet.lock',
  'mcp.log',
  'mcp.log.old',
  'open-tickets.md',
  'secret.json',
  'settings.json',
  'settings.local.json',
  'ticket-drafts',
  'worklist.md',
]);
export function hs1MigrationArgs(root: string, ticketStore: string, exporter: string): string[] {
  return [resolve(root, '.hotsheet'), '-C', ticketStore, '--migrator', exporter];
}
export function preserveHs1Entry(name: string): boolean {
  return !HS1_CLEANUP_ENTRIES.has(name);
}
export function isHs2SettingsValue(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    Number.isSafeInteger((value as Record<string, unknown>).$hotsheetSchema) &&
    Number((value as Record<string, unknown>).$hotsheetSchema) >= 1
  );
}
async function isHs2SettingsFile(path: string): Promise<boolean> {
  try {
    return isHs2SettingsValue(JSON.parse(await readFile(path, 'utf8')));
  } catch {
    return false;
  }
}

export interface Hs1MigrationResult {
  ticketStore: string;
  connectionId: string;
  tickets: number;
  attachments: number;
  toolsConfigured: boolean;
}
export type ProcessRunner = (command: string, args: string[], cwd: string) => Promise<string>;
const runProcess: ProcessRunner = (command, args, cwd) =>
  new Promise((resolveRun, reject) => {
    const child = spawn(command, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] }),
      stdout: Buffer[] = [],
      stderr: Buffer[] = [];
    child.stdout.on('data', (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on('data', (chunk: Buffer) => stderr.push(chunk));
    child.once('error', reject);
    child.once('close', (code) => {
      const output = Buffer.concat(stdout).toString('utf8'),
        error = Buffer.concat(stderr).toString('utf8').trim();
      if (code === 0) resolveRun(output);
      else reject(new Error(error || `${command} exited with status ${code ?? 'unknown'}.`));
    });
  });

async function cliCompatibility(store: string | undefined, runner: ProcessRunner): Promise<CliCompatibility> {
  const args = [...(store ? ['-C', store] : []), 'compatibility', '--json'];
  return JSON.parse(await runner(toolBinary(), args, developmentRepositoryRoot())) as CliCompatibility;
}

export async function developmentSetupAssetsFingerprint(repositoryRoot = developmentRepositoryRoot()): Promise<string> {
  const root = resolve(repositoryRoot, 'plugins'),
    hash = createHash('sha256');
  const byByteName = (a: { name: string }, b: { name: string }) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
  const directories = (await readdir(root, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .sort(byByteName);
  for (const directory of directories) {
    hash.update(directory.name);
    hash.update('\0');
    const path = resolve(root, directory.name),
      files = (await readdir(path, { withFileTypes: true })).filter((entry) => entry.isFile()).sort(byByteName);
    for (const file of files) {
      hash.update(file.name);
      hash.update('\0');
      hash.update(await readFile(resolve(path, file.name)));
      hash.update('\0');
    }
  }
  return hash.digest('hex');
}

export function requireCurrentSetupAssets(cli: CliCompatibility, sourceFingerprint: string): void {
  if (cli.setup_assets_fingerprint === sourceFingerprint) return;
  const detail = cli.setup_assets_fingerprint
    ? 'does not match the current setup templates'
    : 'does not report a setup-template fingerprint';
  throw new Error(
    `The development Hot Sheet CLI ${detail} and may overwrite newer project guidance. Run cargo build -p hotsheet-cli, then reopen the project. No setup files were changed.`,
  );
}

async function requireCurrentSetupCli(store: string | undefined, runner: ProcessRunner): Promise<CliCompatibility> {
  const [cli, sourceFingerprint] = await Promise.all([
    cliCompatibility(store, runner),
    developmentSetupAssetsFingerprint(),
  ]);
  requireCurrentSetupAssets(cli, sourceFingerprint);
  return cli;
}

function sessionForRoot(root: string): SessionTarget | undefined {
  return [...sessions.values()].find((target) => target.root === root);
}

/** Run current-app setup writers independently of the detached server's build, after a
 * source/compiled-template handshake proves that the CLI cannot restore stale guidance. */
export async function refreshLocalProjectSetup(
  root: string,
  store: string,
  runner: ProcessRunner = runProcess,
): Promise<void> {
  await requireCurrentSetupCli(store, runner);
  await runner(toolBinary(), ['-C', store, 'setup', '--refresh', '--project', root], developmentRepositoryRoot());
}

export async function migrateHs1Project(
  rootInput: string,
  locationInput?: string,
  runner: ProcessRunner = runProcess,
): Promise<Hs1MigrationResult> {
  const root = await realpath(rootInput.trim());
  if (!(await exists(resolve(root, HS1_MARKER))))
    throw new Error('Hot Sheet 1 data is no longer present in this project.');
  const ticketStore = locationInput?.trim() ? resolve(locationInput.trim()) : `${root}.hs2`,
    binary = migrateBinary(),
    exporter = migratorScript();
  if (!(await exists(binary)))
    throw new Error(
      `Hot Sheet migrator is not built at ${binary}. Run cargo build -p hotsheet-cli --bin hotsheet-migrate.`,
    );
  if (!(await exists(exporter))) throw new Error(`Hot Sheet 1 exporter is not available at ${exporter}.`);
  await requireCurrentSetupCli(undefined, runner);
  const output = await runner(binary, hs1MigrationArgs(root, ticketStore, exporter), developmentRepositoryRoot());
  const match = output.match(/Imported (\d+) ticket\(s\) \((\d+) attachment file\(s\)\), skipped (\d+)/),
    tickets = match ? Number(match[1]) + Number(match[3]) : 0,
    attachments = match ? Number(match[2]) : 0;
  let toolsConfigured = true;
  try {
    await runner(
      toolBinary(),
      ['-C', ticketStore, 'setup', '--detect', '--project', root],
      developmentRepositoryRoot(),
    );
  } catch {
    toolsConfigured = false;
  }
  const canonicalStore = await realpath(ticketStore);
  for (const session of sessions.values()) {
    if (session.root === root) session.ticketStore = canonicalStore;
  }
  return {
    ticketStore: canonicalStore,
    connectionId: gitTicketStoreConnectionId(canonicalStore),
    tickets,
    attachments,
    toolsConfigured,
  };
}

type ProcessProbe = (pid: number) => boolean;
function processIsRunning(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === 'EPERM';
  }
}
interface Hs1ChannelRegistration {
  pid: number;
  slug?: string;
}
/** Mirror HS1's channelSlug identity contract for `<project>/.hotsheet`. */
export function hs1ChannelSlug(directory: string): string {
  const name = basename(dirname(resolve(directory))),
    slug = name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '');
  return slug || 'project';
}
async function hs1ChannelPids(directory: string): Promise<number[]> {
  const paths = [resolve(directory, 'channel-port')];
  try {
    for (const entry of await readdir(resolve(directory, 'channel-ports.d'), { withFileTypes: true }))
      if (entry.isFile() && entry.name.endsWith('.json')) paths.push(resolve(directory, 'channel-ports.d', entry.name));
  } catch {
    /* no per-process registry */
  }
  const registrations = await Promise.all(
    paths.map(async (path) => {
      try {
        const value = JSON.parse(await readFile(path, 'utf8')) as { pid?: unknown; slug?: unknown };
        return typeof value.pid === 'number' && Number.isSafeInteger(value.pid) && value.pid > 0
          ? { pid: value.pid, ...(typeof value.slug === 'string' && value.slug ? { slug: value.slug } : {}) }
          : undefined;
      } catch {
        return undefined;
      }
    }),
  );
  const expectedSlug = hs1ChannelSlug(directory);
  return [
    ...new Set(
      registrations
        .filter(
          (entry): entry is Hs1ChannelRegistration =>
            entry !== undefined && (entry.slug === undefined || entry.slug === expectedSlug),
        )
        .map((entry) => entry.pid),
    ),
  ];
}

export async function removeHs1LiveData(directory: string, probe: ProcessProbe = processIsRunning): Promise<string[]> {
  if (!(await exists(resolve(directory, 'db/PG_VERSION')))) return [];
  const running = (await hs1ChannelPids(directory)).filter(probe);
  if (running.length)
    throw new Error(
      `Hot Sheet 1 is still running for this project (process${running.length === 1 ? '' : 'es'} ${running.join(', ')}). Quit Hot Sheet 1 and its AI-tool channel sessions, then retry; no files were removed.`,
    );
  const removed: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (preserveHs1Entry(entry.name)) continue;
    if (
      (entry.name === 'settings.json' || entry.name === 'settings.local.json') &&
      (await isHs2SettingsFile(resolve(directory, entry.name)))
    )
      continue;
    await rm(resolve(directory, entry.name), { recursive: true, force: true });
    removed.push(entry.name);
  }
  return removed.sort();
}

export async function removeImportedHs1Data(projectId: string): Promise<string[]> {
  const session = sessions.get(projectId),
    root = session?.root;
  if (!root || !session.ticketStore) throw new Error('Project session has no imported ticket store.');
  await requireHs1Backup(session.ticketStore, root);
  return removeHs1LiveData(resolve(root, '.hotsheet'));
}

export function localStoreInitArgs(path: string, standalone = false): string[] {
  return standalone
    ? ['init', '--standalone', '--at', path, '--prefix', 'HS2']
    : ['init', '-C', path, '--prefix', 'HS2'];
}

export function projectBootstrapArgs(root: string, store: string, remote?: string): string[] {
  return ['bootstrap', '--project', root, '--store', store, '--prefix', 'HS2', ...(remote ? ['--remote', remote] : [])];
}

export function projectServerPlan(
  bootstrapStore: string,
  root: string,
  ticketStore?: string,
): { serverStore: string; openBody: { root: string; stores?: string[] } } {
  return { serverStore: bootstrapStore, openBody: { root, ...(ticketStore ? { stores: [ticketStore] } : {}) } };
}

async function initializeStore(path: string, standaloneRoot?: string): Promise<void> {
  const binary = toolBinary();
  if (!(await exists(binary)))
    throw new Error(`Hot Sheet CLI is not built at ${binary}. Run cargo build -p hotsheet-cli.`);
  await new Promise<void>((resolveInit, reject) => {
    const child = spawn(binary, localStoreInitArgs(path, Boolean(standaloneRoot)), {
      cwd: standaloneRoot ?? developmentRepositoryRoot(),
      stdio: 'ignore',
    });
    child.once('error', reject);
    child.once('close', (code) => {
      if (code === 0) resolveInit();
      else reject(new Error(`Hot Sheet store setup exited with status ${code ?? 'unknown'}.`));
    });
  });
}

async function bootstrapStore(): Promise<string> {
  const path = resolve(hotsheetHome(), 'server-bootstrap.hs2');
  if (!(await exists(resolve(path, 'hotsheet-store.json')))) await initializeStore(path);
  return realpath(path);
}

export async function createLocalGitTicketStore(
  rootInput: string,
  locationInput?: string,
  runner: ProcessRunner = runProcess,
): Promise<string> {
  const root = await realpath(rootInput.trim()),
    path = locationInput?.trim() ? resolve(locationInput.trim()) : `${root}.hs2`,
    binary = toolBinary();
  if (!(await exists(binary)))
    throw new Error(`Hot Sheet CLI is not built at ${binary}. Run cargo build -p hotsheet-cli.`);
  const cli = await requireCurrentSetupCli((await exists(path)) ? path : undefined, runner),
    target = sessionForRoot(root);
  if (target) {
    const existing = await exists(resolve(path, 'hotsheet-store.json'));
    const server = await serverRequest<ServerCompatibility>(target, '/compatibility').catch(() => undefined);
    requireStoreSchemaCompatibility(server, cli, existing ? 'open' : 'create');
  }
  await runner(binary, projectBootstrapArgs(root, path), developmentRepositoryRoot());
  return realpath(path);
}

export const runGitCommand: GitRunner = (command, args) =>
  new Promise((resolveRun, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'] }),
      stdout: Buffer[] = [],
      stderr: Buffer[] = [];
    child.stdout.on('data', (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on('data', (chunk: Buffer) => stderr.push(chunk));
    child.once('error', reject);
    child.once('close', (code) => {
      if (code === 0) {
        resolveRun();
        return;
      }
      const detail = Buffer.concat(stderr).toString('utf8').trim() || Buffer.concat(stdout).toString('utf8').trim();
      reject(new Error(detail || `Git exited with status ${code ?? 'unknown'}.`));
    });
  });

export type GitRemoteOperation = 'add' | 'push';

/** Add actionable guidance for common Git-host failures while retaining Git's exact
 * diagnostic text. Unknown failures still keep both operation context and stderr. */
export function describeGitRemoteFailure(error: unknown, operation: GitRemoteOperation): Error {
  const detail = (error instanceof Error ? error.message : String(error)).trim() || 'Git did not report any details.';
  const lower = detail.toLowerCase();
  let guidance: string;
  if (/remote .* already exists/.test(lower))
    guidance =
      'This ticket repository already has an origin remote. Use that remote, or remove or rename it before connecting a different one.';
  else if (/repository not found|does not appear to be a git repository/.test(lower))
    guidance =
      'The remote repository was not found, or your account cannot access it. Verify the clone URL and your access on the Git host, then try again.';
  else if (
    /permission denied|authentication failed|authentication required|could not read username|invalid username or (?:password|token)|access denied/.test(
      lower,
    )
  )
    guidance =
      'Git could not authenticate. Verify the clone URL and configure the required SSH key, access token, or credential manager for this account, then try again.';
  else if (/host key verification failed/.test(lower))
    guidance =
      'Git could not verify the SSH host key. Connect to the Git host once from a terminal to review and trust its host key, then try again.';
  else if (/could not resolve (?:host|hostname)|network is unreachable|connection (?:timed out|refused)/.test(lower))
    guidance =
      'Git could not reach the remote host. Check the hostname, network connection, VPN, or proxy, then try again.';
  else if (/non-fast-forward|fetch first|failed to push some refs/.test(lower))
    guidance =
      'The remote already contains commits that this new ticket repository does not have. Use an empty remote, or reconcile the two histories from a terminal before retrying.';
  else if (/src refspec .* does not match any|no commits yet/.test(lower))
    guidance = 'The ticket repository has no commit to push yet. Create its initial commit, then try again.';
  else
    guidance =
      operation === 'add'
        ? 'Git could not add the remote. Review Git’s diagnostic below and correct the repository or remote configuration before retrying.'
        : 'Git could not push the ticket repository. Review Git’s diagnostic below and correct the remote or authentication configuration before retrying.';
  return new Error(`${guidance} Git details: ${detail}`);
}

export async function connectGitTicketStoreRemote(
  storeInput: string,
  remoteInput: string,
  runner: GitRunner = runGitCommand,
  git: BackupGit = backupGit,
): Promise<void> {
  const remote = remoteInput.trim();
  if (!remote || remote.startsWith('-') || /[\r\n]/.test(remote)) throw new Error('Enter a valid Git remote URL.');
  const store = await realpath(storeInput.trim());
  if (!(await exists(resolve(store, 'hotsheet-store.json'))))
    throw new Error('The ticket repository is no longer available.');
  const completion = await readImportCompletion(store, git),
    existing = await git(store, ['remote', 'get-url', 'origin']).catch(() => undefined);
  let added = false;
  if (existing && existing !== remote)
    throw describeGitRemoteFailure(new Error('remote origin already exists.'), 'add');
  if (completion) await invalidateHs1Backup(store, git);
  if (!existing) {
    try {
      await runner('git', ['-C', store, 'remote', 'add', 'origin', remote]);
      added = true;
    } catch (error) {
      throw describeGitRemoteFailure(error, 'add');
    }
  }
  try {
    await runner('git', ['-C', store, 'push', '-u', 'origin', 'HEAD']);
    if (completion) await recordHs1Backup(store, completion, git);
  } catch (error) {
    // A failed first push must remain retryable from the setup screen. Roll back only
    // the origin this operation just added; never leave a half-configured repository.
    if (added && (await git(store, ['remote', 'get-url', 'origin']).catch(() => undefined)) === remote)
      await runner('git', ['-C', store, 'remote', 'remove', 'origin']).catch(() => undefined);
    throw describeGitRemoteFailure(error, 'push');
  }
}

export function gitTicketStoreConnectionId(path: string): string {
  return createHash('sha256').update(path).digest('hex').slice(0, 16);
}

export async function suggestedTicketStore(root: string): Promise<string | undefined> {
  const canonical = await realpath(root);
  const candidate = `${canonical}.hs2`;
  return (await exists(resolve(candidate, 'hotsheet-store.json'))) ? candidate : undefined;
}

async function instanceFor(store: string): Promise<InstanceInfo | undefined> {
  const canonical = await realpath(store);
  const id = createHash('sha256').update(canonical).digest('hex').slice(0, 16);
  try {
    const info = JSON.parse(await readFile(resolve(hotsheetHome(), 'instances', `${id}.json`), 'utf8')) as InstanceInfo;
    if (!Number.isSafeInteger(info.pid) || info.pid <= 0 || !/^https?:\/\//.test(info.url) || !info.secret)
      throw new Error('invalid instance registration');
    process.kill(info.pid, 0);
    return info;
  } catch {
    return undefined;
  }
}

export interface ServerSupervisorPlatform {
  discover(): Promise<InstanceInfo | undefined>;
  probe(instance: InstanceInfo): Promise<boolean>;
  launch(): Promise<void>;
  wait(): Promise<void>;
}

export class UnhealthyManagedServerError extends Error {
  constructor(
    readonly instance: InstanceInfo,
    readonly store?: string,
  ) {
    super(
      `The Hot Sheet server process ${instance.pid} is registered but unhealthy. Active work was preserved; use the explicit local recovery action to stop this exact process before starting another server.`,
    );
  }
}

function sameInstance(left: InstanceInfo | undefined, right: InstanceInfo | undefined): boolean {
  return Boolean(
    left && right && left.pid === right.pid && left.url === right.url && left.started_at === right.started_at,
  );
}

/** Discover and health-check the machine server, launching only after its registered process
 * is gone. This is safe under concurrent clients: the server writer lock remains the final
 * join-don't-collide boundary. Calls are event-driven by project open/API/WS reconnects. */
export async function superviseServer(platform: ServerSupervisorPlatform, maxAttempts = 80): Promise<InstanceInfo> {
  const first = await platform.discover();
  if (first && (await platform.probe(first))) return first;
  let launched = false;
  if (!first) {
    await platform.launch();
    launched = true;
  }
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    await platform.wait();
    const current = await platform.discover();
    if (current && (await platform.probe(current))) return current;
    if (!current && !launched) {
      await platform.launch();
      launched = true;
    }
  }
  if (first && !launched) throw new UnhealthyManagedServerError(first);
  throw new Error('Timed out waiting for a healthy Hot Sheet server.');
}

function serverPlatform(store: string): ServerSupervisorPlatform {
  const repoRoot = developmentRepositoryRoot();
  const binary = process.env.HOTSHEET_SERVER_BIN || resolve(repoRoot, 'target/debug/hotsheet-server');
  return {
    discover: () => instanceFor(store),
    probe: async (instance) => {
      try {
        const response = await fetch(`${instance.url}/health`, { signal: AbortSignal.timeout(750) });
        if (!response.ok) return false;
        return ((await response.json()) as { status?: string }).status === 'ok';
      } catch {
        return false;
      }
    },
    launch: async () => {
      if (!(await exists(binary)))
        throw new Error(`Hot Sheet server is not built at ${binary}. Run cargo build -p hotsheet-server.`);
      await new Promise<void>((resolveLaunch, reject) => {
        const child = spawn(binary, ['-C', store, '--bind', '127.0.0.1:0'], {
          cwd: repoRoot,
          detached: true,
          stdio: 'ignore',
        });
        child.once('error', reject);
        child.once('spawn', () => {
          child.unref();
          resolveLaunch();
        });
      });
    },
    wait: () => new Promise((resolveWait) => setTimeout(resolveWait, 100)),
  };
}

async function ensureServer(store: string): Promise<InstanceInfo> {
  try {
    return await superviseServer(serverPlatform(store));
  } catch (error) {
    if (error instanceof UnhealthyManagedServerError) throw new UnhealthyManagedServerError(error.instance, store);
    throw error;
  }
}

export interface UnhealthyServerRecoveryPlatform extends ServerSupervisorPlatform {
  terminate(instance: InstanceInfo, signal: 'SIGTERM' | 'SIGKILL'): Promise<void>;
}

/** Explicit local-only recovery for a registered process whose health endpoint cannot
 * answer. Never signal a replacement: every signal is preceded by the complete
 * pid/url/start-identity check. */
export async function recoverUnhealthyServer(
  recovery: UnhealthyServerRecovery,
  platform?: UnhealthyServerRecoveryPlatform,
  graceAttempts = 20,
  killAttempts = 20,
): Promise<InstanceInfo> {
  const native = serverPlatform(recovery.store),
    host = platform ?? {
      ...native,
      terminate: (instance, signal) => {
        process.kill(instance.pid, signal);
        return Promise.resolve();
      },
    };
  const expected = (value: InstanceInfo | undefined) =>
    Boolean(
      value &&
      value.pid === recovery.expected.pid &&
      value.url === recovery.expected.url &&
      value.started_at === recovery.expected.started_at,
    );
  const replacement = async () => superviseServer(host);
  let current = await host.discover();
  if (!expected(current)) return replacement();
  await host.terminate(current!, 'SIGTERM');
  for (let attempt = 0; attempt < graceAttempts; attempt += 1) {
    await host.wait();
    current = await host.discover();
    if (!expected(current)) return replacement();
  }
  current = await host.discover();
  if (!expected(current)) return replacement();
  await host.terminate(current!, 'SIGKILL');
  for (let attempt = 0; attempt < killAttempts; attempt += 1) {
    await host.wait();
    current = await host.discover();
    if (!expected(current)) return replacement();
  }
  throw new Error(`Hot Sheet server process ${recovery.expected.pid} remained registered after forced local recovery.`);
}

export function unhealthyServerRecovery(error: unknown): UnhealthyServerRecovery | undefined {
  if (!(error instanceof UnhealthyManagedServerError) || !error.store || !error.instance.started_at) return undefined;
  return {
    store: error.store,
    expected: { pid: error.instance.pid, url: error.instance.url, started_at: error.instance.started_at },
  };
}

export interface SafeRestartPlatform {
  request(): Promise<void>;
  discover(): Promise<InstanceInfo | undefined>;
  supervise(): Promise<InstanceInfo>;
  wait(): Promise<void>;
}

/** Ask the server to restart itself only through its quiescence gate, then wait for its
 * registration to change before supervising the replacement. The client never sends a
 * process signal and never starts a duplicate beside a still-live server. */
export async function safelyRestartServer(
  current: InstanceInfo,
  platform: SafeRestartPlatform,
  maxAttempts = 80,
): Promise<InstanceInfo> {
  await platform.request();
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    await platform.wait();
    const discovered = await platform.discover();
    if (!sameInstance(current, discovered)) return platform.supervise();
  }
  throw new Error(
    `Timed out waiting for Hot Sheet server process ${current.pid} to stop after accepting a safe restart.`,
  );
}

export function storeNeedsServerUpgrade(
  server: ServerCompatibility | undefined,
  cli: CliCompatibility | undefined,
): boolean {
  return Boolean(
    server?.store_schema && cli?.selected_store_schema && cli.selected_store_schema > server.store_schema.max,
  );
}

function supportsSafeRestart(server: ServerCompatibility | undefined): boolean {
  return server?.capabilities?.lifecycle_restart === true && server.capabilities.lifecycle_quiescence === true;
}

async function restartForUpgrade(store: string, target: SessionTarget, current: InstanceInfo): Promise<InstanceInfo> {
  const platform = serverPlatform(store);
  return safelyRestartServer(current, {
    request: () => serverRequest(target, '/lifecycle/restart', { method: 'POST' }).then(() => undefined),
    discover: () => platform.discover(),
    supervise: () => superviseServer(platform),
    wait: () => platform.wait(),
  });
}

async function serverRequest<T>(
  target: SessionTarget,
  path: string,
  init: Omit<RequestInit, 'headers'> & { headers?: Record<string, string> } = {},
): Promise<T> {
  const response = await fetch(`${target.url}${path}`, {
    ...init,
    headers: { 'content-type': 'application/json', 'x-hotsheet-secret': target.secret, ...init.headers },
  });
  if (!response.ok)
    throw new Error(
      ((await response.json().catch(() => null)) as { error?: string } | null)?.error || `${response.status}`,
    );
  return response.json() as Promise<T>;
}

export async function openLocalProject(rootInput: string, ticketStoreInput?: string): Promise<ProjectSession> {
  const root = await realpath(rootInput.trim());
  const ticketStore = ticketStoreInput?.trim()
    ? await realpath(ticketStoreInput.trim())
    : await suggestedTicketStore(root);
  if (ticketStore) await refreshLocalProjectSetup(root, ticketStore);
  const plan = projectServerPlan(await bootstrapStore(), root, ticketStore);
  let instance = await ensureServer(plan.serverStore),
    target: SessionTarget = { url: instance.url, secret: instance.secret, root, serverStore: plan.serverStore };
  let metadata = await serverRequest<ServerCompatibility>(target, '/compatibility').catch(() => undefined);
  const cli = ticketStore ? await cliCompatibility(ticketStore, runProcess) : undefined;
  let compatibility = assessCompatibility(metadata, undefined, process.env.HOT_SHEET_BUILD_REVISION);
  if (
    (compatibility.kind === 'server_too_old' || storeNeedsServerUpgrade(metadata, cli)) &&
    supportsSafeRestart(metadata)
  ) {
    instance = await restartForUpgrade(plan.serverStore, target, instance);
    target = { url: instance.url, secret: instance.secret, root, serverStore: plan.serverStore };
    metadata = await serverRequest<ServerCompatibility>(target, '/compatibility').catch(() => undefined);
    compatibility = assessCompatibility(metadata, undefined, process.env.HOT_SHEET_BUILD_REVISION);
  }
  if (ticketStore && cli) requireStoreSchemaCompatibility(metadata, cli, 'open');
  requireCompatibleServer(compatibility);
  const opened = await serverRequest<{
    checkout: { id: string; root: string; alias: string; stores: string[]; sources: unknown[] };
  }>(target, '/projects/open', {
    method: 'POST',
    body: JSON.stringify(plan.openBody),
  });
  const activeStore = ticketStore ?? opened.checkout.stores[0],
    hs1SourcePath = resolve(root, '.hotsheet'),
    hs1DatabasePath = resolve(root, '.hotsheet/db'),
    hs1MarkerPath = resolve(root, HS1_MARKER),
    hs1DataPresent = await exists(hs1MarkerPath),
    imported = await hasCompletedHs1Import(activeStore, root),
    hs1PostgresVersion = hs1DataPresent ? (await readFile(hs1MarkerPath, 'utf8').catch(() => '')).trim() : '';
  sessions.set(opened.checkout.id, { ...target, ticketStore: activeStore });
  return {
    id: opened.checkout.id,
    root: opened.checkout.root,
    name: opened.checkout.alias,
    stores: opened.checkout.stores,
    apiPath: `/__hotsheet/project-api/${encodeURIComponent(opened.checkout.id)}`,
    compatibility,
    needsTicketSetup: opened.checkout.sources.length === 0,
    needsHs1Migration: hs1DataPresent && !imported,
    hs1ImportCompleted: imported,
    hs1CleanupEligible: hs1DataPresent && imported && (await hasHs1Backup(activeStore, root)),
    ...(hs1DataPresent ? { hs1SourcePath, hs1DatabasePath, hs1PostgresVersion } : {}),
  };
}

/** List the checkouts the bootstrap server currently knows about, for the remote/cross-device project
 * picker: a non-loopback client can't browse the server filesystem, so it picks from these (HS2-VFNCXG).
 * This queries the same singleton bootstrap server `openLocalProject` opens projects through — it was
 * never wired to the client's `GET /__hotsheet/checkouts`, so the remote picker only ever errored
 * (HS2-QMR41J). Dependencies are injectable for testing. */
export async function listServerCheckouts(
  ensure: (store: string) => Promise<InstanceInfo> = ensureServer,
  request: <T>(target: SessionTarget, path: string) => Promise<T> = serverRequest,
  resolveStore: () => Promise<string> = bootstrapStore,
): Promise<Checkout[]> {
  const store = await resolveStore();
  const instance = await ensure(store);
  return request<Checkout[]>({ url: instance.url, secret: instance.secret, serverStore: store }, '/checkouts');
}

export async function proxyProjectRequest(projectId: string, path: string, request: Request): Promise<Response> {
  const target = sessions.get(projectId);
  if (!target) return Response.json({ error: 'Project session is not open.' }, { status: 404 });
  const headers = new Headers(request.headers);
  headers.set('x-hotsheet-secret', target.secret);
  headers.delete('host');
  const body = request.method === 'GET' || request.method === 'HEAD' ? undefined : await request.arrayBuffer();
  const serverPath = projectScopedServerPath(projectId, path);
  const forward = async () => {
    const response = await fetch(authenticatedServerUrl(target.url, serverPath, target.secret), {
      method: request.method,
      headers,
      body,
      redirect: 'manual',
    });
    return new Response(response.body, { status: response.status, headers: response.headers });
  };
  try {
    return await forward();
  } catch (error) {
    if (!target.serverStore) throw error;
    await refreshSupervisedTarget(target);
    if (request.method !== 'GET' && request.method !== 'HEAD')
      return Response.json(
        {
          error:
            'The server restarted while this write was in flight. Its completion is unknown; refresh before retrying.',
        },
        { status: 503 },
      );
    return forward();
  }
}

/** Route project-owned settings through checkout-scoped server APIs. Ticket and host-wide
 * endpoints retain their existing paths; this boundary also works for source-free projects. */
export function projectScopedServerPath(projectId: string, path: string): string {
  const [pathname, ...query] = path.split('?'),
    scoped =
      pathname === '/commands' ||
      pathname === '/command-runs' ||
      pathname === '/views' ||
      pathname === '/terminal-settings' ||
      /^\/commands\/[^/]+\/run$/.test(pathname) ||
      /^\/command-runs\/[^/]+(?:\/cancel)?$/.test(pathname);
  return `${scoped ? `/checkouts/${encodeURIComponent(projectId)}` : ''}${pathname}${query.length ? `?${query.join('?')}` : ''}`;
}

async function refreshSupervisedTarget(target: SessionTarget): Promise<void> {
  if (!target.serverStore) return;
  const instance = await ensureServer(target.serverStore);
  for (const session of sessions.values())
    if (session.serverStore === target.serverStore) {
      session.url = instance.url;
      session.secret = instance.secret;
    }
}

/** Reveal only a path the authenticated server still reports as corrupt for this checkout. */
export async function revealCorruptTicket(
  projectId: string,
  path: string,
  launch: RevealLauncher = launchReveal,
): Promise<void> {
  const target = sessions.get(projectId);
  if (!target) throw new Error('Project session is not open.');
  const diagnostics = await serverRequest<CorruptDiagnostic[]>(
    target,
    `/checkouts/${encodeURIComponent(projectId)}/corrupt-tickets`,
  );
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

/** Resolve a browser-facing project session to its authenticated terminal attach URL.
 * This runs only inside the local bridge; the returned URL and secret never reach browser JS. */
export async function projectTerminalWebSocketUrl(projectId: string, terminalId: string): Promise<string | undefined> {
  const target = sessions.get(projectId);
  if (!target) return undefined;
  await refreshSupervisedTarget(target);
  return authenticatedTerminalWebSocketUrl(target.url, terminalId, target.secret);
}

/** Resolve a browser-facing project session to its authenticated change-stream URL.
 * The browser connects only to the credential-free Vite bridge path. */
export async function projectChangeWebSocketUrl(projectId: string): Promise<string | undefined> {
  const target = sessions.get(projectId);
  if (!target) return undefined;
  await refreshSupervisedTarget(target);
  const url = new URL('/ws/sync', target.url);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('secret', target.secret);
  return url.toString();
}

export function authenticatedTerminalWebSocketUrl(origin: string, terminalId: string, secret: string): string {
  const url = new URL(`/terminals/${encodeURIComponent(terminalId)}/attach`, origin);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('secret', secret);
  return url.toString();
}
