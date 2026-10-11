import { describe, expect, it, vi } from 'vitest';

import { errorMessageOf, isRecord, parseJson, responseJson, stringArray, tryParseJson } from './json-value';
import { parseUnhealthyServerRecovery } from './local-bridge-client';
import { parseBinaryRevisionStatus } from './local-host';
import { parseMigrationProgress } from './migration-stream';
import { parseCliCompatibility, parseInstanceInfo } from './project-bridge';
import { openProjectFetch } from './project-startup';
import { isTerminalReplacementReplay, parseTerminalSizeMessage } from './terminal-viewport';

describe('json-value helpers (HS2-3DA0FQ)', () => {
  it('narrows records and parses tolerantly', () => {
    expect(isRecord({})).toBe(true);
    expect(isRecord([])).toBe(false);
    expect(isRecord(null)).toBe(false);
    expect(parseJson('{"a":1}')).toEqual({ a: 1 });
    expect(() => parseJson('{')).toThrow();
    expect(tryParseJson('{')).toBeUndefined();
    expect(stringArray(['a', 1, 'b'])).toEqual(['a', 'b']);
    expect(stringArray({ a: 'x' })).toEqual([]);
    expect(errorMessageOf({ error: 'denied' })).toBe('denied');
    expect(errorMessageOf({ error: '' })).toBeUndefined();
    expect(errorMessageOf('denied')).toBeUndefined();
  });
  it('reads response bodies as unknown without throwing on empty or invalid JSON', async () => {
    expect(await responseJson(Response.json({ ok: true }))).toEqual({ ok: true });
    expect(await responseJson(new Response('not json'))).toBeUndefined();
    expect(await responseJson(new Response(null, { status: 204 }))).toBeUndefined();
  });
});

describe('validating parsers that replaced unchecked casts (HS2-3DA0FQ)', () => {
  it('rejects malformed migration progress records', () => {
    expect(parseMigrationProgress('{"version":1,"phase":"copy","completed":1,"total":2}')).toMatchObject({
      phase: 'copy',
    });
    for (const line of [
      '[]',
      '{"version":1,"phase":"copy","completed":"1"}',
      '{"version":1,"phase":"copy","error":5}',
      '{"version":1,"phase":"done","result":{"tickets":"3","attachments":0}}',
    ])
      expect(() => parseMigrationProgress(line)).toThrow('Unsupported migration progress record.');
  });
  it('validates CLI compatibility, instance registrations, and revision status', () => {
    const compatibility = { generation: 'g', store_schema: { min: 1, max: 2, creates: 2 } };
    expect(parseCliCompatibility(compatibility)).toEqual(compatibility);
    expect(() => parseCliCompatibility({ generation: 'g' })).toThrow('invalid compatibility report');
    expect(() => parseCliCompatibility({ ...compatibility, selected_store_schema: '2' })).toThrow();
    const instance = { pid: 7, url: 'http://127.0.0.1:1', secret: 's' };
    expect(parseInstanceInfo(instance)).toEqual(instance);
    for (const value of [
      { ...instance, pid: -1 },
      { ...instance, url: 'file:///x' },
      { ...instance, secret: '' },
      null,
    ])
      expect(parseInstanceInfo(value)).toBeUndefined();
    expect(parseBinaryRevisionStatus({ build_revision: 'a', source_revision: null, source_stale: false })).toEqual({
      build_revision: 'a',
      source_revision: null,
      source_stale: false,
    });
    expect(parseBinaryRevisionStatus({ source_stale: 'yes' })).toBeUndefined();
    expect(parseBinaryRevisionStatus('x')).toBeUndefined();
  });
  it('accepts only well-formed terminal control frames', () => {
    expect(parseTerminalSizeMessage('{"pty_size":{"cols":80,"rows":24},"driven_by":"v"}')).toMatchObject({
      pty_size: { cols: 80, rows: 24 },
    });
    expect(parseTerminalSizeMessage('{"pty_size":{"cols":"80","rows":24}}')).toBeUndefined();
    expect(parseTerminalSizeMessage('{"pty_size":{"cols":80,"rows":24},"driven_by":3}')).toBeUndefined();
    expect(parseTerminalSizeMessage('null')).toBeUndefined();
    expect(isTerminalReplacementReplay('{"terminal_replay":"replace"}')).toBe(true);
    expect(isTerminalReplacementReplay('null')).toBe(false);
  });
  it('parses recovery identity and routes project open through the typed local bridge', async () => {
    const recovery = { store: '/a.hs2', expected: { pid: 1, url: 'http://x', started_at: 'now' } };
    expect(parseUnhealthyServerRecovery(recovery)).toEqual(recovery);
    expect(parseUnhealthyServerRecovery({ store: '/a.hs2', expected: { pid: '1' } })).toBeUndefined();
    const request = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ error: 'Busy', recovery: { store: 3 } }, { status: 409 }))
      .mockResolvedValueOnce(Response.json({ id: 'not-a-project' }));
    expect(await openProjectFetch('a', undefined, request, async () => [])).toEqual({ ok: false, error: 'Busy' });
    expect(await openProjectFetch('a', undefined, request, async () => [])).toMatchObject({
      ok: false,
      error: expect.stringContaining('invalid project'),
    });
  });
});
