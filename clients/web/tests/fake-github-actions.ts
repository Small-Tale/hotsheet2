import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

/**
 * HS2-8KV14Y: a loopback GitHub Actions REST fake for browser tests, so a real `job watch ci` (and the Job
 * monitor it starts) judges a commit's CI gate through the real HTTP client, as
 * `crates/hotsheet-cli/tests/job_ci_watch.rs` does. Point a Hot Sheet process at it with
 * `HOTSHEET_GITHUB_API_URL` and a synthetic `GH_TOKEN`. It answers the push-run list of each repository's
 * branch and the jobs of each run, with weak ETags and 304 for an unchanged conditional request; every
 * other path is 404 (no compare, no artifacts). Nothing reaches the network. Synthetic runs only.
 */
export const FAKE_GITHUB_TOKEN = 'synthetic-token-for-browser-tests';

export interface FakeRun {
  id: number;
  sha: string;
  status: 'queued' | 'in_progress' | 'completed';
  conclusion?: 'success' | 'failure' | 'cancelled';
}

export async function fakeGithubActions() {
  const runs = new Map<string, FakeRun[]>();
  const versions = new Map<string, number>();
  const requests: string[] = [];
  /** HS2-A7YCB2: repositories the provider does not let the token read: every request answers 404. */
  const unreadable = new Set<string>();
  const answer = (request: IncomingMessage, response: ServerResponse, key: string, body: unknown) => {
    const etag = `W/"${key}-v${versions.get(key) ?? 0}"`;
    if (request.headers['if-none-match'] === etag) {
      response.writeHead(304, { etag });
      response.end();
      return;
    }
    response.writeHead(200, { 'content-type': 'application/json', etag });
    response.end(JSON.stringify(body));
  };
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://fake.invalid');
    requests.push(`${request.method ?? 'GET'} ${url.pathname}`);
    const list = /^\/repos\/([^/]+\/[^/]+)\/actions\/runs$/.exec(url.pathname);
    const jobs = /^\/repos\/([^/]+\/[^/]+)\/actions\/runs\/(\d+)\/jobs$/.exec(url.pathname);
    const repository = (list ?? jobs)?.[1];
    if (repository && unreadable.has(repository)) {
      response.writeHead(404, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ message: 'Not Found' }));
      return;
    }
    if (request.method === 'GET' && list) {
      const repo = list[1];
      const branch = url.searchParams.get('branch') ?? 'main';
      const found = (runs.get(repo) ?? []).map((run) => ({
        id: run.id,
        name: 'CI',
        head_branch: branch,
        head_sha: run.sha,
        event: 'push',
        status: run.status,
        conclusion: run.conclusion ?? null,
        run_attempt: 1,
        html_url: `https://github.example.test/${repo}/actions/runs/${String(run.id)}`,
        created_at: '2026-11-17T00:00:00Z',
        updated_at: '2026-11-17T00:00:30Z',
        run_started_at: '2026-11-17T00:00:00Z',
      }));
      answer(request, response, repo, { total_count: found.length, workflow_runs: found });
      return;
    }
    if (request.method === 'GET' && jobs) {
      const run = (runs.get(jobs[1]) ?? []).find((candidate) => String(candidate.id) === jobs[2]);
      if (run) {
        answer(request, response, jobs[1], {
          total_count: 1,
          jobs: [
            {
              id: run.id * 10,
              name: 'test',
              status: run.status,
              conclusion: run.conclusion ?? null,
              steps: [{ name: 'unit', status: run.status, number: 1 }],
            },
          ],
        });
        return;
      }
    }
    response.writeHead(404, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ message: 'Not Found' }));
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${String(port)}`,
    /** Every request as `METHOD /path`, for asserting the client only read. */
    requests,
    /** The environment a Hot Sheet process needs to watch gates on this fake. */
    env(): NodeJS.ProcessEnv {
      return { HOTSHEET_GITHUB_API_URL: `http://127.0.0.1:${String(port)}`, GH_TOKEN: FAKE_GITHUB_TOKEN };
    },
    /** Report (or replace) one push run of `repo`. */
    run(repo: string, run: FakeRun) {
      runs.set(repo, [...(runs.get(repo) ?? []).filter((existing) => existing.id !== run.id), run]);
      versions.set(repo, (versions.get(repo) ?? 0) + 1);
    },
    /** HS2-A7YCB2: `repo` answers 404 (unreadable) or is readable again, as with credentials fixed. */
    readable(repo: string, readable: boolean) {
      if (readable) unreadable.delete(repo);
      else unreadable.add(repo);
    },
    close: () =>
      new Promise<void>((resolve) => {
        server.closeAllConnections();
        server.close(() => {
          resolve();
        });
      }),
  };
}
