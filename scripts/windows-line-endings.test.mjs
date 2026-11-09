// HS2-AHE5SV: Windows checkout conversion must preserve formatted text and the migrator bin shebang.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function git(cwd, ...args) {
  return execFileSync('git', args, {
    cwd,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

test('HS2-AHE5SV Windows autocrlf checkout keeps formatted text and executable shebangs LF', () => {
  const temporary = mkdtempSync(join(tmpdir(), 'hs2-windows-eol-'));
  const source = join(temporary, 'source');
  const checkout = join(temporary, 'checkout');
  const unprotected = join(temporary, 'unprotected');
  try {
    mkdirSync(source);
    git(source, 'init', '-q');
    git(source, 'config', 'user.name', 'Synthetic Test');
    git(source, 'config', 'user.email', 'synthetic@example.invalid');
    writeFileSync(join(source, '.gitattributes'), readFileSync(join(projectRoot, '.gitattributes')));
    mkdirSync(join(source, 'docs'));
    mkdirSync(join(source, 'migrator', 'src'), { recursive: true });
    writeFileSync(join(source, 'docs', 'sample.md'), '# Synthetic heading\n\nA line.\n');
    writeFileSync(join(source, 'migrator', 'src', 'export.mjs'), '#!/usr/bin/env node\nexport const value = 1;\n');
    writeFileSync(join(source, 'asset.bin'), Buffer.from([0, 13, 10, 255]));
    git(source, 'add', '.gitattributes', 'docs/sample.md', 'migrator/src/export.mjs', 'asset.bin');
    git(source, 'commit', '-qm', 'Synthetic fixture');

    git(temporary, 'clone', '-q', '-c', 'core.autocrlf=true', source, checkout);
    for (const relative of ['docs/sample.md', 'migrator/src/export.mjs']) {
      const bytes = readFileSync(join(checkout, relative));
      assert.equal(bytes.includes(Buffer.from('\r\n')), false, `${relative} must keep LF on Windows`);
      assert.equal(bytes.includes(Buffer.from('\n')), true, `${relative} needs line breaks`);
    }
    assert.equal(
      readFileSync(join(checkout, 'migrator', 'src', 'export.mjs'))
        .subarray(0, 20)
        .toString(),
      '#!/usr/bin/env node\n',
    );
    assert.deepEqual(readFileSync(join(checkout, 'asset.bin')), Buffer.from([0, 13, 10, 255]));

    git(source, 'rm', '-q', '.gitattributes');
    git(source, 'commit', '-qm', 'Remove checkout rule for control');
    git(temporary, 'clone', '-q', '-c', 'core.autocrlf=true', source, unprotected);
    assert.equal(
      readFileSync(join(unprotected, 'docs', 'sample.md')).includes(Buffer.from('\r\n')),
      true,
      'the control checkout must reproduce the Windows conversion that Prettier rejects',
    );
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
});
