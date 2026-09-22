import { spawnSync } from 'node:child_process';
import { lstatSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function formattingTargets(cwd, stat = lstatSync) {
  const targets = ['.', '../../migrator', '../../spikes/kerf-webawesome', '../../docs', '../../.github'];
  const localConfig = '../../opencode.json';
  try {
    stat(resolve(cwd, localConfig));
    targets.push(localConfig);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  return targets;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2];
  if (!['--check', '--write'].includes(mode) || process.argv.length !== 3)
    throw new Error('Usage: node scripts/format-sources.mjs <--check|--write>');
  const result = spawnSync(
    process.execPath,
    [
      fileURLToPath(import.meta.resolve('prettier/bin/prettier.cjs')),
      '--config',
      '../../.prettierrc.json',
      '--ignore-path',
      '../../.prettierignore',
      mode,
      ...formattingTargets(process.cwd()),
    ],
    { stdio: 'inherit' },
  );
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}
