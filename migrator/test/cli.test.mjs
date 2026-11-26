// HS2-6JNDRY: the exporter's command line rejects bad arguments with a usage error and a
// non-zero exit, and every failure prints an actionable message (never a silent exit).
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { PGlite } from '@electric-sql/pglite';
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { errorText, parseCliArgs, PROGRESS_VERSION, USAGE } from '../src/export.mjs';

const EXPORTER = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'export.mjs');

describe('parseCliArgs (HS2-6JNDRY)', () => {
  it('accepts the datadir before or after the options', () => {
    const expected = { hotsheetDir: 'p/.hotsheet', outPath: 'x.json', progressJson: true };
    expect(parseCliArgs(['p/.hotsheet', '--out', 'x.json', '--progress-json'])).toEqual(expected);
    expect(parseCliArgs(['--out', 'x.json', '--progress-json', 'p/.hotsheet'])).toEqual(expected);
    expect(parseCliArgs(['--out=x.json', 'p/.hotsheet', '--progress-json'])).toEqual(expected);
  });

  it('defaults the output file and progress mode', () => {
    expect(parseCliArgs(['p'])).toEqual({ hotsheetDir: 'p', outPath: 'hotsheet-export.json', progressJson: false });
  });

  it('takes a dash-leading output only in the explicit --out=<value> form, and a positional after --', () => {
    expect(parseCliArgs(['p', '--out=-odd.json']).outPath).toBe('-odd.json');
    expect(parseCliArgs(['--', '--odd dir']).hotsheetDir).toBe('--odd dir');
  });

  it('reports help', () => {
    expect(parseCliArgs(['--help'])).toEqual({ help: true });
    expect(parseCliArgs(['p', '-h'])).toEqual({ help: true });
  });

  it.each([
    [[], /Missing the path/],
    [['p', '--out'], /--out.*argument missing/],
    [['--out', 'p'], /Missing the path/],
    [['--out', '--progress-json', 'p'], /--out.*ambiguous/],
    [['p', '--out', '--progress-json'], /--out.*ambiguous/],
    [['p', '--out', '-x'], /--out.*ambiguous/],
    [['p', '--out='], /non-empty file path/],
    [['p', '--out', 'a.json', '--out', 'b.json'], /more than once/],
    [['p', '--bogus'], /Unknown option '--bogus'/],
    [['--bogus', 'p'], /Unknown option '--bogus'/],
    [['p', '-x'], /Unknown option '-x'/],
    [['p', '--progress-json=yes'], /does not take an argument/],
    [['a', 'b'], /Expected one .hotsheet directory, got 2: a, b/],
  ])('rejects %j', (args, message) => {
    const result = parseCliArgs(args);
    expect(Object.keys(result)).toEqual(['error']);
    expect(result.error).toMatch(message);
    expect(result.error).not.toContain('\n');
  });
});

describe('errorText (HS2-6JNDRY)', () => {
  it('never returns an empty message', () => {
    expect(errorText(new Error('boom'))).toBe('boom');
    expect(errorText(new Error(''))).toBe('Error');
    expect(errorText('plain')).toBe('plain');
    expect(errorText({})).toBe('export failed with an unknown error');
    expect(errorText(undefined)).toBe('export failed with an unknown error');
  });
});

describe('exporter command line (HS2-6JNDRY)', () => {
  let root, hs, cwd;

  beforeAll(async () => {
    root = mkdtempSync(join(tmpdir(), 'hs2-export-cli-'));
    hs = join(root, 'synthetic project', '.hotsheet');
    mkdirSync(hs, { recursive: true });
    const db = new PGlite(join(hs, 'db'));
    await db.exec(`
      create table tickets (
        id serial primary key, ticket_number text, title text not null, status text,
        created_at timestamptz, updated_at timestamptz
      );
      insert into tickets (ticket_number, title, status, created_at, updated_at) values
        ('SYN-1', 'Synthetic one', 'not_started', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
        ('SYN-2', 'Synthetic two', 'completed', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z');
    `);
    await db.close();
    writeFileSync(join(hs, 'settings.json'), JSON.stringify({ appName: 'Synthetic', ticketPrefix: 'SYN' }));
  }, 30000);

  afterAll(() => rmSync(root, { recursive: true, force: true }));

  // Each run gets an empty working directory so a default or stray output file is visible.
  const run = (args) => {
    cwd = mkdtempSync(join(root, 'cwd-'));
    return spawnSync(process.execPath, [EXPORTER, ...args], { cwd, encoding: 'utf8', timeout: 60000 });
  };

  it.each([
    ['no arguments', () => []],
    ['--out without a value after the datadir', () => [hs, '--out']],
    ['--out consuming the datadir', () => ['--out', hs]],
    ['--out followed by a flag', () => ['--out', '--progress-json', hs]],
    ['--out followed by a flag after the datadir', () => [hs, '--out', '--progress-json']],
    ['an empty --out', () => [hs, '--out=']],
    ['a repeated --out', () => [hs, '--out', 'a.json', '--out', 'b.json']],
    ['an unknown flag after the datadir', () => [hs, '--bogus']],
    ['an unknown flag before the datadir', () => ['--bogus', hs]],
    ['a value on --progress-json', () => [hs, '--progress-json=yes']],
    ['two datadirs', () => [hs, hs]],
  ])('exits 2 with a usage error for %s and writes nothing', (_label, args) => {
    const result = run(args());
    expect(result.status).toBe(2);
    const lines = result.stderr.trim().split('\n');
    expect(lines[0]).toMatch(/^hotsheet-export: \S/);
    expect(lines.at(-1)).toBe(USAGE);
    expect(result.stdout).toBe('');
    expect(readdirSync(cwd)).toEqual([]);
  });

  it('prints usage for --help and exits 0', () => {
    const result = run(['--help']);
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe(USAGE);
  });

  it('exits 1 with an actionable message when the datadir has no database', () => {
    const missing = join(root, 'missing', '.hotsheet');
    const result = run([missing]);
    expect(result.status).toBe(1);
    expect(result.stderr).toContain(`hotsheet-export: no HS1 database directory at ${join(missing, 'db')}`);
    expect(readdirSync(cwd)).toEqual([]);
  });

  it('reports the failure as a typed progress event too', () => {
    const missing = join(root, 'missing', '.hotsheet');
    const result = run(['--progress-json', missing]);
    expect(result.status).toBe(1);
    const events = result.stdout
      .trim()
      .split('\n')
      .map((line) => JSON.parse(line));
    expect(events.at(-1)).toEqual({
      version: PROGRESS_VERSION,
      phase: 'failed',
      error: expect.stringContaining('no HS1 database directory'),
    });
    expect(result.stderr).toContain('hotsheet-export: no HS1 database directory');
  });

  it('exits 1 for a damaged bundled-major datadir without fetching an engine (HS2-D0V2NB)', () => {
    const damaged = join(root, 'damaged', '.hotsheet');
    cpSync(hs, damaged, { recursive: true });
    writeFileSync(join(damaged, 'db', 'global', 'pg_control'), Buffer.alloc(8192, 7));
    const out = join(root, 'damaged.json');
    const result = run(['--progress-json', damaged, '--out', out]);
    expect(result.status).toBe(1);
    expect(result.stderr).toMatch(/hotsheet-export: could not open the HS1 database \(PG_VERSION 17\)/);
    expect(result.stderr).toContain('No engine was downloaded');
    const events = result.stdout
      .trim()
      .split('\n')
      .map((line) => JSON.parse(line));
    expect(events.map((event) => event.phase)).not.toContain('export_tickets');
    expect(events.at(-1)).toMatchObject({ phase: 'failed', error: expect.stringContaining('looks damaged') });
    expect(existsSync(out)).toBe(false);
  }, 60000);

  it('accepts a dash-leading datadir after --', () => {
    const result = run(['--', '--not-a-flag']);
    expect(result.status).toBe(1);
    expect(result.stderr).toContain(join('--not-a-flag', 'db'));
  });

  it.each([
    ['datadir first', (out) => [hs, '--out', out]],
    ['options first', (out) => ['--out', out, hs]],
    ['--out= form with progress', (out) => [`--out=${out}`, '--progress-json', hs]],
  ])(
    'exports with %s',
    (label, args) => {
      const out = join(root, `${label.replace(/\W+/g, '-')}.json`);
      const result = run(args(out));
      expect(result.stderr).toContain(`Wrote 2 ticket(s) to ${out}`);
      expect(result.status).toBe(0);
      const exported = JSON.parse(readFileSync(out, 'utf8'));
      expect(exported.tickets.map((ticket) => ticket.ticket_number).sort()).toEqual(['SYN-1', 'SYN-2']);
      expect(exported.project.name).toBe('Synthetic');
      expect(readdirSync(cwd)).toEqual([]);
      if (args(out).includes('--progress-json')) {
        const phases = result.stdout
          .trim()
          .split('\n')
          .map((line) => JSON.parse(line));
        expect(phases.every((event) => event.version === PROGRESS_VERSION)).toBe(true);
        expect(phases.at(-1).phase).toBe('write_export');
      } else {
        expect(result.stdout).toBe('');
      }
    },
    60000,
  );

  it('writes the default output file in the working directory', () => {
    const result = run([hs]);
    expect(result.status).toBe(0);
    expect(existsSync(join(cwd, 'hotsheet-export.json'))).toBe(true);
    expect(result.stderr).toContain('Wrote 2 ticket(s) to hotsheet-export.json');
  }, 60000);
});
