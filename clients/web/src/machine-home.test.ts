// HS2-N2HB7G: the Node bridges resolve the machine homes with the Rust `machine_home` rule.
// Every case injects the environment and the user home, so no test reads or writes a real home.
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { nonBlank as scriptNonBlank } from '../scripts/non-blank.mjs';
import { clientDiscoveryPath } from './client-discovery';
import { devLatencyLogPath } from './dev-latency-log';
import { hotsheetHome, hotsheetLegacyHome, nonBlank } from './machine-home';

const user = () => '/users/a';

describe('HS2-N2HB7G machine home resolution', () => {
  it('treats unset, empty, and whitespace-only values as unset', () => {
    expect(nonBlank(undefined)).toBeUndefined();
    for (const blank of ['', ' ', '\t', ' \n ']) {
      expect(nonBlank(blank)).toBeUndefined();
      expect(hotsheetHome({ HOTSHEET_HOME: blank }, user)).toBe(resolve('/users/a', '.hotsheet2'));
      expect(hotsheetLegacyHome({ HOTSHEET_LEGACY_HOME: blank }, user)).toBe(resolve('/users/a', '.hotsheet'));
    }
    expect(hotsheetHome({}, user)).toBe(resolve('/users/a', '.hotsheet2'));
    expect(hotsheetLegacyHome({}, user)).toBe(resolve('/users/a', '.hotsheet'));
  });

  it('HS2-MVF6X3 keeps the scripts/ twin of nonBlank identical to this one', () => {
    for (const value of [undefined, '', ' ', '\t', ' \n ', '\u00a0', 'relative/path', '/abs/path', ' padded ', '0'])
      expect(scriptNonBlank(value)).toBe(nonBlank(value));
  });

  it('uses a non-empty value as given, relative or absolute', () => {
    expect(hotsheetHome({ HOTSHEET_HOME: '/srv/hs2' }, user)).toBe('/srv/hs2');
    expect(hotsheetHome({ HOTSHEET_HOME: 'relative/home' }, user)).toBe('relative/home');
    expect(hotsheetHome({ HOTSHEET_HOME: ' padded ' }, user)).toBe(' padded ');
    expect(hotsheetLegacyHome({ HOTSHEET_LEGACY_HOME: '/srv/hs1' }, user)).toBe('/srv/hs1');
    expect(hotsheetLegacyHome({ HOTSHEET_LEGACY_HOME: 'relative/legacy' }, user)).toBe('relative/legacy');
  });

  it('falls back to the temp directory when the user home is blank', () => {
    for (const blank of ['', '  ']) {
      expect(hotsheetHome({ HOTSHEET_HOME: '' }, () => blank)).toBe(resolve(tmpdir(), '.hotsheet2'));
      expect(hotsheetLegacyHome({}, () => blank)).toBe(resolve(tmpdir(), '.hotsheet'));
    }
  });

  it('keeps each variable independent', () => {
    const environment = { HOTSHEET_HOME: '/h2', HOTSHEET_LEGACY_HOME: ' ' };
    expect(hotsheetHome(environment, user)).toBe('/h2');
    expect(hotsheetLegacyHome(environment, user)).toBe(resolve('/users/a', '.hotsheet'));
  });

  it('routes the bridges through the shared rule', () => {
    // A blank home used to resolve the latency log against the working directory.
    expect(devLatencyLogPath({ HOTSHEET_HOME: '   ' })).not.toBe(resolve('   ', 'logs', 'dev-latency.jsonl'));
    expect(devLatencyLogPath({ HOTSHEET_HOME: '   ' })).toBe(resolve(hotsheetHome({}), 'logs', 'dev-latency.jsonl'));
    // HS2-GVSJ5Y: a blank latency-log override counts as unset; any other value is used as given.
    for (const blank of ['', ' ', '\t\n '])
      expect(devLatencyLogPath({ HOTSHEET_HOME: '/h2', HOTSHEET_DEV_LATENCY_LOG: blank })).toBe(
        resolve('/h2', 'logs', 'dev-latency.jsonl'),
      );
    expect(devLatencyLogPath({ HOTSHEET_DEV_LATENCY_LOG: 'relative.jsonl' })).toBe('relative.jsonl');
    expect(devLatencyLogPath({ HOTSHEET_DEV_LATENCY_LOG: '/logs/latency.jsonl' })).toBe('/logs/latency.jsonl');
    // The vitest config isolates HOTSHEET_HOME, so the default is the per-run temporary home.
    expect(clientDiscoveryPath()).toBe(resolve(hotsheetHome(), 'client.json'));
  });
});
