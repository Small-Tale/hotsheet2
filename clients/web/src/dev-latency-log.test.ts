import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { createDevLatencyLog, devLatencyLogPath } from './dev-latency-log';
import { createDevApp } from './dev-server';
import { projectSessionRegistry } from './project-bridge';

const report = (label: string, duration_ms = 420) => ({
  at: '2026-10-16T00:00:00.000Z',
  kind: 'interaction',
  label,
  duration_ms,
  reasons: ['peak'],
  message: `Slow interaction: ${label} took ${duration_ms} ms`,
});

let scratch: string | undefined;
afterEach(() => {
  if (scratch) rmSync(scratch, { recursive: true, force: true });
  scratch = undefined;
});

describe('dev latency log', () => {
  it('defaults under HOTSHEET_HOME and honors an explicit file', () => {
    expect(devLatencyLogPath({ HOTSHEET_HOME: '/h' })).toBe('/h/logs/dev-latency.jsonl');
    expect(devLatencyLogPath({ HOTSHEET_HOME: '/h', HOTSHEET_DEV_LATENCY_LOG: '/x.jsonl' })).toBe('/x.jsonl');
    expect(devLatencyLogPath({})).toMatch(/\.hotsheet2\/logs\/dev-latency\.jsonl$/);
  });

  it('warns, appends JSON lines, keeps recent reports, and drops malformed entries', async () => {
    scratch = mkdtempSync(join(tmpdir(), 'hs-dev-latency-'));
    const file = join(scratch, 'nested', 'dev-latency.jsonl'),
      warnings: string[] = [];
    const log = createDevLatencyLog({ file, warn: (line) => warnings.push(line) });
    expect(await log.record({ page: '/', reports: [report('click a'), { label: 'bad' }, report('click b')] })).toBe(2);
    expect(await log.record({ reports: [report('click c')] })).toBe(1);
    const lines = readFileSync(file, 'utf8')
      .trim()
      .split('\n')
      .map((line) => JSON.parse(line) as { label: string; page?: string });
    expect(lines.map((line) => line.label)).toEqual(['click a', 'click b', 'click c']);
    expect(lines[0].page).toBe('/');
    expect(lines[2].page).toBeUndefined();
    expect(log.recent().map((entry) => entry.label)).toEqual(['click a', 'click b', 'click c']);
    expect(warnings[0]).toContain('Slow interaction: click a took 420 ms');
    expect(warnings[0]).toContain(file);
    await expect(log.record({ reports: 'nope' })).rejects.toThrow('reports must be an array');
    expect(await log.record({ reports: [] })).toBe(0);
  });

  it('bounds memory and batch size and survives a failed write', async () => {
    const warnings: string[] = [];
    const log = createDevLatencyLog({
      file: '/unwritable/log.jsonl',
      warn: (line) => warnings.push(line),
      append: () => Promise.reject(new Error('disk full')),
    });
    const many = Array.from({ length: 80 }, (_, index) => report(`click ${index}`));
    expect(await log.record({ reports: many })).toBe(50);
    for (let round = 0; round < 4; round += 1) await log.record({ reports: many });
    expect(log.recent()).toHaveLength(200);
    expect(warnings.some((line) => line.includes('could not write /unwritable/log.jsonl: Error: disk full'))).toBe(
      true,
    );
    // A later write still runs after a failure.
    expect(await log.record({ reports: [report('after')] })).toBe(1);
    expect(log.recent().at(-1)?.label).toBe('after');
  });
});

describe('dev bridge latency route', () => {
  const app = (dev: boolean, recorded: unknown[] = []) =>
    createDevApp(
      dev,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      {
        file: '/tmp/dev-latency.jsonl',
        record: (body: unknown) => {
          recorded.push(body);
          if (!(body as { reports?: unknown }).reports) return Promise.reject(new Error('reports must be an array'));
          return Promise.resolve(1);
        },
        recent: () => [{ ...report('click a'), page: '/' }],
      },
    );

  it('records posted reports and lists recent ones', async () => {
    const recorded: unknown[] = [];
    const posted = await app(true, recorded).request('/__hotsheet/dev/latency', {
      method: 'POST',
      body: JSON.stringify({ reports: [report('click a')] }),
      headers: { 'content-type': 'application/json' },
    });
    expect(posted.status).toBe(200);
    expect(await posted.json()).toEqual({ accepted: 1 });
    expect(recorded).toHaveLength(1);
    const listed = await app(true).request('/__hotsheet/dev/latency');
    expect(await listed.json()).toEqual({
      file: '/tmp/dev-latency.jsonl',
      reports: [{ ...report('click a'), page: '/' }],
    });
  });

  it('rejects malformed bodies and is absent outside dev', async () => {
    const bad = await app(true).request('/__hotsheet/dev/latency', { method: 'POST', body: '{}' });
    expect(bad.status).toBe(400);
    expect(await bad.json()).toEqual({ error: 'reports must be an array' });
    expect((await app(false).request('/__hotsheet/dev/latency')).status).toBe(404);
    expect((await app(false).request('/__hotsheet/dev/latency', { method: 'POST', body: '{}' })).status).toBe(404);
  });

  it('adds correlation and bridge stages only to a development project API response', async () => {
    const registry = projectSessionRegistry(),
      project = 'timing-fixture-project',
      fetchMock = vi
        .spyOn(globalThis, 'fetch')
        .mockResolvedValue(Response.json({ requests: [] }, { headers: { 'server-timing': 'handler;dur=0.5' } }));
    registry.set(project, { url: 'http://127.0.0.1:19999', secret: 'private-fixture', storeIds: [] });
    try {
      const path = `/__hotsheet/project-api/${project}/permissions`;
      const response = await app(true).request(path);
      expect(response.status).toBe(200);
      expect(await response.json()).toEqual({ requests: [] });
      expect(response.headers.get('server-timing')).toMatch(
        /handler;dur=0\.5, bridge;dur=[\d.]+, bridge_queue;dur=[\d.]+, upstream;dur=[\d.]+/,
      );
      expect(response.headers.get('x-hotsheet-trace-id')).toMatch(/^[\da-f-]{36}$/);
      expect(response.headers.get('server-timing')).not.toContain('private-fixture');
      expect((await app(false).request(path)).status).toBe(404);
    } finally {
      fetchMock.mockRestore();
      registry.delete(project);
    }
  });
});
