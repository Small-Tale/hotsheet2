import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';

const execute = promisify(execFile);
const fixtureCleanup = { recursive: true, force: true, maxRetries: 5, retryDelay: 50 } as const;

export interface RealTicketFixtureContext {
  store: string;
  root: string;
  env: NodeJS.ProcessEnv;
}

/** Real ticket storage, index, and HTTP mutations, isolated from the developer's registry. */
export async function realTicketServer(
  options: { seed?: (fixture: RealTicketFixtureContext) => Promise<void>; autoCommit?: boolean } = {},
) {
  const repo = fileURLToPath(new URL('../../../', import.meta.url)),
    suffix = process.platform === 'win32' ? '.exe' : '',
    cli = process.env.HOTSHEET_TEST_CLI_BIN ?? resolve(repo, `target/debug/hotsheet-cli${suffix}`),
    binary = process.env.HOTSHEET_TEST_SERVER_BIN ?? resolve(repo, `target/debug/hotsheet-server${suffix}`);
  if (!existsSync(cli) || !existsSync(binary))
    throw new Error(
      'Build the browser test binaries with npm run test:e2e, or cargo build -p hotsheet-cli -p hotsheet-server --bins.',
    );
  const directory = await mkdtemp(resolve(tmpdir(), 'hotsheet-browser-')),
    store = resolve(directory, 'tickets'),
    root = resolve(directory, 'project'),
    secret = 'isolated-browser-test',
    // An inherited GIT_DIR (git hook, `git bisect run`) must not redirect the isolated store
    // into the developer checkout (HS2-RRD417).
    env = {
      ...withoutGitRepositoryEnv(process.env),
      HOTSHEET_HOME: resolve(directory, 'home'),
      HOTSHEET_NO_AUTOCOMMIT: options.autoCommit ? undefined : '1',
      GIT_AUTHOR_NAME: 'Hot Sheet browser test',
      GIT_AUTHOR_EMAIL: 'browser@example.invalid',
      GIT_COMMITTER_NAME: 'Hot Sheet browser test',
      GIT_COMMITTER_EMAIL: 'browser@example.invalid',
    };
  await mkdir(root);
  try {
    await execute(cli, ['init', '--standalone', '--at', store, '--prefix', 'HS2'], { cwd: root, env });
    await options.seed?.({ store, root, env });
  } catch (error) {
    await rm(directory, fixtureCleanup);
    throw error;
  }
  // `--exit-on-stdin-eof` + a held stdin pipe: if this runner dies without reaching `stop()`
  // (SIGKILL, hard timeout, crash), the kernel closes the pipe and the server stops itself
  // instead of lingering as an orphan (HS2-VQ8ZWT).
  const child = spawn(
    binary,
    ['-C', store, '--bind', '127.0.0.1:0', '--secret', secret, '--no-terminal-broker', '--exit-on-stdin-eof'],
    { cwd: root, env, stdio: ['pipe', 'pipe', 'pipe'] },
  );
  let output = '';
  const stop = async () => {
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, 'exit');
      child.kill('SIGTERM');
      const forced = setTimeout(() => {
        child.kill('SIGKILL');
      }, 5000);
      try {
        await exited;
      } finally {
        clearTimeout(forced);
      }
    }
    await rm(directory, fixtureCleanup);
  };
  try {
    const url = await new Promise<string>((resolveReady, reject) => {
      const timer = setTimeout(() => {
          reject(new Error(`Ticket server did not start: ${output}`));
        }, 20_000),
        inspect = (chunk: Buffer) => {
          output = `${output}${chunk.toString()}`.slice(-16_384);
          const address = output.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
          if (address) {
            clearTimeout(timer);
            resolveReady(address);
          }
        };
      child.stdout.on('data', inspect);
      child.stderr.on('data', inspect);
      child.once('error', (error) => {
        clearTimeout(timer);
        reject(error);
      });
      child.once('exit', () => {
        clearTimeout(timer);
        reject(new Error(`Ticket server stopped: ${output}`));
      });
    });
    const request = async <T>(path: string, method = 'GET', body?: unknown): Promise<T> => {
      const response = await fetch(`${url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': secret },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
      if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${await response.text()}`);
      return response.json() as Promise<T>;
    };
    const checkout = await request<{ id: string }>('/checkouts', 'POST', { root, stores: [store] });
    return {
      url,
      secret,
      checkoutId: checkout.id,
      root,
      store,
      pid: child.pid,
      home: env.HOTSHEET_HOME,
      request,
      stop,
    };
  } catch (error) {
    await stop();
    throw error;
  }
}
