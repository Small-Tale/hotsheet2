import { describe, expect, it } from 'vitest';

import { assessCompatibility, type ServerCompatibility } from './compatibility';

const server = (min: number, max: number, extra: Partial<ServerCompatibility> = {}): ServerCompatibility => ({ generation: 'hs2', protocol: { min, max }, ...extra });

describe('assessCompatibility', () => {
  it.each([
    [1, 1, 1, 1], [1, 2, 2, 3], [2, 4, 1, 2], [1, 5, 2, 3],
  ])('accepts intersecting inclusive ranges', (serverMin, serverMax, clientMin, clientMax) => {
    expect(assessCompatibility(server(serverMin, serverMax), { min: clientMin, max: clientMax }).kind).toBe('compatible');
  });

  it('distinguishes which side is too old', () => {
    expect(assessCompatibility(server(1, 2), { min: 3, max: 4 }).kind).toBe('server_too_old');
    expect(assessCompatibility(server(3, 4), { min: 1, max: 2 }).kind).toBe('client_too_old');
  });

  it.each([
    [undefined, 'did not provide'],
    [{ generation: 'hs1', protocol: { min: 1, max: 1 } }, 'Expected an HS2'],
    [{ generation: 'hs2' }, 'invalid protocol'],
    [{ generation: 'hs2', protocol: { min: 2, max: 1 } }, 'invalid protocol'],
  ])('degrades unavailable or invalid metadata to unknown', (value, detail) => {
    expect(assessCompatibility(value)).toMatchObject({ kind: 'unknown', detail: expect.stringContaining(detail) });
  });

  it('treats revision differences as warnings and requires both restart safeguards', () => {
    expect(assessCompatibility(server(1, 1, { build_revision: 'server' }), { min: 1, max: 1 }, 'client')).toMatchObject({ kind: 'compatible', revisionMismatch: true });
    expect(assessCompatibility(server(1, 1, { build_revision: 'built', source_revision: 'current' }))).toMatchObject({ kind: 'compatible', revisionMismatch: true, sourceStale: true });
    expect(assessCompatibility(server(1, 1, { source_stale: true }))).toMatchObject({ kind: 'compatible', revisionMismatch: true, sourceStale: true });
    expect(assessCompatibility(server(1, 1, { build_revision: 'release', source_revision: null, source_stale: false }))).toMatchObject({ kind: 'compatible', revisionMismatch: false, sourceStale: false });
    expect(assessCompatibility(server(1, 1, { capabilities: { lifecycle_restart: true } }), { min: 2, max: 2 }).canRestartServer).toBe(false);
    expect(assessCompatibility(server(1, 1, { capabilities: { lifecycle_restart: true, lifecycle_quiescence: true } }), { min: 2, max: 2 }).canRestartServer).toBe(true);
  });
});
