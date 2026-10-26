import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { expect, it } from 'vitest';

import { stageMigratorDependencies } from './stage-migrator-dependencies.mjs';

it('HS2-9XAJXR stages transitive runtime packages without development binaries', () => {
  const root = mkdtempSync(join(tmpdir(), 'hotsheet-migrator-package-'));
  const migrator = join(root, 'migrator');
  const staged = join(root, 'staged');
  const runtime = join(migrator, 'node_modules/runtime');
  const nested = join(runtime, 'node_modules/nested');
  const dev = join(migrator, 'node_modules/build-tool');
  try {
    for (const directory of [migrator, runtime, nested, dev]) mkdirSync(directory, { recursive: true });
    writeFileSync(
      join(migrator, 'package.json'),
      JSON.stringify({
        name: 'fixture',
        version: '1.0.0',
        dependencies: { runtime: '1.0.0' },
        devDependencies: { 'build-tool': '1.0.0' },
      }),
    );
    writeFileSync(
      join(runtime, 'package.json'),
      JSON.stringify({ name: 'runtime', version: '1.0.0', dependencies: { nested: '1.0.0' } }),
    );
    writeFileSync(join(nested, 'package.json'), JSON.stringify({ name: 'nested', version: '1.0.0' }));
    writeFileSync(join(nested, 'index.js'), 'export const ready = true;\n');
    writeFileSync(join(dev, 'package.json'), JSON.stringify({ name: 'build-tool', version: '1.0.0' }));
    writeFileSync(join(dev, 'native.node'), 'build-only native binary');

    stageMigratorDependencies(migrator, staged);

    expect(existsSync(join(staged, 'node_modules/runtime/package.json'))).toBe(true);
    expect(existsSync(join(staged, 'node_modules/runtime/node_modules/nested/index.js'))).toBe(true);
    expect(existsSync(join(staged, 'node_modules/build-tool/native.node'))).toBe(false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
