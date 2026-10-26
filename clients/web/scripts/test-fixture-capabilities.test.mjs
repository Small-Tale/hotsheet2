import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { assertFixtureCli, assertFixtureServer } from './test-fixture-capabilities.mjs';

describe('HS2-SFVWN5 unattended credential boundary', () => {
  it('checks the CLI native capability without assuming permission-vault activation', () => {
    expect(() =>
      assertFixtureCli('fixture', () => ({ status: 0, stdout: '{"isolated_native_credentials":true}' })),
    ).not.toThrow();
    expect(() =>
      assertFixtureCli('fixture', () => ({ status: 0, stdout: '{"isolated_native_credentials":false}' })),
    ).toThrow(/HS2-SFVWN5/);
  });
  it('accepts only the actual compiled isolated capability with bounded execution', () => {
    expect(() =>
      assertFixtureServer('fixture', (binary, args, options) => {
        expect(binary).toBe('fixture');
        expect(args).toEqual(['--test-fixture-capabilities']);
        expect(options.timeout).toBe(5000);
        return { status: 0, stdout: '{"isolated_permission_vault":true,"isolated_native_credentials":true}' };
      }),
    ).not.toThrow();
    for (const result of [
      { status: 0, stdout: '{"isolated_permission_vault":false}' },
      { status: 0, stdout: '{}' },
      { status: 0, stdout: '{"isolated_permission_vault":true}' },
      { status: 1, stdout: '' },
      { status: null, error: new Error('deadline') },
    ])
      expect(() => assertFixtureServer('fixture', () => result)).toThrow(/Temporary HOTSHEET_HOME/);
  });

  it('pins isolated builds and preflight before real browser fixture mutation', () => {
    const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
    const scripts = JSON.parse(read('../package.json')).scripts;
    for (const name of [
      'desktop:test',
      'stress:scale',
      'test:e2e',
      'test:real-world-performance',
      'profile:pty:real',
      'profile:worker:real',
      'profile:pty:firefox',
    ])
      expect(scripts[name]).toContain('--features hotsheet-aitools/test-permission-vault');
    expect(read('../../../scripts/test-quiet.mjs')).toContain('"--features", "hotsheet-aitools/test-permission-vault"');
    expect(read('./vitest-build-binaries.mjs')).toContain("'hotsheet-aitools/test-permission-vault'");
    const harness = read('../tests/real-ticket-server.ts');
    expect(harness.indexOf('assertFixtureServer(binary)')).toBeLessThan(harness.indexOf('await mkdtemp'));
    expect(scripts['desktop:test:native']).toContain('HOTSHEET_RUN_NATIVE_CREDENTIAL_TESTS=1');
    expect(read('./desktop-host-smoke.mjs')).toContain('assertFixtureServer(server)');
    expect(read('./scale-stress.mjs')).toContain('assertFixtureServer(server)');
    expect(read('./change-stream-worker-profile.mjs')).toContain('assertFixtureServer(binaries.HOTSHEET_SERVER_BIN)');
    expect(read('./local-host.e2e.test.mjs')).toContain('assertFixtureServer(workingTreeBinaries.HOTSHEET_SERVER_BIN)');
    const stable = read('./stable-dev.e2e.test.mjs');
    expect(stable).toContain('assertFixtureServer(fixtureBinaries.HOTSHEET_SERVER_BIN)');
    expect(stable).toContain('...fixtureBinaries,');
    expect(stable).toContain('HOTSHEET_HOME: fixtureHome');
    expect(stable).toContain('hotsheet-stable-fixture-home-');
    expect(read('../../../crates/hotsheet-cli/tests/cli.rs')).toContain('c["isolated_native_credentials"].as_bool()');
  });

  it('HS2-Q97FR7 documents the same isolated build the fixture refusal names', () => {
    const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
    const command =
      'cargo build -p hotsheet-cli -p hotsheet-server --bins --features hotsheet-aitools/test-permission-vault';
    const flat = (text) => text.replace(/'\s*\+\s*'/g, '').replace(/\s+/g, ' ');
    expect(flat(read('./test-fixture-capabilities.mjs'))).toContain(command);
    const docs = read('../../../docs/12-code-organization-and-testing.md'),
      start = docs.indexOf('### Real ticket-server browser regression'),
      section = docs.slice(start, docs.indexOf('\n### ', start + 1));
    expect(start).toBeGreaterThan(-1);
    expect(flat(section)).toContain(command);
    // Every server or CLI build this section tells a person to run carries the isolation feature.
    for (const build of flat(section).match(/cargo build [^`\n]*/g) ?? [])
      expect(build).toContain('--features hotsheet-aitools/test-permission-vault');
    expect(section).toContain('--test-fixture-capabilities');
    expect(section).toContain('desktop:test:native');
  });
});
