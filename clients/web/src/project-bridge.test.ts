import { describe, expect, it } from 'vitest';

import { authenticatedServerUrl, developmentRepositoryRoot, requireCompatibleServer } from './project-bridge';

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
