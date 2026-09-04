import { describe, expect, it, vi } from 'vitest';

import { createDevApp } from './dev-server';
import { authenticatedServerUrl, authenticatedTerminalWebSocketUrl, developmentRepositoryRoot, requireCompatibleServer, requireReportedCorruptPath, revealCommand } from './project-bridge';

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
