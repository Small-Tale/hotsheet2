import { spawnSync } from 'node:child_process';

/**
 * The fixture web server's port for this Playwright run (HS2-6WH4PQ).
 *
 * An explicit HOTSHEET_WEB_TEST_PORT is honored. Otherwise the runner picks a free loopback port
 * once and records it in `env`, so the workers it starts later (which load this config again and
 * inherit the runner's environment) use the same port. Two concurrent runs in different worktrees
 * therefore never collide on a fixed default; HS2-BJ00D1 still proves readiness against this run's
 * own server.
 *
 * @param {NodeJS.ProcessEnv} env the runner's environment, updated with the chosen port
 * @param {() => number} pick returns a currently free loopback port
 * @returns {number}
 */
export function playwrightServerPort(env, pick = freeLoopbackPort) {
  const explicit = env.HOTSHEET_WEB_TEST_PORT;
  if (explicit !== undefined && explicit !== '') {
    const port = Number(explicit);
    if (!Number.isInteger(port) || port <= 0 || port > 65_535)
      throw new Error(`HOTSHEET_WEB_TEST_PORT must be a TCP port, not ${JSON.stringify(explicit)}`);
    return port;
  }
  const port = pick();
  env.HOTSHEET_WEB_TEST_PORT = String(port);
  return port;
}

/** Bind port 0 on loopback in a child process, which reports the port the OS assigned. */
export function freeLoopbackPort() {
  const result = spawnSync(
    process.execPath,
    [
      '-e',
      "const s=require('node:net').createServer().listen(0,'127.0.0.1',()=>{process.stdout.write(String(s.address().port));s.close()})",
    ],
    { encoding: 'utf8' },
  );
  const port = Number(result.stdout);
  if (result.status !== 0 || !Number.isInteger(port) || port <= 0)
    throw new Error(`Could not allocate a Playwright fixture port: ${result.stderr || result.stdout}`);
  return port;
}
