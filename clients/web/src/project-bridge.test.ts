import { describe, expect, it, vi } from 'vitest';

import { createDevApp } from './dev-server';
import { authenticatedServerUrl, authenticatedTerminalWebSocketUrl,chooseLocalFolder,connectGitTicketStoreRemote, developmentRepositoryRoot,folderChooserCommand,hs1MigrationArgs,localStoreInitArgs,preserveHs1Entry, requireCompatibleServer, requireReportedCorruptPath, revealCommand } from './project-bridge';

describe('projectSessionRegistry',()=>{
  it('shares project sessions across separately evaluated Vite module graphs',async()=>{
    const moduleUrl=new URL('./project-bridge.ts',import.meta.url).href;
    const configGraph=await import(`${moduleUrl}?graph=config`),ssrGraph=await import(`${moduleUrl}?graph=ssr`);
    configGraph.projectSessionRegistry().set('module-graph-checkout',{url:'http://127.0.0.1:1',secret:'private'});
    expect(ssrGraph.projectTerminalWebSocketUrl('module-graph-checkout','terminal')).toBe('ws://127.0.0.1:1/terminals/terminal/attach?secret=private');
  });
});

describe('developmentRepositoryRoot', () => {
  it('uses the explicit original repository root inside a stable snapshot', () => {
    expect(developmentRepositoryRoot('/tmp/hotsheet-web-stable-123', {
      HOTSHEET_REPO_ROOT: '/work/hotsheet2',
    })).toBe('/work/hotsheet2');
  });

  it('retains the normal clients/web fallback for hot development', () => {
    expect(developmentRepositoryRoot('/work/hotsheet2/clients/web', {})).toBe('/work/hotsheet2');
  });
});

describe('authenticatedServerUrl', () => {
  it('uses legacy query authentication for loopback polling without changing browser URLs', () => {
    expect(authenticatedServerUrl('http://127.0.0.1:55560', '/ws/poll?timeout_ms=25000&since=7', 'old secret'))
      .toBe('http://127.0.0.1:55560/ws/poll?timeout_ms=25000&since=7&secret=old+secret');
  });

  it('does not put secrets into ordinary upstream request URLs', () => {
    expect(authenticatedServerUrl('http://127.0.0.1:55560', '/tickets?text=one', 'secret'))
      .toBe('http://127.0.0.1:55560/tickets?text=one');
  });
});

describe('authenticatedTerminalWebSocketUrl',()=>{
  it('adds the secret only to the loopback upstream and escapes terminal identity',()=>{
    expect(authenticatedTerminalWebSocketUrl('http://127.0.0.1:5511','codex/main','private value')).toBe('ws://127.0.0.1:5511/terminals/codex%2Fmain/attach?secret=private+value');
    expect(authenticatedTerminalWebSocketUrl('https://hs.test','term','secret')).toMatch(/^wss:/);
  });
});

describe('revealCommand',()=>{
  it('uses argument arrays and the platform-native file location action without a shell',()=>{
    expect(revealCommand('/tmp/broken ticket.md','darwin')).toEqual({command:'open',args:['-R','/tmp/broken ticket.md']});
    expect(revealCommand('C:\\work\\broken.md','win32')).toEqual({command:'explorer.exe',args:['/select,C:\\work\\broken.md']});
    expect(revealCommand('/work/tickets/broken.md','linux')).toEqual({command:'xdg-open',args:['/work/tickets']});
  });

  it('allows only an exact path from current authenticated corrupt diagnostics',()=>{
    const diagnostics=[{path:'/work/store/tickets/broken.md'}];
    expect(()=>{requireReportedCorruptPath(diagnostics,'/work/store/tickets/broken.md')}).not.toThrow();
    expect(()=>{requireReportedCorruptPath(diagnostics,'/work/store/../secrets.txt')}).toThrow(/no longer present/);
  });

  it('routes a specific project and path through an injected launcher boundary',async()=>{
    const reveal=vi.fn().mockResolvedValue(undefined);
    const response=await createDevApp(true,undefined,reveal).request('/__hotsheet/projects/demo/corrupt-tickets/reveal',{method:'POST',headers:{'content-type':'application/json'},body:'{"path":"/work/broken.md"}'});
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({revealed:true});
    expect(reveal).toHaveBeenCalledWith('demo','/work/broken.md');
    expect((await createDevApp(false,undefined,reveal).request('/__hotsheet/projects/demo/corrupt-tickets/reveal',{method:'POST'})).status).toBe(404);
  });
});

describe('native folder chooser',()=>{
  it('uses fixed argument arrays for each host platform and returns the selected path',async()=>{
    expect(folderChooserCommand('darwin')).toMatchObject({command:'osascript',args:['-e',expect.stringContaining('choose folder')]});
    expect(folderChooserCommand('win32')).toMatchObject({command:'powershell.exe'});
    expect(folderChooserCommand('linux')).toEqual({command:'zenity',args:['--file-selection','--directory','--title=Choose a folder for Hot Sheet']});
    const runner=vi.fn().mockResolvedValue('/work/selected');await expect(chooseLocalFolder(runner,'darwin')).resolves.toBe('/work/selected');expect(runner).toHaveBeenCalledWith('osascript',expect.any(Array));
  });
  it('exposes selection and cancellation only from the local development bridge',async()=>{
    const choose=vi.fn().mockResolvedValueOnce('/work/selected').mockResolvedValueOnce(undefined),app=createDevApp(true,undefined,undefined,choose);
    expect(await (await app.request('/__hotsheet/folders/choose',{method:'POST'})).json()).toEqual({path:'/work/selected'});
    expect(await (await app.request('/__hotsheet/folders/choose',{method:'POST'})).json()).toEqual({});
    expect((await createDevApp(false).request('/__hotsheet/folders/choose',{method:'POST'})).status).toBe(404);
  });
  it('creates bootstrap and standalone stores through explicit CLI argument arrays',()=>{
    expect(localStoreInitArgs('/tmp/bootstrap')).toEqual(['init','-C','/tmp/bootstrap','--prefix','HS2']);
    expect(localStoreInitArgs('/work/demo.hs2',true)).toEqual(['init','--standalone','--at','/work/demo.hs2','--prefix','HS2']);
  });
  it('exposes git ticket-store setup only through the local development bridge',async()=>{
    const setup=vi.fn().mockResolvedValueOnce('/work/demo.hs2').mockResolvedValueOnce('/chosen/tickets'),app=createDevApp(true,undefined,undefined,undefined,setup);
    const response=await app.request('/__hotsheet/projects/setup-git',{method:'POST',headers:{'content-type':'application/json'},body:'{"root":"/work/demo"}'});
    expect(response.status).toBe(201);expect(await response.json()).toEqual({ticketStore:'/work/demo.hs2',connectionId:'719abfebc935ba14'});expect(setup).toHaveBeenCalledWith('/work/demo',undefined);
    const custom=await app.request('/__hotsheet/projects/setup-git',{method:'POST',headers:{'content-type':'application/json'},body:'{"root":"/work/demo","location":"/chosen/tickets"}'});expect(custom.status).toBe(201);expect(setup).toHaveBeenLastCalledWith('/work/demo','/chosen/tickets');expect(await custom.json()).toMatchObject({ticketStore:'/chosen/tickets',connectionId:expect.stringMatching(/^[a-f0-9]{16}$/)});
    expect((await createDevApp(false,undefined,undefined,undefined,setup).request('/__hotsheet/projects/setup-git',{method:'POST'})).status).toBe(404);
  });
});

describe('Hot Sheet 1 project import bridge',()=>{
  it('uses argument arrays for the standalone migrator and preserves backups plus the HS2 link',()=>{
    expect(hs1MigrationArgs('/work/demo','/tickets/demo.hs2','/app/migrator/export.mjs')).toEqual(['/work/demo/.hotsheet','-C','/tickets/demo.hs2','--migrator','/app/migrator/export.mjs']);
    expect(['db','attachments','settings.json'].filter(preserveHs1Entry)).toEqual([]);
    expect(['store','db.hs1-backup','Backup-2026'].filter(preserveHs1Entry)).toEqual(['store','db.hs1-backup','Backup-2026']);
  });
  it('exposes explicit import and cleanup actions only through the local bridge',async()=>{
    const migrate=vi.fn().mockResolvedValue({ticketStore:'/tickets/demo.hs2',connectionId:'source',tickets:12,attachments:3,toolsConfigured:true}),remove=vi.fn().mockResolvedValue(['db','settings.json']),app=createDevApp(true,undefined,undefined,undefined,undefined,undefined,migrate,remove);
    const imported=await app.request('/__hotsheet/projects/migrate-hs1',{method:'POST',headers:{'content-type':'application/json'},body:'{"root":"/work/demo","location":"/tickets/demo.hs2"}'});
    expect(imported.status).toBe(201);expect(await imported.json()).toMatchObject({tickets:12,attachments:3});expect(migrate).toHaveBeenCalledWith('/work/demo','/tickets/demo.hs2');
    const cleaned=await app.request('/__hotsheet/projects/demo/hs1-data',{method:'DELETE'});expect(await cleaned.json()).toEqual({removed:['db','settings.json']});expect(remove).toHaveBeenCalledWith('demo');
    const disabled=createDevApp(false,undefined,undefined,undefined,undefined,undefined,migrate,remove);expect((await disabled.request('/__hotsheet/projects/migrate-hs1',{method:'POST'})).status).toBe(404);expect((await disabled.request('/__hotsheet/projects/demo/hs1-data',{method:'DELETE'})).status).toBe(404);
  });
});

describe('Git ticket-store remote setup',()=>{
  it('adds origin and performs the first push with argument arrays',async()=>{const calls:Array<[string,string[]]>=[],runner=async(command:string,args:string[])=>{calls.push([command,args])};await connectGitTicketStoreRemote('/Users/westphal/Documents/hotsheet2.hs2','git@github.com:Small-Tale/tickets.git',runner);expect(calls).toEqual([['git',['-C','/Users/westphal/Documents/hotsheet2.hs2','remote','add','origin','git@github.com:Small-Tale/tickets.git']],['git',['-C','/Users/westphal/Documents/hotsheet2.hs2','push','-u','origin','HEAD']]])});
  it('removes the just-added origin when the first push fails so setup can be retried',async()=>{const calls:Array<[string,string[]]>=[],runner=async(command:string,args:string[])=>{calls.push([command,args]);if(args.includes('push'))throw new Error('push failed')};await expect(connectGitTicketStoreRemote('/Users/westphal/Documents/hotsheet2.hs2','git@example.com:team/tickets.git',runner)).rejects.toThrow('push failed');expect(calls.at(-1)).toEqual(['git',['-C','/Users/westphal/Documents/hotsheet2.hs2','remote','remove','origin']])});
  it('rejects option-like and multiline remote values before running Git',async()=>{const runner=vi.fn();await expect(connectGitTicketStoreRemote('/Users/westphal/Documents/hotsheet2.hs2','--upload-pack=bad',runner)).rejects.toThrow(/valid Git remote URL/);await expect(connectGitTicketStoreRemote('/Users/westphal/Documents/hotsheet2.hs2','good\nbad',runner)).rejects.toThrow(/valid Git remote URL/);expect(runner).not.toHaveBeenCalled()});
  it('exposes remote connection only through the local bridge',async()=>{const connect=vi.fn().mockResolvedValue(undefined),app=createDevApp(true,undefined,undefined,undefined,undefined,connect),request={method:'POST',headers:{'content-type':'application/json'},body:'{"store":"/tickets","remote":"git@example.com:team/tickets.git"}'};const response=await app.request('/__hotsheet/projects/setup-git-remote',request);expect(response.status).toBe(200);expect(connect).toHaveBeenCalledWith('/tickets','git@example.com:team/tickets.git');expect((await createDevApp(false,undefined,undefined,undefined,undefined,connect).request('/__hotsheet/projects/setup-git-remote',request)).status).toBe(404)});
});

describe('requireCompatibleServer', () => {
  const base = { revisionMismatch: false, sourceStale: false, canRestartServer: false };

  it('blocks newer-server and older-server protocol boundaries with explicit upgrade guidance', () => {
    expect(() => { requireCompatibleServer({ ...base, kind: 'client_too_old', detail: 'Client protocol 1–1 is older.' }); })
      .toThrow(/update required.*cannot be opened.*requires a newer HS2 client/i);
    expect(() => { requireCompatibleServer({ ...base, kind: 'server_too_old', detail: 'Server protocol 0–0 is older.' }); })
      .toThrow(/server update required.*cannot open/i);
  });

  it('allows intersecting ranges and legacy servers with unknown metadata', () => {
    expect(() => { requireCompatibleServer({ ...base, kind: 'compatible' }); }).not.toThrow();
    expect(() => { requireCompatibleServer({ ...base, kind: 'unknown' }); }).not.toThrow();
  });
});
