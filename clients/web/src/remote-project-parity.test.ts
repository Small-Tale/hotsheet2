import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { realTicketServer } from '../tests/real-ticket-server';
import type { WirePermissionRequest } from './permission-notifications';
import { permissionBelongsToProject } from './permission-notifications';
import { openReviewedRemoteProject, projectSessionRegistry, proxyProjectRequest } from './project-bridge';
import { discoverRemoteProjects, privateRemoteConnection, type RemoteServerView } from './remote-projects/host';

// HS2-HSFVHC: remote parity for in-app AI question prompts. A real hosting server owns the
// terminal and its question; this machine's real bridge opens the checkout as a reviewed remote
// project and answers through the same proxied project API the browser uses. Only the two
// credential-store CLI reads (`remote checkouts` and `remote bridge-connection`, backed by the
// OS keychain) are replaced, with the session this test paired over HTTP.
vi.mock('./remote-projects/host', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./remote-projects/host')>()),
  discoverRemoteProjects: vi.fn(),
  privateRemoteConnection: vi.fn(),
}));

const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const PEER_ORIGIN = 'http://127.0.0.1:4999';

type Host = Awaited<ReturnType<typeof realTicketServer>>;

interface TerminalView {
  id: string;
  launch_cwd?: string;
  question?: { question: string; tool_use_id: string; at: string; questions: unknown };
}

/** Pair a read-write device with the hosting server the way a peer's host does (HS2-AW2AKF). */
async function pairReadWrite(host: Host): Promise<{ token: string; csrf: string }> {
  const pair = (await (
    await fetch(`${host.url}/remote-auth/pair`, {
      method: 'POST',
      headers: { origin: PEER_ORIGIN, 'content-type': 'application/json' },
      body: JSON.stringify({ label: 'Synthetic peer desktop' }),
    })
  ).json()) as { id: string; challenge: string };
  const approved = await fetch(`${host.url}/remote-auth/owner/approve`, {
    method: 'POST',
    headers: { 'x-hotsheet-secret': host.secret, 'content-type': 'application/json' },
    body: JSON.stringify({ id: pair.id, role: 'read_write' }),
  });
  expect(approved.status).toBe(204);
  const redeemed = await fetch(`${host.url}/remote-auth/redeem`, {
    method: 'POST',
    headers: { origin: PEER_ORIGIN, 'content-type': 'application/json' },
    body: JSON.stringify({ id: pair.id, challenge: pair.challenge }),
  });
  expect(redeemed.status).toBe(200);
  const cookies = redeemed.headers.getSetCookie().map((line) => line.split(';')[0]);
  const value = (name: string) => cookies.find((cookie) => cookie.startsWith(`${name}=`))?.slice(name.length + 1) ?? '';
  return { token: value('hs2_remote_session'), csrf: value('hs2_remote_csrf') };
}

describe('HS2-HSFVHC remote project parity through a real hosting server', { timeout: 120_000 }, () => {
  let host: Host;
  let bridgeHome = '';
  let previousHome: string | undefined;
  let previousProbes: string | undefined;
  let remoteId = '';
  let otherRemoteId = '';
  let otherCheckoutId = '';
  let localId = '';

  beforeAll(async () => {
    previousProbes = process.env.HOTSHEET_AI_TOOL_PROBES;
    process.env.HOTSHEET_AI_TOOL_PROBES = 'off';
    host = await realTicketServer({ canonicalRoot: true, terminalBroker: true });
    // This machine (the bridge) keeps its own home: only its reviewed-server registry lives here.
    previousHome = process.env.HOTSHEET_HOME;
    bridgeHome = await mkdtemp(join(tmpdir(), 'hs2-remote-parity-bridge-'));
    process.env.HOTSHEET_HOME = bridgeHome;
    const { token, csrf } = await pairReadWrite(host);
    const binding: RemoteServerView = {
      id: BINDING,
      label: 'Synthetic host',
      origin: host.url,
      identity: { server_id: 'synthetic-server', ca_fingerprint: null },
      revision: 1,
      revoked: false,
      last_failure: null,
      last_confirmed_checkouts: [],
      stale: false,
    };
    await mkdir(bridgeHome, { recursive: true });
    await writeFile(join(bridgeHome, 'remotes.json'), JSON.stringify({ servers: [binding] }));
    // A second checkout on the same host, open as its own remote tab, for project context.
    const otherRoot = join(dirname(host.root), 'other');
    await mkdir(otherRoot);
    otherCheckoutId = (
      await host.request<{ id: string }>('/checkouts', 'POST', { root: otherRoot, stores: [host.store] })
    ).id;
    vi.mocked(discoverRemoteProjects).mockResolvedValue({
      remote: binding,
      checkouts: [
        { id: host.checkoutId, name: 'Synthetic project', state: 'available', sources: [] },
        { id: otherCheckoutId, name: 'Other project', state: 'available', sources: [] },
      ],
    });
    vi.mocked(privateRemoteConnection).mockResolvedValue({
      binding,
      credentials: { bearer_secret: `session:${token}`, session_origin: PEER_ORIGIN, session_csrf: csrf },
    });
    remoteId = (await openReviewedRemoteProject(BINDING, host.checkoutId)).id;
    otherRemoteId = (await openReviewedRemoteProject(BINDING, otherCheckoutId)).id;
    // The same hosting server opened as a local tab (a local tab's id is its checkout id), so each
    // step runs against both paths.
    localId = host.checkoutId;
    projectSessionRegistry().set(localId, { url: host.url, secret: host.secret, root: host.root });
  });

  afterAll(async () => {
    projectSessionRegistry().delete(remoteId);
    projectSessionRegistry().delete(otherRemoteId);
    projectSessionRegistry().delete(localId);
    await host.stop();
    if (previousHome === undefined) delete process.env.HOTSHEET_HOME;
    else process.env.HOTSHEET_HOME = previousHome;
    if (previousProbes === undefined) delete process.env.HOTSHEET_AI_TOOL_PROBES;
    else process.env.HOTSHEET_AI_TOOL_PROBES = previousProbes;
    await rm(bridgeHome, { recursive: true, force: true, maxRetries: 5 });
  });

  /** A browser request through the bridge's project API, exactly as `Api(apiPath)` sends it. */
  const browser = (projectId: string, path: string, method = 'GET', body?: unknown) =>
    proxyProjectRequest(
      projectId,
      path,
      new Request(`http://bridge.invalid${path}`, {
        method,
        headers: { 'content-type': 'application/json' },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }),
    );

  /** The hosting machine's Claude hook, which talks to its own server with the machine secret. */
  const hook = (path: string, method = 'GET', body?: unknown) =>
    fetch(`${host.url}${path}`, {
      method,
      headers: { 'content-type': 'application/json', 'x-hotsheet-secret': host.secret },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });

  for (const tab of ['local', 'remote'] as const)
    it(`delivers an in-app question answer to the hosting server's hook from a ${tab} tab`, async () => {
      const projectId = tab === 'local' ? localId : remoteId,
        terminal = `question-${tab}`,
        toolUse = `tool-${tab}`,
        session = `session-${tab}`;
      const launched = await browser(projectId, '/terminals', 'POST', {
        id: terminal,
        command: 'cat',
        ...(tab === 'remote' ? { cwd: projectId } : { cwd: host.root }),
      });
      expect(launched.status).toBe(200);
      // The tab's change stream (through the same bridge) is what makes the prompt appear.
      const { cursor } = (await (await browser(projectId, '/ws/poll')).json()) as { cursor: number };
      const questions = [
        { question: 'Which path?', options: [{ label: 'Left' }, { label: 'Right' }] },
        { question: 'Why?', options: [] },
      ];
      expect(
        (
          await hook(`/terminals/${terminal}/question`, 'POST', {
            question: 'Which path?',
            tool_use_id: toolUse,
            session_id: session,
            questions,
          })
        ).status,
      ).toBe(204);

      const changes = (await (await browser(projectId, `/ws/poll?since=${cursor}`)).json()) as {
        events: Array<{ kind: string; id: string }>;
      };
      expect(changes.events).toContainEqual(expect.objectContaining({ kind: 'terminal_question', id: terminal }));

      // What the tab displays: its own terminal, addressed by the tab's root, with the live question.
      const listed = (await (await browser(projectId, '/terminals')).json()) as TerminalView[];
      const shown = listed.find((item) => item.id === terminal);
      expect(shown?.launch_cwd).toBe(tab === 'remote' ? remoteId : host.root);
      expect(shown?.question).toMatchObject({ question: 'Which path?', tool_use_id: toolUse, questions });
      const at = shown?.question?.at ?? '';

      const answers = { 'Which path?': 'Left', 'Why?': 'Shorter' };
      const answered = await browser(projectId, `/terminals/${encodeURIComponent(terminal)}/question/answer`, 'POST', {
        tool_use_id: toolUse,
        at,
        answers,
      });
      expect(answered.status, await answered.clone().text()).toBe(204);

      // The hook waiting on the hosting machine receives exactly the answer the tab sent.
      const polled = await hook(`/terminals/${terminal}/question?tool_use_id=${toolUse}&session_id=${session}`);
      expect(polled.status).toBe(200);
      expect(await polled.json()).toEqual({ answers });

      // "Answer in terminal" releases the hook to the native prompt from either tab too.
      expect(
        (
          await hook(`/terminals/${terminal}/question`, 'POST', {
            question: 'Again?',
            tool_use_id: `${toolUse}-2`,
            session_id: session,
            questions: [{ question: 'Again?', options: [] }],
          })
        ).status,
      ).toBe(204);
      const again = (
        (await (await browser(projectId, `/terminals/${encodeURIComponent(terminal)}`)).json()) as TerminalView
      ).question;
      expect(again?.tool_use_id).toBe(`${toolUse}-2`);
      const native = await browser(projectId, `/terminals/${encodeURIComponent(terminal)}/question/answer`, 'POST', {
        tool_use_id: `${toolUse}-2`,
        at: again?.at,
        native: true,
      });
      expect(native.status).toBe(204);
      expect(
        (await hook(`/terminals/${terminal}/question?tool_use_id=${toolUse}-2&session_id=${session}`)).status,
      ).toBe(409);
      expect((await browser(projectId, `/terminals/${encodeURIComponent(terminal)}`, 'DELETE')).ok).toBe(true);
    });

  for (const tab of ['local', 'remote'] as const)
    it(`shows a host permission request only in its own ${tab} project and returns the answer`, async () => {
      const projectId = tab === 'local' ? localId : remoteId,
        action = `echo synthetic-${tab}`;
      // The hosting machine's permission hook blocks on this request until someone answers.
      const asked = hook('/permissions/ask', 'POST', {
        project: `checkout:${host.checkoutId}`,
        connection: '',
        tool: 'Bash',
        action,
        agent: 'claude',
      }).then(async (response) => (await response.json()) as { decision: string });
      await expect
        .poll(async () =>
          ((await (await hook('/permissions')).json()) as WirePermissionRequest[]).some(
            (item) => item.action === action,
          ),
        )
        .toBe(true);

      // Every tab of this machine, in browser terms, as the app's permission inbox sees them.
      const tabProject = (id: string, root = id) => ({ id, root, name: id, apiPath: `/__hotsheet/project-api/${id}` });
      const projects =
        tab === 'local' ? [tabProject(localId, host.root)] : [tabProject(remoteId), tabProject(otherRemoteId)];
      for (const viewer of projects) {
        const listed = (await (await browser(viewer.id, '/permissions')).json()) as WirePermissionRequest[];
        const request = listed.find((item) => item.action === action);
        expect(request, viewer.id).toBeDefined();
        expect(permissionBelongsToProject(request!, [], projects, viewer.id), viewer.id).toBe(viewer.id === projectId);
      }

      const request = ((await (await browser(projectId, '/permissions')).json()) as WirePermissionRequest[]).find(
        (item) => item.action === action,
      )!;
      const resolved = await browser(projectId, `/permissions/${request.id}`, 'POST', {
        decision: 'allow',
        scope: 'once',
      });
      expect(resolved.status, await resolved.clone().text()).toBe(200);
      expect(await asked).toEqual({ decision: 'allow' });
    });
});
