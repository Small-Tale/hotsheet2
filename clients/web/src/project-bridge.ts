import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, readdir, readFile, realpath, rm } from 'node:fs/promises';
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
  needsTicketSetup: boolean;
  needsHs1Migration: boolean;
  hs1ImportCompleted: boolean;
  hs1CleanupEligible: boolean;
  hs1SourcePath?: string;
  hs1DatabasePath?: string;
  hs1PostgresVersion?: string;
}

interface InstanceInfo { pid:number; url:string; secret:string }
interface SessionTarget { url:string; secret:string; root?:string }
interface CorruptDiagnostic { path:string }

export type RevealLauncher = (command: string, args: string[]) => Promise<void>;
export type FolderChooserRunner=(command:string,args:string[])=>Promise<string|undefined>;
export type GitRunner=(command:string,args:string[])=>Promise<void>;

type ProjectBridgeProcess=typeof process&{__hotsheetProjectSessions?:Map<string,SessionTarget>};

/** Vite evaluates config plugins and its SSR dev entry in separate module graphs. Keep the
 * authenticated project-session registry on their shared process object so HTTP opens and
 * terminal WebSocket upgrades resolve the same session without exposing credentials. */
export function projectSessionRegistry(host:ProjectBridgeProcess=process):Map<string,SessionTarget>{
  return host.__hotsheetProjectSessions??=(new Map<string,SessionTarget>());
}

const sessions = projectSessionRegistry();

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

export function folderChooserCommand(hostPlatform=process.platform):{command:string;args:string[]}{
  if(hostPlatform==='darwin')return{command:'osascript',args:['-e','POSIX path of (choose folder with prompt "Choose a folder for Hot Sheet")']};
  if(hostPlatform==='win32')return{command:'powershell.exe',args:['-NoProfile','-Command',"Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }"]};
  return{command:'zenity',args:['--file-selection','--directory','--title=Choose a folder for Hot Sheet']};
}

const runFolderChooser:FolderChooserRunner=(command,args)=>new Promise((resolveChoice,reject)=>{const child=spawn(command,args,{stdio:['ignore','pipe','ignore']}),chunks:Buffer[]=[];child.stdout.on('data',(chunk:Buffer)=>{chunks.push(chunk)});child.once('error',reject);child.once('close',code=>{resolveChoice(code===0?Buffer.concat(chunks).toString('utf8').trim()||undefined:undefined)})});

export async function chooseLocalFolder(runner:FolderChooserRunner=runFolderChooser,hostPlatform=process.platform):Promise<string|undefined>{const command=folderChooserCommand(hostPlatform);return runner(command.command,command.args)}

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

function toolBinary() {
  return process.env.HOTSHEET_CLI_BIN || resolve(developmentRepositoryRoot(), 'target/debug/hotsheet-cli');
}

function migrateBinary() {
  return process.env.HOTSHEET_MIGRATE_BIN || resolve(developmentRepositoryRoot(), 'target/debug/hotsheet-migrate');
}

function migratorScript() {
  return process.env.HOTSHEET_MIGRATOR || resolve(developmentRepositoryRoot(), 'migrator/src/export.mjs');
}

const HS1_MARKER='.hotsheet/db/PG_VERSION',HS1_RECEIPT='hotsheet-hs1-import.json';
export function hs1MigrationArgs(root:string,ticketStore:string,exporter:string):string[]{return[resolve(root,'.hotsheet'),'-C',ticketStore,'--migrator',exporter]}
export function preserveHs1Entry(name:string):boolean{return name==='store'||/backup/i.test(name)}

async function receiptMatchesProject(store:string|undefined,root:string):Promise<boolean>{
  if(!store)return false;
  try{const receipt=JSON.parse(await readFile(resolve(store,HS1_RECEIPT),'utf8')) as {sourceProject?:string};return receipt.sourceProject===root}catch{return false}
}

async function hasGitRemote(store:string|undefined):Promise<boolean>{
  if(!store)return false;
  return new Promise(resolveResult=>{const child=spawn('git',['-C',store,'remote','get-url','origin'],{stdio:'ignore'});child.once('error',()=> { resolveResult(false); });child.once('close',code=> { resolveResult(code===0); })})
}

export interface Hs1MigrationResult {ticketStore:string;connectionId:string;tickets:number;attachments:number;toolsConfigured:boolean}
export type ProcessRunner=(command:string,args:string[],cwd:string)=>Promise<string>;
const runProcess:ProcessRunner=(command,args,cwd)=>new Promise((resolveRun,reject)=>{const child=spawn(command,args,{cwd,stdio:['ignore','pipe','pipe']}),stdout:Buffer[]=[],stderr:Buffer[]=[];child.stdout.on('data',(chunk:Buffer)=>stdout.push(chunk));child.stderr.on('data',(chunk:Buffer)=>stderr.push(chunk));child.once('error',reject);child.once('close',code=>{const output=Buffer.concat(stdout).toString('utf8'),error=Buffer.concat(stderr).toString('utf8').trim();if(code===0)resolveRun(output);else reject(new Error(error||`${command} exited with status ${code??'unknown'}.`))})});

export async function migrateHs1Project(rootInput:string,locationInput?:string,runner:ProcessRunner=runProcess):Promise<Hs1MigrationResult>{
  const root=await realpath(rootInput.trim());
  if(!await exists(resolve(root,HS1_MARKER)))throw new Error('Hot Sheet 1 data is no longer present in this project.');
  const ticketStore=locationInput?.trim()?resolve(locationInput.trim()):`${root}.hs2`,binary=migrateBinary(),exporter=migratorScript();
  if(!await exists(binary))throw new Error(`Hot Sheet migrator is not built at ${binary}. Run cargo build -p hotsheet-cli --bin hotsheet-migrate.`);
  if(!await exists(exporter))throw new Error(`Hot Sheet 1 exporter is not available at ${exporter}.`);
  const output=await runner(binary,hs1MigrationArgs(root,ticketStore,exporter),developmentRepositoryRoot());
  const match=output.match(/Imported (\d+) ticket\(s\) \((\d+) attachment file\(s\)\), skipped (\d+)/),tickets=match?Number(match[1])+Number(match[3]):0,attachments=match?Number(match[2]):0;
  let toolsConfigured=true;
  try{await runner(toolBinary(),['-C',ticketStore,'setup','--detect','--project',root],developmentRepositoryRoot())}catch{toolsConfigured=false}
  const canonicalStore=await realpath(ticketStore);
  return{ticketStore:canonicalStore,connectionId:gitTicketStoreConnectionId(canonicalStore),tickets,attachments,toolsConfigured};
}

export async function removeImportedHs1Data(projectId:string):Promise<string[]>{
  const root=sessions.get(projectId)?.root;
  if(!root)throw new Error('Project session is not open.');
  const directory=resolve(root,'.hotsheet');
  if(!await exists(resolve(directory,'db/PG_VERSION')))throw new Error('Hot Sheet 1 data is no longer present in this project.');
  const removed:string[]=[];
  for(const entry of await readdir(directory,{withFileTypes:true})){
    if(preserveHs1Entry(entry.name))continue;
    await rm(resolve(directory,entry.name),{recursive:true,force:true});removed.push(entry.name);
  }
  return removed;
}

export function localStoreInitArgs(path:string,standalone=false):string[]{
  return standalone?['init','--standalone','--at',path,'--prefix','HS2']:['init','-C',path,'--prefix','HS2'];
}

export function projectBootstrapArgs(root:string,store:string,remote?:string):string[]{
  return['bootstrap','--project',root,'--store',store,'--prefix','HS2',...(remote?['--remote',remote]:[])];
}

async function initializeStore(path:string,standaloneRoot?:string):Promise<void>{
  const binary=toolBinary();
  if(!await exists(binary))throw new Error(`Hot Sheet CLI is not built at ${binary}. Run cargo build -p hotsheet-cli.`);
  await new Promise<void>((resolveInit,reject)=>{const child=spawn(binary,localStoreInitArgs(path,Boolean(standaloneRoot)),{cwd:standaloneRoot??developmentRepositoryRoot(),stdio:'ignore'});child.once('error',reject);child.once('close',code=>{if(code===0)resolveInit();else reject(new Error(`Hot Sheet store setup exited with status ${code??'unknown'}.`))})});
}

async function bootstrapStore():Promise<string>{
  const path=resolve(hotsheetHome(),'server-bootstrap.hs2');
  if(!await exists(resolve(path,'hotsheet-store.json')))await initializeStore(path);
  return realpath(path);
}

export async function createLocalGitTicketStore(rootInput:string,locationInput?:string,runner:ProcessRunner=runProcess):Promise<string>{
  const root=await realpath(rootInput.trim()),path=locationInput?.trim()?resolve(locationInput.trim()):`${root}.hs2`,binary=toolBinary();
  if(!await exists(binary))throw new Error(`Hot Sheet CLI is not built at ${binary}. Run cargo build -p hotsheet-cli.`);
  await runner(binary,projectBootstrapArgs(root,path),developmentRepositoryRoot());
  return realpath(path);
}

const runGit:GitRunner=(command,args)=>new Promise((resolveRun,reject)=>{const child=spawn(command,args,{stdio:'ignore'});child.once('error',reject);child.once('close',code=>{if(code===0)resolveRun();else reject(new Error(`Git exited with status ${code??'unknown'}.`))})});

export async function connectGitTicketStoreRemote(storeInput:string,remoteInput:string,runner:GitRunner=runGit):Promise<void>{
  const store=await realpath(storeInput.trim()),remote=remoteInput.trim();
  if(!remote||remote.startsWith('-')||/[\r\n]/.test(remote))throw new Error('Enter a valid Git remote URL.');
  if(!await exists(resolve(store,'hotsheet-store.json')))throw new Error('The ticket repository is no longer available.');
  await runner('git',['-C',store,'remote','add','origin',remote]);
  try{await runner('git',['-C',store,'push','-u','origin','HEAD'])}
  catch(error){
    // A failed first push must remain retryable from the setup screen. Roll back only
    // the origin this operation just added; never leave a half-configured repository.
    await runner('git',['-C',store,'remote','remove','origin']).catch(()=>undefined);
    throw error;
  }
}

export function gitTicketStoreConnectionId(path:string):string{return createHash('sha256').update(path).digest('hex').slice(0,16)}

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
  const instance = await ensureServer(ticketStore??await bootstrapStore());
  const target = { url: instance.url, secret: instance.secret, root };
  const metadata = await serverRequest<ServerCompatibility>(target, '/compatibility').catch(() => undefined);
  const compatibility = assessCompatibility(metadata, undefined, process.env.HOT_SHEET_BUILD_REVISION);
  requireCompatibleServer(compatibility);
  const opened = await serverRequest<{checkout:{id:string;root:string;alias:string;stores:string[];sources:unknown[]}}>(target, '/projects/open', {
    method: 'POST',
    body: JSON.stringify({ root, ...(ticketStoreInput?.trim() ? { stores: [ticketStore] } : {}) }),
  });
  sessions.set(opened.checkout.id, target);
  const activeStore=ticketStore??opened.checkout.stores[0],hs1SourcePath=resolve(root,'.hotsheet'),hs1DatabasePath=resolve(root,'.hotsheet/db'),hs1MarkerPath=resolve(root,HS1_MARKER),hs1DataPresent=await exists(hs1MarkerPath),imported=await receiptMatchesProject(activeStore,root),hs1PostgresVersion=hs1DataPresent?(await readFile(hs1MarkerPath,'utf8').catch(()=>'' )).trim():'';
  return { id: opened.checkout.id, root: opened.checkout.root, name: opened.checkout.alias, stores: opened.checkout.stores, apiPath: `/__hotsheet/project-api/${encodeURIComponent(opened.checkout.id)}`, compatibility, needsTicketSetup: opened.checkout.sources.length===0,needsHs1Migration:hs1DataPresent&&!imported,hs1ImportCompleted:imported,hs1CleanupEligible:hs1DataPresent&&imported&&await hasGitRemote(activeStore),...(hs1DataPresent?{hs1SourcePath,hs1DatabasePath,hs1PostgresVersion}: {}) };
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

/** Resolve a browser-facing project session to its authenticated terminal attach URL.
 * This runs only inside the local bridge; the returned URL and secret never reach browser JS. */
export function projectTerminalWebSocketUrl(projectId:string,terminalId:string):string|undefined {
  const target=sessions.get(projectId);
  if(!target)return undefined;
  return authenticatedTerminalWebSocketUrl(target.url,terminalId,target.secret);
}

export function authenticatedTerminalWebSocketUrl(origin:string,terminalId:string,secret:string):string {
  const url=new URL(`/terminals/${encodeURIComponent(terminalId)}/attach`,origin);
  url.protocol=url.protocol==='https:'?'wss:':'ws:';
  url.searchParams.set('secret',secret);
  return url.toString();
}
