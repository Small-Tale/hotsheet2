import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
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
  question?: { question: string; tool_use_id: string; at: string; questions: unknown; agent?: string };
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
  // A stand-in `codex` on the hosting server's PATH: `codex queue` records its arguments under
  // the session's CODEX_HOME and confirms like codex-cli does (HS2-FKM97G).
  let fakeCodexBin = '';

  beforeAll(async () => {
    previousProbes = process.env.HOTSHEET_AI_TOOL_PROBES;
    process.env.HOTSHEET_AI_TOOL_PROBES = 'off';
    fakeCodexBin = await mkdtemp(join(tmpdir(), 'hs2-remote-parity-codex-'));
    await writeFile(
      join(fakeCodexBin, 'codex'),
      '#!/bin/sh\nprintf \'%s\\n\' "$@" > "$CODEX_HOME/queued.txt"\necho "Queued message m-1 for thread $2."\n',
    );
    await chmod(join(fakeCodexBin, 'codex'), 0o755);
    host = await realTicketServer({
      canonicalRoot: true,
      terminalBroker: true,
      env: { PATH: `${fakeCodexBin}:${process.env.PATH ?? ''}` },
    });
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
    await rm(fakeCodexBin, { recursive: true, force: true, maxRetries: 5 });
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

  /** A Codex PermissionRequest hook in `terminal`, answered on the hosting machine. */
  const answeredPermissionAsk = async (terminal: string, session: string, action: string) => {
    const asked = hook('/permissions/ask', 'POST', {
      project: `checkout:${host.checkoutId}`,
      connection: '',
      tool: 'Bash',
      action,
      agent: 'codex',
      terminal_id: terminal,
      session_id: session,
    });
    let pending: WirePermissionRequest | undefined;
    await expect
      .poll(async () => {
        pending = ((await (await hook('/permissions')).json()) as WirePermissionRequest[]).find(
          (item) => item.action === action,
        );
        return pending !== undefined;
      })
      .toBe(true);
    expect((await hook(`/permissions/${pending!.id}`, 'POST', { decision: 'allow', scope: 'once' })).status).toBe(200);
    expect((await asked).status).toBe(200);
  };

  for (const [index, tab] of (['local', 'remote'] as const).entries())
    it(`shows a Codex request_user_input_async question in a ${tab} tab and queues its answer`, async () => {
      const projectId = tab === 'local' ? localId : remoteId,
        terminal = `codex-question-${tab}`,
        session = `01a20000-0000-7000-8000-00000000000${index + 1}`,
        itemId = `msg_synthetic${index + 1}`;
      const codexHome = await mkdtemp(join(tmpdir(), 'hs2-remote-parity-codex-home-'));
      try {
        const launched = await browser(projectId, '/terminals', 'POST', {
          id: terminal,
          command: 'cat',
          ...(tab === 'remote' ? { cwd: projectId } : { cwd: host.root }),
        });
        expect(launched.status).toBe(200);
        // The Hot Sheet-launched Codex TUI's SessionStart hook, on the hosting machine.
        expect(
          (
            await hook(`/terminals/${terminal}/ai-connection`, 'POST', {
              agent: 'codex',
              session_id: session,
              codex_home: codexHome,
            })
          ).status,
        ).toBe(204);
        // The live repro: a permission hook run by hand inside the Codex terminal (it inherits the
        // terminal id) named another session id. That ask must not replace the session the TUI's
        // SessionStart reported, or its rollout watch stops and the question never appears.
        await answeredPermissionAsk(terminal, 'synthetic-hand-run-session', `echo replaced-${tab}`);
        const { cursor } = (await (await browser(projectId, '/ws/poll')).json()) as { cursor: number };
        // The session's rollout, in the record shapes codex-cli 0.171.0 writes (synthetic text).
        const day = join(codexHome, 'sessions', '2026', '11', '25');
        await mkdir(day, { recursive: true });
        const later = new Date(Date.now() + 2_000).toISOString();
        const rollout = join(day, `rollout-2026-11-25T06-00-00-${session}.jsonl`);
        const record = (value: unknown) => `${JSON.stringify(value)}\n`;
        await writeFile(
          rollout,
          record({
            timestamp: later,
            ordinal: 0,
            type: 'session_meta',
            payload: {
              id: session,
              session_id: session,
              originator: 'codex-tui',
              cli_version: '0.171.0',
              source: 'cli',
            },
          }) +
            record({
              timestamp: later,
              ordinal: 1,
              type: 'event_msg',
              payload: {
                type: 'item_completed',
                thread_id: session,
                turn_id: '01a20000-0000-7000-8000-0000000000aa',
                item: {
                  type: 'AgentMessage',
                  id: itemId,
                  content: [{ type: 'text', text: 'Synthetic question text.' }],
                  phase: 'final_answer',
                  delivery: 'async',
                  questions: [{ title: 'Synthetic choice?', options: ['Alpha', 'Beta'] }],
                },
                started_at_ms: 1,
                completed_at_ms: 1,
              },
            }),
        );

        // What the tab displays: the question in Claude's AskUserQuestion shape, marked Codex.
        await expect
          .poll(
            async () =>
              ((await (await browser(projectId, '/terminals')).json()) as TerminalView[]).find(
                (item) => item.id === terminal,
              )?.question?.tool_use_id,
            { timeout: 20_000 },
          )
          .toBe(itemId);
        const shown = ((await (await browser(projectId, '/terminals')).json()) as TerminalView[]).find(
          (item) => item.id === terminal,
        );
        expect(shown?.question).toMatchObject({
          question: 'Synthetic choice?',
          agent: 'codex',
          questions: [{ question: 'Synthetic choice?', options: [{ label: 'Alpha' }, { label: 'Beta' }] }],
        });
        const changes = (await (await browser(projectId, `/ws/poll?since=${cursor}`)).json()) as {
          events: Array<{ kind: string; id: string }>;
        };
        expect(changes.events).toContainEqual(expect.objectContaining({ kind: 'terminal_question', id: terminal }));
        // The asynchronous question leaves the turn running; a permission ask the same session
        // raises meanwhile keeps the question open.
        await answeredPermissionAsk(terminal, session, `echo still-asking-${tab}`);
        expect(
          ((await (await browser(projectId, `/terminals/${encodeURIComponent(terminal)}`)).json()) as TerminalView)
            .question?.tool_use_id,
        ).toBe(itemId);

        // The tab's answer is queued to the Codex session as user input.
        const answered = await browser(
          projectId,
          `/terminals/${encodeURIComponent(terminal)}/question/answer`,
          'POST',
          {
            tool_use_id: itemId,
            at: shown?.question?.at,
            answers: { 'Synthetic choice?': 'Beta' },
          },
        );
        expect(answered.status, await answered.clone().text()).toBe(204);
        expect(await readFile(join(codexHome, 'queued.txt'), 'utf8')).toBe(
          `queue\n--thread=${session}\n--message=Answer from Hot Sheet to your question:\n- Synthetic choice?: Beta\n`,
        );
        const after = (await (
          await browser(projectId, `/terminals/${encodeURIComponent(terminal)}`)
        ).json()) as TerminalView;
        expect(after.question).toBeUndefined();
      } finally {
        await browser(projectId, `/terminals/${encodeURIComponent(terminal)}`, 'DELETE');
        await rm(codexHome, { recursive: true, force: true, maxRetries: 5 });
      }
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
