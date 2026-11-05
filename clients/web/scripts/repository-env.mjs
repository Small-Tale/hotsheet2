/**
 * Repository-locating git environment variables a test helper must not pass to a child process
 * (HS2-RRD417).
 *
 * Git hooks, `git rebase --exec`, and `git bisect run` export `GIT_DIR`, `GIT_WORK_TREE`,
 * `GIT_INDEX_FILE`, and friends. A Hot Sheet binary or `git` started with them inherited would
 * operate on the *calling* repository instead of the isolated temp store the helper created. The
 * list mirrors `hotsheet_ticketing::git::REPOSITORY_ENV_VARS` in Rust: exactly what
 * `git rev-parse --local-env-vars` prints, plus namespace, discovery, and quarantine variables.
 * Identity (`GIT_AUTHOR_*`, `GIT_COMMITTER_*`) and global config selection are kept.
 */
export const GIT_REPOSITORY_ENV_VARS = Object.freeze([
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

/**
 * A copy of `env` without any repository-locating git variable.
 *
 * @param {Record<string, string | undefined>} env the environment to copy, usually `process.env`
 * @returns {Record<string, string | undefined>} the copy, safe to hand to a git-spawning child
 */
export function withoutGitRepositoryEnv(env) {
  const clean = { ...env };
  for (const name of GIT_REPOSITORY_ENV_VARS) Reflect.deleteProperty(clean, name);
  return clean;
}

/**
 * Remove every repository-locating git variable from `env` in place (HS2-KKDZF4).
 *
 * An entry point that git itself may start (the local gate under a `pre-push` hook) calls this
 * on `process.env` before it spawns anything, so every stage, test runner, and fixture it starts
 * locates repositories from its own working directory instead of the hook's `GIT_DIR`.
 *
 * @param {Record<string, string | undefined>} env the environment to clean, usually `process.env`
 * @returns {string[]} the names that were present and removed
 */
export function removeGitRepositoryEnv(env) {
  const removed = GIT_REPOSITORY_ENV_VARS.filter((name) => Object.hasOwn(env, name));
  for (const name of removed) Reflect.deleteProperty(env, name);
  return removed;
}
