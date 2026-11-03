// HS2-FYXRRM: the development signing bundle must import with macOS `security`. The
// maintainer's first run failed with "SecKeychainItemImport: The user name or passphrase you
// entered is not correct" (EC key, empty passphrase, Homebrew OpenSSL first on PATH).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const helper = fileURLToPath(new URL('./macos-dev-signing-identity.sh', import.meta.url));

test('HS2-FYXRRM development signing bundle imports into a keychain', { skip: process.platform !== 'darwin' }, () => {
  const directory = mkdtempSync(join(tmpdir(), 'hs2-fyxrrm-'));
  const keychain = join(directory, 'probe.keychain-db');
  try {
    // A Homebrew OpenSSL first on PATH must not change the bundle format.
    const created = spawnSync('bash', [helper, directory], {
      encoding: 'utf8',
      env: { ...process.env, PATH: `/opt/homebrew/bin:/usr/local/bin:${process.env.PATH ?? ''}` },
    });
    assert.equal(created.status, 0, created.stderr);
    const fingerprint = created.stdout.trim();
    assert.match(fingerprint, /^[0-9A-F]{40}$/i);
    assert.equal(statSync(join(directory, 'passphrase')).mode & 0o077, 0, 'passphrase is owner-only');
    assert.throws(() => statSync(join(directory, 'key.pem')), /ENOENT/, 'the plain private key is removed');
    const passphrase = readFileSync(join(directory, 'passphrase'), 'utf8').trim();
    assert.ok(passphrase.length >= 32, 'a non-empty random passphrase');

    const run = (args) => spawnSync('security', args, { encoding: 'utf8' });
    assert.equal(run(['create-keychain', '-p', 'probe', keychain]).status, 0);
    assert.equal(run(['unlock-keychain', '-p', 'probe', keychain]).status, 0);
    const imported = run([
      'import',
      join(directory, 'identity.p12'),
      '-k',
      keychain,
      '-f',
      'pkcs12',
      '-P',
      passphrase,
      '-x',
      '-T',
      '/usr/bin/codesign',
    ]);
    assert.equal(imported.status, 0, `${imported.stdout}${imported.stderr}`);
    const found = run(['find-certificate', '-a', '-Z', '-c', 'Hot Sheet Local Development Code Signing', keychain]);
    assert.equal(found.status, 0, found.stderr);
    assert.match(found.stdout, new RegExp(`SHA-1 hash: ${fingerprint}`, 'i'));
    const keys = run(['find-key', '-t', 'private', keychain]);
    // `find-key` is absent on some releases; when present the private key must be there.
    if (keys.status === 0) assert.match(keys.stdout, /keychain:/);
  } finally {
    spawnSync('security', ['delete-keychain', keychain]);
    rmSync(directory, { recursive: true, force: true });
  }
});
