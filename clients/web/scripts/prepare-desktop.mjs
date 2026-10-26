// HS2-5R706Q: assemble the production web bridge and its own runtimes for Tauri.
// The generated resources are ignored; the checked-in sources remain the build authority.
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { build } from 'esbuild';
import { stageMigratorDependencies } from './stage-migrator-dependencies.mjs';

const web = fileURLToPath(new URL('..', import.meta.url));
const root = resolve(web, '../..');
const resources = resolve(web, 'src-tauri/resources');
const binaries = resolve(web, 'src-tauri/binaries');
const migrator = resolve(root, 'migrator');
const triple =
  process.platform === 'darwin' && process.arch === 'arm64'
    ? 'aarch64-apple-darwin'
    : process.platform === 'darwin' && process.arch === 'x64'
      ? 'x86_64-apple-darwin'
      : undefined;

function copyExecutable(source, destination) {
  if (!existsSync(source)) throw new Error(`Missing desktop runtime: ${source}`);
  copyFileSync(source, destination);
  chmodSync(destination, 0o755);
}

if (!existsSync(resolve(migrator, 'node_modules/@electric-sql/pglite')))
  throw new Error('Build the migrator first with `npm ci` from migrator/.');
if (!triple) throw new Error('The first Hot Sheet desktop package supports macOS arm64 and x64.');

rmSync(resources, { recursive: true, force: true });
rmSync(binaries, { recursive: true, force: true });
mkdirSync(binaries, { recursive: true });
mkdirSync(resolve(resources, 'dist-host'), { recursive: true });
mkdirSync(resolve(resources, 'migrator'), { recursive: true });
copyExecutable(process.execPath, resolve(binaries, `node-${triple}`));
for (const name of ['hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate', 'hotsheet-mcp'])
  copyExecutable(resolve(root, 'target/release', name), resolve(binaries, `${name}-${triple}`));
cpSync(resolve(web, 'dist'), resolve(resources, 'dist'), { recursive: true });
// The bridge checks these live template bytes against the CLI's embedded copy
// before refresh, so a packaged checkout needs the same source tree.
cpSync(resolve(root, 'plugins'), resolve(resources, 'plugins'), { recursive: true });
cpSync(resolve(migrator, 'src'), resolve(resources, 'migrator/src'), { recursive: true });
copyFileSync(resolve(migrator, 'package.json'), resolve(resources, 'migrator/package.json'));
stageMigratorDependencies(migrator, resolve(resources, 'migrator'));
await build({
  entryPoints: [resolve(web, 'src/local-host.ts')],
  outfile: resolve(resources, 'dist-host/local-host.mjs'),
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node22',
  legalComments: 'inline',
  // ws contains CommonJS dynamic requires of Node builtins. Give esbuild's
  // bundled ESM helper a real require in the packaged Node process.
  banner: { js: "import { createRequire } from 'node:module'; const require = createRequire(import.meta.url);" },
});
console.log('Prepared standalone Hot Sheet desktop resources.');
