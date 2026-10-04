/**
 * The one sanctioned `node:child_process` boundary in the web client (HS2-T1H6NP).
 *
 * Git hooks, `git rebase --exec`, and `git bisect run` export `GIT_DIR`, `GIT_WORK_TREE`,
 * `GIT_INDEX_FILE`, and friends. A `git` (or Hot Sheet binary) child started with them inherited
 * would operate on the *calling* repository instead of the store or checkout it names. Every spawn
 * in `src/` goes through these wrappers, which always remove {@link GIT_REPOSITORY_ENV_VARS} from the
 * passed or inherited environment; ESLint forbids importing `node:child_process` anywhere else.
 */
import {
  execFile as nodeExecFile,
  type ExecFileOptions,
  spawn as nodeSpawn,
  type SpawnOptions,
  spawnSync as nodeSpawnSync,
  type SpawnSyncOptions,
  type SpawnSyncReturns,
} from 'node:child_process';
import { promisify } from 'node:util';

/**
 * Repository-locating git environment variables no child process may inherit. Mirrors
 * `hotsheet_ticketing::git::REPOSITORY_ENV_VARS` in Rust and `scripts/repository-env.mjs` for test
 * helpers: exactly what `git rev-parse --local-env-vars` prints, plus namespace, discovery, and
 * quarantine variables. Identity (`GIT_AUTHOR_*`, `GIT_COMMITTER_*`) and global config selection
 * are kept.
 */
export const GIT_REPOSITORY_ENV_VARS: readonly string[] = Object.freeze([
  'GIT_ALTERNATE_OBJECT_DIRECTORIES',
  'GIT_CONFIG',
  'GIT_CONFIG_PARAMETERS',
  'GIT_CONFIG_COUNT',
  'GIT_OBJECT_DIRECTORY',
  'GIT_DIR',
  'GIT_WORK_TREE',
  'GIT_IMPLICIT_WORK_TREE',
  'GIT_GRAFT_FILE',
  'GIT_INDEX_FILE',
  'GIT_NO_REPLACE_OBJECTS',
  'GIT_REPLACE_REF_BASE',
  'GIT_PREFIX',
  'GIT_SHALLOW_FILE',
  'GIT_COMMON_DIR',
  'GIT_NAMESPACE',
  'GIT_CEILING_DIRECTORIES',
  'GIT_DISCOVERY_ACROSS_FILESYSTEM',
  'GIT_QUARANTINE_PATH',
]);

/** A copy of `env` (default `process.env`) without any repository-locating git variable. */
export function withoutGitRepositoryEnv(env: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  return Object.fromEntries(Object.entries(env).filter(([name]) => !GIT_REPOSITORY_ENV_VARS.includes(name)));
}

function isolated<T extends { env?: NodeJS.ProcessEnv }>(options: T | undefined): T {
  return { ...options, env: withoutGitRepositoryEnv(options?.env ?? process.env) } as T;
}

/**
 * `node:child_process` `spawn` with the repository-locating git variables removed. It accepts every
 * `spawn` call form (`(command)`, `(command, options)`, `(command, args)`,
 * `(command, args, options)`), so the overloaded Node signature, including its stdio-specific
 * return types, is kept.
 */
export const spawn = ((command: string, argsOrOptions?: readonly string[] | SpawnOptions, options?: SpawnOptions) =>
  Array.isArray(argsOrOptions)
    ? nodeSpawn(command, argsOrOptions as readonly string[], isolated(options))
    : nodeSpawn(command, isolated(argsOrOptions as SpawnOptions | undefined))) as typeof nodeSpawn;

/** `node:child_process` `spawnSync` with the repository-locating git variables removed. */
export function spawnSync(
  command: string,
  args: readonly string[],
  options?: SpawnSyncOptions,
): SpawnSyncReturns<string | Buffer> {
  return nodeSpawnSync(command, args, isolated(options));
}

const nodeExecFileAsync = promisify(nodeExecFile);

/**
 * Promisified `execFile` with the repository-locating git variables removed. Output decodes as
 * UTF-8 unless `options.encoding` says otherwise, matching `promisify(execFile)`.
 */
export async function execFileAsync(
  file: string,
  args: readonly string[],
  options?: ExecFileOptions,
): Promise<{ stdout: string; stderr: string }> {
  const { stdout, stderr } = await nodeExecFileAsync(file, args, { encoding: 'utf8', ...isolated(options) });
  return { stdout: stdout.toString(), stderr: stderr.toString() };
}
