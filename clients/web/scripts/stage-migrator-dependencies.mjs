// HS2-9XAJXR: only runtime packages belong in the signed desktop resources.
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, realpathSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep } from 'node:path';

export function stageMigratorDependencies(migrator, destination) {
  const nodeModules = realpathSync(resolve(migrator, 'node_modules'));
  const installed = execFileSync('npm', ['ls', '--omit=dev', '--all', '--parseable'], {
    cwd: migrator,
    encoding: 'utf8',
  })
    .trim()
    .split(/\r?\n/)
    .slice(1);
  if (installed.length === 0) throw new Error('The migrator has no installed production dependencies.');

  for (const installedPath of installed) {
    const source = realpathSync(installedPath);
    const path = relative(nodeModules, source);
    if (!path || path === '..' || path.startsWith(`..${sep}`) || isAbsolute(path))
      throw new Error(`Unexpected migrator dependency path: ${source}`);
    const target = resolve(destination, 'node_modules', path);
    mkdirSync(resolve(target, '..'), { recursive: true });
    cpSync(source, target, {
      recursive: true,
      dereference: true,
      filter: (entry) => entry === source || !relative(source, entry).split(sep).includes('node_modules'),
    });
  }
}
