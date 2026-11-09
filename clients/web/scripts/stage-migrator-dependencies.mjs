// HS2-9XAJXR: only runtime packages belong in the signed desktop resources.
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, realpathSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep } from 'node:path';

const npmListArguments = ['ls', '--omit=dev', '--all', '--parseable'];

// HS2-YMXBD6: execFileSync cannot run Windows .cmd shims directly. An npm script exposes
// its JavaScript entry point; direct callers use the Windows command launcher.
export function npmListInvocation(platform = process.platform, environment = process.env, node = process.execPath) {
  if (platform !== 'win32') return { file: 'npm', args: npmListArguments };
  if (environment.npm_execpath?.toLowerCase().endsWith('.js'))
    return { file: node, args: [environment.npm_execpath, ...npmListArguments] };
  return { file: environment.ComSpec || 'cmd.exe', args: ['/d', '/s', '/c', `npm.cmd ${npmListArguments.join(' ')}`] };
}

export function stageMigratorDependencies(migrator, destination) {
  const nodeModules = realpathSync(resolve(migrator, 'node_modules'));
  const { file, args } = npmListInvocation();
  const installed = execFileSync(file, args, {
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
