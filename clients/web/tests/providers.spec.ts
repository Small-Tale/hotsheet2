import { existsSync, readFileSync } from 'node:fs';
import { basename } from 'node:path';

import { expect, type Locator, test } from '@playwright/test';

import type { ConversationMessage } from '../src/ai-conversation';
import type { FullTicket, MediaAnnotation, TicketRow } from '../src/api';
import type { ConversationExportPayload } from '../src/conversation-export';
import { expectResponsiveFeedbackRectangle, measureFeedbackRectangle } from './dev-review-performance';
import { realTicketServer } from './real-ticket-server';

test.use({ video: process.env.HOTSHEET_MEDIA_RECORD_VIDEO === '1' ? 'on' : 'off' });

const project = {
  id: 'demo-checkout',
  root: '/work/demo',
  name: 'demo',
  stores: ['/work/demo.hs2'],
  apiPath: '/__hotsheet/project-api/demo-checkout',
};
const row = {
  connection_id: 'git-local',
  native_id: '01',
  qualified_id: 'git-local:01',
  id: '01',
  slug: 'HS2-DEMO01',
  title: 'Use real project tickets',
  category: 'feature',
  priority: 'high',
  status: 'started',
  up_next: true,
  feedback_needed: false,
  tags: ['client'],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-08-30T00:00:00Z',
  updated_at: '2026-08-30T01:00:00Z',
};
const backlogRow = {
  ...row,
  native_id: '03',
  qualified_id: 'git-local:03',
  id: '03',
  slug: 'HS2-BACK01',
  title: 'Deferred backlog ticket',
  status: 'backlog',
  up_next: false,
};
const archiveRow = {
  ...row,
  native_id: '04',
  qualified_id: 'git-local:04',
  id: '04',
  slug: 'HS2-ARCH01',
  title: 'Archived ticket',
  status: 'archive',
  up_next: false,
};
const deletedRow = {
  ...row,
  native_id: '12',
  qualified_id: 'git-local:12',
  id: '12',
  slug: 'HS2-DEL001',
  title: 'Deleted ticket',
  status: 'deleted',
  up_next: false,
};
const movedRow = {
  ...row,
  native_id: '13',
  qualified_id: 'git-local:13',
  id: '13',
  slug: 'HS2-MOVED1',
  title: 'Moved ticket',
  status: 'moved',
  up_next: false,
};
const notStartedRow = {
  ...row,
  native_id: '05',
  qualified_id: 'git-local:05',
  id: '05',
  slug: 'HS2-NEXT01',
  title: 'Not started ticket',
  status: 'not_started',
  up_next: false,
};
const completedRow = {
  ...row,
  native_id: '06',
  qualified_id: 'git-local:06',
  id: '06',
  slug: 'HS2-DONE01',
  title: 'Completed ticket',
  status: 'completed',
  up_next: false,
};
const verifiedRow = {
  ...row,
  native_id: '07',
  qualified_id: 'git-local:07',
  id: '07',
  slug: 'HS2-VERIFY01',
  title: 'Verified ticket',
  status: 'verified',
  up_next: false,
};
const startedRow2 = {
  ...row,
  native_id: '08',
  qualified_id: 'git-local:08',
  id: '08',
  slug: 'HS2-START02',
  title: 'Second started ticket',
  status: 'started',
  up_next: false,
};
const startedRow3 = {
  ...row,
  native_id: '09',
  qualified_id: 'git-local:09',
  id: '09',
  slug: 'HS2-START03',
  title: 'Third started ticket',
  status: 'started',
  up_next: false,
};
const searchSlugRow = {
  ...row,
  native_id: '10',
  qualified_id: 'git-local:10',
  id: '10',
  slug: 'HS2-QQRY00',
  title: 'Parser boundary bug',
  status: 'started',
  up_next: false,
};
const searchDetailsRow = {
  ...row,
  native_id: '11',
  qualified_id: 'git-local:11',
  id: '11',
  slug: 'HS2-SHG7YS',
  title: 'Unrelated visible title',
  status: 'started',
  up_next: false,
};
const full = {
  ...row,
  details: 'The real ticket body. [Project guide](/docs/project-guide)',
  blocked_reason: null,
  concurrency_token: 'token',
  notes: [
    {
      id: 'N1',
      kind: 'activity',
      created_at: '2026-08-30T00:30:00Z',
      edited_at: '2026-08-30T00:30:00Z',
      summary: 'Connected project client',
      text: 'Connected the client\nLoaded checkout-scoped tickets.\n\n<!-- hotsheet:activity-distillation:v1:demo -->',
    },
    {
      id: 'N2',
      kind: 'feedback_needed',
      created_at: '2026-08-30T00:35:00Z',
      edited_at: '2026-08-30T00:35:00Z',
      text: 'Should this reader preserve the current draft?',
    },
    {
      id: 'N3',
      kind: 'regular',
      created_at: '2026-08-30T00:36:00Z',
      edited_at: '2026-08-30T00:36:00Z',
      text: 'Editable note with [runbook](/docs/runbook)\n\nEvidence: attachment:proof.png.\n\nFollowing content after image.',
    },
  ],
  attachments: [{ id: 'A1', filename: 'proof.png', created_at: '2026-08-30T00:40:00Z' }],
};
const aiToolCatalog = [
  {
    id: 'codex',
    display_name: 'Codex',
    models: [
      { id: 'gpt-6-astra', label: 'GPT-6 Astra', effort_levels: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'] },
      { id: 'gpt-5.6-sol', label: 'GPT-5.6 Sol', effort_levels: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'] },
      {
        id: 'gpt-5.6-terra',
        label: 'GPT-5.6 Terra',
        effort_levels: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
      },
      { id: 'gpt-5.6-luna', label: 'GPT-5.6 Luna', effort_levels: ['low', 'medium', 'high', 'xhigh', 'max'] },
      { id: 'gpt-5.5', label: 'GPT-5.5', effort_levels: ['low', 'medium', 'high', 'xhigh'] },
      { id: 'gpt-5.3-codex-spark', label: 'GPT-5.3 Codex Spark', effort_levels: ['low', 'medium', 'high', 'xhigh'] },
    ],
    default_model: 'gpt-6-astra',
    default_effort: 'medium',
    actions: ['change_model', 'change_effort'],
  },
  {
    id: 'claude',
    display_name: 'Claude',
    models: [
      { id: 'fable', label: 'Fable', effort_levels: ['low', 'medium', 'high', 'xhigh', 'max'] },
      { id: 'opus', label: 'Opus', effort_levels: ['low', 'medium', 'high', 'xhigh', 'max'] },
      { id: 'sonnet', label: 'Sonnet', effort_levels: ['low', 'medium', 'high', 'xhigh', 'max'] },
      { id: 'haiku', label: 'Haiku' },
    ],
    default_model: 'sonnet',
    default_effort: 'medium',
    actions: ['change_model', 'change_effort'],
  },
];

function normalizedCreatedTicket(body: Record<string, unknown>) {
  const original = (typeof body.title === 'string' ? body.title : '').trim();
  let title = original,
    tags = Array.isArray(body.tags) ? body.tags.filter((tag): tag is string => typeof tag === 'string') : [];
  if (original.startsWith('\\[')) title = original.slice(1);
  else {
    let rest = original;
    const found: string[] = [];
    while (rest.startsWith('[')) {
      const close = rest.indexOf(']'),
        content = close < 0 ? '' : rest.slice(1, close);
      if (close < 0 || !content.trim() || content.includes('[')) break;
      found.push(content.trim().replaceAll(/\s+/g, '-'));
      rest = rest.slice(close + 1).trimStart();
    }
    if (found.length && rest) {
      title = rest.trim();
      tags = [...new Set([...tags, ...found])];
    }
  }
  return { title, tags };
}

async function resolvedColor(locator: Locator, value: string) {
  return locator.evaluate((node, color) => {
    const probe = document.createElement('span');
    probe.style.color = color;
    node.append(probe);
    const resolved = getComputedStyle(probe).color;
    probe.remove();
    return resolved;
  }, value);
}

async function captureInspectorStatus(surface: Locator, path: string) {
  const clip = await surface.locator('.ticket-inspector__status-field').evaluate((node) => {
    const field = node.getBoundingClientRect(),
      inspector = node.closest('[data-component="ticket-inspector"]')!.getBoundingClientRect();
    return { x: inspector.left, y: field.top - 8, width: inspector.width, height: field.height + 16 };
  });
  await surface.page().screenshot({ path, clip });
}

async function mockProject(
  page: import('@playwright/test').Page,
  canUpdate = true,
  primaryFeedbackNeeded: boolean | 'choices' | 'details' = false,
  ticketLoadDelay = 0,
  batchResponseDelay = 0,
  patchResponseDelay = 0,
  emptyAtFirst = false,
  terminalCount = 2,
  eventDuringBatch = false,
  hs1Migration = false,
  atomicBatch = true,
  stoppedTerminal = false,
) {
  let rows: TicketRow[] = [
    { ...row, feedback_needed: Boolean(primaryFeedbackNeeded) },
    backlogRow,
    archiveRow,
    deletedRow,
    movedRow,
    notStartedRow,
    completedRow,
    verifiedRow,
    startedRow2,
    startedRow3,
    searchSlugRow,
    searchDetailsRow,
  ];
  let selectedFull =
    primaryFeedbackNeeded === 'details'
      ? {
          ...full,
          feedback_needed: true,
          details:
            'FEEDBACK NEEDED: Which implementation?\n\nCHOICE:\n- Keep the **current behavior**\n- Use `attachment:proof.png`\n\nExplain another direction if needed.',
          notes: [full.notes[0]],
        }
      : primaryFeedbackNeeded
        ? {
            ...full,
            feedback_needed: true,
            notes: [
              ...full.notes,
              {
                id: 'N4',
                kind: 'feedback_needed',
                created_at: '2026-08-30T00:37:00Z',
                edited_at: '2026-08-30T00:37:00Z',
                text:
                  primaryFeedbackNeeded === 'choices'
                    ? 'Which direction should I implement?\n\nCHOICE:\n- Keep the **current behavior**\n- Use `attachment:proof.png`\n- Defer this change\n\nYou can also explain another direction.'
                    : 'FEEDBACK NEEDED\n\nHello there\n\n1. Something\n2. Another thing',
              },
            ],
          }
        : { ...full, feedback_needed: false };
  const evidenceByTicket = new Map<string, Array<{ id: string; filename: string; created_at: string }>>();
  const patches: Record<string, unknown>[] = [];
  let commandDefinitions = [{ id: 'check', title: 'Run checks', program: '/usr/bin/true', args: [], group: 'Quality' }];
  let terminalSettings = { inherit_global_shell_history: false };
  let trashSettings = { trash_cleanup_days: 30 };
  let aiSettings = { tool: 'codex', model: 'gpt-6-astra', effort: 'medium' };
  let commandRuns: Array<{
    id: string;
    command_id: string;
    state: 'running' | 'completed' | 'failed' | 'cancelled';
    exit_code?: number;
    output: Array<{ seq: number; stream: string; text: string }>;
  }> = [];
  type MockToolConnection = {
    id: string;
    tool: string;
    project: string;
    role: 'main';
    busy: boolean;
    actions: Array<'send_turn' | 'interrupt'>;
    session_id?: string;
    model?: string;
    effort?: string;
  };
  type MockDriveEvent = {
    cursor: number;
    store: string;
    kind: string;
    id: string;
    slug: string;
    message?: string;
    activity?: {
      id: string;
      ts: string;
      tool: string;
      session?: string;
      kind: string;
      summary: string;
      importance: 'low' | 'normal' | 'high';
    };
    turn?: { connection_id: string; event: Record<string, unknown> };
  };
  let toolConnections: MockToolConnection[] = [],
    driveCursor = 0,
    driveEvents: MockDriveEvent[] = [],
    drivePermissions: Array<{
      id: number;
      connection: string;
      tool: string;
      action: string;
      always_allow_supported: boolean;
    }> = [];
  const pushDriveEvent = (event: Omit<MockDriveEvent, 'cursor' | 'store'>) => {
    driveCursor += 1;
    driveEvents.push({ cursor: driveCursor, store: '/work/demo.hs2', ...event });
  };
  const replayLastDriveEvent = () => {
    const event = driveEvents.at(-1);
    if (event) driveEvents.push({ ...event });
  };
  const driveEvent = (connection: MockToolConnection) => {
    pushDriveEvent({ kind: 'drive_updated', id: connection.id, slug: connection.tool });
  };
  const turnEvent = (connection: MockToolConnection, event: Record<string, unknown>) => {
    pushDriveEvent({
      kind: 'turn_event',
      id: connection.id,
      slug: connection.tool,
      turn: { connection_id: connection.id, event },
    });
  };
  let createdTerminal = false;
  const closedTerminals = new Set<string>();
  let folderChoice = 0;
  let ticketSourceConfigured = !emptyAtFirst;
  let gitStores = ticketSourceConfigured ? [...project.stores] : [];
  let providerConnectionRecords: Array<{
    id: string;
    provider: string;
    locator: string;
    name: string | null;
    default: boolean;
    settings: Record<string, unknown>;
  }> = [];
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open') {
      const opened = hs1Migration
        ? {
            ...project,
            needsHs1Migration: true,
            needsTicketSetup: true,
            hs1SourcePath: '/work/demo/.hotsheet',
            hs1DatabasePath: '/work/demo/.hotsheet/db',
            hs1PostgresVersion: '17',
          }
        : project;
      return route.fulfill({
        status: 201,
        json: ticketSourceConfigured ? opened : { ...opened, stores: [], needsTicketSetup: true },
      });
    }
    if (path === '/__hotsheet/projects/setup-git' && request.method() === 'POST') {
      const body = request.postDataJSON(),
        ticketStore = body.location ?? '/work/demo.hs2';
      ticketSourceConfigured = true;
      return route.fulfill({ status: 201, json: { ticketStore, connectionId: `git-${gitStores.length + 1}` } });
    }
    if (path === '/__hotsheet/projects/setup-git-remote' && request.method() === 'POST')
      return route.fulfill({ json: { connected: true } });
    if (path === '/__hotsheet/folders/choose' && request.method() === 'POST')
      return route.fulfill({ json: { path: ['/picked/project', '/picked/tickets.hs2'][folderChoice++] } });
    if (path.endsWith('/provider-connections') && request.method() === 'GET')
      return route.fulfill({ json: providerConnectionRecords });
    if (path.endsWith('/provider-connections') && request.method() === 'POST') {
      const created = request.postDataJSON();
      providerConnectionRecords = [...providerConnectionRecords, created];
      return route.fulfill({ status: 201, json: created });
    }
    if (path.endsWith('/github-auth/device') && request.method() === 'POST')
      return route.fulfill({
        status: 202,
        json: {
          session_id: 'auth-1',
          user_code: 'ABCD-EFGH',
          verification_uri: 'https://github.test/login/device',
          expires_in: 900,
        },
      });
    if (path.endsWith('/github-auth/device/auth-1') && request.method() === 'GET') {
      await new Promise((resolve) => setTimeout(resolve, 100));
      return route.fulfill({ json: { state: 'authorized', credential_reference: 'github-app-auth-1' } });
    }
    if (path.endsWith('/github-auth/device/auth-1/repositories') && request.method() === 'GET')
      return route.fulfill({ json: { repositories: ['small-tale/hotsheet2', 'small-tale/secondary'] } });
    if (path.endsWith('/github-auth/device/auth-1') && request.method() === 'DELETE')
      return route.fulfill({ status: 204 });
    const providerConnection = path.match(/\/provider-connections\/([^/]+)$/);
    if (providerConnection && request.method() === 'PATCH') {
      const id = decodeURIComponent(providerConnection[1]),
        updated = { ...request.postDataJSON(), id };
      providerConnectionRecords = providerConnectionRecords.map((item) => (item.id === id ? updated : item));
      return route.fulfill({ json: updated });
    }
    if (path.includes('/sources/') && request.method() === 'PUT') {
      const body = request.postDataJSON();
      ticketSourceConfigured = true;
      if (body.provider === 'git' && !gitStores.includes(body.locator)) gitStores = [...gitStores, body.locator];
      return route.fulfill({
        json: {
          id: 'demo-checkout',
          root: '/work/demo',
          alias: 'demo',
          stores: gitStores,
          sources: [
            ...gitStores.map((locator, index) => ({ connection_id: `git-${index + 1}`, provider: 'git', locator })),
            ...providerConnectionRecords.map((connection) => ({
              connection_id: connection.id,
              provider: connection.provider,
              locator: connection.locator,
            })),
          ],
          default_source: body.make_default ? path.split('/').pop() : undefined,
        },
      });
    }
    if (path.endsWith('/default-source') && request.method() === 'PUT')
      return route.fulfill({
        json: {
          id: 'demo-checkout',
          root: '/work/demo',
          alias: 'demo',
          stores: [],
          sources: providerConnectionRecords.map((connection) => ({
            connection_id: connection.id,
            provider: connection.provider,
            locator: connection.locator,
          })),
          default_source: request.postDataJSON().connection_id,
        },
      });
    if (path.endsWith('/providers')) {
      const capabilities = {
        create: true,
        update: canUpdate,
        close: true,
        notes: true,
        note_edit: canUpdate,
        note_delete: canUpdate,
        attachments: true,
        assignment: true,
        review_requests: true,
        dependencies: true,
        up_next: true,
        close_reasons: true,
        claims: true,
        atomic_batch: atomicBatch,
        not_working_report: canUpdate,
        offline_mutation: true,
        history: true,
        watch: true,
        provider_idempotency: true,
        query_fields: [],
      };
      return route.fulfill({
        json: providerConnectionRecords.length
          ? providerConnectionRecords.map((connection) => ({
              connection_id: connection.id,
              provider: connection.provider,
              display_name: connection.name ?? connection.id,
              locator: connection.locator,
              default: connection.default,
              capabilities,
            }))
          : ticketSourceConfigured
            ? [
                {
                  connection_id: 'git-local',
                  provider: 'git',
                  display_name: 'Hot Sheet git',
                  locator: '/tickets',
                  default: true,
                  capabilities,
                },
              ]
            : [],
      });
    }
    if (path.endsWith('/permissions') && request.method() === 'GET') return route.fulfill({ json: drivePermissions });
    if (path.endsWith('/ai-tools') && request.method() === 'GET') return route.fulfill({ json: aiToolCatalog });
    if (path.endsWith('/ai-settings') && request.method() === 'GET') return route.fulfill({ json: aiSettings });
    if (path.endsWith('/ai-settings') && request.method() === 'PUT') {
      aiSettings = request.postDataJSON();
      return route.fulfill({ json: aiSettings });
    }
    const drivePermission = path.match(/\/permissions\/(\d+)$/);
    if (drivePermission && request.method() === 'POST') {
      const id = Number(drivePermission[1]),
        body = request.postDataJSON(),
        item = drivePermissions.find((value) => value.id === id);
      drivePermissions = drivePermissions.filter((value) => value.id !== id);
      pushDriveEvent({
        kind: 'permission_resolved',
        id: String(id),
        slug: item?.tool ?? 'tool',
        message: `${body.decision}:${body.scope}`,
      });
      if (item?.action === 'date ordering check') {
        const current = toolConnections.find((value) => value.id === item.connection)!;
        turnEvent(current, {
          type: 'output',
          content: "It's 3:23 PM according to your system clock.",
          truncated: false,
        });
        const idle = { ...current, busy: false, session_id: 'ordered-thread' };
        toolConnections = toolConnections.map((value) => (value.id === item.connection ? idle : value));
        turnEvent(idle, { type: 'done', reason: 'completed' });
        driveEvent(idle);
      }
      return route.fulfill({
        json: {
          connection: item?.connection ?? 'hotsheet-sidebar',
          decision: body.decision,
          persisted: body.scope === 'always',
        },
      });
    }
    if (path.endsWith('/connections') && request.method() === 'GET') return route.fulfill({ json: toolConnections });
    if (path.endsWith('/drive/sessions') && request.method() === 'GET') return route.fulfill({ json: [] });
    if (path.endsWith('/drive/connections') && request.method() === 'POST') {
      const body = request.postDataJSON(),
        created: MockToolConnection = {
          id: body.connection_id ?? 'generated',
          tool: body.tool,
          project: '/work/demo.hs2',
          role: 'main',
          busy: false,
          actions: ['send_turn', 'interrupt'],
          ...(body.session_id ? { session_id: body.session_id } : {}),
          ...(body.model ? { model: body.model } : {}),
          ...(body.effort ? { effort: body.effort } : {}),
        };
      toolConnections = [...toolConnections.filter((item) => item.id !== created.id), created];
      driveEvent(created);
      return route.fulfill({ status: 201, json: created });
    }
    const driveTurn = path.match(/\/drive\/connections\/([^/]+)\/turns$/);
    if (driveTurn && request.method() === 'POST') {
      const id = decodeURIComponent(driveTurn[1]),
        current = toolConnections.find((item) => item.id === id)!,
        body = request.postDataJSON(),
        running = { ...current, busy: true };
      toolConnections = toolConnections.map((item) => (item.id === id ? running : item));
      if (body.content === 'background-output-only') {
        for (let index = 0; index < 20; index += 1)
          turnEvent(running, { type: 'output', content: `background event ${index}`, truncated: false });
        return route.fulfill({ status: 202, json: running });
      }
      if (body.content === 'Export the referenced proof.') {
        turnEvent(running, {
          type: 'output',
          content: 'The referenced proof is ready.',
          truncated: false,
          files: [
            {
              id: 'proof-image',
              filename: 'proof.png',
              mime_type: 'image/png',
              kind: 'media',
              url: '/conversation-files/proof.png',
            },
          ],
        });
        const idle = { ...running, busy: false };
        toolConnections = toolConnections.map((item) => (item.id === id ? idle : item));
        turnEvent(idle, { type: 'done', reason: 'completed' });
        driveEvent(idle);
        return route.fulfill({ status: 202, json: idle });
      }
      driveEvent(running);
      if (body.content === 'Check message ordering across a permission pause.') {
        pushDriveEvent({
          kind: 'activity',
          id: 'activity-ordering',
          slug: '',
          activity: {
            id: 'activity-ordering',
            ts: '2026-09-14T07:23:40Z',
            tool: 'codex',
            session: running.session_id,
            kind: 'command',
            summary: 'codex ran `date`',
            importance: 'normal',
          },
        });
        drivePermissions = [
          { id: 73, connection: id, tool: 'Bash', action: 'date ordering check', always_allow_supported: true },
        ];
        pushDriveEvent({
          kind: 'permission_asked',
          id: '73',
          slug: 'Bash',
          turn: { connection_id: id, event: { type: 'permission_asked', tool: 'Bash', summary: 'Run date' } },
        });
        return route.fulfill({ status: 202, json: running });
      }
      if (body.content === 'What time is it in California?') {
        turnEvent(running, {
          type: 'output',
          content: 'I found the relevant client boundary. **The event stream remains authoritative.**',
          truncated: false,
        });
        replayLastDriveEvent();
        const idle = { ...running, busy: false, session_id: 'claude-thread-1' };
        toolConnections = toolConnections.map((item) => (item.id === id ? idle : item));
        turnEvent(idle, { type: 'done', reason: 'completed' });
        driveEvent(idle);
        return route.fulfill({ status: 202, json: idle });
      }
      if (body.content !== '$hotsheet') {
        turnEvent(running, {
          type: 'output',
          content: 'I found the relevant client boundary. **The event stream remains authoritative.**',
          truncated: false,
        });
        turnEvent(running, { type: 'usage', model: 'codex-5.6', tokens_in: 12_000, tokens_out: 840, cost_usd: 0.0423 });
        pushDriveEvent({
          kind: 'activity',
          id: 'activity-1',
          slug: '',
          activity: {
            id: 'activity-1',
            ts: '2026-09-08T00:00:00Z',
            tool: 'codex',
            session: running.session_id,
            kind: 'edit',
            summary: 'Edited the conversation projection',
            importance: 'normal',
          },
        });
        drivePermissions = [
          { id: 42, connection: id, tool: 'Bash', action: 'npm run test', always_allow_supported: true },
        ];
        pushDriveEvent({
          kind: 'permission_asked',
          id: '42',
          slug: 'Bash',
          turn: {
            connection_id: id,
            event: { type: 'permission_asked', tool: 'Bash', summary: 'Run the focused test' },
          },
        });
      }
      return route.fulfill({ status: 202, json: running });
    }
    const driveInterrupt = path.match(/\/drive\/connections\/([^/]+)\/interrupt$/);
    if (driveInterrupt && request.method() === 'POST') {
      const id = decodeURIComponent(driveInterrupt[1]),
        current = toolConnections.find((item) => item.id === id)!,
        accepted = { ...current, busy: true },
        idle = { ...current, busy: false, session_id: 'thread-1' };
      toolConnections = toolConnections.map((item) => (item.id === id ? idle : item));
      turnEvent(idle, { type: 'done', reason: 'interrupted' });
      driveEvent(idle);
      return route.fulfill({ status: 202, json: accepted });
    }
    const driveDelete = path.match(/\/checkouts\/[^/]+\/drive\/connections\/([^/]+)$/);
    if (driveDelete && request.method() === 'DELETE') {
      const id = decodeURIComponent(driveDelete[1]);
      toolConnections = toolConnections.filter((item) => item.id !== id);
      drivePermissions = drivePermissions.filter((item) => item.connection !== id);
      return route.fulfill({ status: 204 });
    }
    if (path.endsWith('/commands') && request.method() === 'GET') return route.fulfill({ json: commandDefinitions });
    if (path.endsWith('/commands') && request.method() === 'PUT') {
      commandDefinitions = request.postDataJSON();
      return route.fulfill({ json: commandDefinitions });
    }
    if (path.endsWith('/terminal-settings') && request.method() === 'GET')
      return route.fulfill({ json: terminalSettings });
    if (path.endsWith('/terminal-settings') && request.method() === 'PUT') {
      terminalSettings = request.postDataJSON();
      return route.fulfill({ json: terminalSettings });
    }
    if (path.endsWith('/trash-settings') && request.method() === 'GET') return route.fulfill({ json: trashSettings });
    if (path.endsWith('/trash-settings') && request.method() === 'PUT') {
      const next = request.postDataJSON();
      if (!Number.isSafeInteger(next.trash_cleanup_days) || next.trash_cleanup_days < 1)
        return route.fulfill({ status: 400, json: { error: 'must be a positive whole number of days' } });
      trashSettings = next;
      return route.fulfill({ json: trashSettings });
    }
    if (path.endsWith('/command-runs') && request.method() === 'GET') return route.fulfill({ json: commandRuns });
    if (path.endsWith('/terminals') && request.method() === 'POST') {
      createdTerminal = true;
      return route.fulfill({ json: { id: 'terminal-new', alive: true, busy: false, cwd: '/work/demo' } });
    }
    if (path.endsWith('/terminals') && request.method() === 'GET') {
      const terminals = Array.from({ length: terminalCount }, (_, index) =>
        index === 0
          ? { id: 'codex-main', alive: true, busy: true, cwd: '/work/demo', progress: 68 }
          : index === 1
            ? { id: 'tests', alive: !stoppedTerminal, busy: false, cwd: '/work/demo' }
            : {
                id: `worker-${String(index + 1).padStart(2, '0')}`,
                alive: true,
                busy: index % 3 === 0,
                cwd: '/work/demo',
              },
      );
      return route.fulfill({
        json: [
          ...terminals,
          ...(createdTerminal ? [{ id: 'terminal-new', alive: true, busy: false, cwd: '/work/demo' }] : []),
        ].filter((item) => !closedTerminals.has(item.id)),
      });
    }
    const terminalDelete = path.match(/\/terminals\/([^/]+)$/);
    if (terminalDelete && request.method() === 'DELETE') {
      closedTerminals.add(decodeURIComponent(terminalDelete[1]));
      return route.fulfill({ status: 204 });
    }
    if (path.endsWith('/terminals/codex-main') && request.method() === 'GET')
      return route.fulfill({
        json: {
          id: 'codex-main',
          alive: true,
          busy: true,
          cwd: '/work/demo',
          progress: 68,
          scrollback: 'Implementing terminal dashboard\nRunning browser checks…',
        },
      });
    if (path.endsWith('/terminals/tests') && request.method() === 'GET')
      return route.fulfill({
        json: {
          id: 'tests',
          alive: true,
          busy: false,
          cwd: '/work/demo',
          scrollback: 'Test Files 42 passed\nWaiting for changes.',
        },
      });
    if (path.endsWith('/terminals/terminal-new') && request.method() === 'GET')
      return route.fulfill({
        json: { id: 'terminal-new', alive: true, busy: false, cwd: '/work/demo', scrollback: 'New project terminal' },
      });
    const commandStart = path.match(/\/commands\/([^/]+)\/run$/);
    if (commandStart && request.method() === 'POST') {
      const run = { id: 'run-1', command_id: commandStart[1], state: 'running' as const, output: [] };
      commandRuns = [run, ...commandRuns];
      return route.fulfill({ status: 202, json: run });
    }
    const commandCancel = path.match(/\/command-runs\/([^/]+)\/cancel$/);
    if (commandCancel && request.method() === 'POST') {
      const run = {
        ...commandRuns.find((item) => item.id === commandCancel[1])!,
        state: 'cancelled' as const,
        output: [{ seq: 1, stream: 'stdout', text: 'Stopped by user' }],
      };
      commandRuns = commandRuns.map((item) => (item.id === run.id ? run : item));
      return route.fulfill({ json: run });
    }
    const commandRun = path.match(/\/command-runs\/([^/]+)$/);
    if (commandRun && request.method() === 'GET')
      return route.fulfill({ json: commandRuns.find((item) => item.id === commandRun[1]) });
    if (path.endsWith('/ws/poll') && request.method() === 'GET') {
      const rawSince = url.searchParams.get('since');
      if (rawSince === null) return route.fulfill({ json: { cursor: driveCursor, events: [], overflow: false } });
      const since = Number(rawSince),
        respond = () => {
          const events = driveEvents.filter((event) => event.cursor > since);
          if (events.length) driveEvents = [];
          return { cursor: events.length ? driveCursor : since, events, overflow: false };
        };
      const immediate = respond();
      if (immediate.events.length) return route.fulfill({ json: immediate });
      await new Promise((resolve) => setTimeout(resolve, 1_000));
      return route.fulfill({ json: respond() });
    }
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: {
          branch: 'main',
          ahead: 1,
          behind: 0,
          staged: 0,
          unstaged: 1,
          untracked: 0,
          conflicted: 0,
          clean: false,
        },
      });
    if (path.endsWith('/tickets/01/code-review') && request.method() === 'GET')
      return route.fulfill({
        json: {
          difftool: 'Glassbox',
          truncated: false,
          summary: { files: { total: 9, docs: 2, tests: 3, source: 3, other: 1 }, tests_added: 2, tests_modified: 1 },
          files: [
            { path: 'docs/06-clients.md', change: 'modified', category: 'docs' },
            { path: 'clients/web/src/change-evidence.test.ts', change: 'added', category: 'tests' },
            { path: 'clients/web/src/main.tsx', change: 'modified', category: 'source' },
            { path: 'Cargo.toml', change: 'modified', category: 'other' },
          ],
          ranges: [
            { from: 'aaa1111', to: 'bbb2222', count: 2 },
            { from: 'ccc3333', to: 'ddd4444', count: 2 },
          ],
          commits: [
            {
              sha: 'ddd4444',
              short_sha: 'ddd4444',
              subject: 'docs(workflow): require follow-up UI evidence',
              body: 'Refs: HS2-DEMO01',
              committed_at: '2026-09-02T10:00:00Z',
            },
            {
              sha: 'ccc3333',
              short_sha: 'ccc3333',
              subject: 'HS2-DEMO01: start the later review bundle',
              committed_at: '2026-09-02T09:00:00Z',
            },
            {
              sha: 'bbb2222',
              short_sha: 'bbb2222',
              subject: 'HS2-DEMO01: finish the responsive review segment',
              committed_at: '2026-09-02T08:00:00Z',
            },
            {
              sha: 'aaa1111',
              short_sha: 'aaa1111',
              subject: 'HS2-DEMO01: discover associated commits',
              committed_at: '2026-09-02T07:00:00Z',
            },
          ],
        },
      });
    if (path.endsWith('/tickets/01/code-review') && request.method() === 'POST') {
      patches.push({ operation: 'code-review', ...request.postDataJSON() });
      return route.fulfill({ status: 204 });
    }
    if (path.endsWith('/corrupt-tickets') && request.method() === 'GET') return route.fulfill({ json: [] });
    if (path.endsWith('/batch') && request.method() === 'POST') {
      const updates = request.postDataJSON().updates as Array<Record<string, unknown> & { id: string }>;
      for (const update of updates)
        patches.push(Object.fromEntries(Object.entries(update).filter(([key]) => key !== 'id')));
      if (eventDuringBatch)
        for (const { id } of updates)
          pushDriveEvent({ kind: 'ticket_updated', id, slug: rows.find((item) => item.id === id)?.slug ?? id });
      if (batchResponseDelay) await new Promise((resolve) => setTimeout(resolve, batchResponseDelay));
      rows = rows.map((item) => {
        const update = updates.find((value) => value.id === item.id);
        return update ? { ...item, ...update, updated_at: '2026-08-30T02:00:00Z' } : item;
      });
      const changed = updates.map(({ id }) => {
        const ticket = rows.find((item) => item.id === id)!;
        return {
          store: 'git-local',
          ...ticket,
          details: '',
          blocked_reason: null,
          notes: [],
          attachments: evidenceByTicket.get(id) ?? [],
          concurrency_token: `next-${id}`,
        };
      });
      return route.fulfill({ json: changed });
    }
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const pageSize = url.searchParams.get('page_size');
      if (!ticketSourceConfigured)
        return route.fulfill({
          json: pageSize
            ? {
                items: [],
                counts: {
                  total: 0,
                  queued: 0,
                  backlog: 0,
                  archive: 0,
                  trash: 0,
                  open: 0,
                  up_next: 0,
                  active: 0,
                  started: 0,
                  completed_today: 0,
                },
              }
            : [],
        });
      const text = url.searchParams.get('text');
      let result =
        text === 'QQRY00' || text === 'HS2-QQRY00'
          ? [
              { ...searchSlugRow, details: '', notes: [] },
              { ...searchDetailsRow, details: 'Related work references HS2-QQRY00.', notes: [] },
            ]
          : text === 'HS2-ARCH01'
            ? [{ ...archiveRow, details: 'Archived exact-slug result.', notes: [] }]
            : rows;
      if (text === 'QQRY00' || text === 'HS2-QQRY00') await new Promise((resolve) => setTimeout(resolve, 150));
      const collection = url.searchParams.get('collection'),
        status = url.searchParams.get('status'),
        archived = (item: TicketRow) => ['archive', 'moved'].includes(item.status ?? ''),
        trashed = (item: TicketRow) => item.status === 'deleted',
        queued = (item: TicketRow) => item.status !== 'backlog' && !archived(item) && !trashed(item);
      if (collection === 'queue') result = result.filter(queued);
      if (collection === 'archive') result = result.filter(archived);
      if (collection === 'trash') result = result.filter(trashed);
      if (status) result = result.filter((item) => item.status === status);
      if (!pageSize) return route.fulfill({ json: result });
      const size = Number(pageSize),
        offset = Number(url.searchParams.get('cursor') ?? 0),
        items = result.slice(offset, offset + size),
        next = offset + size < result.length ? String(offset + size) : undefined,
        now = Date.now();
      const counts = {
        total: rows.length,
        queued: rows.filter(queued).length,
        backlog: rows.filter((item) => item.status === 'backlog').length,
        archive: rows.filter(archived).length,
        trash: rows.filter(trashed).length,
        open: rows.filter((item) => ['not_started', 'started'].includes(item.status ?? 'not_started')).length,
        up_next: rows.filter(
          (item) => item.up_next && ['not_started', 'started'].includes(item.status ?? 'not_started'),
        ).length,
        active: rows.filter((item) => item.claim_lease_expires_at && Date.parse(item.claim_lease_expires_at) > now)
          .length,
        started: rows.filter((item) => item.status === 'started').length,
        completed_today: 0,
      };
      return route.fulfill({ json: { items, counts, ...(next ? { next_cursor: next } : {}) } });
    }
    if (path.endsWith('/trash/empty') && request.method() === 'POST') {
      const purged = rows.filter((item) => item.status === 'deleted'),
        tickets = purged.map((item) => item.slug);
      rows = rows.filter((item) => item.status !== 'deleted');
      return route.fulfill({ json: { purged: purged.length, tickets } });
    }
    // Mirrors POST /checkouts/{reference}/tickets/{id}/restore: flattened ticket + store, 409 outside Trash.
    const restoreTicket = path.match(/\/tickets\/([^/]+)\/restore$/);
    if (restoreTicket && request.method() === 'POST') {
      const id = decodeURIComponent(restoreTicket[1]),
        current = rows.find((item) => item.id === id);
      if (!current) return route.fulfill({ status: 404, json: { error: `no ticket matching '${id}'` } });
      if (current.status !== 'deleted')
        return route.fulfill({
          status: 409,
          json: { error: `only a ticket in Trash can be restored (found ${current.status})` },
        });
      const restored = { ...current, status: 'not_started', updated_at: '2026-08-30T03:00:00Z' };
      rows = rows.map((item) => (item.id === id ? restored : item));
      return route.fulfill({ json: { ...restored, details: '', notes: [], attachments: [], store: '/work/demo.hs2' } });
    }
    if (path.endsWith('/tickets') && request.method() === 'POST') {
      const body = request.postDataJSON(),
        normalized = normalizedCreatedTicket(body);
      const created = {
        ...row,
        id: '02',
        native_id: '02',
        slug: 'HS2-NEW001',
        title: normalized.title,
        tags: normalized.tags,
        category: body.category,
        status: body.status ?? 'not_started',
        up_next: Boolean(body.up_next),
        created_at: '2026-08-30T02:00:00Z',
        updated_at: '2026-08-30T02:00:00Z',
      };
      rows = [created, ...rows];
      return route.fulfill({
        status: 201,
        json: { ...created, details: body.details ?? '', notes: [], attachments: [] },
      });
    }
    if (path.endsWith('/provider-attachments/copy') && request.method() === 'POST') {
      const destination = rows.find((item) => item.native_id === request.postDataJSON().destination.native_id)!;
      return route.fulfill({
        status: 201,
        json: {
          ...destination,
          details: '',
          notes: [],
          attachments: [{ id: 'A-COPY', filename: 'proof.png', created_at: '2026-08-30T01:15:00Z' }],
        },
      });
    }
    if (path.endsWith('/tickets/01') && request.method() === 'GET') {
      if (ticketLoadDelay) await new Promise((resolve) => setTimeout(resolve, ticketLoadDelay));
      return route.fulfill({ json: { store: 'git-local', ...selectedFull } });
    }
    const attachmentMatch = path.match(/\/tickets\/([^/]+)\/attachments$/);
    if (attachmentMatch && attachmentMatch[1] !== '01' && request.method() === 'POST') {
      const id = attachmentMatch[1],
        items = evidenceByTicket.get(id) ?? [],
        filename = decodeURIComponent(request.headers()['x-hotsheet-filename'] ?? 'attachment'),
        attachment = { id: `E${items.length + 1}`, filename, created_at: '2026-08-30T01:10:00Z' },
        next = [...items, attachment];
      evidenceByTicket.set(id, next);
      const ticket = rows.find((item) => item.id === id)!;
      return route.fulfill({
        status: 201,
        json: { store: 'git-local', ...ticket, details: '', blocked_reason: null, notes: [], attachments: next },
      });
    }
    const attachmentDeleteMatch = path.match(/\/tickets\/([^/]+)\/attachments\/([^/]+)$/);
    if (attachmentDeleteMatch && attachmentDeleteMatch[1] !== '01' && request.method() === 'DELETE') {
      const [, id, attachmentId] = attachmentDeleteMatch,
        next = (evidenceByTicket.get(id) ?? []).filter((item) => item.id !== attachmentId);
      evidenceByTicket.set(id, next);
      const ticket = rows.find((item) => item.id === id)!;
      return route.fulfill({
        json: { store: 'git-local', ...ticket, details: '', blocked_reason: null, notes: [], attachments: next },
      });
    }
    const notWorkingMatch = path.match(/\/providers\/[^/]+\/tickets\/([^/]+)\/not-working$/);
    if (notWorkingMatch && request.method() === 'POST') {
      const id = notWorkingMatch[1],
        raw = request.postData() ?? '',
        attachments = raw.includes('filename=')
          ? [{ id: 'NW1', filename: 'proof ünicode.png', created_at: '2026-08-30T01:10:00Z' }]
          : [];
      evidenceByTicket.set(id, attachments);
      rows = rows.map((item) => (item.id === id ? { ...item, status: 'not_started', up_next: true } : item));
      patches.push({ operation: 'not-working', status: 'not_started', up_next: true, raw });
      const changed = rows.find((item) => item.id === id)!,
        notes = [
          {
            id: 'NW-ACTIVITY',
            kind: 'activity',
            created_at: '2026-08-30T01:10:00Z',
            edited_at: '2026-08-30T01:10:00Z',
            text: 'Brian reported as not working\nThe fix regressed after restart.',
          },
        ];
      return route.fulfill({
        json: { store: 'git-local', ...changed, details: '', blocked_reason: null, notes, attachments },
      });
    }
    if (path.includes('/tickets/') && request.method() === 'GET') {
      if (ticketLoadDelay) await new Promise((resolve) => setTimeout(resolve, ticketLoadDelay));
      const id = path.split('/').pop(),
        ticket = rows.find((item) => item.id === id);
      if (ticket)
        return route.fulfill({
          json: {
            store: 'git-local',
            ...ticket,
            details: '',
            blocked_reason: null,
            notes: [],
            attachments: evidenceByTicket.get(id!) ?? [],
            concurrency_token: `token-${id}`,
          },
        });
    }
    if (path.endsWith('/tickets/01/attachments') && request.method() === 'POST') {
      const filename = request.headers()['x-hotsheet-filename'] ?? 'attachment';
      selectedFull = {
        ...selectedFull,
        attachments: [
          ...selectedFull.attachments,
          { id: `A${selectedFull.attachments.length + 1}`, filename, created_at: '2026-08-30T01:10:00Z' },
        ],
      };
      return route.fulfill({ status: 201, json: { store: 'git-local', ...selectedFull } });
    }
    if (path.includes('/attachments/') && request.method() === 'POST' && path.endsWith('/action'))
      return route.fulfill({ json: { path: '/work/demo.hs2/attachments/proof.png' } });
    if (path.includes('/attachments/') && request.method() === 'GET') {
      const label = path.includes('/A2') ? 'second.svg' : 'proof.png';
      return route.fulfill({
        body: `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180"><rect width="320" height="180" fill="#ddd"/><text x="30" y="95">${label}</text></svg>`,
        headers: { 'content-type': 'image/svg+xml', 'x-hotsheet-filename': label },
      });
    }
    if (path.includes('/tickets/01/attachments/') && request.method() === 'PUT') {
      const annotations = request.postDataJSON().annotations,
        noteNumber = selectedFull.notes.filter((note) => note.id.startsWith('N-annotation-')).length + 1,
        activity = {
          id: `N-annotation-${noteNumber}`,
          kind: 'activity' as const,
          created_at: '2026-09-09T06:30:00Z',
          edited_at: '2026-09-09T06:30:00Z',
          summary: 'Updated annotations for proof.png',
          text: `Annotations changed for [attachment:proof.png](attachment:proof.png)\n- ${annotations.length ? 'Updated' : 'Removed'} \`(x 15.0%, y 20.0%, w 50.0%, h 50.0%)\` — Updated annotation`,
        };
      selectedFull = {
        ...selectedFull,
        attachments: selectedFull.attachments.map((item) =>
          item.id === path.split('/').pop() ? { ...item, annotations } : item,
        ),
        notes: [...selectedFull.notes, activity],
      };
      return route.fulfill({ json: { store: 'git-local', ...selectedFull } });
    }
    if (path.includes('/tickets/01/attachments/') && request.method() === 'DELETE') {
      const attachmentId = path.split('/').pop();
      selectedFull = {
        ...selectedFull,
        attachments: selectedFull.attachments.filter((item) => item.id !== attachmentId),
      };
      return route.fulfill({ json: { store: 'git-local', ...selectedFull } });
    }
    if (path.includes('/tickets/01/notes/') && request.method() === 'DELETE') {
      const noteId = path.split('/').pop();
      selectedFull = {
        ...selectedFull,
        notes: selectedFull.notes.filter((note) => {
          return note.id !== noteId;
        }),
      };
      return route.fulfill({ json: { store: 'git-local', ...selectedFull } });
    }
    if (path.includes('/tickets/') && request.method() === 'PATCH') {
      const id = path.split('/').pop(),
        body = request.postDataJSON();
      patches.push(body);
      if (body.note_kind === 'regular')
        rows = rows.map((item) =>
          item.id === id ? { ...item, feedback_needed: false, updated_at: '2026-08-30T02:00:00Z' } : item,
        );
      else
        rows = rows.map((item) => (item.id === id ? { ...item, ...body, updated_at: '2026-08-30T02:00:00Z' } : item));
      if (id === '01') {
        const label = (value: string) =>
            value
              .split('_')
              .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
              .join(' '),
          statusNote =
            typeof body.status === 'string' && body.status !== selectedFull.status
              ? {
                  id: `N-status-${patches.length}`,
                  kind: 'activity' as const,
                  created_at: '2026-09-02T02:00:00Z',
                  edited_at: '2026-09-02T02:00:00Z',
                  text: `Status changed from ${label(selectedFull.status)} to ${label(body.status)}`,
                }
              : undefined,
          appendedNote =
            typeof body.note === 'string' && body.note_kind === 'regular'
              ? {
                  id: `N-response-${patches.length}`,
                  kind: 'regular' as const,
                  created_at: '2026-09-02T02:01:00Z',
                  edited_at: '2026-09-02T02:01:00Z',
                  text: body.note,
                }
              : undefined;
        selectedFull = {
          ...selectedFull,
          ...body,
          updated_at: '2026-08-30T02:00:00Z',
          feedback_needed: appendedNote ? false : (selectedFull as { feedback_needed?: boolean }).feedback_needed,
          notes: [...selectedFull.notes, ...(statusNote ? [statusNote] : []), ...(appendedNote ? [appendedNote] : [])],
        };
        if (patchResponseDelay) await new Promise((resolve) => setTimeout(resolve, patchResponseDelay));
        return route.fulfill({ json: { store: 'git-local', ...selectedFull } });
      }
      const changed = rows.find((item) => item.id === id)!;
      return route.fulfill({
        json: { store: 'git-local', ...changed, details: '', notes: [], attachments: evidenceByTicket.get(id!) ?? [] },
      });
    }
    return route.continue();
  });
  return patches;
}

async function installFakeTerminalSockets(page: import('@playwright/test').Page, followClaims = false) {
  await page.addInitScript(
    ({ followClaims }) => {
      const sockets: FakeTerminalSocket[] = [];
      class FakeTerminalSocket extends EventTarget {
        static CONNECTING = 0;
        static OPEN = 1;
        static CLOSING = 2;
        static CLOSED = 3;
        readyState = FakeTerminalSocket.CONNECTING;
        binaryType = 'blob';
        sent: unknown[] = [];
        sizeSent = false;
        constructor(public url: string) {
          super();
          sockets.push(this);
          setTimeout(() => {
            this.readyState = FakeTerminalSocket.OPEN;
            this.dispatchEvent(new Event('open'));
            this.emitBytes('\u001b[32mLive terminal ready\u001b[0m\r\n');
          });
        }
        send(value: unknown) {
          this.sent.push(value);
          if (typeof value !== 'string') return;
          try {
            const claim = JSON.parse(value) as { resize?: { viewer_id: string; cols: number; rows: number } };
            if (claim.resize) {
              if (followClaims || !this.sizeSent) {
                this.sizeSent = true;
                const size = followClaims ? claim.resize : { cols: 100, rows: 30 };
                this.dispatchEvent(
                  new MessageEvent('message', {
                    data: JSON.stringify({
                      pty_size: { cols: size.cols, rows: size.rows },
                      driven_by: claim.resize.viewer_id,
                    }),
                  }),
                );
              }
              return;
            }
          } catch {
            /* terminal input */
          }
          this.emitBytes(value);
        }
        close() {
          if (this.readyState === FakeTerminalSocket.CLOSED) return;
          this.readyState = FakeTerminalSocket.CLOSED;
          this.dispatchEvent(new CloseEvent('close'));
        }
        emitBytes(value: string) {
          this.dispatchEvent(new MessageEvent('message', { data: new TextEncoder().encode(value).buffer }));
        }
        emitSize(cols: number, rows: number, drivenBy: string) {
          this.dispatchEvent(
            new MessageEvent('message', { data: JSON.stringify({ pty_size: { cols, rows }, driven_by: drivenBy }) }),
          );
        }
      }
      Object.assign(window, { WebSocket: FakeTerminalSocket, __terminalSockets: sockets });
    },
    { followClaims },
  );
}

const devReviewTestTitles = new Set([
  'activates Dev Review by default and preserves its desktop/mobile lifecycle',
  'honors the explicit Dev Review false opt-out after application readiness (HS2-9TZ9AF)',
  'does not report intentional render bursts during remembered-project startup',
  'suppresses interaction-bound render bursts but reports a storm that persists afterward',
  'keeps feedback rectangle input within its frame budget in the populated main app',
  'switches large ticket views without cloning every row into motion ghosts',
]);

test.beforeEach(async ({ page }, testInfo) => {
  if (devReviewTestTitles.has(testInfo.title)) return;
  await page.addInitScript(() => {
    if (
      (location.protocol === 'http:' || location.protocol === 'https:') &&
      !new URLSearchParams(location.search).has('dev-review')
    ) {
      const url = new URL(location.href);
      url.searchParams.set('dev-review', 'false');
      history.replaceState(null, '', url);
    }
  });
});

test('activates Dev Review by default and preserves its desktop/mobile lifecycle', async ({ page }) => {
  let submission: { actorRole: string; attachments: Array<{ filename: string; mimeType: string }> } | undefined;
  await page.route('**/__hotsheet/dev-review/tickets', async (route) => {
    submission = route.request().postDataJSON();
    await route.fulfill({ status: 201, json: { slug: 'HS2-DIAG01' } });
  });
  await page.goto('/');
  await expect(page.locator('.hs-dev-review')).toBeVisible();
  await page.getByRole('button', { name: 'Feedback' }).click();
  await page.getByRole('button', { name: 'New Ticket' }).click();
  const dialog = page.getByRole('dialog', { name: 'New Hot Sheet ticket' }),
    diagnostics = dialog.locator('.hs-dev-review__diagnostics');
  await expect(diagnostics.getByText('Attach diagnostic logs')).toBeVisible();
  await expect(diagnostics.locator('input')).toBeChecked();
  expect(
    await diagnostics.evaluate((element) => {
      const checkbox = element.querySelector('input')!.getBoundingClientRect(),
        label = element.querySelector('span')!.getBoundingClientRect();
      return Math.abs((checkbox.top + checkbox.bottom - label.top - label.bottom) / 2);
    }),
  ).toBeLessThanOrEqual(1);
  await page.screenshot({ path: '/private/tmp/hs2-48w3ys-main-dev-review-default-wide.png', fullPage: true });
  await dialog.getByRole('textbox', { name: 'Feedback notes' }).fill('Select menus close unexpectedly.');
  await dialog.getByRole('button', { name: 'Create Ticket' }).click();
  await expect.poll(() => submission).toBeTruthy();
  expect(submission!.actorRole).toBe('human');
  expect(submission!.attachments).toHaveLength(1);
  expect(submission!.attachments[0]).toMatchObject({ mimeType: 'application/json' });
  expect(submission!.attachments[0].filename).toMatch(/^hotsheet-ui-diagnostics-/);
  // The Dev Review overlay is desktop-only: it is removed on a mobile-width viewport and returns on
  // resize back to desktop, while the headless UI-stability diagnostics stay installed (HS2-9KT6RQ).
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('.hs-dev-review')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-sv3f5g-main-dev-review-narrow.png', fullPage: true });
  expect(
    await page.evaluate(() =>
      Boolean(
        (window as typeof window & { __hotsheetUiStabilityDiagnostics?: unknown }).__hotsheetUiStabilityDiagnostics,
      ),
    ),
  ).toBe(true);
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(page.locator('.hs-dev-review')).toBeVisible();
});

test('honors the explicit Dev Review false opt-out after application readiness (HS2-9TZ9AF)', async ({ page }) => {
  await page.goto('/?dev-review=false');
  await expect(page.getByRole('button', { name: 'Open project', exact: true })).toBeVisible();
  await expect(page.locator('.hs-dev-review')).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      Boolean(
        (window as typeof window & { __hotsheetUiStabilityDiagnostics?: unknown }).__hotsheetUiStabilityDiagnostics,
      ),
    ),
  ).toBe(false);
});

test('opens a roomy project dialog with native browse controls and working cancel', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  const dialog = page.locator('[data-project-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(page.locator('wa-input[name="project-root"]')).toHaveJSProperty('value', '.');
  expect((await dialog.boundingBox())!.width).toBeGreaterThan(700);
  await page.getByRole('button', { name: 'Browse for project folder' }).click();
  await expect(page.locator('wa-input[name="project-root"]')).toHaveJSProperty('value', '/picked/project');
  await expect(dialog).toHaveJSProperty('open', true);
  await page.getByRole('button', { name: 'Browse for ticket store' }).click();
  await expect(page.locator('wa-input[name="ticket-store"]')).toHaveJSProperty('value', '/picked/tickets.hs2');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.locator('.project-dialog__error')).toBeEmpty();
  await expect(page.locator('.app-error')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-nvd50p-open-project-dialog.png', fullPage: true });
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveJSProperty('open', false);
  await expect(dialog).toBeHidden();
  await page.getByRole('button', { name: 'Open project' }).click();
  await expect(dialog).toHaveJSProperty('open', true);
});

test('remote clients pick from the server open-projects list instead of the file picker (HS2-VFNCXG)', async ({
  page,
}) => {
  await mockProject(page);
  const longRoot = `/work/projects/${'long-project-folder-without-spaces-'.repeat(5)}best-in-manila`;
  await page.route('**/__hotsheet/checkouts', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            { id: 'demo-checkout', root: '/work/demo', alias: 'demo', stores: ['/work/demo.hs2'] },
            { id: 'long-checkout', root: longRoot, alias: 'best-in-manila', stores: [] },
            ...['code-a', 'code-b', 'domotion', 'hotsheet2', 'karwan', 'kerf', 'procurement'].map((name) => ({
              id: name,
              root: `/work/projects/${name}`,
              alias: name,
              stores: [],
            })),
          ],
        })
      : route.fallback(),
  );
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.goto('/?device=remote');
  await page.getByRole('button', { name: 'Open project' }).click();
  const remote = page.locator('[data-remote-project-dialog]');
  await expect(remote).toHaveJSProperty('open', true);
  await expect(remote.locator('.project-dialog__error')).toHaveCount(0);
  await expect(page.locator('[data-project-dialog]')).toHaveJSProperty('open', false);
  const item = remote.locator('[data-action="open-remote-checkout"]').first();
  await expect(item).toBeVisible();
  await expect(item).toContainText('demo');
  await expect(item).toContainText('/work/demo');
  await expect(item).toHaveAttribute('data-component', 'list-item');
  await expect
    .poll(() =>
      remote.evaluate(
        (element) =>
          element.shadowRoot
            ?.querySelector('dialog')
            ?.getAnimations()
            .filter((animation) => animation.playState === 'running').length ?? -1,
      ),
    )
    .toBe(0);
  await page.screenshot({ path: '/private/tmp/hs2-xx5y2x-remote-project-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  const longPath = remote.locator('.remote-project-dialog__path').nth(1);
  await expect(longPath).toHaveText(longRoot);
  const geometry = await remote.evaluate((node) => {
    const list = node.querySelector<HTMLElement>('.remote-project-dialog__list')!;
    const path = node.querySelectorAll<HTMLElement>('.remote-project-dialog__path')[1];
    const pathStyle = getComputedStyle(path);
    return {
      listWidth: list.clientWidth,
      listScrollWidth: list.scrollWidth,
      pathHeight: path.getBoundingClientRect().height,
      pathLineHeight: parseFloat(pathStyle.lineHeight),
      wordBreak: pathStyle.wordBreak,
      rowsFit: [...list.querySelectorAll('button')].every((button) => {
        const row = button.getBoundingClientRect();
        const bounds = list.getBoundingClientRect();
        return row.x >= bounds.x && row.right <= bounds.right && row.width >= bounds.width - 20;
      }),
    };
  });
  expect(geometry.listScrollWidth).toBeLessThanOrEqual(geometry.listWidth);
  expect(geometry.pathHeight).toBeGreaterThan(geometry.pathLineHeight * 2);
  expect(geometry.wordBreak).toBe('break-all');
  expect(geometry.rowsFit).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-xx5y2x-remote-project-mobile.png' });
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(remote.locator('dialog')).toHaveCSS('background-color', 'rgb(44, 44, 46)');
  await expect(item).toHaveCSS('color', 'rgb(245, 245, 247)');
  await page.screenshot({ path: '/private/tmp/hs2-xx5y2x-remote-project-mobile-dark-production.png' });
  await item.focus();
  await item.press('Enter');
  await expect(remote).toHaveJSProperty('open', false);
  await expect(page.locator('wa-select[name="mobile-project"]')).toHaveJSProperty('value', 'demo-checkout');
  await page.setViewportSize({ width: 1100, height: 760 });
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
});

test('shows a clear message, not a raw browser exception, when the remote project list fails to load (HS2-91PCDZ)', async ({
  page,
}) => {
  await mockProject(page);
  await page.route('**/__hotsheet/checkouts', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ status: 500, body: 'boom' }) : route.fallback(),
  );
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.goto('/?device=remote');
  await page.getByRole('button', { name: 'Open project' }).click();
  const remote = page.locator('[data-remote-project-dialog]');
  await expect(remote).toHaveJSProperty('open', true);
  const error = remote.locator('.project-dialog__error');
  await expect(error).toContainText('Could not load the projects open on the Hot Sheet server');
  // Never surface a cryptic engine exception (e.g. iOS Safari's "The string did not match the expected pattern.").
  await expect(error).not.toContainText('did not match the expected pattern');
});

test('offers explicit identity-guarded recovery for an unresponsive local server', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await mockProject(page);
  let failOpen = true,
    recoveryBody: unknown;
  await page.route('**/__hotsheet/projects/open', (route) => {
    if (failOpen) {
      failOpen = false;
      return route.fulfill({
        status: 400,
        json: {
          error: 'The registered local server is not responding.',
          recovery: {
            store: '/work/demo.hs2',
            expected: { pid: 4242, url: 'http://127.0.0.1:8787', started_at: '2026-09-12T01:00:00Z' },
          },
        },
      });
    }
    return route.fallback();
  });
  await page.route('**/__hotsheet/server/recover-unhealthy', (route) => {
    recoveryBody = route.request().postDataJSON();
    return route.fulfill({ json: { recovered: true } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const dialog = page.locator('[data-project-dialog]'),
    recovery = dialog.locator('.project-dialog__server-recovery');
  await expect(recovery).toBeVisible();
  await expect(recovery).toContainText('cannot verify active work');
  await expect(recovery).toContainText('process 4242');
  await page.screenshot({ path: '/private/tmp/hs2-21e6g6-unhealthy-recovery-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(recovery).toBeInViewport();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-21e6g6-unhealthy-recovery-narrow.png', fullPage: true });
  await recovery.getByRole('button', { name: 'Stop server and retry' }).click();
  await expect(page.locator('wa-select[name="mobile-project"]')).toHaveJSProperty('value', 'demo-checkout');
  expect(recoveryBody).toEqual({
    store: '/work/demo.hs2',
    expected: { pid: 4242, url: 'http://127.0.0.1:8787', started_at: '2026-09-12T01:00:00Z' },
  });
});

test('always confirms before closing a project without running resources', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const projectTab = page.locator('[data-tab-kind="project"]');
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  const dialog = page.locator('[data-component="project-close-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).not.toContainText('No terminals or AI chats are currently running');
  await expect(dialog.locator('.project-close-dialog__intro')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: 'Close Project' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Stop & Close' })).toHaveCount(0);
  await expect
    .poll(() =>
      dialog.evaluate(
        (element) =>
          element.shadowRoot
            ?.querySelector('dialog')
            ?.getAnimations()
            .filter((animation) => animation.playState === 'running').length ?? -1,
      ),
    )
    .toBe(0);
  await page.screenshot({ path: '/private/tmp/hs2-d7haq1-project-close-wide.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(projectTab).toHaveCount(1);
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog).toHaveJSProperty('open', true);
  await expect
    .poll(() =>
      dialog.evaluate((element) => {
        const box = element.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth && box.bottom <= innerHeight;
      }),
    )
    .toBe(true);
  await expect
    .poll(() =>
      dialog.evaluate(
        (element) =>
          element.shadowRoot
            ?.querySelector('dialog')
            ?.getAnimations()
            .filter((animation) => animation.playState === 'running').length ?? -1,
      ),
    )
    .toBe(0);
  await page.screenshot({ path: '/private/tmp/hs2-d7haq1-project-close-narrow.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Close Project' }).click();
  await expect(projectTab).toHaveCount(0);
  await expect(
    page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]')),
  ).resolves.toEqual([]);
});

test('falls back to the project dialog when the direct native chooser fails', async ({ page }) => {
  await mockProject(page);
  await page.route('**/__hotsheet/folders/choose', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 250));
    await route.fulfill({ status: 500, json: { error: 'The native folder chooser failed.' } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.getByRole('button', { name: 'Add project' }).click();
  const dialog = page.locator('[data-project-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.locator('wa-input[name="project-root"]')).toHaveJSProperty('value', '/work/demo');
  await expect(dialog.locator('.project-dialog__error')).toContainText('The native folder chooser failed.');
  await dialog.getByRole('button', { name: 'Close' }).click();
  await expect(dialog).toBeHidden();
  await page.waitForTimeout(500);
  await expect(dialog).toBeHidden();
  await expect(dialog).toHaveJSProperty('open', false);
});

test('clears a failed project-open error when retrying successfully', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  let failNextOpen = true;
  await page.route('**/__hotsheet/projects/open', async (route) => {
    if (failNextOpen) {
      failNextOpen = false;
      return route.fulfill({ status: 409, json: { error: 'The previous detached server only supports schema 2.' } });
    }
    return route.fallback();
  });
  const retry = async () => page.getByRole('button', { name: 'Add project' }).click();
  await retry();
  const appError = page.locator('[data-component="app-error"]');
  await expect(appError).toContainText('only supports schema 2');
  await expect(appError.getByRole('button', { name: 'Dismiss error' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-c6at65-dismissible-error-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(appError).toBeInViewport();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-c6at65-dismissible-error-narrow.png', fullPage: true });
  await appError.getByRole('button', { name: 'Dismiss error' }).click();
  await expect(appError).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Add project' })).toBeEnabled();
  await page.setViewportSize({ width: 1280, height: 800 });
  await retry();
  await expect(page.locator('.app-error')).toHaveCount(0);
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-nzffdh-successful-project-retry.png', fullPage: true });
});

test('opens the native folder chooser directly from Add project and only onboards a source-less checkout once', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await mockProject(page, true, false, 0, 0, 0, false, 0);
  let configured = false;
  const openedRoots: string[] = [];
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    openedRoots.push(root);
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? {
              ...project,
              id: 'other-checkout',
              root,
              name: 'other',
              stores: configured ? ['/work/other.hs2'] : [],
              apiPath: '/__hotsheet/project-api/other-checkout',
              needsTicketSetup: !configured,
            }
          : project,
    });
  });
  await page.route('**/__hotsheet/projects/setup-git', (route) => {
    configured = true;
    return route.fulfill({ status: 201, json: { ticketStore: '/work/other.hs2', connectionId: 'git-other' } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  expect(openedRoots).toEqual(['.', '/work/other']);
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const setup = page.locator('[data-ticket-source-setup-dialog]');
  await expect(setup).toHaveJSProperty('open', true);
  await expect(setup).toContainText('other is open, but it does not have a ticket source yet.');
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-gcbc3e-direct-add-project-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-gcbc3e-direct-add-project-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1100, height: 760 });
  await setup.getByRole('button', { name: 'Create a Hot Sheet 2 git ticket repository', exact: true }).click();
  await expect(setup.getByText('Back up this ticket repository')).toBeVisible();
  await setup.getByRole('button', { name: 'Close' }).click();
  const otherTab = page.locator('[data-tab-kind="project"]').filter({ has: page.getByRole('tab', { name: 'other' }) });
  await otherTab.hover();
  await otherTab.getByRole('button', { name: 'Close other' }).click();
  await page.locator('[data-component="project-close-dialog"]').getByRole('button', { name: 'Close Project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  await expect(setup).toBeHidden();
  await expect(setup).toHaveJSProperty('open', false);
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  expect(openedRoots).toEqual(['.', '/work/other', '/work/other']);
});

test('uses one provider dialog for onboarding, repeated connection creation, and editing', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await mockProject(page, true, false, 0, 0, 0, true);
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { ...project, root: '/work/best-in-manila', stores: [], needsTicketSetup: true },
    }),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  await expect(page.locator('.app-error')).toHaveCount(0);
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(0);
  const setup = page.locator('[data-ticket-source-setup-dialog]'),
    sourceOptions = setup.locator('.ticket-source-setup__options .kui-list-item');
  await expect(setup).toHaveJSProperty('open', true);
  await expect(setup).toHaveAttribute('data-component', 'ticket-source-setup-dialog');
  await expect(sourceOptions).toHaveCount(5);
  await expect(
    setup.getByRole('button', { name: 'Create a Hot Sheet 2 git ticket repository', exact: true }),
  ).toBeVisible();
  await expect(
    setup.getByRole('button', { name: 'Create a Hot Sheet 2 git ticket repository in a custom location' }),
  ).toBeVisible();
  await expect(setup.getByRole('button', { name: 'Connect GitHub Issues' })).toBeVisible();
  await expect(setup.getByRole('button', { name: 'Connect GitLab Issues' })).toBeVisible();
  await expect(setup.getByRole('button', { name: 'Connect Jira Cloud' })).toBeVisible();
  await expect(sourceOptions.locator('[data-lucide="chevron-right"]')).toHaveCount(5);
  await expect(sourceOptions.locator('[data-provider-icon="github"]')).toBeVisible();
  await expect(sourceOptions.locator('[data-provider-icon="gitlab"]')).toBeVisible();
  await expect(sourceOptions.locator('[data-provider-icon="jira"]')).toBeVisible();
  await setup.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  for (const option of await sourceOptions.all()) {
    const box = (await option.boundingBox())!,
      label = (await option.locator('.kui-list-item__label').boundingBox())!,
      icon = (await option.locator('.kui-list-item__icon').boundingBox())!,
      trailing = (await option.locator('.kui-list-item__trailing').boundingBox())!;
    expect(label.y).toBeGreaterThanOrEqual(box.y);
    expect(label.y + label.height, (await option.textContent()) ?? undefined).toBeLessThanOrEqual(box.y + box.height);
    expect(
      box.y + box.height - (label.y + label.height),
      (await option.textContent()) ?? undefined,
    ).toBeGreaterThanOrEqual(10);
    expect(Math.abs(icon.y + icon.height / 2 - (box.y + box.height / 2))).toBeLessThanOrEqual(6);
    expect(Math.abs(trailing.y + trailing.height / 2 - (box.y + box.height / 2))).toBeLessThanOrEqual(6);
  }
  await page.screenshot({ path: '/private/tmp/hs2-4fw7wm-source-setup-wide.png', fullPage: true });
  await page.setViewportSize({ width: 620, height: 680 });
  await expect
    .poll(async () =>
      sourceOptions.evaluateAll((options) =>
        options.every((option) => {
          const box = option.getBoundingClientRect(),
            label = option.querySelector('.kui-list-item__label')!.getBoundingClientRect();
          return label.right <= box.right && label.bottom <= box.bottom - 10;
        }),
      ),
    )
    .toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-4fw7wm-source-setup-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1100, height: 760 });
  await setup.getByRole('button', { name: 'Connect GitHub Issues' }).click();
  await expect(setup).toHaveJSProperty('open', true);
  await expect(setup).toHaveAttribute('data-navigation', 'push');
  const providerForm = setup.locator('[data-action="save-provider-connection"]');
  await expect(providerForm).toBeVisible();
  const transition = setup.locator('[data-component="content-transition"][data-transition-region="content"]'),
    titleTransition = setup.locator('[data-transition-region="label"]'),
    footerTransition = setup.locator('[data-transition-region="footer"]');
  await expect(transition).toHaveAttribute('data-active-side', 'b');
  await expect(transition.locator('.content-transition__side')).toHaveCount(2);
  await expect
    .poll(() => transition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-in-end');
  await expect
    .poll(() => transition.locator('[data-side="a"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-out-start');
  await expect
    .poll(() => titleTransition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-fade-in');
  await expect
    .poll(() => footerTransition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-fade-in');
  await transition.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  await page.screenshot({ path: '/private/tmp/hs2-y4zpqq-provider-config-dialog-after.png', fullPage: true });
  await providerForm.getByRole('button', { name: 'Ticket source types' }).click();
  await expect(setup).toHaveAttribute('data-navigation', 'pop');
  await expect(transition).toHaveAttribute('data-active-side', 'a');
  await expect(sourceOptions).toHaveCount(5);
  await expect
    .poll(() => transition.locator('[data-side="a"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-in-start');
  await expect
    .poll(() => transition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-out-end');
  await setup.getByRole('button', { name: 'Connect GitHub Issues' }).click();
  await providerForm.getByRole('button', { name: 'Sign in with GitHub' }).click();
  await expect(providerForm.getByRole('status')).toContainText('Signed in securely');
  await providerForm.getByLabel('Connection ID').fill('GitHub Main');
  await providerForm.getByLabel('Repository').selectOption('small-tale/hotsheet2');
  await setup.getByRole('button', { name: 'Connect provider' }).click();
  await expect(setup.getByRole('alert')).toContainText('lowercase letters');
  await providerForm.getByLabel('Connection ID').fill('github-main');
  await providerForm.getByLabel('Display name').fill('GitHub Issues');
  await setup.getByRole('button', { name: 'Connect provider' }).click();
  await expect(setup).toHaveJSProperty('open', false);
  await expect(page.locator('.app-toast')).toContainText('GitHub Issues connected.');
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await page.getByLabel('Settings view').click();
  await expect(page.locator('[data-component="settings-workspace"]')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Connected sources' })).toBeVisible();
  await expect(page.locator('[data-action="save-provider-connection"]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Add data source' })).toBeVisible();
  const primaryConnection = page.getByRole('button', { name: 'Edit GitHub Issues' });
  await expect(primaryConnection).toContainText('small-tale/hotsheet2');
  await expect(primaryConnection.locator('[data-lucide="chevron-right"]')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-y4zpqq-provider-settings-list-after.png', fullPage: true });
  await page.getByRole('button', { name: 'Add data source' }).click();
  await setup.getByRole('button', { name: 'Connect GitHub Issues' }).click();
  await providerForm.getByRole('button', { name: 'Sign in with GitHub' }).click();
  await expect(providerForm.getByRole('status')).toContainText('Signed in securely');
  await providerForm.getByLabel('Connection ID').fill('github-secondary');
  await providerForm.getByLabel('Display name').fill('GitHub Secondary');
  await providerForm.getByLabel('Repository').selectOption('small-tale/secondary');
  await providerForm.getByLabel('Use as the default ticket source').uncheck();
  await setup.getByRole('button', { name: 'Connect provider' }).click();
  await expect(setup).toHaveJSProperty('open', false);
  await expect(page.getByRole('button', { name: 'Edit GitHub Issues' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Edit GitHub Secondary' })).toBeVisible();
  await page.getByRole('button', { name: 'Edit GitHub Issues' }).click();
  await expect(providerForm.getByLabel('Connection ID')).toBeDisabled();
  await expect(providerForm.getByLabel('Repository')).toHaveValue('small-tale/hotsheet2');
  await providerForm.getByLabel('Display name').fill('GitHub Primary');
  await setup.getByRole('button', { name: 'Save changes' }).click();
  await expect(setup).toHaveJSProperty('open', false);
  await expect(page.getByRole('button', { name: 'Edit GitHub Primary' })).toBeVisible();
  await expect(page.locator('.app-error')).toHaveCount(0);
});

test('blocks a new ticket store before an older project server can accept it', async ({ page }) => {
  await page.setViewportSize({ width: 1536, height: 960 });
  await mockProject(page, true, false, 0, 0, 0, true);
  await page.route('**/__hotsheet/projects/setup-git', (route) =>
    route.fulfill({
      status: 400,
      json: {
        error:
          'No ticket repository was created. This project is connected to an older Hot Sheet server build that supports ticket-store schema through 2, while the current Hot Sheet CLI creates schema 3. Finish any active work in this project, stop or restart its detached Hot Sheet server, then reopen the project.',
      },
    }),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const setup = page.locator('[data-ticket-source-setup-dialog]');
  await expect(setup).toHaveJSProperty('open', true);
  await setup.getByRole('button', { name: 'Create a Hot Sheet 2 git ticket repository', exact: true }).click();
  const alert = setup.getByRole('alert');
  await expect(alert).toContainText('No ticket repository was created');
  await expect(alert).toContainText('supports ticket-store schema through 2');
  await expect(alert).toContainText('stop or restart its detached Hot Sheet server');
  await expect(setup).toHaveAttribute('data-navigation', 'none');
  await page.screenshot({ path: '/private/tmp/hs2-cew85a-store-schema-preflight-after.png', fullPage: true });
});

test('keeps a dismissed ticket-source setup dialog closed across later project renders (HS2-4Y37T9)', async ({
  page,
}) => {
  await mockProject(page, true, false, 0, 0, 0, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const setup = page.locator('[data-ticket-source-setup-dialog]');
  await expect(setup).toHaveJSProperty('open', true);
  await setup.getByRole('button', { name: 'Close' }).click();
  await expect(setup).toBeHidden();
  await page.getByRole('tab', { name: /demo/ }).click();
  await page.getByLabel('Columns view').click();
  await page.waitForTimeout(350);
  await expect(setup).toBeHidden();
  await expect(setup).toHaveJSProperty('open', false);
});

test('identifies detected HS1 data and keeps a dismissed import modal closed across launches', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 840 });
  await mockProject(page, true, false, 0, 0, 0, false, 2, false, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const dialog = page.locator('[data-component="hs1-migration-dialog"]'),
    dialogOpacity = () =>
      dialog.evaluate((node) => getComputedStyle(node.shadowRoot!.querySelector('dialog')!).opacity);
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.getByText('/work/demo/.hotsheet/db', { exact: true })).toBeVisible();
  await expect(dialog.getByText('17', { exact: true })).toBeVisible();
  await expect.poll(dialogOpacity).toBe('1');
  await page.screenshot({ path: '/private/tmp/hs2-k4306s-hs1-source-dialog-wide.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Not now' }).click();
  const banner = page.locator('.hs1-migration-banner');
  await expect(dialog).toBeHidden();
  await page.waitForTimeout(250);
  await expect(banner).toHaveAttribute('data-component', 'state-banner');
  await expect(banner).toHaveAttribute('data-tone', 'info');
  await expect(banner).toHaveAttribute('role', 'status');
  await expect(banner).toHaveAttribute('aria-live', 'polite');
  await expect(banner).toContainText('/work/demo/.hotsheet/db');
  await banner.screenshot({ path: '/private/tmp/hs2-750wsy-hs1-migration-banner-wide.png' });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await banner.screenshot({ path: '/private/tmp/hs2-750wsy-hs1-migration-banner-narrow.png' });
  await page.setViewportSize({ width: 1100, height: 840 });
  await page.reload();
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  await expect(dialog).toBeHidden();
  await expect(banner).toBeVisible();
  await banner.getByRole('button', { name: 'Import…' }).click();
  await expect(dialog.getByText('/work/demo/.hotsheet/db', { exact: true })).toBeVisible();
  await expect.poll(dialogOpacity).toBe('1');
  await page.setViewportSize({ width: 560, height: 720 });
  await expect.poll(dialogOpacity).toBe('1');
  await page.screenshot({ path: '/private/tmp/hs2-k4306s-hs1-source-dialog-narrow.png', fullPage: true });
});

test('adds a second git ticket store and connects its remote from one guided form', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 840 });
  await mockProject(page);
  let remoteAttempts = 0;
  await page.route('**/__hotsheet/projects/setup-git-remote', (route) => {
    remoteAttempts += 1;
    return remoteAttempts === 1
      ? route.fulfill({
          status: 400,
          json: {
            error:
              'The remote repository was not found, or your account cannot access it. Verify the clone URL and your access on the Git host, then try again. Git details: ERROR: Repository not found.\nfatal: Could not read from remote repository.',
          },
        })
      : route.fulfill({ json: { connected: true } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Add data source' }).click();
  const setup = page.locator('[data-ticket-source-setup-dialog]'),
    recommended = setup.getByRole('button', { name: 'Create a Hot Sheet 2 git ticket repository', exact: true });
  await expect(recommended).toBeDisabled();
  await expect(recommended).toContainText('Already connected');
  await setup.getByRole('button', { name: 'Create a Hot Sheet 2 git ticket repository in a custom location' }).click();
  await expect(setup).toHaveAttribute('data-navigation', 'push');
  await expect(setup.getByText('Back up this ticket repository')).toBeVisible();
  const remote = setup.getByRole('textbox', { name: 'Remote URL' });
  await expect(remote).toBeFocused();
  await remote.fill('git@github.com:small-tale/demo-tickets.git');
  await expect(setup.getByRole('link', { name: 'How to create a remote repository' })).toHaveAttribute(
    'href',
    /ticket-repository-remotes/,
  );
  await setup.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  await page.screenshot({ path: '/private/tmp/hs2-73q89e-git-remote-form-after.png', fullPage: true });
  await page.setViewportSize({ width: 560, height: 720 });
  await expect(setup).toBeInViewport();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-73q89e-git-remote-form-after-narrow.png', fullPage: true });
  await setup.getByRole('button', { name: 'Connect & push' }).click();
  const error = setup.getByRole('alert');
  await expect(error).toContainText('The remote repository was not found');
  await expect(error).toContainText('Verify the clone URL and your access');
  await expect(error).toContainText('Git details: ERROR: Repository not found');
  await page.setViewportSize({ width: 1100, height: 840 });
  await page.screenshot({ path: '/private/tmp/hs2-mddrcm-remote-error-wide.png', fullPage: true });
  await page.setViewportSize({ width: 560, height: 720 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-mddrcm-remote-error-narrow.png', fullPage: true });
  await setup.getByRole('button', { name: 'Connect & push' }).click();
  await expect(setup).toHaveJSProperty('open', false);
  expect(remoteAttempts).toBe(2);
  await expect(page.getByText('This checkout uses 2 git ticket sources')).toBeVisible();
  await expect(page.getByText('/picked/project', { exact: true })).toBeVisible();
});

test('uses independent width and height terminal dashboard zoom scales', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1100 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    operations = page.getByRole('complementary', { name: 'Terminal operations sidebar' });
  await expect(dashboard).toBeVisible();
  const zoomToolbar = dashboard.getByRole('toolbar', { name: 'Workspace tile zoom' });
  await expect(zoomToolbar).toHaveAttribute('data-component', 'floating-toolbar');
  await expect(zoomToolbar).toHaveAttribute('data-position', 'bottom-end');
  await expect(zoomToolbar.locator('[data-component="toolbar-control-group"]')).toHaveAttribute('data-tone', 'default');
  await expect(operations).toBeVisible();
  await expect(operations.getByText('demo', { exact: true })).toBeVisible();
  await expect(operations.locator('[data-component="project-summary"]')).toHaveCount(1);
  await expect(operations.getByText('All projects')).toHaveCount(0);
  await expect(dashboard).toHaveAttribute('data-basis', 'across');
  await expect(dashboard).toHaveAttribute('data-fit', '4');
  await expect(dashboard.locator('.terminal-dashboard__project')).toHaveCount(0);
  await expect(dashboard.locator('[data-terminal-key="demo-checkout:codex-main"] .xterm-rows')).toContainText(
    'Live terminal ready',
  );
  await expect(dashboard.locator('.terminal-tile__preview').first()).toHaveCSS('background-color', 'rgb(0, 0, 0)');
  const terminalSurface = dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]');
  const scaledPreview = terminalSurface.locator('[data-display-mode="scaled-preview"]');
  await expect(scaledPreview.locator('.terminal')).toBeVisible();
  await expect(scaledPreview).toHaveAttribute('data-natural-size', '1280x768');
  await expect(scaledPreview).toHaveAttribute('data-grid-size', '80x24');
  await expect(scaledPreview).toHaveAttribute('data-font-size', /\d/);
  await expect(scaledPreview).toHaveAttribute('data-line-height', /\d/);
  await expect(scaledPreview).toHaveCSS('pointer-events', 'none');
  await expect(terminalSurface).toHaveCSS('border-width', '0px');
  await expect(scaledPreview).toHaveCSS('transform-origin', '0px 0px');
  const latestClaim = (terminalId: string) =>
    page.evaluate((id) => {
      const sockets = (
          window as unknown as { __terminalSockets: Array<{ url: string; sent: unknown[] }> }
        ).__terminalSockets.filter((socket) => socket.url.includes(id)),
        claims = sockets.flatMap((socket) =>
          socket.sent
            .filter((value): value is string => typeof value === 'string' && value.startsWith('{'))
            .map((value) => JSON.parse(value).resize),
        );
      return claims.at(-1) as { cols: number; rows: number; focus: boolean } | undefined;
    }, terminalId);
  await expect.poll(() => latestClaim('codex-main')).toMatchObject({ cols: 80, rows: 24, focus: true });
  const naturalClaim = await latestClaim('codex-main');
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-workspace-zoom-wide.png', fullPage: true });
  await dashboard.getByRole('button', { name: /Zoom in, fit fewer items across/ }).click();
  await expect(dashboard).toHaveAttribute('data-fit', '3');
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.terminals.fit-across'))).resolves.toBe('3');
  await expect
    .poll(() => latestClaim('codex-main'))
    .toMatchObject({ cols: naturalClaim!.cols, rows: naturalClaim!.rows, focus: true });
  await expect(page.locator('.project-tab-bar')).toHaveCSS('border-bottom-width', '1px');
  await page.screenshot({ path: '/private/tmp/hs2-aaacfq-terminal-header-separator.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(dashboard).toHaveAttribute('data-basis', 'high');
  await expect(dashboard).toHaveAttribute('data-fit', '2');
  await expect(operations).toBeVisible();
  await operations.getByRole('button', { name: 'Hide operations sidebar' }).click();
  await expect(page.locator('section[data-region-id="app-sidebar"]')).toHaveAttribute('data-collapsed', 'true');
  await page.getByRole('button', { name: 'Show operations sidebar' }).click();
  await expect(operations).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-mh8qn2-terminal-operations-1024x600.png', fullPage: true });
  await dashboard.getByRole('button', { name: /Zoom out, fit more items high/ }).click();
  await expect(dashboard).toHaveAttribute('data-fit', '3');
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.terminals.fit-high'))).resolves.toBe('3');
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.terminals.fit-across'))).resolves.toBe('3');
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-workspace-zoom-narrow.png', fullPage: true });
  const compactTile = dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]');
  await expect(compactTile).toHaveAttribute('data-preview-only', 'true');
  await expect(compactTile.locator('[data-display-mode="scaled-preview"]')).toHaveCount(1);
  await compactTile.click();
  const magnified = dashboard.getByRole('dialog', { name: 'Magnified Codex Main' });
  await expect(magnified).toBeVisible();
  const magnifiedViewport = magnified.locator('[data-display-mode="interactive"]');
  await expect(magnifiedViewport).not.toHaveAttribute('data-grid-policy', 'dashboard-80x24');
  await expect(magnifiedViewport.locator('.xterm-helper-textarea')).toBeFocused();
  await expect
    .poll(async () => {
      const claim = await latestClaim('codex-main');
      return `${claim?.cols}x${claim?.rows}`;
    })
    .not.toBe('80x24');
  await expect.poll(() => latestClaim('codex-main')).toMatchObject({ focus: true });
  await expect
    .poll(() =>
      magnifiedViewport.evaluate((element) => {
        const screen = element.querySelector<HTMLElement>('.xterm-screen')?.getBoundingClientRect(),
          frame = element.closest<HTMLElement>('.terminal-tile__viewport-frame')?.getBoundingClientRect(),
          terminal = element.querySelector<HTMLElement>('.terminal');
        if (!screen || !frame || !terminal) return false;
        const matrix = new DOMMatrixReadOnly(getComputedStyle(terminal).transform),
          contained =
            screen.left >= frame.left - 1 &&
            screen.top >= frame.top - 1 &&
            screen.right <= frame.right + 1 &&
            screen.bottom <= frame.bottom + 1,
          usesFrame = screen.width > frame.width * 0.9 && screen.height > frame.height * 0.85;
        return contained && usesFrame && Math.abs(matrix.a - matrix.d) < 0.001 && frame.height > 400;
      }),
    )
    .toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-kke1pm-fitted-magnified-provider-after.png', fullPage: true });
  await magnified.click({ position: { x: 5, y: 5 } });
  await expect(magnified).toHaveCount(0);
  await compactTile.click({ button: 'right' });
  const tileMenu = dashboard.getByRole('menu');
  await expect(tileMenu.getByText('Open')).toBeVisible();
  await expect(tileMenu.getByText('Hide Terminal')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-3d8frb-terminal-context-menu.png', fullPage: true });
  await tileMenu.getByText('Hide Terminal').click();
  const manageVisibility = page.getByRole('button', { name: 'Manage workspace visibility' });
  await expect(page.locator('.terminal-dashboard-controls__count')).toHaveText('1');
  await manageVisibility.click();
  const visibilityDialog = page.locator('[data-terminal-visibility-dialog]');
  await visibilityDialog.getByRole('button', { name: 'Show Codex Main' }).click();
  await page.keyboard.press('Escape');
  await expect(compactTile).toBeVisible();
  await expect(page.locator('.terminal-dashboard-controls__count')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-terminal-dashboard-small.png', fullPage: true });
  await expect(page.locator('wa-select[name="terminal-grouping"]')).toHaveCount(0);
  const visibilitySelect = page.locator('wa-select[name="terminal-visibility-group"]'),
    visibilityPopupGeometry = () =>
      visibilitySelect.evaluate((node) => {
        const listbox = node.shadowRoot!.querySelector<HTMLElement>('[part="listbox"]')!,
          box = listbox.getBoundingClientRect(),
          combobox = node.shadowRoot!.querySelector<HTMLElement>('[part="combobox"]')!.getBoundingClientRect(),
          options = [...node.querySelectorAll<HTMLElement>('wa-option')].map((option) => {
            const bounds = option.getBoundingClientRect();
            return {
              left: bounds.left,
              right: bounds.right,
              scrollWidth: option.scrollWidth,
              clientWidth: option.clientWidth,
            };
          });
        return {
          left: box.left,
          right: box.right,
          width: box.width,
          triggerWidth: combobox.width,
          scrollWidth: listbox.scrollWidth,
          clientWidth: listbox.clientWidth,
          viewport: innerWidth,
          options,
        };
      });
  await visibilitySelect.click();
  await expect(visibilitySelect).toHaveJSProperty('open', true);
  await expect.poll(async () => (await visibilityPopupGeometry()).left).toBeGreaterThanOrEqual(10);
  let popupBounds = await visibilityPopupGeometry();
  expect(popupBounds.right).toBeLessThanOrEqual(popupBounds.viewport - 10);
  expect(popupBounds.width).toBeGreaterThan(popupBounds.triggerWidth);
  expect(popupBounds.scrollWidth).toBeLessThanOrEqual(popupBounds.clientWidth + 1);
  for (const option of popupBounds.options) {
    expect(option.left).toBeGreaterThanOrEqual(popupBounds.left);
    expect(option.right).toBeLessThanOrEqual(popupBounds.right);
    expect(option.scrollWidth).toBeLessThanOrEqual(option.clientWidth + 1);
  }
  await page.screenshot({ path: '/private/tmp/hs2-fkx6mb-visibility-select-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(visibilitySelect).toHaveJSProperty('open', false);
  await page.setViewportSize({ width: 760, height: 632 });
  await visibilitySelect.click();
  await expect(visibilitySelect).toHaveJSProperty('open', true);
  await expect(visibilitySelect.locator('[part="listbox"]')).toBeVisible();
  await expect.poll(async () => (await visibilityPopupGeometry()).left).toBeGreaterThanOrEqual(10);
  popupBounds = await visibilityPopupGeometry();
  expect(popupBounds.right).toBeLessThanOrEqual(popupBounds.viewport - 10);
  expect(popupBounds.scrollWidth).toBeLessThanOrEqual(popupBounds.clientWidth + 1);
  await page.screenshot({ path: '/private/tmp/hs2-fkx6mb-visibility-select-narrow.png', fullPage: true });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 1100, height: 720 });
  const liveTile = dashboard.locator('[data-terminal-key="demo-checkout:tests"]');
  await liveTile.getByRole('button', { name: 'Open Tests in demo' }).click();
  await expect(page.getByRole('button', { name: 'Workspace grid' })).toHaveAttribute('aria-pressed', 'false');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  await expect(drawer.getByRole('button', { name: 'Manage workspace visibility' })).toHaveCount(0);
  await expect(drawer.locator('wa-select[name="terminal-visibility-group"]')).toHaveCount(0);
  const dedicatedViewport = drawer.locator('[data-display-mode="interactive"]');
  await expect(dedicatedViewport).not.toHaveAttribute('data-grid-policy', 'dashboard-80x24');
  await expect(drawer.locator('.xterm-helper-textarea')).toBeFocused();
  await expect
    .poll(() => latestClaim('tests'))
    .toMatchObject({ cols: expect.any(Number), rows: expect.any(Number), focus: true });
  await expect
    .poll(async () => {
      const claim = await latestClaim('tests');
      return `${claim?.cols}x${claim?.rows}`;
    })
    .not.toBe('80x24');
  await expect(dedicatedViewport).toHaveAttribute('data-driving', 'true');
  await page.getByRole('button', { name: 'Workspace grid' }).focus();
  await expect(drawer.locator('.xterm-helper-textarea')).not.toBeFocused();
  await page.setViewportSize({ width: 1280, height: 840 });
  await expect.poll(() => latestClaim('tests')).toMatchObject({ focus: true });
  await expect
    .poll(async () => {
      const claim = await latestClaim('tests');
      return `${claim?.cols}x${claim?.rows}`;
    })
    .not.toBe('80x24');
  await expect(dedicatedViewport).not.toHaveAttribute('data-viewing-label');
  await expect(dedicatedViewport).not.toHaveAttribute('aria-description');
  await page.screenshot({ path: '/private/tmp/hs2-kke1pm-terminal-fit-with-external-focus-after.png', fullPage: true });
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await expect(dashboard).toBeVisible();
  await dashboard.locator('[data-terminal-key="demo-checkout:tests"]').dblclick();
  await expect(drawer.locator('.xterm-helper-textarea')).toBeFocused();
  await expect
    .poll(() => latestClaim('tests'))
    .toMatchObject({ cols: expect.any(Number), rows: expect.any(Number), focus: true });
  await expect(drawer.locator('[data-display-mode="interactive"]')).toHaveAttribute('data-driving', 'true');
  await page.screenshot({ path: '/private/tmp/hs2-terminal-dashboard-drawer-refit.png', fullPage: true });
});

test('preserves column view after visiting the terminal dashboard (HS2-BH8ZVD)', async ({ page }) => {
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await expect(page.getByRole('region', { name: 'Workspace grid' })).toBeVisible();
  await page.getByRole('tab', { name: /demo/ }).click();
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(page.getByLabel('Columns view')).toHaveAttribute('aria-pressed', 'true');
});

test('uses Kerf floating toolbars for workspace zoom and collapsed drawer restore (HS2-W3GPHW)', async ({
  page,
}, testInfo) => {
  const expectDarkChildren = async (toolbar: Locator, name: string) => {
    const group = toolbar.locator('[data-component="toolbar-control-group"]');
    for (const theme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: theme });
      await expect(group).toHaveCSS('color-scheme', 'dark');
      await expect(group).toHaveCSS('background-color', 'rgb(58, 58, 60)');
      await expect(group).toHaveCSS('color', 'rgb(174, 174, 178)');
      await expect(group.locator('button').first()).toHaveCSS('color', 'rgb(174, 174, 178)');
      await toolbar.screenshot({ path: testInfo.outputPath(`floating-${name}-${theme}.png`) });
    }
    await page.emulateMedia({ colorScheme: 'light' });
  };
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const workspace = page.getByRole('region', { name: 'Workspace grid' }),
    workspaceZoom = workspace.getByRole('toolbar', { name: 'Workspace tile zoom' });
  await expect(workspaceZoom).toHaveAttribute('data-component', 'floating-toolbar');
  await expect(workspaceZoom).toHaveAttribute('data-position', 'bottom-end');
  await expect(workspaceZoom.locator('[data-component="toolbar-control-group"]')).toHaveAttribute(
    'data-tone',
    'default',
  );
  await expectDarkChildren(workspaceZoom, 'workspace-zoom');
  await workspaceZoom.getByRole('button', { name: /Zoom in/ }).click();
  await expect(workspace).toHaveAttribute('data-fit', '3');
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-workspace-zoom-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(workspaceZoom).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-workspace-zoom-narrow.png', fullPage: true });

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('tab', { name: /demo/ }).click();
  const restore = page.getByRole('toolbar', { name: 'Terminal drawer controls' });
  await expect(restore).toHaveAttribute('data-component', 'floating-toolbar');
  await expect(restore.locator('[data-component="toolbar-control-group"]')).toHaveAttribute('data-tone', 'default');
  await expectDarkChildren(restore, 'drawer-restore');
  const mainColumn = page.locator('.app-shell__main');
  await expect
    .poll(async () => {
      const [mainBox, restoreBox] = await Promise.all([mainColumn.boundingBox(), restore.boundingBox()]);
      if (!mainBox || !restoreBox) return undefined;
      return {
        rightInset: Math.round(mainBox.x + mainBox.width - restoreBox.x - restoreBox.width),
        bottomInset: Math.round(mainBox.y + mainBox.height - restoreBox.y - restoreBox.height),
      };
    })
    .toEqual({ rightInset: 16, bottomInset: 16 });
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-drawer-restore-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(restore).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-drawer-restore-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  await restore.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toHaveAttribute('data-mode', 'grid');
  const drawerGrid = drawer.getByRole('region', { name: 'Workspace grid' }),
    drawerZoom = drawerGrid.getByRole('toolbar', { name: 'Workspace tile zoom' });
  await expect(drawerZoom).toHaveAttribute('data-component', 'floating-toolbar');
  await expectDarkChildren(drawerZoom, 'drawer-zoom');
  await drawerZoom.getByRole('button', { name: /Zoom in/ }).click();
  await expect(drawerGrid).toHaveAttribute('data-fit', '1');
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-drawer-grid-zoom.png', fullPage: true });
});

test('paints terminal dashboard transitions immediately and progressively mounts only visible previews at scale (HS2-XEGA6V)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page, true, false, 0, 0, 0, false, 48);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' });
  await expect(dashboard.locator('[data-component="terminal-tile"]')).toHaveCount(48);
  await expect.poll(() => page.locator('.xterm').count()).toBeGreaterThan(0);
  const initialRuntimeCount = await page.locator('.xterm').count();
  expect(initialRuntimeCount).toBeLessThan(48);
  await expect(dashboard.locator('[data-terminal-key="demo-checkout:worker-48"] .xterm')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-xega6v-progressive-terminal-dashboard.png', fullPage: true });
  const projectTransition = await page.getByRole('tab', { name: /demo/ }).evaluate((tab) => {
    const start = performance.now();
    (tab as HTMLElement).click();
    return {
      elapsed: performance.now() - start,
      projectVisible: Boolean(
        document.querySelector('[data-component="ticket-list"], [data-component="ticket-board"]'),
      ),
      terminalVisible: Boolean(document.querySelector('[data-component="terminal-dashboard"]')),
    };
  });
  expect(projectTransition.projectVisible).toBe(true);
  expect(projectTransition.terminalVisible).toBe(false);
  expect(projectTransition.elapsed).toBeLessThan(100);
  const dashboardTransition = await page.getByRole('button', { name: 'Workspace grid' }).evaluate((button) => {
    const start = performance.now();
    (button as HTMLElement).click();
    return {
      elapsed: performance.now() - start,
      dashboardVisible: Boolean(document.querySelector('[data-component="terminal-dashboard"]')),
    };
  });
  expect(dashboardTransition.dashboardVisible).toBe(true);
  expect(dashboardTransition.elapsed).toBeLessThan(100);
  await expect(dashboard).toBeVisible();
  await expect(dashboard.locator('[data-component="terminal-tile"]')).toHaveCount(48);
  const lastTile = dashboard.locator('[data-terminal-key="demo-checkout:worker-48"]'),
    lastViewport = lastTile.locator('[data-component="terminal-viewport"]');
  await lastTile.scrollIntoViewIfNeeded();
  await expect(lastViewport).toHaveAttribute('data-connection', 'connected', { timeout: 15_000 });
  await expect(lastViewport).toHaveAttribute('data-geometry-ready', 'true');
  await expect(lastTile.locator('.xterm')).toBeVisible();
});

for (const theme of ['light', 'dark'] as const) {
  test(`projects Kerf workspace segments through mode transitions in ${theme} (HS2-F29QAT)`, async ({ page }) => {
    await page.setViewportSize({ width: 1728, height: 971 });
    await page.emulateMedia({ colorScheme: theme });
    await installFakeTerminalSockets(page, true);
    await mockProject(page);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open project' }).click();
    await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
    const toolbar = page.locator('.app-shell__main > .kui-toolbar');
    const segments = toolbar.getByRole('group', { name: 'View mode', exact: true });
    const icons = { list: 'list', board: 'columns-3', notifications: 'bell', settings: 'settings' };
    const expectMode = async (group: Locator, selected: keyof typeof icons, modes = Object.keys(icons)) => {
      await expect(group).toHaveAttribute('data-component', 'segmented-control');
      await expect(group).toHaveAttribute('data-appearance', 'toolbar');
      await expect(group).toHaveAttribute('data-value', selected);
      await expect(group.getByRole('button')).toHaveCount(modes.length);
      for (const mode of modes) {
        const button = group.locator(`[data-segment-value="${mode}"]`);
        await expect(button).toHaveAttribute('data-action', 'set-view-mode');
        await expect(button).toHaveAttribute('tabindex', '0');
        await expect(button).toHaveAttribute('data-selected', String(mode === selected));
        await expect(button).toHaveAttribute('aria-pressed', String(mode === selected));
        await expect(button.locator(`[data-lucide="${icons[mode as keyof typeof icons]}"]`)).toBeVisible();
      }
    };
    await expectMode(segments, 'list');
    await expect(segments).toHaveAttribute('data-layout', 'content');
    await expect(segments).toHaveAttribute('data-shape', 'pill');
    const listMode = segments.getByRole('button', { name: 'List view' });
    await listMode.click();
    await expect(listMode).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(segments.getByRole('button', { name: 'Columns view' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expectMode(segments, 'board');
    await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
    await page.screenshot({ path: `/private/tmp/hs2-f29qat-workspace-${theme}-wide.png`, animations: 'disabled' });
    await segments.getByRole('button', { name: 'Notifications view' }).click();
    await expectMode(segments, 'notifications');
    await expect(page.locator('[data-component="notification-center"]')).toBeVisible();
    await segments.getByRole('button', { name: 'Settings view' }).click();
    await expectMode(segments, 'settings');
    await expect(page.getByRole('complementary', { name: 'Settings categories' })).toBeVisible();
    // An independent state producer resets the selected control; another native edit still works.
    await page.keyboard.press('ControlOrMeta+Shift+L');
    await expectMode(segments, 'list');
    await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
    await segments.getByRole('button', { name: 'Columns view' }).click();
    await expectMode(segments, 'board');
    await page.setViewportSize({ width: 390, height: 844 });
    await expectMode(segments, 'list', ['list', 'notifications', 'settings']);
    await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
    await page.screenshot({ path: `/private/tmp/hs2-f29qat-workspace-${theme}-mobile.png`, animations: 'disabled' });
    await page.setViewportSize({ width: 1728, height: 971 });
    await expectMode(segments, 'board');
    await page.getByRole('button', { name: 'Workspace grid' }).click();
    const rail = page.locator('[data-component="terminal-ticket-rail"]');
    const railSegments = rail.getByRole('group', { name: 'View mode', exact: true });
    await expectMode(railSegments, 'list', ['list', 'notifications']);
    await expect(railSegments).toHaveAttribute('data-layout', 'equal');
    await expect(railSegments).toHaveAttribute('data-shape', 'rounded');
    const widths = await railSegments
      .getByRole('button')
      .evaluateAll((buttons) => buttons.map((button) => button.getBoundingClientRect().width));
    expect(widths[0]).toBeCloseTo(widths[1], 0);
    expect(widths[0]).toBeGreaterThan(100);
    await railSegments.getByRole('button', { name: 'Notifications view' }).click();
    await expectMode(railSegments, 'notifications', ['list', 'notifications']);
    await expect(rail.locator('[data-component="notification-center"]')).toBeVisible();
    await railSegments.getByRole('button', { name: 'List view' }).click();
    await expectMode(railSegments, 'list', ['list', 'notifications']);
    await expect(rail.locator('[data-component="ticket-list"]')).toBeVisible();
    await rail.screenshot({ path: `/private/tmp/hs2-f29qat-rail-${theme}-wide.png`, animations: 'disabled' });
    await page.setViewportSize({ width: 1024, height: 600 });
    await railSegments.getByRole('button', { name: 'Notifications view' }).click();
    await expectMode(railSegments, 'notifications', ['list', 'notifications']);
    await page.screenshot({ path: `/private/tmp/hs2-f29qat-rail-${theme}-narrow.png`, animations: 'disabled' });
  });
}

test('keeps a compact ticket rail beside the terminal dashboard and pushes into the inspector', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const rail = page.locator('[data-component="terminal-ticket-rail"]'),
    projectSelect = rail.locator('wa-select[name="terminal-rail-project"]'),
    viewSelect = rail.locator('wa-select[name="terminal-rail-view"]'),
    launcher = rail.getByRole('button', { name: 'Ticket…' });
  await expect(rail).toBeVisible();
  await expect(projectSelect).toHaveAttribute('value', 'demo-checkout');
  await expect(viewSelect).toHaveAttribute('value', 'all');
  await expect(launcher).toHaveClass(/quick-ticket-composer__launcher/);
  await expect(rail.getByRole('button', { name: /List view/ })).toBeVisible();
  await expect(rail.getByRole('button', { name: /Notifications view/ })).toBeVisible();
  await expect(rail.getByRole('button', { name: /Columns view/ })).toBeHidden();
  await expect(rail.getByRole('button', { name: /Settings view/ })).toBeHidden();
  const railGeometry = await rail.evaluate((node) => {
    const modeElement = node.querySelector<HTMLElement>('.view-mode-switcher')!,
      mode = modeElement.getBoundingClientRect(),
      sort = node.querySelector('.workspace-header__sort-group')!.getBoundingClientRect(),
      search = node.querySelector('.workspace-header__search-group')!.getBoundingClientRect(),
      utility = node.querySelector('.workspace-header__utility-group')!.getBoundingClientRect(),
      project = node.querySelector('wa-select[name="terminal-rail-project"]')!.getBoundingClientRect(),
      hide = node.querySelector<HTMLElement>('[aria-label="Hide ticket rail"]')!.getBoundingClientRect(),
      heading = getComputedStyle(node.querySelector<HTMLElement>('.terminal-ticket-rail__heading')!),
      launcherStyle = getComputedStyle(node.querySelector<HTMLElement>('.quick-ticket-composer__launcher')!);
    return {
      modeBottom: mode.bottom,
      modeWidth: mode.width,
      modeRadius: getComputedStyle(modeElement).borderRadius,
      sortTop: sort.top,
      searchTop: search.top,
      searchLeft: search.left,
      utilityTop: utility.top,
      utilityRight: utility.right,
      projectWidth: project.width,
      hideRight: hide.right,
      railRight: node.getBoundingClientRect().right,
      railWidth: node.getBoundingClientRect().width,
      headingBorderBottom: heading.borderBottomWidth,
      launcherBackground: launcherStyle.backgroundColor,
    };
  });
  expect(railGeometry.modeBottom).toBeLessThanOrEqual(railGeometry.sortTop);
  expect(railGeometry.searchTop).toBeCloseTo(railGeometry.sortTop, 0);
  expect(railGeometry.utilityTop).toBeCloseTo(railGeometry.sortTop, 0);
  expect(railGeometry.searchLeft).toBeGreaterThanOrEqual(railGeometry.utilityRight);
  expect(railGeometry.modeRadius).not.toBe('9999px');
  expect(railGeometry.hideRight).toBeGreaterThan(railGeometry.railRight - 24);
  expect(railGeometry.modeWidth).toBeGreaterThan(railGeometry.railWidth * 0.8);
  expect(railGeometry.projectWidth).toBeLessThan(railGeometry.railWidth * 0.8);
  expect(railGeometry.headingBorderBottom).toBe('1px');
  expect(railGeometry.launcherBackground).not.toBe('rgba(0, 0, 0, 0)');
  await launcher.click();
  const composer = page.getByRole('dialog', { name: 'Create ticket' });
  await expect(composer).toBeVisible();
  await composer.getByRole('button', { name: 'Cancel' }).click();
  await expect(composer).toBeHidden();
  await page.screenshot({ path: '/private/tmp/hs2-r292m4-workspace-grid-rail-wide.png', fullPage: true });
  await viewSelect.click();
  await viewSelect.locator('wa-option[value="backlog"]').click();
  await expect(viewSelect).toHaveJSProperty('value', 'backlog');
  await viewSelect.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'all';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(viewSelect).toHaveJSProperty('value', 'all');
  await rail.getByRole('button', { name: 'Search tickets' }).click();
  const expandedSearch = rail.locator('.workspace-header__search-group'),
    searchInput = rail.getByRole('searchbox', { name: 'Search tickets' });
  await expect(expandedSearch).toHaveAttribute('data-expanded', 'true');
  await expect.poll(() => expandedSearch.evaluate((node) => node.getBoundingClientRect().width)).toBeGreaterThan(80);
  await expect(expandedSearch).toHaveCSS('border-width', '1px');
  await expect(expandedSearch).toHaveCSS('border-style', 'solid');
  await expect(expandedSearch).not.toHaveCSS('box-shadow', 'none');
  await expect(expandedSearch.locator('.kui-token-search')).toHaveCSS('border-width', '0px');
  await rail.screenshot({ path: '/private/tmp/hs2-tnsd4k-rail-search-empty-focused.png', animations: 'disabled' });
  await searchInput.fill('has:attachment ');
  await expect(rail.locator('[data-component="token-search-token"][data-token-value="has:attachment"]')).toBeVisible();
  await rail.getByRole('button', { name: 'List view', exact: true }).focus();
  await expect(expandedSearch).toHaveAttribute('data-expanded', 'true');
  await expect(expandedSearch).toHaveCSS('border-width', '1px');
  await expect(expandedSearch).toHaveCSS('box-shadow', 'none');
  await searchInput.focus();
  await expect(expandedSearch).not.toHaveCSS('box-shadow', 'none');
  await searchInput.fill('updated-after:4h ago');
  await searchInput.press('Enter');
  await expect(
    rail.locator('[data-component="token-search-token"][data-token-value="updated-after:4h ago"]'),
  ).toBeVisible();
  await rail.getByRole('button', { name: 'Search syntax help' }).click();
  await expect(rail.getByRole('dialog', { name: 'Search syntax' })).toBeVisible();
  await rail.getByRole('button', { name: 'Search syntax help' }).click();
  await page.screenshot({ path: '/private/tmp/hs2-egekzg-terminal-ticket-rail-search-wide.png', fullPage: true });
  await rail.getByRole('button', { name: 'Clear search' }).click();
  await rail.getByRole('button', { name: 'List view', exact: true }).focus();
  await expect(expandedSearch).toHaveAttribute('data-expanded', 'false');
  await expect(expandedSearch).toHaveCSS('border-width', '1px');
  await rail.getByRole('button', { name: 'Search tickets' }).click();
  await expect(searchInput).toBeFocused();
  await expect(expandedSearch).toHaveCSS('border-width', '1px');
  await expect(expandedSearch).not.toHaveCSS('box-shadow', 'none');
  await rail.getByRole('button', { name: 'Hide ticket rail' }).click();
  const railRegion = page.locator('section[data-region-id="app-inspector"]');
  await expect(railRegion).toHaveAttribute('data-collapsed', 'true');
  await page.getByRole('button', { name: 'Show ticket rail' }).click();
  await expect(railRegion).toHaveAttribute('data-collapsed', 'false');
  await expect(rail).toBeVisible();
  await expect(rail.locator('[data-ticket-slug="HS2-START03"]')).toBeVisible();
  await rail.locator('[data-ticket-slug="HS2-START03"]').click();
  await expect(rail).toHaveAttribute('data-screen', 'ticket');
  const back = rail.getByRole('button', { name: 'Back to ticket list' });
  await expect(back).toBeVisible();
  await expect(rail.getByRole('button', { name: /Copy ticket number HS2-START03/ })).toBeVisible();
  await expect(rail.locator('[data-component="ticket-inspector"]')).toBeVisible();
  await rail.locator('[data-component="content-transition"]').evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  const railInfo = rail.getByRole('tab', { name: 'Info' }),
    railTimeline = rail.getByRole('tab', { name: 'Timeline' });
  await expect(railInfo).toHaveAttribute('aria-selected', 'true');
  await railInfo.focus();
  await page.keyboard.press('ArrowRight');
  await expect(railTimeline).toHaveAttribute('aria-selected', 'true');
  const inspectorHeader = await rail.evaluate((node) => {
    const railBox = node.getBoundingClientRect(),
      backBox = node.querySelector<HTMLElement>('.terminal-ticket-rail__back')!.getBoundingClientRect(),
      slugBox = node.querySelector<HTMLElement>('.ticket-inspector__slug')!.getBoundingClientRect();
    return {
      backCenter: backBox.top + backBox.height / 2,
      slugCenter: slugBox.top + slugBox.height / 2,
      slugHorizontalCenter: slugBox.left + slugBox.width / 2,
      railHorizontalCenter: railBox.left + railBox.width / 2,
      backWidth: backBox.width,
      backColor: getComputedStyle(node.querySelector('.terminal-ticket-rail__back')!).color,
    };
  });
  expect(inspectorHeader.backCenter).toBeCloseTo(inspectorHeader.slugCenter, 0);
  expect(inspectorHeader.slugHorizontalCenter).toBeCloseTo(inspectorHeader.railHorizontalCenter, 0);
  expect(inspectorHeader.backWidth).toBeGreaterThanOrEqual(36);
  expect(inspectorHeader.backColor).not.toBe('rgb(0, 0, 0)');
  await page.screenshot({ path: '/private/tmp/hs2-gzn2hz-terminal-ticket-tabs-wide.png', fullPage: true });
  await back.click();
  await expect(rail).toHaveAttribute('data-screen', 'root');
  await page.waitForTimeout(350);
  await rail.getByRole('button', { name: /Notifications view/ }).click();
  await expect(rail.locator('[data-component="notification-center"]')).toBeVisible();
  await rail.getByRole('button', { name: /List view/ }).click();
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(page.getByRole('region', { name: 'Workspace grid' })).toBeVisible();
  await expect(rail).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-r292m4-workspace-grid-rail-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(expandedSearch).toHaveAttribute('data-expanded', 'false');
  await rail.getByRole('button', { name: 'Search tickets' }).click();
  await expect.poll(() => expandedSearch.evaluate((node) => node.getAnimations().length)).toBeGreaterThan(0);
  await searchInput.fill('tag:');
  const suggestions = rail.getByRole('listbox', { name: 'Matching tags' });
  await expect(suggestions).toBeVisible();
  await expandedSearch.evaluate(async (node) => {
    await Promise.all(node.getAnimations().map((animation) => animation.finished));
  });
  const activeSearchGeometry = await rail.evaluate((node) => {
    const bounds = (selector: string) => {
        const rect = node.querySelector<HTMLElement>(selector)!.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right, width: rect.width };
      },
      options = [...node.querySelectorAll<HTMLElement>('.workspace-header__search-suggestions button')].map(
        (option) => ({
          left: option.getBoundingClientRect().left,
          right: option.getBoundingClientRect().right,
          width: option.getBoundingClientRect().width,
          textAlign: getComputedStyle(option).textAlign,
        }),
      );
    return {
      actions: bounds('.workspace-header__actions'),
      search: bounds('.workspace-header__search-group'),
      sort: bounds('.workspace-header__sort-group'),
      utility: bounds('.workspace-header__utility-group'),
      suggestions: bounds('.workspace-header__search-suggestions'),
      options,
    };
  });
  expect(activeSearchGeometry.search.top).toBeGreaterThan(
    Math.max(activeSearchGeometry.sort.bottom, activeSearchGeometry.utility.bottom),
  );
  expect(activeSearchGeometry.search.left).toBeCloseTo(activeSearchGeometry.actions.left, 0);
  expect(activeSearchGeometry.search.right).toBeCloseTo(activeSearchGeometry.actions.right, 0);
  expect(activeSearchGeometry.options.length).toBeGreaterThan(0);
  for (const option of activeSearchGeometry.options) {
    expect(option.textAlign).toBe('left');
    expect(option.left).toBeGreaterThanOrEqual(activeSearchGeometry.suggestions.left);
    expect(option.right).toBeLessThanOrEqual(activeSearchGeometry.suggestions.right);
    expect(option.width).toBeGreaterThan(activeSearchGeometry.suggestions.width - 24);
  }
  await page.screenshot({ path: '/private/tmp/hs2-rqfz8n-f6n412-sidebar-search-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-rqfz8n-f6n412-sidebar-search-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  await rail.getByRole('button', { name: 'Clear search' }).click();
});

test('streams ANSI terminal output, input, viewport leases, driver state, and reconnects without exposing credentials', async ({
  page,
}) => {
  await installFakeTerminalSockets(page);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const viewport = page.locator('[data-component="terminal-viewport"][data-terminal-id="codex-main"]').first();
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport).toHaveAttribute('data-renderer', 'dom');
  await expect(viewport).toHaveAttribute('data-display-mode', 'scaled-preview');
  await expect(viewport).toHaveAttribute('data-grid-size', '80x24');
  await expect(viewport.locator('.xterm-rows')).toContainText('Live terminal ready');
  await expect(viewport).toHaveAttribute('data-pty-size', '100x30');
  await expect(viewport).toHaveAttribute('data-driving', 'true');
  await viewport.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type('ignored-dashboard-input');
  const previewInputCount = await page.evaluate(
    () =>
      (window as unknown as { __terminalSockets: Array<{ sent: unknown[] }> }).__terminalSockets
        .flatMap((socket) => socket.sent)
        .filter((value) => typeof value === 'string' && !value.startsWith('{')).length,
  );
  expect(previewInputCount).toBe(0);
  await expect(viewport.locator('.xterm-rows')).not.toContainText('ignored-dashboard-input');
  const protocolSnapshot = () =>
    page.evaluate(() => {
      const sockets = (
        window as unknown as { __terminalSockets: Array<{ url: string; sent: unknown[] }> }
      ).__terminalSockets.filter((socket) =>
        socket.sent.some((value) => typeof value === 'string' && value.includes('viewer_id')),
      );
      return {
        urls: sockets.map((socket) => socket.url),
        claims: sockets.flatMap((socket) =>
          socket.sent
            .filter((value): value is string => typeof value === 'string' && value.includes('viewer_id'))
            .map((value) => JSON.parse(value)),
        ),
      };
    });
  await expect
    .poll(async () => {
      const protocol = await protocolSnapshot();
      return protocol.claims.some(
        (claim) => claim.resize.cols === 80 && claim.resize.rows === 24 && claim.resize.focus && claim.resize.visible,
      );
    })
    .toBe(true);
  const protocol = await protocolSnapshot();
  expect(protocol.urls.filter((url) => !url.includes('/__hotsheet/project-api/') || url.includes('secret'))).toEqual(
    [],
  );
  await page.setViewportSize({ width: 1100, height: 720 });
  const dashboard = page.getByRole('region', { name: 'Workspace grid' });
  await dashboard.getByRole('button', { name: /Zoom out, fit more items high/ }).click();
  const preview = dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]');
  await expect(preview).toHaveAttribute('data-preview-only', 'true');
  await preview.click();
  const magnified = page
    .getByRole('dialog', { name: 'Magnified Codex Main' })
    .locator('[data-display-mode="interactive"]');
  await expect(magnified).toHaveAttribute('data-connection', 'connected');
  await expect(magnified.locator('.xterm-helper-textarea')).toBeFocused();
  await page.keyboard.type('browser input');
  await expect(magnified.locator('.xterm-rows')).toContainText('browser input');
  const viewers = await page.evaluate(() => {
    const sockets = (window as unknown as { __terminalSockets: Array<{ sent: unknown[] }> }).__terminalSockets.filter(
      (socket) => socket.sent.some((value) => typeof value === 'string' && value.includes('viewer_id')),
    );
    return sockets.map(
      (socket) =>
        JSON.parse(socket.sent.find((value) => typeof value === 'string' && value.includes('viewer_id')) as string)
          .resize.viewer_id,
    );
  });
  expect(new Set(viewers).size).toBe(viewers.length);
  await expect(magnified).not.toHaveAttribute('data-grid-size', '80x24');
  const magnifiedGrid = await magnified.getAttribute('data-grid-size');
  expect(magnifiedGrid).toMatch(/^\d+x\d+$/);
  await page.evaluate(() => {
    const sockets = (
        window as unknown as {
          __terminalSockets: Array<{ sent: unknown[]; emitSize(cols: number, rows: number, driver: string): void }>;
        }
      ).__terminalSockets.filter((socket) =>
        socket.sent.some((value) => typeof value === 'string' && value.includes('viewer_id')),
      ),
      driver = JSON.parse(
        sockets[0].sent.find((value) => typeof value === 'string' && value.includes('viewer_id')) as string,
      ).resize.viewer_id;
    sockets.at(-1)!.emitSize(300, 120, driver);
  });
  await expect(magnified).toHaveAttribute('data-driving', 'false');
  await expect(magnified).toHaveAttribute('data-pty-size', '300x120');
  await expect(magnified).toHaveAttribute('data-grid-size', magnifiedGrid!);
  await expect(magnified).toHaveAttribute('data-scale', '1');
  await expect(magnified).not.toHaveAttribute('data-viewing-label');
  await page.waitForTimeout(200);
  await magnified.locator('.xterm-helper-textarea').evaluate((node) => {
    (node as HTMLElement).blur();
  });
  await page.evaluate(() => {
    const sockets = (
        window as unknown as {
          __terminalSockets: Array<{ url: string; emitSize(cols: number, rows: number, driver: string): void }>;
        }
      ).__terminalSockets,
      driver = sockets.find((socket) => socket.url.includes('tests'))!.url;
    sockets
      .filter((socket) => socket.url.includes('codex-main'))
      .at(-1)!
      .emitSize(300, 120, driver);
  });
  await expect(magnified).toHaveAttribute('data-grid-size', magnifiedGrid!);
  await expect(magnified).toHaveAttribute('data-sizing-focus', 'true');
  await expect(magnified).toHaveAttribute('data-scale', '1');
  await expect(magnified).not.toHaveAttribute('data-viewing-label');
  const contained = await magnified.evaluate((element) => {
    const viewport = element.getBoundingClientRect(),
      style = getComputedStyle(element),
      screen = element.querySelector<HTMLElement>('.xterm-screen')!.getBoundingClientRect();
    return (
      screen.right <= viewport.right - parseFloat(style.paddingRight) + 1 &&
      screen.bottom <= viewport.bottom - parseFloat(style.paddingBottom) + 1
    );
  });
  expect(contained).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-pd4mz9-live-terminal.png', fullPage: true });
  const before = await page.evaluate(
    () => (window as unknown as { __terminalSockets: unknown[] }).__terminalSockets.length,
  );
  await page.evaluate(() => {
    (window as unknown as { __terminalSockets: Array<{ sent: unknown[]; close(): void }> }).__terminalSockets
      .filter((socket) => socket.sent.some((value) => typeof value === 'string' && value.includes('viewer_id')))
      .at(-1)!
      .close();
  });
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __terminalSockets: unknown[] }).__terminalSockets.length))
    .toBeGreaterThan(before);
});

test('keeps dashboard terminals inset and scaled through aggressive viewport resizing', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1100 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    tile = dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]'),
    preview = tile.locator('[data-display-mode="scaled-preview"]');
  await expect(preview).toHaveAttribute('data-connection', 'connected');
  await expect(preview).toHaveAttribute('data-grid-size', /\d+x\d+/);
  const initialGrid = await preview.getAttribute('data-grid-size');
  await page.evaluate(() => {
    const socket = (
      window as unknown as { __terminalSockets: Array<{ url: string; emitBytes(value: string): void }> }
    ).__terminalSockets.find((item) => item.url.includes('codex-main'));
    socket?.emitBytes(
      '\u001b[2J\u001b[H  GNU nano — terminal resize proof\u001b[999;1H^G Help   ^O Write Out   ^X Exit',
    );
  });
  await expect(preview.locator('.xterm-rows')).toContainText('GNU nano');
  await expect(preview.locator('.xterm-rows')).toContainText('Help');
  const initialScale = Number(await preview.getAttribute('data-scale'));
  await dashboard.getByRole('button', { name: /Zoom in, fit fewer items across/ }).click();
  await expect.poll(async () => Number(await preview.getAttribute('data-scale'))).toBeGreaterThan(initialScale);
  await expect(preview).toHaveAttribute('data-grid-size', initialGrid!);
  for (const size of [
    { width: 980, height: 590 },
    { width: 1600, height: 1000 },
    { width: 840, height: 560 },
    { width: 1280, height: 820 },
  ]) {
    await page.setViewportSize(size);
    await expect
      .poll(async () =>
        tile.evaluate((element) => {
          const box = element.getBoundingClientRect(),
            body = element.querySelector('.terminal-tile__preview')!.getBoundingClientRect(),
            footer = element.querySelector('.terminal-tile__footer')!.getBoundingClientRect();
          return (
            Math.abs(box.height - (body.height + footer.height)) <= 2.5 &&
            footer.bottom <= box.bottom + 1 &&
            body.top >= box.top - 1
          );
        }),
      )
      .toBe(true);
    await expect(preview).toHaveAttribute('data-natural-size', '1280x768');
    await expect(preview).toHaveAttribute('data-grid-size', initialGrid!);
    expect(Number(await preview.getAttribute('data-scale'))).toBeGreaterThan(0);
    expect(
      await preview.evaluate((element) => {
        const frame = element.parentElement!.getBoundingClientRect(),
          screen = element.querySelector<HTMLElement>('.xterm-screen')!.getBoundingClientRect();
        return (
          screen.left >= frame.left - 1 &&
          screen.top >= frame.top - 1 &&
          screen.right <= frame.right + 1 &&
          screen.bottom <= frame.bottom + 1
        );
      }),
    ).toBe(true);
  }
  await page.screenshot({ path: '/private/tmp/hs2-281qc8-terminal-resize-after.png', fullPage: true });
});

test('centers, focuses, and opens the magnified terminal from its footer', async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 760 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    tile = dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]');
  await tile.click();
  const magnified = page.getByRole('dialog', { name: 'Magnified Codex Main' }),
    magnifiedTile = magnified.locator('[data-component="terminal-tile"]');
  await expect(magnified.locator('.xterm-helper-textarea')).toBeFocused();
  const geometry = await magnified.evaluate((element) => {
    const overlay = element.getBoundingClientRect(),
      tile = element.querySelector('[data-component="terminal-tile"]')!.getBoundingClientRect();
    return {
      overlay: { x: overlay.x, y: overlay.y, width: overlay.width, height: overlay.height },
      tileCenter: { x: tile.x + tile.width / 2, y: tile.y + tile.height / 2 },
      viewport: { width: innerWidth, height: innerHeight },
    };
  });
  expect(geometry.overlay).toEqual({ x: 0, y: 0, width: geometry.viewport.width, height: geometry.viewport.height });
  expect(Math.abs(geometry.tileCenter.x - geometry.viewport.width / 2)).toBeLessThanOrEqual(1);
  expect(Math.abs(geometry.tileCenter.y - geometry.viewport.height / 2)).toBeLessThanOrEqual(1);
  await expect(magnified.getByRole('button', { name: 'Open Codex Main in project terminal drawer' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-kc7tv2-magnified-terminal-after.png', fullPage: true });
  await magnified.getByRole('button', { name: 'Open Codex Main in project terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]').click();
  const reopened = page.getByRole('dialog', { name: 'Magnified Codex Main' });
  await reopened.locator('.terminal-tile__footer').dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  await expect(magnifiedTile).toHaveCount(0);
});

test('opens, navigates, resizes, zooms, creates, hides, and restores the project terminal drawer', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toBeVisible();
  const projectTab = page.locator('[data-tab-kind="project"]');
  await expect(projectTab.locator('.kui-app-tab__trailing')).toHaveCount(1);
  expect(
    parseFloat(
      await projectTab.locator('.kui-app-tab__trailing').evaluate((element) => getComputedStyle(element).minWidth),
    ),
  ).toBeGreaterThan(19);
  await projectTab.click({ button: 'right', modifiers: ['Alt'] });
  const projectMenu = page.getByRole('menu', { name: 'Project tab actions' });
  await expect(projectMenu.getByText('Close Tabs to the Left')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const globalTile = page.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:codex-main"]');
  await globalTile.click({ button: 'right' });
  await page.getByRole('menu').getByText('Open').click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toBeVisible();
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  const selectedTerminalTab = drawer.locator('[data-tab-kind="terminal"][data-selected="true"]');
  await expect(selectedTerminalTab.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
  await expect(selectedTerminalTab).toContainText('Codex Main');
  const selectedSurfaces = await selectedTerminalTab.evaluate((element) => ({
    root: getComputedStyle(element).backgroundColor,
    select: getComputedStyle(element.querySelector('.kui-app-tab__select')!).backgroundColor,
  }));
  expect(selectedSurfaces.root).not.toBe('rgba(0, 0, 0, 0)');
  expect(selectedSurfaces.select).toBe('rgba(0, 0, 0, 0)');
  const handle = page.getByRole('separator', { name: 'Resize Terminal drawer' });
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(handle).toHaveAttribute('aria-valuenow', '320');
  await handle.focus();
  await page.keyboard.press('ArrowUp');
  await expect(handle).toHaveAttribute('aria-valuenow', '336');
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.layout.app-terminal-drawer.size'))).resolves.toBe(
    '336',
  );
  await drawer.getByRole('tab', { name: 'Project grid' }).click();
  await expect(drawer).toHaveAttribute('data-mode', 'grid');
  const dashboard = drawer.getByRole('region', { name: 'Workspace grid' });
  await expect(dashboard).toHaveAttribute('data-basis', 'high');
  await expect(dashboard).toHaveAttribute('data-fit', '2');
  await dashboard.getByRole('button', { name: /Zoom in/ }).click();
  await expect(dashboard).toHaveAttribute('data-fit', '1');
  const fitOne = await dashboard.evaluate((element) => {
    const content = element.querySelector('.terminal-dashboard__content')!.getBoundingClientRect(),
      tiles = [...element.querySelectorAll<HTMLElement>('.terminal-tile')].map((item) => item.getBoundingClientRect());
    return {
      contentBottom: content.bottom,
      paddingBottom: parseFloat(getComputedStyle(element.querySelector('.terminal-dashboard__content')!).paddingBottom),
      tiles: tiles.map((tile) => ({ x: tile.x, y: tile.y, bottom: tile.bottom })),
    };
  });
  expect(fitOne.tiles[0].bottom).toBeLessThanOrEqual(fitOne.contentBottom - fitOne.paddingBottom + 1);
  expect(fitOne.tiles[1].y).toBeCloseTo(fitOne.tiles[0].y, 0);
  expect(fitOne.tiles[1].x).toBeGreaterThan(fitOne.tiles[0].x);
  await page.screenshot({ path: '/private/tmp/hs2-5rj7t0-drawer-fit-one.png', fullPage: true });
  await dashboard.getByRole('button', { name: /Zoom out/ }).click();
  await expect(dashboard).toHaveAttribute('data-fit', '2');
  const wrapped = await dashboard
    .locator('.terminal-tile')
    .evaluateAll((items) =>
      items.map((item) => ({ x: item.getBoundingClientRect().x, y: item.getBoundingClientRect().y })),
    );
  expect(wrapped[1].y).toBeCloseTo(wrapped[0].y, 0);
  expect(wrapped[1].x).toBeGreaterThan(wrapped[0].x);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.terminals.drawer-fit-high'))).resolves.toBe('2');
  await page.screenshot({ path: '/private/tmp/hs2-5rj7t0-drawer-grid.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-drawer-grid-zoom.png', fullPage: true });
  const drawerZoom = dashboard.getByRole('toolbar', { name: 'Workspace tile zoom' });
  await expect(drawerZoom).toHaveAttribute('data-component', 'floating-toolbar');
  const codexTile = drawer.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:codex-main"]');
  await codexTile.click({ button: 'right' });
  await expect(drawer.getByRole('menu')).toHaveCount(0);
  await expect(drawer.getByRole('button', { name: 'Manage workspace visibility' })).toHaveCount(0);
  await expect(drawer.locator('wa-select[name="terminal-visibility-group"]')).toHaveCount(0);
  await expect(codexTile).toHaveCount(1);
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('Default shell').click();
  await expect(drawer.getByRole('tab', { name: /Terminal New/ })).toHaveAttribute('aria-selected', 'true');
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  const dedicated = drawer.locator('[data-component="terminal-session"]:not([hidden])');
  await expect(dedicated).toBeVisible();
  await expect(drawer.locator('[data-component="terminal-tile"]')).toHaveCount(0);
  await expect(drawer.getByRole('toolbar', { name: 'Workspace tile zoom' })).toHaveCount(0);
  const viewport = dedicated.locator('[data-component="terminal-viewport"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport).toHaveAttribute('data-renderer', 'webgl');
  const dedicatedSurface = await dedicated.evaluate((element) => {
    const viewportElement = element.querySelector<HTMLElement>('[data-component="terminal-viewport"]')!,
      terminalElement = viewportElement.querySelector<HTMLElement>('.terminal')!;
    return {
      session: getComputedStyle(element).backgroundColor,
      viewport: getComputedStyle(viewportElement).backgroundColor,
      terminal: getComputedStyle(terminalElement).backgroundColor,
      padding: getComputedStyle(viewportElement).padding,
      expectedPadding: (() => {
        const probe = document.createElement('div');
        probe.style.padding = 'var(--kui-space-xs)';
        document.body.append(probe);
        const value = getComputedStyle(probe).padding;
        probe.remove();
        return value;
      })(),
    };
  });
  expect(dedicatedSurface.viewport).toBe(dedicatedSurface.session);
  expect(dedicatedSurface.terminal).toBe(dedicatedSurface.session);
  expect(dedicatedSurface.padding).toBe(dedicatedSurface.expectedPadding);
  await expect(viewport.locator('.xterm-screen canvas').first()).toBeVisible();
  await viewport.click();
  await expect(viewport.locator('.xterm-helper-textarea')).toBeFocused();
  await page.keyboard.type('drawer input');
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as unknown as { __terminalSockets: Array<{ sent: unknown[] }> }).__terminalSockets
          .flatMap((socket) => socket.sent)
          .filter((value): value is string => typeof value === 'string' && !value.startsWith('{'))
          .join(''),
      ),
    )
    .toContain('drawer input');
  const socketCount = () =>
      page.evaluate(
        () =>
          (window as unknown as { __terminalSockets: Array<{ url: string }> }).__terminalSockets.filter((socket) =>
            socket.url.includes('terminal-new'),
          ).length,
      ),
    initialSocketCount = await socketCount(),
    latestRows = () =>
      page.evaluate(() => {
        const sockets = (window as unknown as { __terminalSockets: Array<{ url: string; sent: unknown[] }> })
            .__terminalSockets,
          socket = sockets.filter((item) => item.url.includes('terminal-new')).at(-1),
          claims = (socket?.sent
            .filter((value): value is string => typeof value === 'string' && value.startsWith('{'))
            .map((value) => JSON.parse(value).resize)
            .filter(Boolean) ?? []) as Array<{ rows: number }>;
        return claims.at(-1)?.rows ?? 0;
      });
  await page.waitForTimeout(200);
  const rowsBefore = await latestRows(),
    resizeGrip = (await handle.boundingBox())!;
  await page.mouse.move(resizeGrip.x + resizeGrip.width / 2, resizeGrip.y + resizeGrip.height / 2);
  await page.mouse.down();
  await page.mouse.move(resizeGrip.x + resizeGrip.width / 2, resizeGrip.y - 64);
  expect(await latestRows()).toBe(rowsBefore);
  await page.mouse.up();
  await expect.poll(latestRows).toBeGreaterThan(rowsBefore);
  await expect.poll(socketCount).toBe(initialSocketCount);
  const pointerRows = await latestRows();
  await handle.focus();
  await page.keyboard.press('ArrowUp');
  await expect.poll(latestRows).toBeGreaterThan(pointerRows);
  await expect.poll(socketCount).toBe(initialSocketCount);
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport).toHaveAttribute('data-renderer', 'webgl');
  const lastTab = drawer.getByRole('tab', { name: /Terminal New/ }),
    create = drawer.getByRole('button', { name: 'New drawer item' }),
    tabBox = (await lastTab.boundingBox())!,
    createBox = (await create.boundingBox())!;
  expect(createBox.x - (tabBox.x + tabBox.width)).toBeLessThan(20);
  await expect(drawer.locator('[data-tab-kind="terminal"]')).toHaveCount(3);
  await expect(drawer.getByRole('button', { name: 'Close Terminal New' })).toBeAttached();
  await drawer.getByRole('tab', { name: /Tests/ }).click({ button: 'right' });
  const terminalMenu = page.getByRole('menu', { name: 'Terminal tab actions' });
  await expect(terminalMenu.getByText('Close Tabs to the Right')).toBeVisible();
  await terminalMenu.getByText('Close Tabs to the Right').click();
  await expect(drawer.getByRole('tab', { name: /Terminal New/ })).toHaveCount(0);
  await expect(drawer.getByRole('tab', { name: /Tests/ })).toHaveAttribute('aria-selected', 'true');
  await drawer.getByRole('tab', { name: /Tests/ }).click({ button: 'right', modifiers: ['Alt'] });
  await expect(terminalMenu.getByText('Close Tabs to the Left')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-9n837w-terminal-tabs-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await drawer.getByRole('button', { name: 'Hide terminal drawer' }).click();
  await expect(drawer).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-586bvq-terminal-drawer-collapsed.png', fullPage: true });
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-586bvq-terminal-drawer-short.png', fullPage: true });
});

test('keeps fitted terminal sizing by retaining dedicated drawer sessions across tab switches (HS2-V93PYF)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    codex = drawer.getByRole('tab', { name: /Codex Main/ }),
    tests = drawer.getByRole('tab', { name: /Tests/ }),
    claimsFor = (terminalId: string) =>
      page.evaluate((id) => {
        const sockets = (
          window as unknown as { __terminalSockets: Array<{ url: string; sent: unknown[] }> }
        ).__terminalSockets.filter((socket) => socket.url.includes(`/terminals/${id}/attach`));
        return sockets.map((socket) =>
          socket.sent
            .filter((value): value is string => typeof value === 'string' && value.startsWith('{'))
            .map(
              (value) => JSON.parse(value).resize as { cols: number; rows: number; focus: boolean; visible: boolean },
            ),
        );
      }, terminalId);

  await codex.click();
  const firstViewport = drawer.locator('[data-terminal-id="codex-main"][data-display-mode="interactive"]');
  await expect(firstViewport).toHaveAttribute('data-geometry-ready', 'true');
  await expect(firstViewport).not.toHaveAttribute('data-grid-size', '80x24');
  await expect
    .poll(async () => (await claimsFor('codex-main')).at(-1)?.at(-1))
    .toMatchObject({
      focus: true,
      visible: true,
    });
  await firstViewport.evaluate((element) => {
    Object.assign(element, { __hs2RetainedTerminal: true });
  });
  const socketsBeforeSwitch = (await claimsFor('codex-main')).length;
  const dedicatedClaimsBeforeSwitch = (await claimsFor('codex-main')).at(-1) ?? [];
  expect(dedicatedClaimsBeforeSwitch.map(({ cols, rows }) => `${cols}x${rows}`)).not.toContain('80x24');
  const dedicatedClaimCountBeforeSwitch = dedicatedClaimsBeforeSwitch.length,
    dedicatedSizeBeforeSwitch = `${dedicatedClaimsBeforeSwitch.at(-1)?.cols}x${dedicatedClaimsBeforeSwitch.at(-1)?.rows}`;

  await tests.click();
  await expect(drawer.locator('[data-terminal-id="tests"][data-display-mode="interactive"]')).toHaveAttribute(
    'data-geometry-ready',
    'true',
  );
  expect(
    ((await claimsFor('codex-main')).at(-1) ?? [])
      .slice(dedicatedClaimCountBeforeSwitch)
      .map(({ cols, rows }) => `${cols}x${rows}`),
  ).toEqual(expect.arrayContaining([dedicatedSizeBeforeSwitch]));
  expect(
    new Set(
      ((await claimsFor('codex-main')).at(-1) ?? [])
        .slice(dedicatedClaimCountBeforeSwitch)
        .map(({ cols, rows }) => `${cols}x${rows}`),
    ),
  ).toEqual(new Set([dedicatedSizeBeforeSwitch]));
  const beforeReturn = (await claimsFor('codex-main')).length;
  expect(beforeReturn).toBe(socketsBeforeSwitch);
  await codex.click();
  const retained = drawer.locator('[data-terminal-id="codex-main"][data-display-mode="interactive"]');
  await expect(retained).toHaveAttribute('data-geometry-ready', 'true');
  await expect(retained).not.toHaveAttribute('data-grid-size', '80x24');
  expect(await retained.evaluate((element) => '__hs2RetainedTerminal' in element)).toBe(true);
  expect((await claimsFor('codex-main')).length).toBe(socketsBeforeSwitch);
  expect(((await claimsFor('codex-main')).at(-1) ?? []).map(({ cols, rows }) => `${cols}x${rows}`)).not.toContain(
    '80x24',
  );
  await page.screenshot({ path: '/private/tmp/hs2-v93pyf-terminal-retained-after.png', fullPage: true });
});

test('closes a terminal tab after its process has stopped (HS2-DPTG65)', async ({ page }) => {
  let deletes = 0;
  await mockProject(page, true, false, 0, 0, 0, false, 2, false, false, true, true);
  page.on('request', (request) => {
    if (request.method() === 'DELETE' && new URL(request.url()).pathname.endsWith('/terminals/tests')) deletes += 1;
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    stopped = drawer.getByRole('tab', { name: /Tests/ });
  await expect(stopped).toBeVisible();
  await stopped.hover();
  await drawer.getByRole('button', { name: 'Close Tests' }).click();
  await expect(stopped).toHaveCount(0);
  expect(deletes).toBe(1);
  await expect(drawer.getByRole('tab', { name: /Codex Main/ })).toBeVisible();
});

test('hides the bottom terminal drawer on Notifications and Settings views, preserving the open preference (HS2-EQEJC7)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const drawerRegion = page.locator('[data-component="resizable-region"][data-region-id="app-terminal-drawer"]'),
    restore = page.getByRole('button', { name: 'Show terminal drawer' });
  // Open the drawer on a ticket view.
  await restore.click();
  await expect(drawerRegion).toBeVisible();
  await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
  // Notifications hides the whole drawer area — no drawer and no restore affordance.
  await page.getByRole('button', { name: /Notifications view/ }).click();
  await expect(drawerRegion).toHaveCount(0);
  await expect(restore).toHaveCount(0);
  await expect(page.locator('[data-component="terminal-drawer"]')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/claude/hs2-eqejc7-notifications-no-drawer.png', fullPage: true });
  // Settings hides it the same way.
  await page.getByRole('button', { name: /Settings view/ }).click();
  await expect(drawerRegion).toHaveCount(0);
  await expect(restore).toHaveCount(0);
  await expect(page.locator('[data-component="terminal-drawer"]')).toHaveCount(0);
  // Returning to a ticket view restores the open drawer (preference preserved, not just collapsed).
  await page.getByRole('button', { name: /List view/ }).click();
  await expect(drawerRegion).toBeVisible();
  await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
});

test('changes the chat provider and re-seeds the new provider with the prior transcript (HS2-PRBGRB)', async ({
  page,
}) => {
  const connectionPosts: Array<Record<string, unknown>> = [],
    turnPosts: Array<{ id: string; content: string }> = [];
  await mockProject(page);
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.endsWith('/drive/connections'))
      connectionPosts.push(request.postDataJSON() as Record<string, unknown>);
    const turn = path.match(/\/drive\/connections\/([^/]+)\/turns$/);
    if (turn && request.method() === 'POST')
      turnPosts.push({
        id: decodeURIComponent(turn[1]),
        content: (request.postDataJSON() as { content: string }).content,
      });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const conversation = drawer.locator('[data-component="ai-conversation"][data-presentation="embedded"]');
  await conversation.getByLabel('Message Codex').fill('What time is it in California?');
  await conversation.getByLabel('Message Codex').press('Enter');
  await expect(conversation.getByText('The event stream remains authoritative.')).toBeVisible();
  // Change provider to Claude via the conversation popup's provider submenu.
  await conversation.locator('[data-component="conversation-model-control"] .ai-conversation__model-trigger').click();
  await expect(page.getByRole('menuitem', { name: /Provider/ })).toBeVisible();
  await page.locator('[data-action="select-conversation-provider"][data-value="claude"]').dispatchEvent('click');
  // A fresh Claude chat is created, selected, and seeded with the prior transcript as one context turn.
  const claudeTab = drawer.getByRole('tab', { name: 'Claude chat' });
  await expect(claudeTab).toHaveAttribute('aria-selected', 'true');
  await expect.poll(() => connectionPosts.some((body) => body.tool === 'claude')).toBe(true);
  const claudeId = connectionPosts.find((body) => body.tool === 'claude')!.connection_id as string;
  await expect.poll(() => turnPosts.some((turn) => turn.id === claudeId)).toBe(true);
  const seed = turnPosts.find((turn) => turn.id === claudeId)!;
  expect(seed.content).toContain('transcript of my earlier conversation with Codex');
  expect(seed.content).toContain('do not take any actions based on it');
  expect(seed.content).toContain('What time is it in California?');
  // No prior Codex turn was re-run against Claude: the only Claude turn is the single seed.
  expect(turnPosts.filter((turn) => turn.id === claudeId)).toHaveLength(1);
  await page.screenshot({ path: '/private/tmp/hs2-prbgrb-provider-reseed.png', fullPage: true });
});

test('creates an embedded AI chat from the polished terminal drawer menu and exposes modifier configuration', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.addInitScript(() => {
    document.addEventListener('hotsheet-ai-launch-configuration', (event) => {
      (window as unknown as { __drawerAILaunch?: unknown }).__drawerAILaunch = (event as CustomEvent).detail;
    });
  });
  const answers = ['claude', 'claude-sonnet'];
  page.on('dialog', async (dialog) => {
    await dialog.accept(answers.shift() ?? '');
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const region = page.locator('[data-component="resizable-region"][data-region-id="app-terminal-drawer"]'),
    drawer = page.locator('[data-component="terminal-drawer"]'),
    create = drawer.getByRole('button', { name: 'New drawer item' });
  const createStyle = await create.evaluate((node) => {
    const style = getComputedStyle(node);
    return { background: style.backgroundColor, borderRadius: style.borderRadius, height: style.height };
  });
  expect(createStyle.background).toBe('rgba(0, 0, 0, 0)');
  expect(createStyle.borderRadius).toBe('999px');
  expect(createStyle.height).toBe('32px');
  await create.click();
  await expect(region).toHaveAttribute('data-content-overflow', 'visible');
  const menu = drawer.getByRole('menu', { name: 'New drawer item' });
  await expect(menu.getByText('Default shell')).toBeVisible();
  await expect(menu.getByText('AI shell')).toBeVisible();
  await expect(menu.locator('[data-component="list-header"]')).toHaveCount(0);
  await expect(menu.locator('[data-lucide="chevron-right"]')).toHaveCount(0);
  const captureMenu = async (path: string) => {
    const [menuBox, createBox] = await Promise.all([menu.boundingBox(), create.boundingBox()]);
    expect(menuBox).not.toBeNull();
    expect(createBox).not.toBeNull();
    const left = Math.max(0, Math.min(menuBox!.x, createBox!.x) - 16),
      top = Math.max(0, Math.min(menuBox!.y, createBox!.y) - 16),
      right = Math.min(
        page.viewportSize()!.width,
        Math.max(menuBox!.x + menuBox!.width, createBox!.x + createBox!.width) + 16,
      ),
      bottom = Math.min(
        page.viewportSize()!.height,
        Math.max(menuBox!.y + menuBox!.height, createBox!.y + createBox!.height) + 16,
      );
    await page.screenshot({ path, clip: { x: left, y: top, width: right - left, height: bottom - top } });
  };
  await captureMenu('/private/tmp/hs2-7ctqjc-cv0j2e-rhqatm-drawer-menu-wide.png');
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(menu).toBeVisible();
  await captureMenu('/private/tmp/hs2-7ctqjc-cv0j2e-rhqatm-drawer-menu-narrow.png');
  await page.setViewportSize({ width: 1280, height: 800 });
  await menu.getByText('AI chat').click({ modifiers: ['Alt'] });
  await expect(region).toHaveAttribute('data-content-overflow', 'clip');
  await expect(drawer).toHaveAttribute('data-mode', 'ai-chat');
  await expect(drawer.getByRole('tab', { name: 'Claude chat' })).toHaveAttribute('aria-selected', 'true');
  const conversation = drawer.locator('[data-component="ai-conversation"][data-presentation="embedded"]');
  await expect(conversation).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __drawerAILaunch?: unknown }).__drawerAILaunch))
    .toEqual({ projectId: 'demo-checkout', kind: 'ai-chat', provider: 'claude', model: 'claude-sonnet' });
  const composer = conversation.getByLabel('Message Claude');
  await composer.fill('What time is it in California?');
  await composer.press('Enter');
  const response = conversation.getByText(
    'I found the relevant client boundary. The event stream remains authoritative.',
    { exact: true },
  );
  await expect(response).toHaveCount(1);
  await conversation.screenshot({ path: '/private/tmp/hs2-qfwhgc-claude-single-response-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(response).toHaveCount(1);
  await conversation.screenshot({ path: '/private/tmp/hs2-qfwhgc-claude-single-response-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({ path: '/private/tmp/hs2-0fr30w-terminal-drawer-ai-chat.png', fullPage: true });
  await drawer.locator('[data-tab-kind="ai-chat"]').hover();
  await drawer.getByRole('button', { name: 'Close Claude chat' }).click();
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  await expect(drawer.locator('[data-tab-kind="terminal"] [role="tab"][aria-selected="true"]')).toHaveCount(1);
});

test('keeps the embedded chat composer usable while a long transcript scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const conversation = drawer.locator('[data-component="ai-conversation"][data-presentation="embedded"]'),
    composer = conversation.getByLabel('Message Codex'),
    messages = conversation.locator('[data-message-id]');
  for (let turn = 1; turn <= 5; turn += 1) {
    await composer.fill('What time is it in California?');
    await composer.press('Enter');
    await expect(messages).toHaveCount(turn * 2);
  }
  const geometry = () =>
    conversation.evaluate((element) => {
      const root = element.getBoundingClientRect(),
        drawerContent = element.parentElement!.getBoundingClientRect(),
        transcript = element.querySelector<HTMLElement>('.ai-conversation__transcript')!,
        transcriptBox = transcript.getBoundingClientRect(),
        composerElement = element.querySelector<HTMLElement>('.ai-conversation__composer')!,
        composerBox = composerElement.getBoundingClientRect();
      return {
        rootBottom: root.bottom,
        drawerBottom: drawerContent.bottom,
        transcriptBottom: transcriptBox.bottom,
        composerTop: composerBox.top,
        composerBottom: composerBox.bottom,
        lastMessageBottom: transcript.lastElementChild?.getBoundingClientRect().bottom ?? transcriptBox.top,
        transcriptScrollable: transcript.scrollHeight > transcript.clientHeight,
        composerVisible: composerBox.top >= root.top && composerBox.bottom <= root.bottom + 1,
      };
    });
  await expect.poll(geometry).toMatchObject({ transcriptScrollable: true, composerVisible: true });
  let measured = await geometry();
  expect(measured.rootBottom).toBeLessThanOrEqual(measured.drawerBottom + 1);
  expect(measured.transcriptBottom).toBeGreaterThanOrEqual(measured.composerBottom - 1);
  expect(measured.lastMessageBottom).toBeLessThanOrEqual(measured.composerTop);
  const compactHeight = (await composer.boundingBox())!.height;
  await composer.fill('First line\nSecond line\nThird line\nFourth line');
  await expect.poll(async () => (await composer.boundingBox())!.height).toBeGreaterThan(compactHeight);
  await expect(conversation.locator('.ai-conversation__send [data-lucide="arrow-up"]')).toBeVisible();
  await conversation.screenshot({ path: '/private/tmp/hs2-1rvp5m-chat-composer-wide-after.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect.poll(geometry).toMatchObject({ transcriptScrollable: true, composerVisible: true });
  measured = await geometry();
  expect(measured.rootBottom).toBeLessThanOrEqual(measured.drawerBottom + 1);
  expect(measured.transcriptBottom).toBeGreaterThanOrEqual(measured.composerBottom - 1);
  await composer.fill('Composer remains usable after the narrow transition.');
  await expect(composer).toBeFocused();
  await conversation.screenshot({ path: '/private/tmp/hs2-1rvp5m-chat-composer-narrow-after.png' });
});

test('keeps a scrolled-back transcript in place while selecting a message range', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.locator('[data-action="toggle-terminal-drawer-maximize"]').dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'true');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const conversation = drawer.locator('[data-component="ai-conversation"][data-presentation="embedded"]'),
    composer = conversation.getByLabel('Message Codex'),
    messages = conversation.locator('[data-action="pick-conversation-message"]'),
    transcript = conversation.locator('.ai-conversation__transcript');
  for (let turn = 1; turn <= 5; turn += 1) {
    await composer.fill('What time is it in California?');
    await composer.press('Enter');
    await expect(messages).toHaveCount(turn * 2);
  }
  await expect
    .poll(() =>
      transcript.evaluate((node) => ({
        scrollable: node.scrollHeight > node.clientHeight + 100,
        pinned: node.scrollTop + node.clientHeight >= node.scrollHeight - 2,
        scrollTop: node.scrollTop,
        clientHeight: node.clientHeight,
        scrollHeight: node.scrollHeight,
      })),
    )
    .toMatchObject({ scrollable: true, pinned: true });
  const scrollTop = () => transcript.evaluate((node) => node.scrollTop),
    settle = () =>
      page.evaluate(
        () =>
          new Promise((resolve) => {
            requestAnimationFrame(() => {
              requestAnimationFrame(resolve);
            });
          }),
      );
  // Put a known middle-history pair safely in view before measuring; the selection clicks themselves must not scroll.
  const [anchorIndex, endIndex] = [4, 3];
  await messages.nth(anchorIndex).evaluate((node) => {
    node.scrollIntoView({ block: 'center' });
  });
  await settle();
  const readingTop = await scrollTop();
  expect(readingTop).toBeGreaterThan(40);
  expect(await transcript.evaluate((node) => node.scrollTop + node.clientHeight < node.scrollHeight - 32)).toBe(true);
  const expectReadingPosition = async () => {
    const position = await transcript.evaluate((node) => ({
      top: node.scrollTop,
      atBottom: node.scrollTop + node.clientHeight >= node.scrollHeight - 32,
    }));
    expect(position.atBottom).toBe(false);
    expect(Math.abs(position.top - readingTop)).toBeLessThanOrEqual(24);
  };
  await messages.nth(anchorIndex).locator(':scope > strong').click();
  await expect(messages.nth(anchorIndex)).toHaveAttribute('data-selected', 'true');
  await expect(conversation).toContainText('1 message selected');
  await settle();
  await expectReadingPosition();
  await messages.nth(endIndex).locator(':scope > strong').click();
  await expect(conversation).toContainText(`${anchorIndex - endIndex + 1} messages selected`);
  await settle();
  await expectReadingPosition();
  await conversation.screenshot({ path: '/private/tmp/hs2-eqhary-scroll-selection-after.png' });
  await conversation.getByRole('button', { name: 'Clear message selection' }).click();
  await expect(conversation).toContainText('Select a message, then another to select a range.');
  await settle();
  await expectReadingPosition();
  // Sending is an explicit request to continue the conversation, so it returns to and follows the latest edge.
  await composer.fill('Growth after reading older messages.');
  await composer.press('Enter');
  await expect(messages).toHaveCount(12);
  await expect
    .poll(() => transcript.evaluate((node) => node.scrollTop + node.clientHeight >= node.scrollHeight - 2))
    .toBe(true);
});

test('selects and copies chat messages before saving that range without choosing it again', async ({
  page,
  context,
}) => {
  let savedPayload: ConversationExportPayload | undefined,
    openCount = 0;
  const resumedConnections: Array<Record<string, unknown>> = [];
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page);
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.endsWith('/drive/connections'))
      resumedConnections.push(request.postDataJSON());
  });
  await page.route('**/__hotsheet/conversation-exports/destination', (route) =>
    route.fulfill({
      json: {
        destination: {
          selectionToken: 'selection-1',
          displayPath: '/Exports/client-review.hotsheet-chat',
          kind: 'directory',
        },
      },
    }),
  );
  await page.route('**/conversation-files/proof.png', (route) =>
    route.fulfill({ body: Buffer.from([0x89, 0x50, 0x4e, 0x47]), headers: { 'content-type': 'image/png' } }),
  );
  await page.route('**/__hotsheet/conversation-exports/write', async (route) => {
    savedPayload = route.request().postDataJSON();
    const request = savedPayload!.request;
    return route.fulfill({
      json: {
        displayPath: '/Exports/client-review.hotsheet-chat',
        manifest: {
          format: 'hotsheet-conversation-export',
          manifestVersion: 1,
          exportId: 'export-1',
          revision: 1,
          exportedAt: '2026-09-10T12:00:00Z',
          source: request.source,
          selectedMessageIds: request.selectedMessageIds,
          bundle: request.bundle,
          reopen: request.reopen,
          entries: [],
          assets: [],
        },
      },
    });
  });
  await page.route('**/__hotsheet/conversation-exports/open', (route) => {
    openCount += 1;
    const { request, messages: partialMessages } = savedPayload!,
      allMessages: ConversationMessage[] = [
        ...partialMessages,
        {
          id: 'saved-answer',
          role: 'assistant',
          content: 'The event stream remains authoritative.',
          status: 'completed',
        },
      ],
      messages = openCount === 1 ? partialMessages : allMessages,
      reopen =
        openCount === 1
          ? { ...request.reopen, resumesOriginalSession: false }
          : {
              ...request.reopen,
              lastMessageId: 'saved-answer',
              sessionId: 'claude-thread-1',
              resumesOriginalSession: true,
            };
    return route.fulfill({
      json: {
        conversation: {
          displayPath: '/Exports/client-review.hotsheet-chat',
          manifest: {
            format: 'hotsheet-conversation-export',
            manifestVersion: 1,
            exportId: 'export-1',
            revision: 1,
            exportedAt: '2026-09-10T12:00:00Z',
            source: { ...request.source, sessionId: 'claude-thread-1' },
            selectedMessageIds: messages.map((message) => message.id),
            bundle: { includeAttachments: true, includeMedia: true, includeSummary: true },
            reopen,
            entries: [],
            assets: [],
          },
          messages,
          activity: [],
        },
      },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const conversation = drawer.locator('[data-component="ai-conversation"]');
  const composer = conversation.getByLabel('Message Codex');
  await composer.fill('Export the referenced proof.');
  await composer.press('Enter');
  await expect(conversation.getByText('The referenced proof is ready.', { exact: true })).toBeVisible();
  await expect(conversation.getByRole('link', { name: 'proof.png' })).toBeVisible();
  const messageIds = await conversation
    .locator('[data-message-id]')
    .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-message-id')!));
  expect(messageIds).toHaveLength(2);
  const messageChoices = conversation.locator('[data-action="pick-conversation-message"]');
  await messageChoices.nth(1).locator(':scope > strong').click();
  await expect(messageChoices.nth(1)).toHaveAttribute('data-selected', 'true');
  await expect(messageChoices.nth(0)).toHaveAttribute('data-selected', 'false');
  await expect(conversation).toContainText('1 message selected');
  await conversation.getByRole('button', { name: 'Copy selected messages' }).click();
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toContain('The referenced proof is ready.');
  expect(await page.evaluate(() => navigator.clipboard.readText())).not.toContain('Export the referenced proof.');
  await conversation.screenshot({ path: '/private/tmp/hs2-eqhary-selected-chat-wide.png' });
  const saveConversation = conversation.getByRole('button', { name: 'Save conversation' });
  await expect(saveConversation).toHaveJSProperty('tagName', 'BUTTON');
  await saveConversation.focus();
  await expect(saveConversation).toBeFocused();
  await saveConversation.press('Enter');
  const dialog = page.locator('[data-component="conversation-export-dialog"]'),
    dialogPanel = dialog.getByRole('dialog');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).toContainText('Step 1 of 2');
  await expect(dialog).toContainText('Choose scope');
  await expect(dialog.locator('input[name="conversation-export-scope"][value="range"]')).toBeChecked();
  await expect(dialog).toContainText('1 message selected in the chat.');
  await expect(dialog.locator('[data-action="pick-conversation-export-message"]')).toHaveCount(0);
  await dialogPanel.screenshot({ path: '/private/tmp/hs2-eqhary-save-selected-range-wide.png' });
  await page.setViewportSize({ width: 560, height: 760 });
  await expect
    .poll(() =>
      dialogPanel.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth && box.bottom <= innerHeight;
      }),
    )
    .toBe(true);
  await dialogPanel.screenshot({ path: '/private/tmp/hs2-eqhary-save-selected-range-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  const continueButton = dialog.locator('[data-action="next-conversation-export-step"]');
  await expect(continueButton).toBeEnabled();
  await continueButton.click();
  await expect(dialog).toHaveAttribute('data-navigation', 'push');
  await expect(dialog.locator('[data-transition-region="content"]')).toHaveAttribute(
    'data-transition-direction',
    'forward',
  );
  await expect(dialog.locator('[data-transition-region="content"] > [data-side="b"]')).toHaveAttribute(
    'data-active',
    'true',
  );
  await expect(dialog).toContainText('Step 2 of 2');
  const back = dialog.getByRole('button', { name: 'Message scope' });
  await expect(back.locator('[data-lucide="chevron-left"]')).toBeVisible();
  await dialog.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  await dialogPanel.screenshot({ path: '/private/tmp/hs2-r3qfvc-save-pushed-wide.png' });
  await page.setViewportSize({ width: 560, height: 760 });
  await dialogPanel.screenshot({ path: '/private/tmp/hs2-r3qfvc-save-pushed-narrow.png' });
  await back.click();
  await expect(dialog).toHaveAttribute('data-navigation', 'pop');
  await expect(dialog.locator('[data-transition-region="content"]')).toHaveAttribute(
    'data-transition-direction',
    'backward',
  );
  await expect(dialog.locator('[data-transition-region="content"] > [data-side="a"]')).toHaveAttribute(
    'data-active',
    'true',
  );
  await dialog.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  await dialogPanel.screenshot({ path: '/private/tmp/hs2-r3qfvc-save-popped-narrow.png' });
  await dialog.locator('[data-action="next-conversation-export-step"]').click();
  await expect(dialog).toHaveAttribute('data-navigation', 'push');
  await page.setViewportSize({ width: 1280, height: 900 });
  const activeBundle = dialog.locator('[data-transition-region="content"] > [data-side="b"]');
  await activeBundle.locator('input[name="conversation-export-summary"]').check();
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).not.toContainText('manifest.json');
  const options = activeBundle.locator('.conversation-export-dialog__choice');
  const optionBoxes = await options.evaluateAll((nodes) =>
    nodes.slice(0, 3).map((node) => node.getBoundingClientRect()),
  );
  expect(optionBoxes[1].y).toBeGreaterThan(optionBoxes[0].y + optionBoxes[0].height - 1);
  await dialogPanel.screenshot({ path: '/private/tmp/hs2-zzpf2b-bundle-options-wide-after.png' });
  await dialog.getByRole('button', { name: 'Save conversation' }).click();
  await expect(page.locator('.app-toast')).toContainText('Saved conversation revision 1');
  await expect.poll(() => savedPayload).toBeTruthy();
  expect(savedPayload!.request.selectedMessageIds).toEqual([messageIds[1]]);
  expect(savedPayload!.request.reopen).toMatchObject({ lastMessageId: messageIds[1], resumesOriginalSession: false });
  expect(savedPayload!.request.bundle).toMatchObject({ includeSummary: true });
  expect(savedPayload!.assets).toEqual([
    { id: 'proof-image', filename: 'proof.png', mimeType: 'image/png', kind: 'media', dataBase64: 'iVBORw==' },
  ]);
  const openSaved = async () => {
    await drawer.getByRole('button', { name: 'New drawer item' }).click();
    await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('Saved conversation…').click();
  };
  await openSaved();
  const savedTab = drawer.getByRole('tab', { name: 'Codex saved chat' });
  await expect(savedTab).toHaveAttribute('aria-selected', 'true');
  const savedConversation = drawer.locator('[data-component="ai-conversation"][data-read-only="true"]');
  await expect(savedConversation).toContainText('This selected range is read-only');
  await expect(savedConversation.getByLabel('Message Codex')).toHaveCount(0);
  await savedConversation.screenshot({ path: '/private/tmp/hs2-zzpf2b-saved-read-only-wide.png' });
  const projectTab = page.locator('[data-tab-kind="project"]');
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  const closeDialog = page.locator('[data-component="project-close-dialog"]');
  await expect(closeDialog).toContainText('2 running terminals and 1 AI chat');
  await closeDialog.getByRole('button', { name: 'Cancel' }).click();
  await savedTab.hover();
  await drawer.getByRole('button', { name: 'Close Codex saved chat' }).click();
  await openSaved();
  await expect.poll(() => resumedConnections.some((body) => body.session_id === 'claude-thread-1')).toBe(true);
  const resumed = drawer.locator('[data-component="ai-conversation"][data-read-only="false"]');
  await expect(resumed.getByLabel('Message Codex')).toBeVisible();
  await resumed.getByLabel('Message Codex').fill('Continue from the saved result.');
  await expect(resumed.getByRole('button', { name: 'Send message to Codex' })).toBeEnabled();
});

test('shows selection in the main chat and skips export scope after it is cleared', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open Codex conversation' }).click();
  const conversation = page.locator('[data-component="ai-conversation"]');
  await conversation.getByLabel('Message Codex').fill('What time is it in California?');
  await conversation.getByLabel('Message Codex').press('Enter');
  await expect(conversation.getByText('The event stream remains authoritative.', { exact: true })).toBeVisible();
  const messages = conversation.locator('[data-action="pick-conversation-message"]');
  await messages.nth(1).locator(':scope > strong').click();
  await expect(conversation).toContainText('1 message selected');
  await conversation.getByRole('dialog').screenshot({ path: '/private/tmp/hs2-eqhary-main-chat-selection-wide.png' });
  await page.setViewportSize({ width: 560, height: 760 });
  await conversation.getByRole('dialog').screenshot({ path: '/private/tmp/hs2-eqhary-main-chat-selection-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await conversation.getByRole('button', { name: 'Clear message selection' }).click();
  await expect(conversation).toContainText('Select a message, then another to select a range.');
  await conversation.getByRole('button', { name: 'Save conversation' }).click();
  const dialog = page.locator('[data-component="conversation-export-dialog"]'),
    activeScreen = dialog.locator('[data-transition-region="content"] > [data-active="true"]');
  await expect(dialog).toHaveAttribute('data-navigation', 'none');
  await expect(activeScreen).toContainText('Bundle contents');
  await expect(activeScreen).not.toContainText('Step 1 of 2');
  await expect(activeScreen.locator('input[name="conversation-export-scope"]')).toHaveCount(0);
  await dialog.getByRole('dialog').screenshot({ path: '/private/tmp/hs2-eqhary-save-entire-chat.png' });
});

test('shows AI chats in the project and workspace grids and reopens them from the global grid', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  await expect(drawer).toHaveAttribute('data-mode', 'ai-chat');
  await drawer.getByRole('tab', { name: 'Project grid' }).click();
  const projectChat = drawer.locator('[data-component="workspace-chat-tile"]');
  await expect(projectChat).toContainText('Codex AI chat');
  await expect(projectChat).toContainText('Open Codex chat to continue the conversation.');
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    globalChat = dashboard.locator('[data-component="workspace-chat-tile"]');
  await expect(globalChat).toContainText('demo › Codex chat');
  await expect(page.locator('[data-component="workspace-identity"]')).toContainText('Workspace grid');
  await page.screenshot({ path: '/private/tmp/hs2-hpy5r0-workspace-grid-ai-chat-wide.png', fullPage: true });
  for (let fit = 4; fit < 10; fit += 1)
    await dashboard.getByRole('button', { name: /Zoom out, fit more items/ }).click();
  await expect(dashboard).toHaveAttribute('data-fit', '10');
  const scaledGeometry = await globalChat.evaluate((element) => {
    const preview = element.querySelector<HTMLElement>('.workspace-chat-tile__preview')!,
      surface = element.querySelector<HTMLElement>('.workspace-chat-tile__preview-surface')!,
      previewBox = preview.getBoundingClientRect(),
      surfaceBox = surface.getBoundingClientRect();
    return {
      scale: Number(surface.dataset.previewScale),
      transform: getComputedStyle(surface).transform,
      contained:
        surfaceBox.left >= previewBox.left &&
        surfaceBox.top >= previewBox.top &&
        surfaceBox.right <= previewBox.right + 1 &&
        surfaceBox.bottom <= previewBox.bottom + 1,
    };
  });
  expect(scaledGeometry.scale).toBeGreaterThan(0);
  expect(scaledGeometry.scale).toBeLessThan(0.1);
  expect(scaledGeometry.transform).not.toBe('none');
  expect(scaledGeometry.contained).toBe(true);
  await globalChat.screenshot({ path: '/private/tmp/hs2-7jxdam-chat-grid-scale-after.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await dashboard.locator('.terminal-dashboard__content').evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(globalChat).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-hpy5r0-workspace-grid-ai-chat-narrow.png', fullPage: true });
  await globalChat.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('button', { name: 'Workspace grid' })).toHaveAttribute('aria-pressed', 'false');
  await expect(drawer).toHaveAttribute('data-mode', 'ai-chat');
  await expect(drawer.getByRole('tab', { name: 'Codex chat' })).toHaveAttribute('aria-selected', 'true');
});

test('previews running project resources with shared menus and explicit keep-running consequences', async ({
  page,
}) => {
  const deletes: string[] = [];
  page.on('request', (request) => {
    if (request.method() === 'DELETE') deletes.push(new URL(request.url()).pathname);
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    projectTab = page.locator('[data-tab-kind="project"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  await expect(drawer.getByRole('tab', { name: 'Codex chat' })).toBeVisible();
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  const dialog = page.locator('[data-component="project-close-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).toContainText('2 running terminals and 1 AI chat');
  await expect(dialog.locator('[data-component="list-header"]')).toHaveText('Running items');
  await expect(dialog.getByRole('button', { name: /Codex Main/ })).toHaveAttribute('aria-current', 'page');
  const viewport = dialog.locator('[data-component="terminal-viewport"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport.locator('.xterm-rows')).toContainText('Live terminal ready');
  await expect(dialog).not.toContainText('Working directory');
  await expect(dialog).not.toContainText('Progress');
  await expect(dialog).toContainText('Terminals and AI chat tabs return when reopened');
  await expect(dialog.getByRole('button', { name: 'Keep Running' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Stop & Close' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-6c0wzn-project-close-wide-after.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog).toHaveJSProperty('open', true);
  await expect
    .poll(() =>
      dialog.evaluate((element) => {
        const box = element.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth && box.bottom <= innerHeight;
      }),
    )
    .toBe(true);
  await expect(dialog.locator('.project-close-dialog__consequences')).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hs2-6c0wzn-project-close-narrow-after.png', fullPage: true });
  await dialog.getByRole('button', { name: /Codex chat/ }).click();
  await expect(dialog.getByRole('region', { name: 'Codex chat chat preview' })).toContainText('gpt-6-astra');
  await expect(dialog.getByRole('region', { name: 'Codex chat chat preview' })).toContainText('medium');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(projectTab).toHaveCount(1);
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  await dialog.getByRole('button', { name: 'Keep Running' }).click();
  await expect(projectTab).toHaveCount(0);
  expect(deletes).toEqual([]);
});

test('reuses the read-only conversation and restores borrowed terminal geometry without ghosting', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    projectTab = page.locator('[data-tab-kind="project"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const liveConversation = drawer.locator('[data-component="ai-conversation"]'),
    composer = liveConversation.getByLabel('Message Codex'),
    createViewDialog = page.getByRole('dialog', { name: 'Create View' });
  await composer.evaluate((node) => {
    node.setAttribute('data-action', 'add-view');
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, composed: true }));
    node.removeAttribute('data-action');
  });
  await expect(createViewDialog).toBeHidden();
  await composer.fill('What time is it in California?');
  await composer.press('Enter');
  await expect(liveConversation).toContainText('The event stream remains authoritative.');
  await expect(createViewDialog).toBeHidden();
  await drawer.getByRole('tab', { name: /Codex Main/ }).click();
  const drawerViewport = drawer.locator(
    '[data-component="terminal-session"]:not([hidden]) [data-component="terminal-viewport"]',
  );
  await expect(drawerViewport).toHaveAttribute('data-connection', 'connected');
  const initialGrid = await drawerViewport.getAttribute('data-grid-size');
  const originalSocket = await page.evaluate(() => {
    const sockets = (window as unknown as { __terminalSockets: Array<{ url: string; readyState: number }> })
      .__terminalSockets;
    for (let index = sockets.length - 1; index >= 0; index -= 1)
      if (sockets[index].url.includes('codex-main') && sockets[index].readyState === WebSocket.OPEN) return index;
    return -1;
  });
  expect(originalSocket).toBeGreaterThanOrEqual(0);
  const originalClaims = () =>
      page.evaluate((index) => {
        const socket = (window as unknown as { __terminalSockets: Array<{ sent: unknown[] }> }).__terminalSockets[
            index
          ],
          claims: Array<{ cols: number; rows: number }> = [];
        for (const value of socket.sent) {
          if (typeof value !== 'string') continue;
          try {
            const parsed = JSON.parse(value) as { resize?: { cols: number; rows: number } };
            if (parsed.resize) claims.push(parsed.resize);
          } catch {
            /* terminal input */
          }
        }
        const last = claims.at(-1);
        return { count: claims.length, last: last ? `${last.cols}x${last.rows}` : undefined };
      }, originalSocket),
    beforeOpen = await originalClaims();

  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  const dialog = page.locator('[data-component="project-close-dialog"]');
  const terminalPreview = dialog.locator('[data-component="terminal-viewport"]');
  await expect(terminalPreview).not.toHaveAttribute('data-grid-policy', 'dashboard-80x24');
  await expect(terminalPreview).toHaveAttribute('data-sizing-focus', 'false');
  await dialog.getByRole('button', { name: /Codex chat/ }).click();
  const preview = dialog.locator('[data-component="ai-conversation"]');
  await expect(preview).toHaveAttribute('data-read-only', 'true');
  await expect(preview.locator('[data-message-id]')).toHaveCount(2);
  await expect(preview.getByLabel('Message Codex')).toHaveCount(0);
  await expect(preview.getByRole('button', { name: 'Save conversation' })).toHaveCount(0);
  const answer = preview.locator('.ai-conversation__message--assistant .markdown-preview');
  await expect(answer).toHaveText('I found the relevant client boundary. The event stream remains authoritative.');
  for (let index = 0; index < 2; index += 1) {
    await dialog.getByRole('button', { name: /Codex Main/ }).click();
    await expect(terminalPreview).toHaveAttribute('data-connection', 'connected');
    await dialog.getByRole('button', { name: /Codex chat/ }).click();
    await expect(answer).toHaveText('I found the relevant client boundary. The event stream remains authoritative.');
  }
  await expect
    .poll(() =>
      preview.evaluate(
        (element) => ((element as HTMLElement).innerText.match(/event stream remains authoritative/g) ?? []).length,
      ),
    )
    .toBe(1);
  await page.screenshot({ path: '/private/tmp/hs2-6c0wzn-7se31f-close-chat-wide.png', fullPage: true });

  await dialog.getByRole('button', { name: /Codex Main/ }).click();
  await expect(terminalPreview).not.toHaveAttribute('data-grid-policy', 'dashboard-80x24');
  const beforeCancel = await originalClaims();
  expect(beforeCancel.count).toBeGreaterThanOrEqual(beforeOpen.count);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect.poll(async () => (await originalClaims()).count).toBeGreaterThan(beforeCancel.count);
  await expect.poll(async () => (await originalClaims()).last).toBe(initialGrid);
  await page.waitForTimeout(250);
  await expect(createViewDialog).toBeHidden();
  await page.screenshot({ path: '/private/tmp/hs2-6c0wzn-terminal-restored-after-cancel.png', fullPage: true });

  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  await dialog.getByRole('button', { name: /Codex chat/ }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(preview).toBeVisible();
  await expect(answer).toHaveText('I found the relevant client boundary. The event stream remains authoritative.');
  await page.screenshot({ path: '/private/tmp/hs2-6c0wzn-7se31f-close-chat-narrow.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Cancel' }).click();
});

test('restores a kept-running AI chat tab and its live server session when the project reopens', async ({ page }) => {
  const turnBodies: Array<Record<string, unknown>> = [];
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/turns'))
      turnBodies.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  let drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  let conversation = drawer.locator('[data-component="ai-conversation"]');
  await conversation.getByLabel('Message Codex').fill('What time is it in California?');
  await conversation.getByLabel('Message Codex').press('Enter');
  await expect(conversation.getByText('The event stream remains authoritative.')).toBeVisible();
  await expect(conversation.locator('[data-message-id]')).toHaveCount(2);
  const projectTab = page.locator('[data-tab-kind="project"]');
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  const closeDialog = page.locator('[data-component="project-close-dialog"]');
  await expect(closeDialog).toContainText('Terminals and AI chat tabs return when reopened');
  await closeDialog.getByRole('button', { name: 'Keep Running' }).click();
  await expect(projectTab).toHaveCount(0);
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  drawer = page.locator('[data-component="terminal-drawer"]');
  const restoredTab = drawer.getByRole('tab', { name: 'Codex chat' });
  await expect(restoredTab).toBeVisible();
  await expect(restoredTab).toHaveAttribute('aria-selected', 'true');
  await expect(drawer).toHaveAttribute('data-mode', 'ai-chat');
  conversation = drawer.locator('[data-component="ai-conversation"]');
  await expect(conversation.locator('[data-message-id]')).toHaveCount(2);
  await expect(conversation.getByText('The event stream remains authoritative.')).toBeVisible();
  await expect(conversation.locator('.ai-conversation__model-name')).toHaveAttribute('title', 'gpt-6-astra');
  await expect(conversation.locator('.ai-conversation__model-effort')).toHaveText('medium');
  await page.screenshot({ path: '/private/tmp/hs2-d34c2v-restored-chat-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(restoredTab).toBeVisible();
  await expect(conversation.getByLabel('Message Codex')).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hs2-d34c2v-restored-chat-narrow.png', fullPage: true });
  await conversation.getByLabel('Message Codex').fill('What time is it in California?');
  await conversation.getByLabel('Message Codex').press('Enter');
  await expect(conversation.locator('[data-message-id]')).toHaveCount(4);
  expect(turnBodies.at(-1)).toMatchObject({ session_id: 'claude-thread-1', model: 'gpt-6-astra', effort: 'medium' });
});

test('restores an AI transcript and resumes its durable session after client and server restart (HS2-YHQCS2)', async ({
  page,
}) => {
  const connectionPosts: Array<Record<string, unknown>> = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.endsWith('/drive/connections'))
      connectionPosts.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  let drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  let conversation = drawer.locator('[data-component="ai-conversation"]');
  await conversation.getByLabel('Message Codex').fill('What time is it in California?');
  await conversation.getByLabel('Message Codex').press('Enter');
  await expect(conversation.getByText('The event stream remains authoritative.')).toBeVisible();
  const connectionId = String(connectionPosts.at(-1)?.connection_id);
  expect(connectionId).toMatch(/^hotsheet-drawer-chat-/);
  await page.route('**/__hotsheet/project-api/demo-checkout/connections', (route) => route.fulfill({ json: [] }));
  await page.route('**/__hotsheet/project-api/demo-checkout/drive/sessions', (route) =>
    route.fulfill({
      json: [
        {
          connection_id: connectionId,
          tool: 'codex',
          project: '/work/demo',
          session_id: 'claude-thread-1',
          updated_at_ms: 2,
        },
      ],
    }),
  );
  await page.reload();
  drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer.getByRole('tab', { name: 'Codex chat' })).toBeVisible();
  await drawer.getByRole('tab', { name: 'Codex chat' }).click();
  conversation = drawer.locator('[data-component="ai-conversation"]');
  await expect(conversation.locator('[data-message-id]')).toHaveCount(2);
  await expect(conversation.getByText('The event stream remains authoritative.')).toBeVisible();
  await expect
    .poll(() => connectionPosts.at(-1))
    .toMatchObject({ connection_id: connectionId, session_id: 'claude-thread-1' });
  await page.screenshot({ path: '/private/tmp/hs2-yhqcs2-restarted-conversation-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(conversation.getByLabel('Message Codex')).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hs2-yhqcs2-restarted-conversation-narrow.png', fullPage: true });
});

test('closes every terminal and AI chat before removing a project when Stop All is chosen', async ({ page }) => {
  const deletes: string[] = [];
  page.on('request', (request) => {
    if (request.method() === 'DELETE') deletes.push(new URL(request.url()).pathname);
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    projectTab = page.locator('[data-tab-kind="project"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  await expect(drawer.getByRole('tab', { name: 'Codex chat' })).toBeVisible();
  await projectTab.hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  await page.locator('[data-component="project-close-dialog"]').getByRole('button', { name: 'Stop & Close' }).click();
  await expect(projectTab).toHaveCount(0);
  await expect.poll(() => deletes.length).toBe(3);
  expect(deletes).toContain('/__hotsheet/project-api/demo-checkout/terminals/codex-main');
  expect(deletes).toContain('/__hotsheet/project-api/demo-checkout/terminals/tests');
  expect(
    deletes.some((path) =>
      /^\/__hotsheet\/project-api\/demo-checkout\/checkouts\/demo-checkout\/drive\/connections\/hotsheet-drawer-chat-/.test(
        path,
      ),
    ),
  ).toBe(true);
});

test('holds terminal geometry throughout a slow drawer drag and fits once it settles', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('tab', { name: /Codex Main/ }).click();
  const viewport = drawer.locator(
    '[data-component="terminal-session"]:not([hidden]) [data-component="terminal-viewport"]',
  );
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await viewport.click();
  const claimCount = () =>
    page.evaluate(() => {
      const sockets = (window as unknown as { __terminalSockets: Array<{ url: string; sent: unknown[] }> })
          .__terminalSockets,
        socket = sockets.filter((item) => item.url.includes('codex-main')).at(-1);
      return socket?.sent.filter((value) => typeof value === 'string' && value.startsWith('{')).length ?? 0;
    });
  await expect.poll(claimCount).toBeGreaterThan(0);
  const initialClaims = await claimCount(),
    initialGrid = await viewport.getAttribute('data-grid-size'),
    initialScreen = await viewport.locator('.xterm-screen').boundingBox(),
    handle = page.getByRole('separator', { name: 'Resize Terminal drawer' }),
    grip = (await handle.boundingBox())!;
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await page.mouse.down();
  await page.mouse.move(grip.x + grip.width / 2, grip.y - 80, { steps: 8 });
  await page.waitForTimeout(220);
  expect(await claimCount()).toBe(initialClaims);
  await expect(viewport).toHaveAttribute('data-grid-size', initialGrid!);
  expect(
    await viewport.locator('.xterm-screen').evaluate((element) => ({
      width: element.getBoundingClientRect().width,
      height: element.getBoundingClientRect().height,
    })),
  ).toEqual({ width: initialScreen!.width, height: initialScreen!.height });
  await page.mouse.move(grip.x + grip.width / 2, grip.y - 180, { steps: 10 });
  await page.waitForTimeout(220);
  expect(await claimCount()).toBe(initialClaims);
  await expect(viewport).toHaveAttribute('data-grid-size', initialGrid!);
  await page.screenshot({ path: '/private/tmp/hs2-rjev3s-slow-drawer-drag.png', fullPage: true });
  await page.mouse.up();
  await expect.poll(claimCount, { timeout: 800 }).toBeGreaterThan(initialClaims);
  await expect(viewport).toHaveAttribute('data-driving', 'true');
  await drawer.getByRole('tab', { name: 'Project grid' }).click();
  const dashboard = drawer.getByRole('region', { name: 'Workspace grid' });
  await dashboard.getByRole('button', { name: /Zoom in/ }).click();
  await expect(dashboard).toHaveAttribute('data-fit', '1');
  const tile = dashboard.locator('.terminal-tile').first(),
    initialTileHeight = await tile.evaluate((element) => element.getBoundingClientRect().height),
    gridGrip = (await handle.boundingBox())!;
  await page.mouse.move(gridGrip.x + gridGrip.width / 2, gridGrip.y + gridGrip.height / 2);
  await page.mouse.down();
  await page.mouse.move(gridGrip.x + gridGrip.width / 2, gridGrip.y + 80, { steps: 8 });
  await page.waitForTimeout(220);
  expect(await tile.evaluate((element) => element.getBoundingClientRect().height)).toBe(initialTileHeight);
  await page.mouse.up();
  await expect
    .poll(() => tile.evaluate((element) => element.getBoundingClientRect().height))
    .not.toBe(initialTileHeight);
});

test('resists below-minimum drawer resizing before a deliberate drag collapses it', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    region = page.locator('section[data-region-id="app-terminal-drawer"]'),
    handle = page.getByRole('separator', { name: 'Resize Terminal drawer' });
  await expect(region).toHaveAttribute('data-transitioning', 'false');
  const grip = (await handle.boundingBox())!;
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await page.mouse.down();
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2 + 100, { steps: 8 });
  await expect(handle).toHaveAttribute('aria-valuenow', '228');
  await expect(drawer).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-5rj7t0-drawer-minimum-resistance.png', fullPage: true });
  await page.mouse.up();
  await expect(handle).toHaveAttribute('aria-valuenow', '228');
  const minimumGrip = (await handle.boundingBox())!;
  await page.mouse.move(minimumGrip.x + minimumGrip.width / 2, minimumGrip.y + minimumGrip.height / 2);
  await page.mouse.down();
  await page.mouse.move(minimumGrip.x + minimumGrip.width / 2, minimumGrip.y + minimumGrip.height / 2 + 60, {
    steps: 8,
  });
  await page.mouse.up();
  await expect(drawer).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toBeVisible();
});

for (const theme of ['light', 'dark'] as const) {
  test(`filters workspace visibility by creation kind and chat in ${theme} (HS2-SE3RVM)`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ colorScheme: theme });
    await installFakeTerminalSockets(page, true);
    await mockProject(page);
    await page.route('**/terminals', (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({
            json: [
              { id: 'codex-main', kind: 'ai', alive: true, busy: true, cwd: '/work/demo' },
              { id: 'tests', alive: true, busy: false, cwd: '/work/demo', link: 'https://ai.example/session' },
            ],
          })
        : route.fallback(),
    );
    await page.goto('/');
    await page.getByRole('button', { name: 'Open project' }).click();
    await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show terminal drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await drawer.getByRole('button', { name: 'New drawer item' }).click();
    await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat', { exact: true }).click();
    await expect(drawer.getByRole('tab', { name: 'Codex chat' })).toBeVisible();
    await page.getByRole('button', { name: 'Workspace grid' }).click();
    const dashboard = page.getByRole('region', { name: 'Workspace grid' });
    const chat = dashboard.locator('[data-component="workspace-chat-tile"]');
    await expect(chat).toHaveCount(1);
    await page.getByRole('button', { name: 'Manage workspace visibility' }).click();
    const dialog = page.locator('[data-terminal-visibility-dialog]');
    const types = dialog.locator('wa-select[name="terminal-visibility-types"]');
    const rows = dialog.locator('[data-action="toggle-terminal-visibility"]');
    await expect(types).toHaveJSProperty('value', ['shell', 'ai', 'chat']);
    await expect(rows).toHaveCount(3);
    await dialog
      .locator('dialog')
      .screenshot({ path: `/private/tmp/hs2-se3rvm-dialog-${theme}-after.png`, animations: 'disabled' });
    await types.locator('[part~="expand-icon"]').click();
    const choose = async (value: string, selected: string[], count: number) => {
      await types.locator(`wa-option[value="${value}"]`).click();
      await expect(types).toHaveJSProperty('open', true);
      await expect
        .poll(() => types.evaluate((element: HTMLElement & { value?: string[] }) => [...(element.value ?? [])].sort()))
        .toEqual([...selected].sort());
      await expect(rows).toHaveCount(count);
      for (const kind of ['shell', 'ai', 'chat'])
        await expect(types.locator(`wa-option[value="${kind}"]`)).toHaveJSProperty('selected', selected.includes(kind));
    };
    await expect(types.locator('wa-option[value="browser"]')).toHaveJSProperty('disabled', true);
    await choose('deselect-all', [], 0);
    await expect(dialog.getByText('No workspace items match the selected types.')).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Hide listed' })).toBeDisabled();
    await choose('ai', ['ai'], 1);
    await expect(rows).toHaveAccessibleName('Hide Codex Main');
    await choose('shell', ['shell', 'ai'], 2);
    await choose('ai', ['shell'], 1);
    await expect(rows).toHaveAccessibleName('Hide Tests');
    await choose('chat', ['shell', 'chat'], 2);
    await choose('select-all', ['shell', 'ai', 'chat'], 3);
    await choose('deselect-all', [], 0);
    await choose('chat', ['chat'], 1);
    await page.screenshot({ path: `/private/tmp/hs2-se3rvm-filter-${theme}-open.png`, animations: 'disabled' });
    await page.keyboard.press('Escape');
    await expect(types).toHaveJSProperty('open', false);
    await expect(dialog).toHaveJSProperty('open', true);
    await dialog.getByRole('button', { name: 'Hide listed' }).click();
    await expect(rows).toHaveAccessibleName('Show Codex chat');
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveJSProperty('open', false);
    await expect(chat).toHaveCount(0);
    await expect(dashboard.locator('[data-component="terminal-tile"]')).toHaveCount(2);
    await expect(page.locator('.terminal-dashboard-controls__count')).toHaveText('1');
    await page.reload();
    await page.getByRole('button', { name: 'Workspace grid' }).click();
    await expect(chat).toHaveCount(0);
    await page.getByRole('button', { name: 'Manage workspace visibility' }).click();
    await expect(types).toHaveJSProperty('value', ['shell', 'ai', 'chat']);
    await expect(rows).toHaveCount(3);
    await types.locator('[part~="expand-icon"]').click();
    await choose('deselect-all', [], 0);
    await choose('chat', ['chat'], 1);
    await page.keyboard.press('Escape');
    await dialog.getByRole('button', { name: 'Show listed' }).click();
    await expect(rows).toHaveAccessibleName('Hide Codex chat');
    await page.setViewportSize({ width: 390, height: 844 });
    await types.locator('[part~="expand-icon"]').click();
    await choose('select-all', ['shell', 'ai', 'chat'], 3);
    await page.screenshot({ path: `/private/tmp/hs2-se3rvm-filter-${theme}-mobile.png`, animations: 'disabled' });
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(chat).toBeVisible();
    await expect(page.locator('.terminal-dashboard-controls__count')).toHaveCount(0);
    await page.screenshot({ path: `/private/tmp/hs2-se3rvm-dashboard-${theme}-chat.png`, animations: 'disabled' });
  });
}

test('creates, renames, persists, and context-deletes terminal visibility groups', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    codex = dashboard.locator('[data-terminal-key="demo-checkout:codex-main"]'),
    manage = page.getByRole('button', { name: 'Manage workspace visibility' }),
    selector = page.locator('wa-select[name="terminal-visibility-group"]');
  await manage.click();
  let dialog = page.locator('[data-terminal-visibility-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.locator('.terminal-visibility-dialog__toolbar')).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  await expect(dialog.getByRole('textbox', { name: 'Group name' })).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: 'Delete' })).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Add visibility group' }).click();
  const nameDialog = page.locator('[data-terminal-visibility-name-dialog]'),
    groupName = nameDialog.getByRole('textbox', { name: 'Group name' });
  await expect(groupName).toBeFocused();
  await groupName.fill('Focus');
  await nameDialog.getByRole('button', { name: 'Add' }).click();
  await expect(dialog.getByRole('tab', { name: 'Focus' })).toHaveAttribute('aria-selected', 'true');
  await dialog.getByRole('tab', { name: 'Focus' }).click({ button: 'right' });
  await dialog.getByRole('menu', { name: 'Visibility group actions' }).getByText('Rename…').click();
  await expect(groupName).toHaveValue('Focus');
  await groupName.fill('Deep Focus');
  await nameDialog.getByRole('button', { name: 'Rename' }).click();
  await expect(dialog.getByRole('tab', { name: 'Deep Focus' })).toHaveAttribute('aria-selected', 'true');
  await dialog.getByRole('button', { name: 'Hide Codex Main' }).click();
  await expect(dialog.getByRole('button', { name: 'Show Codex Main' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-wf0xqa-transparent-tab-bar-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(codex).toHaveCount(0);
  await expect(page.locator('.terminal-dashboard-controls__count')).toHaveText('1');
  await selector.click();
  await selector.locator('wa-option[value="default"]').click();
  await expect(selector).toHaveJSProperty('open', false);
  await expect(codex).toBeVisible();
  const focusOption = selector.locator('wa-option').filter({ hasText: 'Deep Focus' }),
    focusId = await focusOption.getAttribute('value');
  await selector.click();
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await expect(selector).toHaveJSProperty('value', focusId!);
  await expect(codex).toHaveCount(0);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.terminals.visibility-groups'))).resolves.toContain(
    'Deep Focus',
  );
  await page.reload();
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await expect(page.locator('wa-select[name="terminal-visibility-group"]')).toHaveJSProperty('value', focusId!);
  await expect(
    page.getByRole('region', { name: 'Workspace grid' }).locator('[data-terminal-key="demo-checkout:codex-main"]'),
  ).toHaveCount(0);
  dialog = page.locator('[data-terminal-visibility-dialog]');
  await dialog.evaluate((element) => {
    element.setAttribute('data-after-show-settled', 'false');
    element.addEventListener(
      'wa-after-show',
      () => {
        element.setAttribute('data-after-show-settled', 'true');
      },
      { once: true },
    );
  });
  await page.getByRole('button', { name: 'Manage workspace visibility' }).click();
  await expect(dialog).toHaveAttribute('data-after-show-settled', 'true');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.locator('.terminal-visibility-dialog__toolbar')).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  expect(
    await dialog.evaluate((element) => {
      const modal = element.shadowRoot?.querySelector('dialog');
      if (!modal) return { open: false, modal: false, topmost: false, runningAnimations: -1 };
      const bounds = modal.getBoundingClientRect(),
        topmost = document.elementFromPoint(bounds.left + bounds.width / 2, bounds.top + 24);
      return {
        open: modal.open,
        modal: modal.matches(':modal'),
        topmost: topmost === element,
        runningAnimations: modal.getAnimations().filter((animation) => animation.playState === 'running').length,
      };
    }),
  ).toEqual({ open: true, modal: true, topmost: true, runningAnimations: 0 });
  await page.screenshot({ path: '/private/tmp/hs2-e7cp5z-settled-narrow-dialog.png', fullPage: true });
  await dialog.getByRole('tab', { name: 'Deep Focus' }).click({ button: 'right' });
  await dialog.getByRole('menu', { name: 'Visibility group actions' }).getByText('Delete').click();
  await expect(dialog.getByRole('tab', { name: 'Default' })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('region', { name: 'Workspace grid' }).locator('[data-terminal-key="demo-checkout:codex-main"]'),
  ).toBeVisible();
});

test('focuses a newly created terminal as soon as its viewport starts', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('Default shell').click();
  const viewport = drawer.locator(
    '[data-component="terminal-session"]:not([hidden]) [data-component="terminal-viewport"]',
  );
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport.locator('.xterm-helper-textarea')).toBeFocused();
  await page.keyboard.type('focused immediately');
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as unknown as { __terminalSockets: Array<{ sent: unknown[] }> }).__terminalSockets
          .flatMap((socket) => socket.sent)
          .filter((value): value is string => typeof value === 'string' && !value.startsWith('{'))
          .join(''),
      ),
    )
    .toContain('focused immediately');
  await page.screenshot({ path: '/private/tmp/hs2-h2m7sp-new-terminal-focus.png', fullPage: true });
});

test('renames a terminal from its tab menu and keeps the device-local name after reload', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer.getByRole('tab', { name: /Codex Main/ })).toBeVisible();
  const tab = drawer.getByRole('tab', { name: /Tests/ });
  await tab.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Terminal tab actions' });
  await menu.getByText('Rename…').click();
  const dialog = page.locator('[data-terminal-rename-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  const name = dialog.getByRole('textbox', { name: /Terminal name/ });
  await expect(name).toBeFocused();
  await name.fill('Quality shell');
  await dialog.getByRole('button', { name: 'Rename' }).click();
  await expect(dialog).toHaveJSProperty('open', false);
  await expect(drawer.getByRole('tab', { name: /Quality shell/ })).toBeVisible();
  await expect(
    page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.terminals.names') ?? '{}')['demo-checkout:tests']),
  ).resolves.toBe('Quality shell');
  await page.screenshot({ path: '/private/tmp/hs2-terminal-renamed.png', fullPage: true });
  await page.reload();
  await expect(
    page.locator('[data-component="terminal-drawer"]').getByRole('tab', { name: /Quality shell/ }),
  ).toBeVisible();
});

test('resizes the terminal drawer to the page-header boundary', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const main = page.locator('.app-shell__main'),
    workArea = page.locator('.app-shell__work-area'),
    region = page.locator('section[data-region-id="app-terminal-drawer"]'),
    handle = page.getByRole('separator', { name: 'Resize Terminal drawer' });
  await expect(region).toHaveAttribute('data-transitioning', 'false');
  const geometry = await main.evaluate(
      (element, workspace) => ({
        bottom: element.getBoundingClientRect().bottom,
        top: (workspace as HTMLElement).getBoundingClientRect().top,
      }),
      await workArea.elementHandle(),
    ),
    expected = Math.floor(geometry.bottom - geometry.top);
  await expect(handle).toHaveAttribute('aria-valuemax', String(expected));
  await handle.focus();
  for (let step = 0; step < 32; step += 1) await page.keyboard.press('ArrowUp');
  await expect(handle).toHaveAttribute('aria-valuenow', String(expected));
  expect(Math.abs((await region.boundingBox())!.y - geometry.top)).toBeLessThanOrEqual(1);
  expect(
    await page.evaluate(
      ({ x, y }) => Boolean(document.elementFromPoint(x, y)?.closest('[data-component="terminal-drawer"]')),
      { x: (await region.boundingBox())!.x + 20, y: geometry.top + 20 },
    ),
  ).toBe(true);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.layout.app-terminal-drawer.size'))).resolves.toBe(
    String(expected),
  );
  await page.screenshot({ path: '/private/tmp/hs2-4fzgm7-drawer-max.png', fullPage: true });
});

test('double-clicks the drawer rail or any terminal tab to toggle maximization', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    rail = drawer.locator('.terminal-drawer__rail'),
    handle = page.getByRole('separator', { name: 'Resize Terminal drawer' });
  await handle.focus();
  await page.keyboard.press('ArrowUp');
  await expect(handle).toHaveAttribute('aria-valuenow', '336');
  await rail.dispatchEvent('dblclick');
  const maximum = await handle.getAttribute('aria-valuemax');
  await expect(drawer).toHaveAttribute('data-maximized', 'true');
  await expect(handle).toHaveAttribute('aria-valuenow', maximum!);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.layout.app-terminal-drawer.size'))).resolves.toBe(
    '336',
  );
  await drawer.getByRole('tab', { name: 'Project grid' }).dispatchEvent('dblclick');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(handle).toHaveAttribute('aria-valuenow', '336');
  await drawer.getByRole('tab', { name: /Codex Main/ }).dispatchEvent('dblclick');
  await expect(drawer).toHaveAttribute('data-maximized', 'true');
  await page.screenshot({ path: '/private/tmp/hs2-terminal-tab-maximized.png', fullPage: true });
  await rail.dispatchEvent('dblclick');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(handle).toHaveAttribute('aria-valuenow', '336');
  await page.screenshot({ path: '/private/tmp/hs2-terminal-drawer-restored.png', fullPage: true });
});

test('snaps the drawer track while compositing visibility motion and suppressing motion during resize', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const region = page.locator('section[data-region-id="app-terminal-drawer"]'),
    drawer = page.locator('[data-component="terminal-drawer"]'),
    handle = page.getByRole('separator', { name: 'Resize Terminal drawer' }),
    content = region.locator('.kui-resizable-region__content');
  await expect(region).toHaveAttribute('data-transitioning', 'true');
  await expect(region).toHaveCSS('transition-duration', '0s');
  expect(await content.evaluate((element) => getComputedStyle(element).transitionDuration)).toContain('0.2s');
  await expect(region).toHaveAttribute('data-transitioning', 'false');
  await handle.focus();
  await page.keyboard.press('ArrowUp');
  await expect(handle).toHaveAttribute('aria-valuenow', '336');
  await expect(region).toHaveAttribute('data-transitioning', 'false');
  await expect(region).toHaveCSS('transition-duration', '0s');
  await drawer.getByRole('button', { name: 'Hide terminal drawer' }).click();
  await expect(region).toHaveAttribute('data-transitioning', 'true');
  await expect(region).toHaveAttribute('data-collapsed', 'true');
  await expect(drawer).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toHaveCount(0);
  await page.waitForTimeout(100);
  await page.screenshot({ path: '/private/tmp/hs2-x8fg23-drawer-hiding.png', fullPage: true });
  await expect(drawer).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toBeVisible();
});

test('keeps the collapsed terminal-drawer restore action on canonical shell insets (HS2-4Y6SM9)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const restore = page.getByRole('button', { name: 'Show terminal drawer' });
  await expect(restore).toBeVisible();
  const restoreToolbar = page.getByRole('toolbar', { name: 'Terminal drawer controls' });
  await expect(restoreToolbar).toHaveAttribute('data-component', 'floating-toolbar');
  await expect(restoreToolbar).toHaveAttribute('data-position', 'bottom-end');
  const restoreGroup = restoreToolbar.locator('[data-component="toolbar-control-group"]');
  await expect(restoreGroup).toHaveAttribute('data-tone', 'default');
  await expect(restoreGroup).toHaveCSS('color-scheme', 'dark');
  await expect(restoreGroup).toHaveCSS('background-color', 'rgb(58, 58, 60)');
  const insets = () =>
    restoreToolbar.evaluate((node) => {
      const style = getComputedStyle(node);
      return { right: style.right, bottom: style.bottom };
    });
  expect(await insets()).toEqual({ right: '16px', bottom: '16px' });
  await page.screenshot({ path: '/private/tmp/hs2-4y6sm9-app-shell-drawer-restore-wide.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-drawer-restore-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(restore).toBeVisible();
  expect(await insets()).toEqual({ right: '16px', bottom: '16px' });
  await page.screenshot({ path: '/private/tmp/hs2-4y6sm9-app-shell-drawer-restore-narrow.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-w3gphw-drawer-restore-narrow.png', fullPage: true });
});

test('keeps feedback rectangle input within its frame budget in the populated main app', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page);
  await page.goto('/?dev-review=1');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const measurement = await measureFeedbackRectangle(page, { x: 440, y: 220 }, { x: 820, y: 520 });
  await testInfo.attach('feedback-performance.json', {
    body: JSON.stringify(measurement, null, 2),
    contentType: 'application/json',
  });
  expectResponsiveFeedbackRectangle(measurement);
  await page.screenshot({ path: '/private/tmp/hs2-6ppvjc-main-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: '/private/tmp/hs2-6ppvjc-main-narrow.png', fullPage: true });
});

test('projects an indexed feedback-needed note into the real row and inspector rails', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page, true, true);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const ticket = page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(ticket.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS(
    'background-color',
    'rgb(203, 48, 224)',
  );
  await expect(ticket.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await ticket.click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  await expect(inspector).toHaveAttribute('data-needs-review', 'true');
  await expect(inspector.locator('.ticket-inspector__feedback')).toContainText('Needs review');
  await expect.poll(() => inspector.evaluate((node) => getComputedStyle(node, '::before').width)).toBe('auto');
  await page.screenshot({ path: '/private/tmp/hs2-9fa1bv-feedback-sidebar-without-rail-wide.png', fullPage: true });
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const readerInspector = page.getByRole('dialog').locator('[data-component="ticket-inspector"]');
  await expect(readerInspector.locator('.ticket-inspector__feedback')).toContainText('Needs review');
  await expect.poll(() => readerInspector.evaluate((node) => getComputedStyle(node, '::before').width)).toBe('auto');
  await page.screenshot({ path: '/private/tmp/hs2-n6wa7y-reader-no-rail.png' });
  await page.getByRole('button', { name: 'Close ticket reader' }).click();
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(ticket.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS(
    'background-color',
    'rgb(203, 48, 224)',
  );
  await expect(inspector).toHaveAttribute('data-needs-review', 'true');
  await page.screenshot({ path: '/private/tmp/hs2-9fa1bv-feedback-sidebar-without-rail-narrow.png', fullPage: true });
});

test('clicks exact feedback character positions, removes a split, and composes an interleaved Markdown reply', async ({
  page,
}) => {
  const patches = await mockProject(page, true, true);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]'),
    clickAfter = async (text: string) => {
      const block = note.locator('.note-card__feedback-block').filter({ hasText: text });
      await block.scrollIntoViewIfNeeded();
      const point = await block.evaluate((element, needle) => {
        const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        let node: Node | null;
        while ((node = walker.nextNode())) {
          const index = node.textContent?.indexOf(needle) ?? -1;
          if (index < 0) continue;
          const range = document.createRange();
          range.setStart(node, index + needle.length - 1);
          range.setEnd(node, index + needle.length);
          const rect = range.getBoundingClientRect();
          return { x: rect.right - 0.5, y: rect.y + rect.height / 2 };
        }
        throw new Error(`Text not found: ${needle}`);
      }, text);
      await page.mouse.click(point.x, point.y);
    };
  expect(
    await note
      .getByRole('textbox', { name: 'Feedback response' })
      .evaluate((node) => node.getBoundingClientRect().height),
  ).toBeLessThan(75);
  await expect(note.getByRole('button', { name: 'Add response at a character position' })).toHaveCount(1);
  await clickAfter('Something');
  const first = note.getByRole('textbox', { name: /Response at character/ });
  await expect(first).toBeFocused();
  await first.fill('Discard me');
  await note.getByRole('button', { name: /Remove response at character/ }).click();
  await expect(note.locator('.note-card__inline-reply')).toHaveCount(0);
  await clickAfter('Something');
  await note.getByRole('textbox', { name: /Response at character/ }).fill('My first response');
  await clickAfter('Another thing');
  const second = note.getByRole('textbox', { name: /Response at character/ }).last();
  await expect(second).toBeFocused();
  await second.fill('My second response');
  await note.screenshot({ path: '/private/tmp/hs2-c5sab3-inline-feedback-replies.png' });
  await page.setViewportSize({ width: 760, height: 900 });
  const noteBox = (await note.boundingBox())!,
    replyBox = (await second.boundingBox())!;
  expect(replyBox.x).toBeGreaterThanOrEqual(noteBox.x);
  expect(replyBox.x + replyBox.width).toBeLessThanOrEqual(noteBox.x + noteBox.width);
  await note.screenshot({ path: '/private/tmp/hs2-c5sab3-inline-feedback-replies-narrow.png' });
  await note.getByRole('button', { name: 'Respond' }).click();
  await expect
    .poll(() => patches.find((patch) => patch.note_kind === 'regular')?.note)
    .toBe(
      '> FEEDBACK NEEDED\n>\n> Hello there\n>\n> 1. Something\n\nMy first response\n\n> 2. Another thing\n\nMy second response',
    );
});

test('selects feedback choices with platform modifiers and preserves a freeform response', async ({ page }) => {
  const patches = await mockProject(page, true, 'choices');
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]'),
    choices = note.getByRole('group', { name: 'Feedback choices' }).locator('.note-card__choice');
  await expect(choices).toHaveCount(3);
  await choices.nth(0).click();
  await expect(choices.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await choices.nth(1).click({ modifiers: [process.platform === 'darwin' ? 'Meta' : 'Control'] });
  await expect(choices.nth(1)).toHaveAttribute('aria-pressed', 'true');
  await expect(choices.nth(1).locator('img')).toHaveAttribute('src', /\/tickets\/HS2-DEMO01\/attachments\/A1$/);
  await note.getByRole('textbox', { name: 'Feedback response' }).fill('The two options can be combined.');
  await note.screenshot({ path: '/private/tmp/hs2-6eeeqb-feedback-choices.png' });
  await note.getByRole('button', { name: 'Respond' }).click();
  await expect
    .poll(() => patches.find((patch) => patch.note_kind === 'regular')?.note)
    .toBe(
      'Selected choices:\n- Keep the **current behavior**\n- Use `attachment:proof.png`\n\nThe two options can be combined.',
    );
});

test('responds to feedback choices embedded in the ticket description', async ({ page }) => {
  const patches = await mockProject(page, true, 'details');
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const ticket = page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(ticket.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await ticket.click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]'),
    sidebarDetails = inspector.locator('.ticket-inspector__details-surface[data-feedback-needed="true"]'),
    open = inspector.getByRole('button', { name: 'Respond to Feedback' });
  await expect(sidebarDetails).toHaveCSS('background-color', 'color(srgb 1 0.982 0.91)');
  const sidebarInsets = await sidebarDetails.evaluate((surface) => {
    const button = surface.querySelector('wa-button')!.getBoundingClientRect(),
      outer = surface.getBoundingClientRect();
    return { left: button.left - outer.left, right: outer.right - button.right, bottom: outer.bottom - button.bottom };
  });
  expect(Math.min(...Object.values(sidebarInsets))).toBeGreaterThanOrEqual(9);
  await sidebarDetails.screenshot({ path: '/private/tmp/hs2-11n1wj-details-feedback-sidebar.png' });
  await expect(open).toBeVisible();
  await open.click();
  const reader = page.getByRole('dialog'),
    readerSurface = reader.locator('.ticket-inspector__details-surface[data-feedback-needed="true"]'),
    details = reader.locator('[data-details-feedback="true"]'),
    choices = details.getByRole('group', { name: 'Feedback choices' }).locator('.note-card__choice');
  await expect(details.locator('.ticket-inspector__details-feedback-header')).toContainText('Feedback needed');
  await expect(
    details.locator('.ticket-inspector__details-feedback-header [data-lucide="circle-alert"]'),
  ).toBeVisible();
  await expect(readerSurface).toHaveCSS('background-color', 'color(srgb 1 0.982 0.91)');
  await expect(choices).toHaveCount(2);
  await expect(details.getByRole('textbox', { name: 'Feedback response' })).toBeFocused();
  await choices.nth(1).click();
  await details.getByRole('textbox', { name: 'Feedback response' }).fill('This matches the exported design.');
  await readerSurface.screenshot({ path: '/private/tmp/hs2-11n1wj-details-feedback-reader-wide.png' });
  await page.setViewportSize({ width: 940, height: 844 });
  await readerSurface.screenshot({ path: '/private/tmp/hs2-11n1wj-details-feedback-reader-narrow.png' });
  await details.getByRole('button', { name: 'Respond' }).click();
  await expect
    .poll(() => patches.find((patch) => patch.note_kind === 'regular')?.note)
    .toBe('Selected choice:\n- Use `attachment:proof.png`\n\nThis matches the exported design.');
  await expect(ticket.locator('.ticket-list-row__feedback')).toHaveCount(0);
});

test('records No response needed as a subtle regular response and clears review state', async ({ page }) => {
  const patches = await mockProject(page, true, true);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const ticket = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await ticket.click();
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog'),
    note = reader.locator('article[data-note-id="N4"]'),
    dismiss = note.getByRole('button', { name: 'No response needed' }),
    respond = note.getByRole('button', { name: 'Respond', exact: true });
  await note.screenshot({ path: '/private/tmp/hs2-9fng50-feedback-actions-after.png' });
  const dismissBox = (await dismiss.boundingBox())!,
    respondBox = (await respond.boundingBox())!;
  await expect(dismiss.locator('svg')).toHaveCount(0);
  expect(Math.abs(dismissBox.height - respondBox.height)).toBeLessThanOrEqual(1);
  expect(Math.abs(dismissBox.y + dismissBox.height / 2 - (respondBox.y + respondBox.height / 2))).toBeLessThanOrEqual(
    1,
  );
  await dismiss.click();
  await expect
    .poll(() => patches.some((patch) => patch.note === 'No response needed' && patch.note_kind === 'regular'))
    .toBe(true);
  await expect(reader.locator('[data-acknowledgement="true"]')).toContainText('No response needed');
  await page.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(ticket.locator('.ticket-list-row__feedback')).toHaveCount(0);
  await ticket.click();
  const acknowledgement = page.locator('[data-component="ticket-inspector"] [data-acknowledgement="true"]');
  await expect(acknowledgement).toBeVisible();
  await acknowledgement.screenshot({ path: '/private/tmp/hs2-yk27gp-no-response-needed.png' });
});

test('opens reader at the active feedback note from its inspector action', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 520 });
  await mockProject(page, true, true);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]'),
    action = inspector.getByRole('button', { name: 'Respond to Feedback' });
  await expect(action).toHaveCount(1);
  await action.scrollIntoViewIfNeeded();
  await inspector
    .locator('article[data-note-id="N4"]')
    .screenshot({ path: '/private/tmp/hs2-f8wrkq-inspector-respond.png' });
  await action.click();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ }),
    active = reader.locator('article[data-note-id="N4"]'),
    response = active.getByRole('textbox', { name: 'Feedback response' });
  await expect(reader.getByRole('button', { name: 'Respond to Feedback' })).toHaveCount(0);
  await expect(active).toBeInViewport();
  await expect(response).toBeFocused();
  await reader.screenshot({ path: '/private/tmp/hs2-f8wrkq-reader-feedback.png' });
});

test('browses repository files and commits with host-native actions', async ({ page }) => {
  await mockProject(page);
  const actions: Array<Record<string, unknown>> = [];
  let failed = false;
  const response = {
    branch: 'feature/repository-dialog',
    upstream: 'origin/main',
    ahead: 2,
    behind: 1,
    staged: 2,
    unstaged: 1,
    untracked: 1,
    conflicted: 1,
    clean: false,
    root: '/work/demo',
    platform: 'macos',
    commit_count: 24,
    difftool: 'Glassbox',
    truncated: false,
    files: [
      { path: 'src/staged.ts', staged: 'added', untracked: false, conflicted: false },
      {
        path: 'src/components/repository/status/very-long-renamed-file-name-for-middle-ellipsis.ts',
        original_path: 'src/components/repository/status/very-long-original-file-name-for-middle-ellipsis.ts',
        staged: 'renamed',
        untracked: false,
        conflicted: false,
      },
      { path: 'src/changed.ts', unstaged: 'modified', untracked: false, conflicted: false },
      { path: 'notes/new.md', untracked: true, conflicted: false },
      { path: 'src/conflict.ts', staged: 'unmerged', unstaged: 'unmerged', untracked: false, conflicted: true },
    ],
    ranges: [{ from: 'aaa1111', to: 'bbb2222', count: 2 }],
    commits: [
      {
        sha: 'bbb2222',
        short_sha: 'bbb2222',
        subject: 'Finish repository status dialog',
        body: 'Visible first line\nSecond **Markdown** line\nExpanded third line',
        committed_at: '2026-09-04T10:00:00Z',
        refs: [
          { label: 'HEAD → main', kind: 'head' },
          { label: 'origin/main', kind: 'remote' },
          { label: 'v0.9.0', kind: 'tag' },
        ],
      },
      {
        sha: 'aaa1111',
        short_sha: 'aaa1111',
        subject: 'Start repository status dialog',
        body: 'Initial implementation details.',
        committed_at: '2026-09-04T09:00:00Z',
      },
    ],
  };
  await page.route('**/repository/**', (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (url.pathname.endsWith('/repository/status'))
      return failed
        ? route.fulfill({ status: 500, body: 'git unavailable' })
        : route.fulfill({ json: { ...response, files: [], commits: [] } });
    if (url.pathname.endsWith('/repository/files')) {
      const view = url.searchParams.get('view'),
        cursor = Number(url.searchParams.get('cursor') ?? 0),
        items = response.files.filter((file) =>
          view === 'staged'
            ? file.staged && !file.conflicted
            : view === 'unstaged'
              ? file.unstaged && !file.untracked && !file.conflicted
              : view === 'untracked'
                ? file.untracked
                : file.conflicted,
        );
      return route.fulfill({
        json: { items: items.slice(cursor, cursor + 1), next_cursor: cursor + 1 < items.length ? cursor + 1 : null },
      });
    }
    if (url.pathname.endsWith('/repository/commits')) {
      const cursor = Number(url.searchParams.get('cursor') ?? 0);
      return route.fulfill({
        json: {
          items: response.commits.slice(cursor, cursor + 1),
          next_cursor: cursor + 1 < response.commits.length ? cursor + 1 : null,
        },
      });
    }
    actions.push({ path: url.pathname, ...request.postDataJSON() });
    return route.fulfill({ status: 204 });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const summary = page.locator('[data-component="repository-summary"]');
  await summary.getByRole('button').click();
  const popover = page.locator('[data-component="repository-status-popover"]');
  await expect(popover).toHaveAttribute('data-state', 'conflicted');
  await expect(popover.locator('dt', { hasText: 'Branch' }).locator('..')).toContainText('feature/repository-dialog');
  await expect(popover.getByRole('navigation', { name: 'Repository views' })).toContainText('24');
  await expect(popover).toHaveAttribute('data-view', 'conflicted');
  await expect(popover.locator('[data-action="select-repository-file"][data-item-id="src/conflict.ts"]')).toBeVisible();
  const master = popover.locator('aside');
  await expect(master).toHaveCSS('overflow-y', 'auto');
  expect(await master.evaluate((element) => element.scrollHeight >= element.clientHeight)).toBe(true);
  await popover.getByRole('button', { name: /Staged 2/ }).click();
  const stagedRows = popover.locator('[data-action="select-repository-file"]');
  await expect(stagedRows).toHaveCount(2);
  const renamed = popover.locator('[data-state="renamed"]');
  await expect(renamed.locator('.repository-status-popover__file-status')).toHaveText('R');
  await expect(renamed).toHaveAttribute('aria-label', /Renamed/);
  await expect(renamed.locator('.repository-status-popover__file-state')).toHaveCount(0);
  await expect(
    popover.getByRole('button', { name: /Untracked 1/ }).locator('[data-lucide="square-pen"]'),
  ).toBeVisible();
  expect(
    await renamed
      .locator('.repository-status-popover__path > span')
      .first()
      .evaluate((element) => element.scrollWidth > element.clientWidth),
  ).toBe(true);
  const staged = stagedRows
    .filter({ has: page.locator('[data-item-id="src/staged.ts"]') })
    .or(stagedRows.locator('[data-item-id="src/staged.ts"]'))
    .first();
  await staged.click();
  await expect(staged).toHaveAttribute('aria-pressed', 'true');
  await expect(popover.getByRole('menu')).toHaveCount(0);
  await renamed.click({ modifiers: ['Meta'] });
  await expect(renamed).toHaveAttribute('aria-pressed', 'true');
  await staged.click({ button: 'right' });
  await expect(popover.getByRole('menuitem', { name: 'Open' })).toBeDisabled();
  await popover.getByRole('menuitem', { name: 'Show Diff' }).click();
  await expect
    .poll(() => actions.filter((action) => action.mode === 'worktree_file' && action.area === 'staged').length)
    .toBe(2);
  await expect(page.locator('.app-toast')).toContainText('Opened 2 staged file diffs in Glassbox.');
  await page.screenshot({ path: '/private/tmp/hs2-jgfm53-multi-file-menu-wide.png' });
  await staged.dblclick();
  await expect
    .poll(() => actions.some((action) => action.action === 'open' && action.path === 'src/staged.ts'))
    .toBe(true);
  await staged.click();
  await staged.click({ button: 'right' });
  await expect(popover.getByRole('menuitem', { name: 'Show in Finder' })).toBeEnabled();
  await popover.getByRole('menuitem', { name: 'Show in Finder' }).click();
  await expect
    .poll(() => actions.some((action) => action.action === 'reveal' && action.path === 'src/staged.ts'))
    .toBe(true);
  await page.evaluate(() => getSelection()?.removeAllRanges());
  await popover.screenshot({ path: '/private/tmp/hs2-stva92-repository-files-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await popover.screenshot({ path: '/private/tmp/hs2-stva92-repository-files-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await popover.getByRole('button', { name: /Commits 24/ }).click();
  await expect(popover).toContainText('Finish repository status dialog');
  await expect(popover).toContainText('Second Markdown line');
  await expect(popover).not.toContainText('Expanded third line');
  const newest = popover.locator('li[data-commit-sha="bbb2222"]');
  await expect(newest.locator('.ticket-code-review__ref[data-ref-kind="head"]')).toContainText('HEAD → main');
  await expect(newest.locator('.ticket-code-review__ref[data-ref-kind="remote"]')).toContainText('origin/main');
  await expect(newest.locator('.ticket-code-review__ref[data-ref-kind="tag"]')).toContainText('v0.9.0');
  await expect(newest.locator('.ticket-code-review__ref[data-ref-kind="tag"] [data-lucide="tag"]')).toBeVisible();
  await expect(popover.locator('li[data-commit-sha="aaa1111"] .ticket-code-review__ref')).toHaveCount(0);
  await popover.screenshot({ path: '/private/tmp/hs2-sfj5te-commit-refs.png' });
  await newest.locator('.ticket-code-review__commit-summary').click();
  await expect(newest).toContainText('Expanded third line');
  await newest.locator('.ticket-code-review__commit-summary').click();
  await expect(newest).not.toContainText('Expanded third line');
  await popover.getByRole('button', { name: /Open 2 commit bundle/ }).click();
  await expect
    .poll(() =>
      actions.some((action) => action.mode === 'range' && action.from === 'aaa1111' && action.to === 'bbb2222'),
    )
    .toBe(true);
  const compareToggle = popover.getByRole('button', { name: 'Compare two commits' });
  await compareToggle.click();
  await expect(compareToggle).toHaveAttribute('aria-pressed', 'true');
  await expect(popover.locator('.ticket-code-review__compare-banner')).toContainText('Select the A side');
  await expect(popover.getByRole('button', { name: 'Cancel' })).toHaveCount(0);
  await newest.locator('.ticket-code-review__commit-summary').click();
  await expect(newest.locator('.ticket-code-review__compare-label')).toHaveText('A');
  await expect(popover.locator('.ticket-code-review__compare-banner')).toContainText('Select the B side');
  const oldest = popover.locator('li[data-commit-sha="aaa1111"]');
  await oldest.locator('.ticket-code-review__commit-summary').click();
  await expect(oldest.locator('.ticket-code-review__compare-label')).toHaveText('B');
  const openCompare = popover.getByRole('button', { name: 'Open comparison in Glassbox' });
  await expect(openCompare).toBeEnabled();
  await openCompare.click();
  await expect
    .poll(() =>
      actions.some((action) => action.mode === 'compare' && action.from === 'bbb2222' && action.to === 'aaa1111'),
    )
    .toBe(true);
  await compareToggle.click();
  await expect(compareToggle).toHaveAttribute('aria-pressed', 'false');
  await expect(popover.locator('.ticket-code-review__compare-banner')).toHaveCount(0);
  const detail = popover.locator('.repository-status-popover__detail');
  await expect(detail).toHaveCSS('overflow-y', 'auto');
  failed = true;
  await popover.getByRole('button', { name: 'Refresh repository status' }).click();
  await expect(popover).toHaveAttribute('data-state', 'error');
  await expect(popover.getByRole('alert')).toBeVisible();
  await expect(summary).toHaveAttribute('data-state', 'error');
});

test('refreshes repository status from the project long-poll stream without simple polling', async ({ page }) => {
  await mockProject(page);
  let ahead = 0,
    statusRequests = 0;
  const pending: Array<import('@playwright/test').Route> = [];
  await page.route('**/repository/status', (route) => {
    statusRequests += 1;
    return route.fulfill({
      json: {
        branch: 'main',
        ahead,
        behind: 0,
        staged: 0,
        unstaged: 0,
        untracked: 0,
        conflicted: 0,
        clean: ahead === 0,
      },
    });
  });
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor: 4, events: [], overflow: false } });
    pending.push(route);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const summary = page.locator('[data-component="repository-summary"]');
  await expect(summary.getByRole('button')).toHaveAccessibleName(/0 ahead/);
  await expect.poll(() => pending.length).toBeGreaterThan(0);
  ahead = 3;
  await pending.shift()!.fulfill({
    json: {
      cursor: 5,
      events: [{ store: 'demo-checkout', kind: 'repository_changed', id: 'demo-checkout', slug: '' }],
      overflow: false,
    },
  });
  await expect(summary.getByRole('button')).toHaveAccessibleName(/3 ahead/);
  expect(statusRequests).toBe(2);
  await summary.screenshot({ path: '/private/tmp/hs2-tr9369-proactive-repository-status.png' });
});

test('keeps exactly 24px above and below the nearly full-height ticket reader', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' }),
    wide = await reader.boundingBox();
  expect(wide!.y).toBe(24);
  expect(wide!.height).toBe(852);
  await page.screenshot({ path: '/private/tmp/hs2-h5vjet-reader-height-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  const narrow = await reader.boundingBox();
  expect(narrow!.y).toBe(24);
  expect(narrow!.height).toBe(852);
  await page.screenshot({ path: '/private/tmp/hs2-h5vjet-reader-height-narrow.png', fullPage: true });
});

test('enlarges reader description and note text 1.5× with a remembered global toggle', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page);
  const richFull = {
    ...full,
    details:
      '## Details heading\n\nDetails paragraph with `inline code`.\n\n- Details list item\n\n> Details quotation\n\n| Column |\n| --- |\n| Details cell |',
    notes: [
      ...full.notes.map((note) =>
        note.id === 'N3'
          ? {
              ...note,
              text: '## Note heading\n\nRegular note paragraph with `note code`.\n\n- Regular note list item\n\n> Regular note quotation\n\n| Column |\n| --- |\n| Note cell |',
            }
          : note,
      ),
      {
        id: 'N4',
        kind: 'feedback_needed',
        created_at: '2026-08-30T00:37:00Z',
        edited_at: '2026-08-30T00:37:00Z',
        text: 'Feedback prompt paragraph with `feedback code`.\n\n- Feedback list item',
      },
    ],
  };
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { store: 'git-local', ...richFull } })
      : route.fallback(),
  );
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  let reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  const details = reader.locator('.ticket-inspector__details-surface .markdown-preview').first(),
    note = reader.locator('article[data-note-id="N3"] .note-card__body .markdown-preview'),
    feedback = reader.locator('article[data-note-id="N4"] .note-card__feedback-prompt .markdown-preview'),
    activity = reader.locator('article[data-note-id="N1"] .note-card__body .markdown-preview'),
    toggle = reader.locator('[data-action="toggle-reader-text-size"]');
  const leaves = {
    detailsParagraph: details.locator('p').first(),
    detailsList: details.locator('li').first(),
    detailsHeading: details.locator('h2'),
    detailsQuote: details.locator('blockquote p'),
    detailsCode: details.locator('p code'),
    detailsCell: details.locator('td'),
    noteParagraph: note.locator('p').first(),
    noteList: note.locator('li').first(),
    noteHeading: note.locator('h2'),
    noteQuote: note.locator('blockquote p'),
    noteCode: note.locator('p code'),
    noteCell: note.locator('td'),
    feedbackParagraph: feedback.locator('p').first(),
    feedbackList: feedback.locator('li').first(),
    feedbackCode: feedback.locator('code').first(),
    activityParagraph: activity.locator('p'),
  };
  const fontSize = async (locator: typeof details) =>
    parseFloat(await locator.evaluate((element) => getComputedStyle(element).fontSize));
  const ordinary = Object.fromEntries(
    await Promise.all(Object.entries(leaves).map(async ([name, locator]) => [name, await fontSize(locator)])),
  );
  await expect(toggle).toHaveAttribute('aria-label', 'Use large reader text size');
  await toggle.click();
  await expect(reader).toHaveAttribute('data-large-text', 'true');
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(toggle).toHaveAttribute('aria-label', 'Use standard reader text size');
  for (const [name, locator] of Object.entries(leaves))
    await expect
      .poll(() => fontSize(locator), { message: `${name} should be exactly 1.5×` })
      .toBeCloseTo(ordinary[name] * 1.5, 1);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.reader.large-text'))).resolves.toBe('true');
  await feedback.scrollIntoViewIfNeeded();
  await page.screenshot({ path: '/private/tmp/hs2-p85gec-reader-large-feedback-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await feedback.scrollIntoViewIfNeeded();
  await page.screenshot({ path: '/private/tmp/hs2-p85gec-reader-large-feedback-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await expect(reader).toHaveAttribute('data-large-text', 'true');
  await reader.getByRole('button', { name: 'Use standard reader text size' }).click();
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.reader.large-text'))).resolves.toBe('false');
});

test('clears needs review when a regular response follows the feedback-needed note', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const ticket = page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(ticket.locator('.ticket-list-row__indicator--needs-review')).toHaveCount(0);
  await expect(ticket.locator('.ticket-list-row__feedback')).toHaveCount(0);
  await ticket.click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toHaveAttribute('data-needs-review', 'false');
  await expect(inspector.locator('.ticket-inspector__feedback')).toHaveCount(0);
  await expect(inspector.locator('[data-component="note-card"][data-note-id="N2"]')).toHaveAttribute(
    'data-kind',
    'regular',
  );
  await inspector
    .locator('[data-component="note-card"][data-note-id="N2"]')
    .screenshot({ path: '/private/tmp/hs2-98q45q-answered-feedback.png' });
});

test('shows Blocked only when the ticket has a visible blocked reason', async ({ page }) => {
  await mockProject(page);
  const target = { ...row, blocked_by: [notStartedRow.id] };
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [target, notStartedRow] }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const ticket = page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(ticket.locator('[data-component="blocked-badge"]')).toHaveCount(0);
  await ticket.screenshot({ path: '/private/tmp/hs2-e3c530-edge-without-reason.png' });
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: [{ ...target, blocked_reason: 'Waiting for access' }, notStartedRow] })
      : route.fallback(),
  );
  await page.reload();
  await expect(ticket.locator('[data-component="blocked-badge"]')).toHaveText('Blocked');
  await ticket.screenshot({ path: '/private/tmp/hs2-e3c530-visible-blocked-reason.png' });
});

test('lists associated commits and opens a validated commit or range in the configured diff tool', async ({ page }) => {
  const actions = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('tab', { name: 'Code Review' }).click();
  const review = inspector.locator('[data-component="ticket-code-review"]');
  const commits = review.locator('.ticket-code-review__commits'),
    firstCommit = commits.locator(':scope > li').first();
  const expectFlushCommit = async () => {
    expect(
      Math.abs(
        (await firstCommit.evaluate((node) => node.getBoundingClientRect().left)) -
          (await commits.evaluate((node) => node.getBoundingClientRect().left)),
      ),
    ).toBeLessThanOrEqual(1);
  };
  await expect(
    inspector.getByRole('tab', { name: 'Code Review' }).locator('[data-lucide="message-square-code"]'),
  ).toBeVisible();
  await expect(review).toContainText('Opens in Glassbox');
  const evidence = review.getByRole('button', { name: 'Open change evidence' });
  await expect(evidence).toContainText('2 docs3 tests3 source1 other2 new test files · 1 existing test file modified');
  await expect(review.locator('.ticket-code-review__commit')).toHaveCount(4);
  await expect(review.locator('.ticket-code-review__range')).toHaveCount(2);
  await expect(review).toContainText('HS2-DEMO01: finish the responsive review segment');
  await expect(review).toContainText('docs(workflow): require follow-up UI evidence');
  await expect(review).toContainText('Refs: HS2-DEMO01');
  await expectFlushCommit();
  await review.screenshot({ path: '/private/tmp/hs2-0jv72t-code-review-evidence.png' });
  await evidence.click();
  const evidenceDialog = page.locator('[data-component="change-evidence-dialog"]');
  await expect(evidenceDialog).toBeVisible();
  await expect(evidenceDialog).toHaveAttribute('data-view', 'docs');
  await evidenceDialog.getByRole('button', { name: /Tests 1/ }).click();
  const testFile = evidenceDialog.locator('[data-action="select-repository-file"]');
  await expect(testFile).toHaveAttribute('data-item-id', 'clients/web/src/change-evidence.test.ts');
  await evidenceDialog.screenshot({ path: '/private/tmp/hs2-s7x4sb-change-evidence.png' });
  await testFile.click();
  await expect(evidenceDialog.getByRole('menu')).toHaveCount(0);
  await testFile.getByRole('button', { name: /Actions for/ }).click();
  await evidenceDialog.getByRole('menuitem', { name: 'Show Diff' }).click();
  await expect
    .poll(() =>
      actions.some(
        (action) =>
          action.operation === 'code-review' &&
          action.mode === 'ticket_file' &&
          action.path === 'clients/web/src/change-evidence.test.ts',
      ),
    )
    .toBe(true);
  await page.keyboard.press('Escape');
  await review.getByRole('button', { name: 'Open 2 commit bundle aaa1111 through bbb2222 in Glassbox' }).click();
  await expect
    .poll(() =>
      actions.some(
        (action) =>
          action.operation === 'code-review' &&
          action.mode === 'range' &&
          action.from === 'aaa1111' &&
          action.to === 'bbb2222',
      ),
    )
    .toBe(true);
  await expect(page.locator('.app-toast')).toContainText('Opened in Glassbox.');
  await expect(review.locator('.ticket-code-review__message')).toHaveCount(0);
  await review.getByRole('button', { name: 'Open 2 commit bundle ccc3333 through ddd4444 in Glassbox' }).click();
  await expect
    .poll(() =>
      actions.some(
        (action) =>
          action.operation === 'code-review' &&
          action.mode === 'range' &&
          action.from === 'ccc3333' &&
          action.to === 'ddd4444',
      ),
    )
    .toBe(true);
  await review.getByRole('button', { name: 'Open commit bbb2222 in Glassbox' }).click();
  await expect
    .poll(() =>
      actions.some(
        (action) => action.operation === 'code-review' && action.mode === 'commit' && action.commit === 'bbb2222',
      ),
    )
    .toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-ggjed1-code-review-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(review.locator('.ticket-code-review__range')).toHaveCount(2);
  await expectFlushCommit();
  await page.screenshot({ path: '/private/tmp/hs2-ggjed1-code-review-floor.png', fullPage: true });
});

test('renders the exact shared Code Review component in the inspector and reader', async ({ page }) => {
  await page.setViewportSize({ width: 2048, height: 1000 });
  await mockProject(page);
  await page.route('**/tickets/01/code-review', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { difftool: 'Glassbox', truncated: false, ranges: [], commits: [] } })
      : route.fallback(),
  );
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const sidebar = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  await sidebar.getByRole('tab', { name: 'Code Review' }).click();
  const sidebarReview = sidebar.locator('[data-component="ticket-code-review"]');
  await expect(sidebarReview).toContainText('Opens in Glassbox');
  await expect(sidebarReview).toContainText('No commits referencing this ticket were found.');
  const sharedMarkup = await sidebarReview.evaluate((element) => element.outerHTML);
  await sidebar.screenshot({ path: '/private/tmp/hs2-g7p7s7-code-review-inspector-after.png' });
  await sidebar.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await reader.getByRole('tab', { name: 'Code Review' }).click();
  const readerReview = reader.locator('[data-component="ticket-code-review"]');
  await expect(readerReview).toContainText('Opens in Glassbox');
  await expect(readerReview).toContainText('No commits referencing this ticket were found.');
  expect(await readerReview.evaluate((element) => element.outerHTML)).toBe(sharedMarkup);
  await page.screenshot({ path: '/private/tmp/hs2-g7p7s7-code-review-reader-after.png', fullPage: true });
});

test('keeps change evidence interactive when launched from the modal ticket reader (HS2-6EV2ES)', async ({ page }) => {
  const actions = await mockProject(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const sidebar = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  await sidebar.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await expect(reader).toBeVisible();
  await reader.getByRole('tab', { name: 'Code Review' }).click();
  const review = reader.locator('[data-component="ticket-code-review"]');
  await review.getByRole('button', { name: 'Open change evidence' }).click();
  const evidenceDialog = page.locator('[data-component="change-evidence-dialog"]');
  await expect(evidenceDialog).toBeVisible();
  // The reader is a modal wa-dialog; a native popover launched from inside it must stay interactive, not sit inert beneath the modal top layer (HS2-EZ10RS / HS2-6EV2ES).
  await expect(evidenceDialog).toHaveAttribute('data-view', 'docs');
  await evidenceDialog.getByRole('button', { name: /Tests 1/ }).click();
  await expect(evidenceDialog).toHaveAttribute('data-view', 'tests');
  const testFile = evidenceDialog.locator('[data-action="select-repository-file"]');
  await expect(testFile).toHaveAttribute('data-item-id', 'clients/web/src/change-evidence.test.ts');
  await testFile.getByRole('button', { name: /Actions for/ }).click();
  await evidenceDialog.getByRole('menuitem', { name: 'Show Diff' }).click();
  await expect
    .poll(() =>
      actions.some(
        (action: Record<string, unknown>) =>
          action.operation === 'code-review' &&
          action.mode === 'ticket_file' &&
          action.path === 'clients/web/src/change-evidence.test.ts',
      ),
    )
    .toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-6ev2es-evidence-over-reader.png', fullPage: true });
});

test('owns and restores sidebar and reader tabs independently', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  let sidebar = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]'),
    sidebarTimeline = sidebar.getByRole('tab', { name: 'Timeline' });
  const sidebarInfo = sidebar.getByRole('tab', { name: 'Info' });
  await expect(sidebarInfo).toHaveAttribute('aria-selected', 'true');
  await expect(sidebarInfo).toHaveAttribute('tabindex', '0');
  await sidebarInfo.focus();
  await page.keyboard.press('ArrowRight');
  await expect(sidebarTimeline).toHaveAttribute('aria-selected', 'true');
  await expect(sidebarTimeline).toHaveAttribute('tabindex', '0');
  await expect(sidebarInfo).toHaveAttribute('tabindex', '-1');
  await sidebar.getByRole('button', { name: 'Open ticket reader' }).click();
  let reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' }),
    readerAttachments = reader.getByRole('tab', { name: /Attachments/ });
  const readerInfo = reader.getByRole('tab', { name: 'Info' });
  await expect(readerInfo).toHaveAttribute('aria-selected', 'true');
  await readerInfo.focus();
  await page.keyboard.press('End');
  await expect(readerAttachments).toHaveAttribute('aria-selected', 'true');
  await expect(sidebarTimeline).toHaveAttribute('aria-selected', 'true');
  await page.screenshot({ path: '/private/tmp/hs2-gzn2hz-tab-bar-reader-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(readerAttachments).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-gzn2hz-tab-bar-reader-narrow.png', fullPage: true });
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(sidebarTimeline).toHaveAttribute('aria-selected', 'true');
  await expect
    .poll(() =>
      page.evaluate(() => {
        const stored = JSON.parse(localStorage.getItem('hotsheet.workspace.project-session.v1.demo-checkout') ?? '{}');
        return `${stored.inspectorTab}:${stored.readerTab}`;
      }),
    )
    .toBe('timeline:attachments');
  await sidebar.screenshot({ path: '/private/tmp/hs2-gzn2hz-tab-bar-inspector-narrow.png' });
  await page.reload();
  sidebar = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  sidebarTimeline = sidebar.getByRole('tab', { name: 'Timeline' });
  await expect(sidebarTimeline).toHaveAttribute('aria-selected', 'true');
  await sidebar.getByRole('button', { name: 'Open ticket reader' }).click();
  reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  readerAttachments = reader.getByRole('tab', { name: /Attachments/ });
  await expect(readerAttachments).toHaveAttribute('aria-selected', 'true');
  await expect(sidebarTimeline).toHaveAttribute('aria-selected', 'true');
});

test('shows animated active state only for the lifetime of a ticket claim lease', async ({ page }) => {
  await mockProject(page);
  const expires = new Date(Date.now() + 12_000).toISOString(),
    active = {
      ...row,
      claimed_by: 'codex-worker',
      worker_label: 'Codex',
      claim_lease_expires_at: expires,
      claim_count: 1,
    },
    previouslyClaimed = { ...startedRow2, claim_count: 4 },
    legacyCompleted = {
      ...completedRow,
      claimed_by: 'stale-worker',
      worker_label: 'Stale',
      claim_lease_expires_at: expires,
      claim_count: 1,
    };
  await page.route(/\/tickets(?:\?.*)?$/, (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    const rows = [active, previouslyClaimed, legacyCompleted],
      url = new URL(route.request().url());
    return route.fulfill({
      json: url.searchParams.has('page_size')
        ? {
            items: rows,
            counts: {
              total: 3,
              queued: 3,
              backlog: 0,
              archive: 0,
              open: 2,
              up_next: 1,
              active: 1,
              started: 2,
              completed_today: 0,
            },
          }
        : rows,
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.waitForTimeout(500);
  const activeRow = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    idleRow = page.locator('[data-ticket-slug="HS2-START02"]'),
    indicator = activeRow.locator('.ticket-list-row__claim');
  await expect(activeRow).toHaveAttribute('data-status', 'started');
  await expect(indicator.locator('[data-component="loading-spinner"]')).toHaveAttribute(
    'aria-label',
    'Codex is actively working on this ticket',
  );
  await expect(indicator.locator('path')).toHaveCSS('animation-duration', '0.85s');
  await expect(idleRow.locator('.ticket-list-row__claim')).toHaveCount(0);
  await expect(page.locator('[data-ticket-slug="HS2-DONE01"] .ticket-list-row__claim')).toHaveCount(0);
  expect(
    await activeRow
      .locator('.ticket-list-row__metadata')
      .evaluate((node) => [...node.children].map((child) => child.className)),
  ).toEqual(expect.arrayContaining(['ticket-list-row__claim']));
  await page.screenshot({ path: '/private/tmp/hs2-s3stys-claimed-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(indicator).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-s3stys-claimed-narrow.png', fullPage: true });
  await expect(indicator).toHaveCount(0, { timeout: 13_000 });
});

test('keeps the visible inspector region mounted while a selected ticket loads', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await mockProject(page, true, false, 300);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const region = page.locator('[data-component="resizable-region"][data-region-id="app-inspector"]');
  await expect(region).toBeVisible();
  const before = await region.evaluate((node) => node.getBoundingClientRect().width);
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.waitForTimeout(75);
  await expect(region).toBeVisible();
  await expect(region.locator('.ticket-inspector-placeholder')).toBeVisible();
  await expect(region.locator('.ticket-inspector-placeholder > .kui-toolbar')).not.toHaveAttribute('divider-sides');
  expect(await region.evaluate((node) => node.getBoundingClientRect().width)).toBe(before);
  await expect(region.locator('[data-component="ticket-inspector"]')).toBeVisible();
  await region.getByRole('button', { name: 'Copy ticket number HS2-DEMO01' }).click();
  await expect(page.getByText('HS2-DEMO01 copied to clipboard.', { exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('HS2-DEMO01');
  await page.setViewportSize({ width: 1024, height: 600 });
  const widthBeforeSwitch = await region.evaluate((node) => node.getBoundingClientRect().width);
  await page.locator('[data-ticket-slug="HS2-START02"]').click();
  await page.waitForTimeout(75);
  // Switching to another ticket keeps the inspector region mounted at the same width and shows the
  // value-free loading skeleton (aria-busy) — not the previous ticket's stale data. Updated for the
  // inspector-loading-skeleton behavior that replaced the dimmed stale inspector (HS2-YSHZQK; the old
  // `.ticket-inspector-transition` dimmed-stale presentation is gone, per the inspector-loading-skeleton feature).
  await expect(region.locator('[data-component="ticket-inspector-skeleton"]')).toHaveAttribute('aria-busy', 'true');
  await expect(region.locator('.ticket-inspector-placeholder')).toHaveCount(0);
  expect(await region.evaluate((node) => node.getBoundingClientRect().width)).toBe(widthBeforeSwitch);
  await expect(region).not.toContainText('Use real project tickets');
  await page.screenshot({ path: '/private/tmp/hs2-zt5qnw-inspector-transition-floor.png', fullPage: true });
  await expect(region.locator('[data-component="ticket-inspector"][data-ticket-slug="HS2-START02"]')).toBeVisible();
});

test('keeps an active editor stable when its already-selected ticket is clicked again', async ({ page }) => {
  const patches = await mockProject(page);
  let detailReads = 0;
  page.on('request', (request) => {
    if (request.method() === 'GET' && new URL(request.url()).pathname.endsWith('/tickets/01')) detailReads += 1;
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const row = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await row.click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const editor = inspector.getByRole('textbox', { name: 'Ticket details' });
  await editor.fill('Draft preserved across a redundant reselect');
  await expect
    .poll(() => patches.some((patch) => patch.details === 'Draft preserved across a redundant reselect'))
    .toBe(true);
  await page.waitForTimeout(100);
  const readsBefore = detailReads;
  await editor.evaluate((node) => {
    (node as HTMLElement & { reselectionMarker?: boolean }).reselectionMarker = true;
  });
  await resetRenderMetrics(page);
  await row.click();
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue('Draft preserved across a redundant reselect');
  expect(
    await editor.evaluate((node) => (node as HTMLElement & { reselectionMarker?: boolean }).reselectionMarker),
  ).toBe(true);
  expect(detailReads).toBe(readsBefore);
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  await page.screenshot({ path: '/private/tmp/hs2-e0mjm8-reselect-editor-wide.png', fullPage: true });
  await page.setViewportSize({ width: 940, height: 844 });
  await expect(editor).toBeFocused();
  await page.screenshot({ path: '/private/tmp/hs2-e0mjm8-reselect-editor-narrow.png', fullPage: true });
});

test('omits separators below every right-sidebar toolbar state', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.waitForTimeout(500);
  const placeholder = page.locator('.ticket-inspector-placeholder'),
    toolbar = placeholder.locator(':scope > .kui-toolbar');
  await expect(placeholder).toContainText('Select a ticket to see and edit its details');
  await expect(toolbar).not.toHaveAttribute('divider-sides');
  await expect(toolbar).toHaveCSS('box-shadow', /^(rgba\(0, 0, 0, 0\) [^,]+)(, rgba\(0, 0, 0, 0\) [^,]+){3}$/);
  await page.screenshot({ path: '/private/tmp/hs2-gvk7zy-empty-inspector-wide.png', fullPage: true });
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();
  await expect(
    page.locator('[data-component="ticket-inspector"] .ticket-inspector__header > .kui-toolbar'),
  ).not.toHaveAttribute('divider-sides');
  await page.getByRole('button', { name: /Notifications view/ }).click();
  const notificationToolbar = page
    .getByRole('complementary', { name: 'Notification inspector' })
    .locator(':scope > .kui-toolbar');
  await expect(notificationToolbar).not.toHaveAttribute('divider-sides');
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(notificationToolbar).toHaveCSS(
    'box-shadow',
    /^(rgba\(0, 0, 0, 0\) [^,]+)(, rgba\(0, 0, 0, 0\) [^,]+){3}$/,
  );
  await page.screenshot({ path: '/private/tmp/hs2-f3nk91-right-sidebar-floor.png', fullPage: true });
});

test('resizes and persists both production shell sidebars', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const sidebar = page.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]'),
    inspector = page.locator('[data-component="resizable-region"][data-region-id="app-inspector"]');
  const sidebarHandle = sidebar.getByRole('separator', { name: 'Resize Project sidebar' }),
    inspectorHandle = inspector.getByRole('separator', { name: 'Resize Ticket inspector' });
  const initialSidebar = await sidebar.evaluate((node) => node.getBoundingClientRect().width),
    initialInspector = await inspector.evaluate((node) => node.getBoundingClientRect().width);
  const sidebarBox = (await sidebarHandle.boundingBox())!,
    dragPoint = { x: sidebarBox.x + sidebarBox.width / 2, y: sidebarBox.y + 300 };
  await expect
    .poll(() =>
      page.evaluate(
        (point) =>
          Boolean(
            (document.elementFromPoint(point.x, point.y) as HTMLElement | null)?.closest<HTMLElement>(
              '[data-kui-resize-handle]',
            ),
          ),
        dragPoint,
      ),
    )
    .toBe(true);
  await page.mouse.move(dragPoint.x, dragPoint.y);
  await page.mouse.down();
  await expect(sidebar).toHaveAttribute('data-resizing', 'true');
  await expect(sidebar.locator('.kui-resizable-region__content')).toHaveCSS('transition-duration', '0s');
  await page.mouse.move(dragPoint.x + 32, dragPoint.y);
  await page.mouse.up();
  await expect(sidebar).not.toHaveAttribute('data-resizing');
  await expect.poll(() => sidebar.evaluate((node) => node.getBoundingClientRect().width)).toBe(initialSidebar + 32);
  await inspectorHandle.press('ArrowLeft');
  await expect.poll(() => inspector.evaluate((node) => node.getBoundingClientRect().width)).toBe(initialInspector + 16);
  const sortSelect = page.locator('wa-select[name="workspace-sort"]'),
    selectSort = (value: string) =>
      sortSelect
        .locator(`wa-option[value="${value}"]`)
        .evaluate((option) => option.dispatchEvent(new MouseEvent('click', { bubbles: true, composed: true })));
  await selectSort('title');
  await selectSort('title');
  await expect(sortSelect).toHaveAttribute('aria-label', 'Sort tickets: Title, descending');
  await page.getByLabel('Columns view').click();
  await expect(sortSelect).toHaveAttribute('aria-label', 'Sort tickets: Recently updated, descending');
  await selectSort('priority');
  const commandGroup = page.getByRole('button', { name: 'Project commands' });
  await commandGroup.click();
  await expect(commandGroup).toHaveAttribute('aria-expanded', 'false');
  await page.screenshot({ path: '/private/tmp/hs2-qgg6pf-resizable-sidebars.png', fullPage: true });
  await page.getByRole('button', { name: 'Hide project sidebar' }).click();
  await page.getByRole('button', { name: 'Hide ticket inspector' }).click();
  await page.reload();
  await expect(page.getByLabel('Columns view')).toHaveAttribute('aria-pressed', 'true');
  await expect(sortSelect).toHaveJSProperty('value', 'priority');
  await expect(sortSelect).toHaveAttribute('aria-label', 'Sort tickets: Priority, ascending');
  await page.getByLabel('List view').click();
  await expect(sortSelect).toHaveJSProperty('value', 'title');
  await expect(sortSelect).toHaveAttribute('aria-label', 'Sort tickets: Title, descending');
  await page.screenshot({ path: '/private/tmp/hs2-w3fdpm-independent-list-sort.png', fullPage: true });
  await page.getByLabel('Columns view').click();
  await expect(page.getByRole('button', { name: 'Show project sidebar' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Show ticket inspector' })).toBeVisible();
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect.poll(() => sidebar.evaluate((node) => node.getBoundingClientRect().width)).toBe(initialSidebar + 32);
  await expect(page.getByRole('button', { name: 'Project commands' })).toHaveAttribute('aria-expanded', 'false');
  await page.getByRole('button', { name: 'Show ticket inspector' }).click();
  await expect.poll(() => inspector.evaluate((node) => node.getBoundingClientRect().width)).toBe(initialInspector + 16);
});

test('orders equal status priority and title groups by most recently updated', async ({ page }) => {
  const recencyRows = [
    {
      ...row,
      id: '21',
      native_id: '21',
      qualified_id: 'git-local:21',
      slug: 'HS2-OLD001',
      title: 'Same title',
      priority: 'high',
      status: 'started',
      updated_at: '2026-09-08T10:00:00Z',
    },
    {
      ...row,
      id: '22',
      native_id: '22',
      qualified_id: 'git-local:22',
      slug: 'HS2-NEW001',
      title: 'Same title',
      priority: 'high',
      status: 'started',
      updated_at: '2026-09-10T10:00:00Z',
    },
    {
      ...row,
      id: '23',
      native_id: '23',
      qualified_id: 'git-local:23',
      slug: 'HS2-MID001',
      title: 'Same title',
      priority: 'high',
      status: 'started',
      updated_at: '2026-09-09T10:00:00Z',
    },
  ];
  await mockProject(page);
  await page.route('**/__hotsheet/project-api/demo-checkout/checkouts/demo-checkout/tickets*', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: recencyRows }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const order = () =>
    page
      .locator('[data-component="ticket-list-row"]')
      .evaluateAll((rows) => rows.map((item) => (item as HTMLElement).dataset.ticketSlug));
  const sort = page.locator('wa-select[name="workspace-sort"]');
  for (const field of ['title', 'priority', 'status']) {
    await sort.click();
    const option = sort.locator(`wa-option[value="${field}"]`);
    await expect(option).toBeVisible();
    await option.click();
    await expect(sort).toHaveJSProperty('value', field);
    await expect.poll(order).toEqual(['HS2-NEW001', 'HS2-MID001', 'HS2-OLD001']);
    await page.waitForTimeout(100);
  }
});

test('contains and centers inspector tabs while showing labels only when they fit (HS2-WKGMN4)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const sidebarTabs = page.locator('[data-region-id="app-inspector"] .ticket-inspector__tabs');
  const captureTabChrome = async (tabs: Locator, path: string) => {
    const clip = await tabs.evaluate((node) => {
      const inspector = node.closest<HTMLElement>('[data-component="ticket-inspector"]')!,
        frame = inspector.getBoundingClientRect(),
        title = inspector.querySelector('h1')!.getBoundingClientRect(),
        strip = node.getBoundingClientRect(),
        top = title.top - 8;
      return { x: frame.left, y: top, width: frame.width, height: strip.bottom - top + 8 };
    });
    await page.screenshot({ path, clip });
  };
  const expectTabGeometry = async (tabs: Locator) => {
    await expect(async () => {
      const geometry = await tabs.evaluate((node) => {
        const inspector = node.closest<HTMLElement>('[data-component="ticket-inspector"]')!,
          inspectorBox = inspector.getBoundingClientRect(),
          inspectorStyle = getComputedStyle(inspector),
          strip = node.getBoundingClientRect(),
          tabBoxes = Array.from(node.querySelectorAll<HTMLElement>('.ticket-inspector__tab')).map((tab) => {
            const cell = tab.getBoundingClientRect(),
              cellStyle = getComputedStyle(tab),
              button = tab.querySelector<HTMLElement>('[role="tab"]')!,
              target = button.getBoundingClientRect(),
              content = Array.from(button.children)
                .filter((child) => getComputedStyle(child).position !== 'absolute')
                .map((child) => child.getBoundingClientRect()),
              contentLeft = Math.min(...content.map((box) => box.left)),
              contentRight = Math.max(...content.map((box) => box.right));
            return {
              width: cell.width,
              targetInset: [
                target.left - cell.left - Number.parseFloat(cellStyle.borderLeftWidth),
                cell.right - target.right - Number.parseFloat(cellStyle.borderRightWidth),
              ],
              centerDelta: (contentLeft + contentRight - cell.left - cell.right) / 2,
            };
          });
        return {
          gutter: [
            strip.left - inspectorBox.left - Number.parseFloat(inspectorStyle.borderLeftWidth),
            inspectorBox.right - strip.right - Number.parseFloat(inspectorStyle.borderRightWidth),
          ],
          padding: getComputedStyle(node).padding,
          overflow: node.scrollWidth - node.clientWidth,
          tabBoxes,
        };
      });
      for (const gutter of geometry.gutter) expect(gutter).toBeCloseTo(8, 1);
      expect(geometry.padding).toBe('0px');
      expect(geometry.overflow).toBeLessThanOrEqual(1);
      const widths = geometry.tabBoxes.map((tab) => tab.width);
      expect(Math.max(...widths) - Math.min(...widths)).toBeLessThan(1);
      for (const tab of geometry.tabBoxes) {
        for (const inset of tab.targetInset) expect(Math.abs(inset)).toBeLessThan(1);
        expect(Math.abs(tab.centerDelta)).toBeLessThan(1);
      }
    }).toPass({ timeout: 5_000 });
  };
  await expect(sidebarTabs.getByRole('tab')).toHaveCount(4);
  await expect(sidebarTabs.getByRole('tab', { name: /Attachments/ })).toContainText('1 attachments');
  await expectTabGeometry(sidebarTabs);
  expect(
    await sidebarTabs.locator('.kui-app-tab__name').evaluateAll((labels) =>
      labels.every((label) => {
        const style = getComputedStyle(label);
        return style.position === 'absolute' && style.width === '1px' && style.clipPath !== 'none';
      }),
    ),
  ).toBe(true);
  await captureTabChrome(sidebarTabs, '/private/tmp/hs2-wkgmn4-inspector-tabs-wide.png');
  await page.setViewportSize({ width: 1024, height: 700 });
  await expectTabGeometry(sidebarTabs);
  await captureTabChrome(sidebarTabs, '/private/tmp/hs2-wkgmn4-inspector-tabs-narrow.png');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' }),
    readerTabs = reader.locator('.ticket-inspector__tabs');
  await expect(readerTabs.locator('.kui-app-tab__name').first()).toBeVisible();
  await expectTabGeometry(readerTabs);
  await captureTabChrome(readerTabs, '/private/tmp/hs2-wkgmn4-reader-tabs-wide.png');
  await page.screenshot({ path: '/private/tmp/hs2-2p9k4y-segments-wide.png', fullPage: true });
  await page.setViewportSize({ width: 868, height: 700 });
  expect(
    await readerTabs.locator('.kui-app-tab__name').evaluateAll((labels) =>
      labels.every((label) => {
        const style = getComputedStyle(label);
        return style.position === 'absolute' && style.width === '1px' && style.clipPath !== 'none';
      }),
    ),
  ).toBe(true);
  await expect(readerTabs.getByRole('tab', { name: 'Code Review' }).locator('svg')).toBeVisible();
  await expectTabGeometry(readerTabs);
  await page.screenshot({ path: '/private/tmp/hs2-2p9k4y-segments-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expectTabGeometry(readerTabs);
  await captureTabChrome(readerTabs, '/private/tmp/hs2-wkgmn4-reader-tabs-mobile.png');
});

interface RenderMetricsSnapshot {
  passes: number;
  mutations: number;
}
type InstrumentedWindow = typeof window & {
  __hotsheetRenderMetrics?: { reset(): void; snapshot(): RenderMetricsSnapshot };
};
const resetRenderMetrics = (page: import('@playwright/test').Page) =>
  page.evaluate(() => {
    (window as InstrumentedWindow).__hotsheetRenderMetrics?.reset();
  });
const renderMetrics = (page: import('@playwright/test').Page) =>
  page.evaluate(() => (window as InstrumentedWindow).__hotsheetRenderMetrics?.snapshot());

test('shows no spurious permission popup or repeated work while an open project is idle', async ({ page }) => {
  const permissionRequests: string[] = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.endsWith('/permissions') || path.endsWith('/connections')) permissionRequests.push(path);
  });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.waitForTimeout(1_000);
  permissionRequests.length = 0;
  await resetRenderMetrics(page);
  await page.waitForTimeout(1_700);
  expect(permissionRequests).toEqual([]);
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-n4r6f3-no-spurious-permission-popup.png', fullPage: true });
});

test('keeps remembered-project startup atomic and does not report its intentional work', async ({ page }) => {
  const submissions: unknown[] = [];
  await mockProject(page);
  await page.route('**/__hotsheet/dev-review/tickets', async (route) => {
    submissions.push(route.request().postDataJSON());
    await route.fulfill({ status: 201, json: { slug: 'HS2-SHOULD-NOT-EXIST' } });
  });
  await page.route('**/__hotsheet/project-api/demo-checkout/checkouts/demo-checkout/tickets*', async (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() === 'GET' && url.pathname.endsWith('/tickets')) {
      await new Promise((resolve) => setTimeout(resolve, 6_500));
      return route.fulfill({
        json: [
          row,
          backlogRow,
          archiveRow,
          deletedRow,
          movedRow,
          notStartedRow,
          completedRow,
          verifiedRow,
          startedRow2,
          startedRow3,
          searchSlugRow,
          searchDetailsRow,
        ],
      });
    }
    await route.fallback();
  });
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, project.root);
  await page.goto('/');
  await page.waitForTimeout(5_200);
  await expect(page.locator('[data-component="project-restore-state"]')).toBeVisible();
  await expect(page.locator('[data-component="app-shell"]')).toHaveCount(0);
  await resetRenderMetrics(page);
  for (let index = 0; index < 14; index += 1) {
    await page.setViewportSize({ width: 1280, height: 760 + index });
    await page.waitForTimeout(20);
  }
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible({ timeout: 10_000 });
  await expect(page.locator('[data-component="project-restore-state"]')).toHaveCount(0);
  await page.waitForTimeout(500);
  expect(submissions).toEqual([]);
});

test('retries a temporarily failed remembered project and restores every project', async ({ page }) => {
  await mockProject(page);
  const restoredRoot = '/work/restarting-server';
  let attempts = 0;
  await page.route('**/__hotsheet/projects/open', (route) => {
    if (route.request().postDataJSON().root !== restoredRoot) return route.fallback();
    attempts += 1;
    return attempts === 1
      ? route.fulfill({ status: 409, json: { error: 'The server is still restarting.' } })
      : route.fulfill({
          status: 201,
          json: {
            ...project,
            id: 'restored-checkout',
            root: restoredRoot,
            name: 'restored',
            apiPath: '/__hotsheet/project-api/restored-checkout',
          },
        });
  });
  await page.addInitScript(
    ({ good, restarting }) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify([good, restarting]));
    },
    { good: project.root, restarting: restoredRoot },
  );
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'restored' })).toBeVisible();
  expect(attempts).toBe(2);
  await expect(page.locator('.app-error')).toHaveCount(0);
  await expect(page.locator('.app-toast')).toHaveCount(0);
  await expect(
    page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]')),
  ).resolves.toEqual([project.root, restoredRoot]);
  await page.screenshot({ path: '/private/tmp/hs2-hes004-restored-projects.png', fullPage: true });
});

test('keeps a persistently failed project remembered without obscuring a successful one', async ({ page }) => {
  await mockProject(page);
  const failedRoot = '/work/older-server';
  let available = false;
  await page.route('**/__hotsheet/projects/open', (route) => {
    if (route.request().postDataJSON().root !== failedRoot) return route.fallback();
    return available
      ? route.fulfill({
          status: 201,
          json: {
            ...project,
            id: 'older-checkout',
            root: failedRoot,
            name: 'older-server',
            apiPath: '/__hotsheet/project-api/older-checkout',
          },
        })
      : route.fulfill({
          status: 409,
          json: {
            error: 'The older server only supports schema 2.',
            recovery: {
              store: '/work/older-server.hs2',
              expected: { pid: 4242, url: 'http://127.0.0.1:8787', started_at: '2026-09-12T01:00:00Z' },
            },
          },
        });
  });
  await page.addInitScript(
    ({ good, bad }) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify([good, bad]));
    },
    { good: project.root, bad: failedRoot },
  );
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  const failedTab = page.getByRole('tab', { name: 'older-server' }),
    failedTabRoot = failedTab.locator('xpath=..');
  await expect(failedTab).toBeVisible();
  await expect(failedTabRoot).toHaveAttribute('data-attention', 'true');
  await expect(failedTabRoot).toHaveAttribute('draggable', 'false');
  await expect(failedTabRoot.locator('[data-lucide="circle-alert"]')).toBeVisible();
  await failedTab.click();
  const failure = page.locator('.project-restore-error');
  await expect(failure).toContainText('older-server could not be reopened');
  await expect(failure).toContainText('The older server only supports schema 2.');
  await expect(failure).toContainText('process (4242) is not responding');
  await expect(failure).toContainText('Project: /work/older-server');
  await page.screenshot({ path: '/private/tmp/hs2-cygqfn-project-restore-error-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.screenshot({ path: '/private/tmp/hs2-cygqfn-project-restore-error-narrow.png', fullPage: true });
  await expect(page.locator('.app-error')).toHaveCount(0);
  await expect(
    page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]')),
  ).resolves.toEqual([project.root, failedRoot]);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.workspace.active-project-root.v1'))).resolves.toBe(
    failedRoot,
  );
  await page.reload();
  await expect(page.getByRole('tab', { name: 'older-server' })).toHaveAttribute('aria-selected', 'true');
  await expect(failure).toContainText('The older server only supports schema 2.');
  available = true;
  await failure.getByRole('button', { name: 'Retry project' }).click();
  await expect(failure).toHaveCount(0);
  await expect(page.getByRole('tab', { name: 'older-server' }).locator('xpath=..')).toHaveAttribute(
    'data-attention',
    'false',
  );
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Queue' })).toBeVisible();
});

test('suppresses interaction-bound render bursts but reports a storm that persists afterward', async ({ page }) => {
  const submissions: unknown[] = [];
  await mockProject(page);
  await page.route('**/__hotsheet/dev-review/tickets', async (route) => {
    submissions.push(route.request().postDataJSON());
    await route.fulfill({ status: 201, json: { slug: 'HS2-DIAGNOSTIC' } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.waitForTimeout(5_100);
  await page.getByLabel('List view').click();
  for (let index = 0; index < 14; index += 1) {
    await page.setViewportSize({ width: 1280, height: 760 + index });
    await page.waitForTimeout(20);
  }
  expect(submissions).toEqual([]);
  await page.waitForTimeout(5_100);
  for (let index = 0; index < 14; index += 1) {
    await page.setViewportSize({ width: 1280, height: 800 + index });
    await page.waitForTimeout(20);
  }
  await expect.poll(() => submissions.length).toBe(1);
});

test('projects background AI activity without rerendering the closed conversation surface', async ({ page }) => {
  let settledPolls = 0;
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (url.pathname.endsWith('/ws/poll') && url.searchParams.has('since')) settledPolls += 1;
  });
  await mockProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const drive = page.locator('.project-sidebar [data-action="toggle-drive"]');
  await drive.click();
  await expect(page.getByRole('button', { name: 'Open Codex conversation' })).toBeEnabled();
  await expect(page.locator('[data-component="ai-conversation"][data-presentation="embedded"]')).toHaveCount(1);
  await page.locator('[data-action="toggle-terminal-drawer"]').click();
  await page.waitForTimeout(300);
  await expect(page.locator('[data-component="ai-conversation"]')).toHaveCount(0);
  await expect.poll(() => settledPolls).toBeGreaterThanOrEqual(2);
  await resetRenderMetrics(page);
  await page.evaluate(async () => {
    await fetch('/__hotsheet/project-api/demo-checkout/drive/connections/hotsheet-sidebar-codex-demo-checkout/turns', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ content: 'background-output-only' }),
    });
  });
  await page.waitForTimeout(1_200);
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  await expect(page.locator('[data-component="ai-conversation"]')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-jsmkfj-hidden-conversation-stable.png', fullPage: true });
  await page.locator('[data-action="toggle-terminal-drawer"]').click();
  await expect(page.locator('[data-component="ai-conversation"][data-presentation="embedded"]')).toContainText(
    'background event',
  );
});

test('defers ticket refresh without hiding an open select popup', async ({ page }) => {
  await mockProject(page);
  let rows = [row, notStartedRow],
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [];
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: rows }) : route.fallback(),
  );
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const select = page.locator('wa-select[name="inspector-status"]');
  await select.click();
  await expect.poll(() => select.evaluate((node) => (node as HTMLElement & { open?: boolean }).open)).toBe(true);
  await expect
    .poll(async () => {
      const current = await renderMetrics(page);
      await page.waitForTimeout(50);
      const next = await renderMetrics(page);
      return current?.mutations === next?.mutations;
    })
    .toBe(true);
  await resetRenderMetrics(page);
  const incoming = { ...startedRow2, slug: 'HS2-INCOMING', title: 'Incoming while choosing' };
  rows = [...rows, incoming];
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  cursor += 1;
  await polls.shift()!.fulfill({
    json: {
      cursor,
      events: [{ store: 'git-local', kind: 'created', id: incoming.id, slug: incoming.slug }],
      overflow: false,
    },
  });
  await page.waitForTimeout(300);
  await expect.poll(() => select.evaluate((node) => (node as HTMLElement & { open?: boolean }).open)).toBe(true);
  await expect(page.locator('[data-ticket-slug="HS2-INCOMING"]')).toHaveCount(0);
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  await page.keyboard.press('Escape');
  await expect.poll(() => select.evaluate((node) => (node as HTMLElement & { open?: boolean }).open)).toBe(false);
  await expect(page.locator('[data-ticket-slug="HS2-INCOMING"]')).toBeVisible();
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const composer = page.getByRole('dialog', { name: 'Create ticket' }),
    category = composer.locator('wa-select[name="new-ticket-category"]');
  await category.click();
  await expect.poll(() => category.evaluate((node) => (node as HTMLElement & { open?: boolean }).open)).toBe(true);
  await expect
    .poll(async () => {
      const current = await renderMetrics(page);
      await page.waitForTimeout(50);
      const next = await renderMetrics(page);
      return current?.mutations === next?.mutations;
    })
    .toBe(true);
  await resetRenderMetrics(page);
  rows = rows.map((item) => (item.slug === notStartedRow.slug ? { ...item, status: 'started' } : item));
  cursor += 1;
  await polls.shift()!.fulfill({
    json: {
      cursor,
      events: [{ store: 'git-local', kind: 'updated', id: notStartedRow.id, slug: notStartedRow.slug }],
      overflow: false,
    },
  });
  await page.waitForTimeout(300);
  await expect.poll(() => category.evaluate((node) => (node as HTMLElement & { open?: boolean }).open)).toBe(true);
  await expect(page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]')).toBeVisible();
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  await composer.getByRole('textbox', { name: 'Ticket title' }).click();
  await expect.poll(() => category.evaluate((node) => (node as HTMLElement & { open?: boolean }).open)).toBe(false);
  await expect(page.locator('[data-column-id="started"] [data-ticket-slug="HS2-NEXT01"]')).toBeVisible();
  await expect(composer).toBeVisible();
});

test('switches to Queue so a ticket created in Backlog with Up Next stays visible (HS2-F6937Q)', async ({ page }) => {
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const sidebar = page.locator('.project-sidebar');
  await sidebar.locator('[data-action="select-view"][data-item-id="backlog"]').click();
  await expect(sidebar.locator('[data-action="select-view"][data-item-id="backlog"]')).toHaveAttribute(
    'aria-current',
    'page',
  );
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const composer = page.getByRole('dialog', { name: 'Create ticket' });
  await composer.getByLabel('Ticket title').fill('Websockets question');
  await composer.locator('[data-action="toggle-new-ticket-up-next"]').click();
  await composer.getByRole('button', { name: 'Create ticket' }).click();
  // The Up Next ticket becomes not_started (a Queue ticket), so the view switches to Queue and the ticket is visible + selected.
  await expect(sidebar.locator('[data-action="select-view"][data-item-id="all"]')).toHaveAttribute(
    'aria-current',
    'page',
  );
  const created = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]');
  await expect(created).toBeVisible();
  await expect(created).toHaveAttribute('data-selected', 'true');
});

test('remembers the last ticket category after cancelling and refreshing', async ({ page }) => {
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const composer = page.getByRole('dialog', { name: 'Create ticket' }),
    category = composer.locator('wa-select[name="new-ticket-category"]');
  await category.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'investigation';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(category).toHaveJSProperty('value', 'investigation');
  await composer.getByRole('button', { name: 'Cancel' }).click();
  await page.reload();
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await expect(
    page.getByRole('dialog', { name: 'Create ticket' }).locator('wa-select[name="new-ticket-category"]'),
  ).toHaveJSProperty('value', 'investigation');
});

test('keeps production ticket creation inside the Web Awesome modal lifecycle', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const launcher = page.getByRole('button', { name: 'New ticket…' }),
    dialog = page.getByRole('dialog', { name: 'Create ticket' });
  await launcher.click();
  await expect(dialog.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  expect(await dialog.evaluate((node) => node.shadowRoot?.querySelector('dialog')?.matches(':modal'))).toBe(true);
  await page.getByRole('button', { name: 'Add project' }).evaluate((node) => {
    (node as HTMLElement).focus();
  });
  expect(await dialog.evaluate((host) => host.contains(document.activeElement))).toBe(true);
  await page.mouse.click(8, 8);
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(launcher).not.toBeFocused();
  const category = dialog.locator('wa-select[name="new-ticket-category"]');
  await category.click();
  await expect(category).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(category).toHaveJSProperty('open', false);
  await expect(dialog).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(launcher).toBeFocused();
  await launcher.click();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toBeHidden();
  await expect(launcher).toBeFocused();
  await launcher.click();
  await dialog.getByRole('textbox', { name: 'Ticket title' }).fill('Focus created ticket');
  await dialog.getByRole('button', { name: 'Create ticket' }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole('textbox', { name: 'Ticket details' })).toBeFocused();
});

test('clears prior ticket text after cancel and successful creation', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const launcher = page.getByRole('button', { name: 'New ticket…' }),
    dialog = page.getByRole('dialog', { name: 'Create ticket' }),
    title = dialog.getByRole('textbox', { name: 'Ticket title' }),
    details = dialog.getByRole('textbox', { name: 'Details' });
  await launcher.click();
  await title.fill('Discarded title');
  await details.fill('Discarded details');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await launcher.click();
  await expect(title).toHaveValue('');
  await expect(details).toHaveValue('');
  await page.waitForTimeout(350);
  await page.screenshot({ path: '/private/tmp/hs2-90ntv5-empty-after-cancel-wide.png', fullPage: true });
  await title.fill('Created title');
  await details.fill('Created details');
  await dialog.getByRole('button', { name: 'Create ticket' }).click();
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]')).toBeVisible();
  await launcher.click();
  await expect(title).toHaveValue('');
  await expect(details).toHaveValue('');
  await page.setViewportSize({ width: 560, height: 760 });
  await page.waitForTimeout(350);
  await page.screenshot({ path: '/private/tmp/hs2-90ntv5-empty-after-create-narrow.png', fullPage: true });
});

test('does not resurrect a dismissed ticket composer after another modal closes', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const conversation = drawer.locator('[data-component="ai-conversation"]'),
    createViewDialog = page.getByRole('dialog', { name: 'Create View' });
  await conversation.getByLabel('Message Codex').fill('Export the referenced proof.');
  await conversation.getByLabel('Message Codex').press('Enter');
  await expect(conversation.getByText('The referenced proof is ready.', { exact: true })).toBeVisible();
  await expect(createViewDialog).toBeHidden();
  const composerHost = page.locator('[data-component="quick-ticket-composer"]');
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Temporary ticket draft');
  await page.keyboard.press('Escape');
  await expect(composerHost).toBeHidden();
  await expect(composerHost.locator('[data-action="create-ticket-form"]')).toHaveCount(0);
  await conversation.getByRole('button', { name: 'Save conversation' }).click();
  const exportDialog = page.locator('[data-component="conversation-export-dialog"]');
  await expect(exportDialog).toHaveJSProperty('open', true);
  await exportDialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(exportDialog).toBeHidden();
  await expect(composerHost).toBeHidden();
  await expect(composerHost.locator('[data-action="create-ticket-form"]')).toHaveCount(0);
  await expect(createViewDialog).toBeHidden();
  await page.waitForTimeout(350);
  await page.screenshot({ path: '/private/tmp/hs2-dbcf7e-no-composer-after-export-wide.png', fullPage: true });
  await page.setViewportSize({ width: 560, height: 760 });
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Second temporary draft');
  await page.getByRole('button', { name: 'Cancel' }).click();
  await conversation.getByRole('button', { name: 'Save conversation' }).click();
  await exportDialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(composerHost).toBeHidden();
  await expect(createViewDialog).toBeHidden();
  await page.waitForTimeout(350);
  await page.screenshot({ path: '/private/tmp/hs2-dbcf7e-no-composer-after-export-narrow.png', fullPage: true });
});

test('restores a saved composer draft without painting the dialog during remembered-project startup', async ({
  page,
}) => {
  await mockProject(page);
  const session = {
    selectedView: 'all',
    selectedTicketSlugs: [],
    searchOpen: false,
    searchQuery: '',
    inspectorTab: 'info',
    readerTab: 'info',
    composer: {
      open: true,
      title: 'Remembered startup draft',
      details: 'Preserved without startup modality',
      category: 'feature',
      upNext: true,
      attachments: [],
    },
    composingNote: false,
    newNoteDraft: '',
    feedbackReplies: {},
    feedbackSelections: {},
    feedbackDraft: '',
  };
  await page.addInitScript(
    ({ root, session }) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
      localStorage.setItem('hotsheet.workspace.active-project-root.v1', root);
      localStorage.setItem('hotsheet.workspace.project-session.v1.demo-checkout', JSON.stringify(session));
      Object.defineProperty(window, '__composerStartupPainted', { configurable: true, writable: true, value: false });
      new MutationObserver(() => {
        const dialog = document.querySelector('[data-component="quick-ticket-composer"]');
        if (dialog?.hasAttribute('open') || dialog?.querySelector('[data-action="create-ticket-form"]'))
          (window as typeof window & { __composerStartupPainted: boolean }).__composerStartupPainted = true;
      }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['open'] });
    },
    { root: project.root, session },
  );
  await page.goto('/?dev-review=false');
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  const dialog = page.locator('[data-component="quick-ticket-composer"]');
  await expect(dialog).not.toHaveAttribute('open', '');
  await expect(dialog).toBeHidden();
  await expect(dialog.locator('[data-action="create-ticket-form"]')).toHaveCount(0);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __composerStartupPainted: boolean }).__composerStartupPainted,
    ),
  ).toBe(false);
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await expect(dialog.getByRole('textbox', { name: 'Ticket title' })).toHaveValue('Remembered startup draft');
  await expect(dialog.getByRole('textbox', { name: 'Details' })).toHaveValue('Preserved without startup modality');
  await expect(dialog.locator('wa-select[name="new-ticket-category"]')).toHaveJSProperty('value', 'feature');
  await expect(dialog.getByRole('button', { name: 'Remove new ticket from Up Next' })).toBeVisible();
});

test('animates ticket moves, arrivals, and departures in sequence', async ({ page }) => {
  await page.setViewportSize({ width: 2400, height: 1000 });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  await page.evaluate(() => {
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const original = Element.prototype.animate;
    const records: Array<{
      slug: string;
      ghost: string;
      frames: Keyframe[];
      delay: number;
      duration: number;
      fill: FillMode | undefined;
    }> = [];
    (window as typeof window & { __ticketAnimations?: typeof records }).__ticketAnimations = records;
    Element.prototype.animate = function (
      keyframes: Keyframe[] | PropertyIndexedKeyframes | null,
      options?: number | KeyframeAnimationOptions,
    ) {
      const animation = original.call(this, keyframes, options),
        effect = animation.effect as KeyframeEffect | null;
      if (
        this.matches(
          '[data-component="ticket-list-row-container"], [data-ticket-motion-ghost][data-ticket-motion-slug]',
        ) &&
        effect
      ) {
        const element = this as HTMLElement,
          row = element.querySelector<HTMLElement>('[data-ticket-slug]'),
          timing = effect.getTiming();
        records.push({
          slug: row?.dataset.ticketSlug ?? element.dataset.ticketMotionSlug ?? '',
          ghost: element.dataset.ticketMotionGhost ?? '',
          frames: effect.getKeyframes(),
          delay: Number(timing.delay),
          duration: Number(timing.duration),
          fill: timing.fill,
        });
        animation.pause();
        animation.currentTime = 0;
      }
      return animation;
    };
  });
  type TicketAnimation = {
    slug: string;
    ghost: string;
    frames: Keyframe[];
    delay: number;
    duration: number;
    fill: FillMode | undefined;
  };
  const records = () =>
      page.evaluate(
        () => (window as typeof window & { __ticketAnimations?: TicketAnimation[] }).__ticketAnimations ?? [],
      ),
    clear = () =>
      page.evaluate(() => {
        const records = (window as typeof window & { __ticketAnimations?: unknown[] }).__ticketAnimations;
        if (records) records.length = 0;
      }),
    seek = (milliseconds: number) =>
      page.evaluate((value) => {
        for (const animation of document.getAnimations()) {
          const target = (animation.effect as KeyframeEffect | null)?.target;
          if (
            target instanceof Element &&
            target.matches(
              '[data-component="ticket-list-row-container"], [data-ticket-motion-ghost][data-ticket-motion-slug]',
            )
          ) {
            animation.currentTime = value;
            animation.pause();
          }
        }
      }, milliseconds),
    finish = () =>
      page.evaluate(() => {
        for (const animation of document.getAnimations()) {
          const target = (animation.effect as KeyframeEffect | null)?.target;
          if (
            target instanceof Element &&
            target.matches(
              '[data-component="ticket-list-row-container"], [data-ticket-motion-ghost][data-ticket-motion-slug]',
            )
          )
            animation.finish();
        }
      });
  const moving = page.locator('[data-column-id="started"] [data-ticket-slug="HS2-DEMO01"]'),
    sourceBox = await moving.boundingBox();
  expect(sourceBox).not.toBeNull();
  await moving.click();
  const status = page.locator('wa-select[name="inspector-status"]');
  await status.click();
  await status.locator('wa-option[value="completed"]').click();
  const moved = page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-DEMO01"]'),
    moveGhost = page.locator('[data-ticket-motion-ghost="move"][data-ticket-motion-slug="HS2-DEMO01"]');
  await expect(moveGhost).toBeAttached();
  await expect(moved).toHaveCount(1);
  await expect(moved).toHaveCSS('visibility', 'hidden');
  await expect
    .poll(async () => {
      const calls = await records();
      return calls.some(
        (call) =>
          call.slug === 'HS2-DEMO01' &&
          call.ghost === 'move' &&
          call.duration === 240 &&
          call.frames.some((frame) => frame.zIndex === '90'),
      );
    })
    .toBe(true);
  const destinationBox = await moved.boundingBox();
  expect(destinationBox).not.toBeNull();
  await seek(120);
  const ghostBox = await moveGhost.boundingBox();
  expect(ghostBox).not.toBeNull();
  expect(ghostBox!.x).toBeGreaterThan(Math.min(sourceBox!.x, destinationBox!.x));
  expect(ghostBox!.x).toBeLessThan(Math.max(sourceBox!.x, destinationBox!.x));
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const composerDialog = page.getByRole('dialog', { name: 'Create ticket' });
  await expect(composerDialog).toBeVisible();
  expect(await composerDialog.evaluate((node) => node.shadowRoot?.querySelector('dialog')?.matches(':modal'))).toBe(
    true,
  );
  await page.screenshot({ path: '/private/tmp/hs2-bm8cgj-ticket-motion-below-dialog-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.screenshot({ path: '/private/tmp/hs2-bm8cgj-ticket-motion-below-dialog-narrow.png', fullPage: true });
  await composerDialog.getByRole('button', { name: 'Cancel' }).click();
  await page.screenshot({ path: '/private/tmp/hs2-v1s4hg-ticket-move-midflight-after.png', fullPage: true });
  await finish();
  await expect(moveGhost).toHaveCount(0);
  await expect(moved).toHaveCSS('visibility', 'visible');
  await clear();
  await moved.click({ button: 'right' });
  await page.getByRole('menu', { name: 'Ticket actions' }).locator('[data-context-action="Archive ticket"]').click();
  const outgoingGhost = page.locator('[data-ticket-motion-ghost="outgoing"][data-ticket-motion-slug="HS2-DEMO01"]');
  await expect(outgoingGhost).toBeAttached();
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toHaveCount(0);
  await expect
    .poll(async () => {
      const calls = await records();
      return (
        calls.some(
          (call) =>
            call.slug === 'HS2-DEMO01' &&
            call.ghost === 'outgoing' &&
            call.duration === 160 &&
            call.frames.some((frame) => Number(frame.opacity) === 0),
        ) && calls.some((call) => call.delay === 160 && call.duration === 240)
      );
    })
    .toBe(true);
  await seek(80);
  await page.screenshot({ path: '/private/tmp/hs2-fp6ms6-ticket-outgoing-fade-after.png', fullPage: true });
  await finish();
  await expect(outgoingGhost).toHaveCount(0);
  await expect(moved).toHaveCount(0);
  await clear();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Animated incoming ticket');
  await page.getByRole('button', { name: 'Create ticket' }).click();
  const incoming = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEW001"]'),
    incomingGhost = page.locator('[data-ticket-motion-ghost="incoming"][data-ticket-motion-slug="HS2-NEW001"]');
  await expect(incoming).toBeAttached();
  await expect(incoming).toHaveCount(1);
  await expect(page.getByLabel('Ticket workspace').getByText('Animated incoming ticket', { exact: true })).toHaveCount(
    1,
  );
  await expect(incomingGhost).toBeAttached();
  await expect
    .poll(async () => {
      const calls = await records();
      return (
        calls.some(
          (call) =>
            call.slug === 'HS2-NEW001' &&
            call.ghost === 'incoming' &&
            call.delay === 240 &&
            call.duration === 160 &&
            call.fill === 'backwards' &&
            call.frames.some((frame) => Number(frame.opacity) === 0) &&
            call.frames.some((frame) => Number(frame.opacity) === 1),
        ) &&
        calls.some(
          (call) =>
            call.slug !== 'HS2-NEW001' &&
            call.delay === 0 &&
            call.duration === 240 &&
            call.frames.some((frame) => String(frame.transform).startsWith('translate(')),
        )
      );
    })
    .toBe(true);
  await seek(120);
  await expect(incomingGhost).toHaveCSS('opacity', '0');
  await page.screenshot({ path: '/private/tmp/hs2-fp6ms6-ticket-incoming-make-room-after.png', fullPage: true });
  await seek(320);
  await expect
    .poll(async () => Number.parseFloat(await incomingGhost.evaluate((element) => getComputedStyle(element).opacity)))
    .toBeGreaterThan(0);
  await expect
    .poll(async () => Number.parseFloat(await incomingGhost.evaluate((element) => getComputedStyle(element).opacity)))
    .toBeLessThan(1);
  const incomingBox = await incoming.boundingBox(),
    incomingGhostBox = await incomingGhost.boundingBox();
  expect(incomingBox).not.toBeNull();
  expect(incomingGhostBox).not.toBeNull();
  expect(Math.abs(incomingGhostBox!.x - incomingBox!.x)).toBeLessThan(1);
  expect(Math.abs(incomingGhostBox!.y - incomingBox!.y)).toBeLessThan(1);
  await page.screenshot({ path: '/private/tmp/hs2-fp6ms6-ticket-incoming-fade-after.png', fullPage: true });
  await finish();
  await expect(incomingGhost).toHaveCount(0);
  await expect(incoming).toHaveCSS('visibility', 'visible');
});

test('naturally makes room before fading in a ticket created from the composer', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  const existing = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]').locator('..'),
    before = await existing.boundingBox();
  expect(before).not.toBeNull();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Naturally animated incoming ticket');
  await page.getByRole('button', { name: 'Create ticket' }).click();
  const incoming = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEW001"]').locator('..'),
    ghost = page.locator('[data-ticket-motion-ghost="incoming"][data-ticket-motion-slug="HS2-NEW001"]');
  await expect(ghost).toBeAttached();
  await expect(incoming).toHaveCSS('visibility', 'hidden');
  await expect.poll(async () => existing.evaluate((element) => getComputedStyle(element).transform)).not.toBe('none');
  await page.waitForTimeout(100);
  const during = await existing.boundingBox();
  expect(during).not.toBeNull();
  expect(during!.y).toBeGreaterThan(before!.y);
  await page.waitForTimeout(200);
  const opacity = Number.parseFloat(await ghost.evaluate((element) => getComputedStyle(element).opacity));
  expect(opacity).toBeGreaterThan(0);
  expect(opacity).toBeLessThan(1);
  await page.screenshot({ path: '/private/tmp/hs2-jgwtjj-natural-create-midflight.png', fullPage: true });
  await expect(ghost).toHaveCount(0);
  await expect(incoming).toHaveCSS('visibility', 'visible');
});

test('finishes local ticket creation motion without redundantly reconciling its acknowledged long-poll event', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await mockProject(page);
  let authoritative = [
      row,
      backlogRow,
      archiveRow,
      deletedRow,
      movedRow,
      notStartedRow,
      completedRow,
      verifiedRow,
      startedRow2,
      startedRow3,
      searchSlugRow,
      searchDetailsRow,
    ],
    cursor = 0,
    ticketGets = 0,
    eventDelivered = false,
    releaseCreate!: () => void;
  const createResponse = new Promise<void>((resolve) => {
      releaseCreate = resolve;
    }),
    polls: Array<import('@playwright/test').Route> = [];
  await page.route(/\/tickets(?:\?.*)?$/, async (route) => {
    const request = route.request();
    if (request.method() === 'GET') {
      ticketGets += 1;
      return route.fulfill({ json: authoritative });
    }
    if (request.method() === 'POST') {
      const body = request.postDataJSON(),
        normalized = normalizedCreatedTicket(body),
        created = {
          ...row,
          id: '02',
          native_id: '02',
          qualified_id: 'git-local:02',
          slug: 'HS2-NEW001',
          title: normalized.title,
          tags: normalized.tags,
          category: body.category,
          status: body.status ?? 'not_started',
          up_next: Boolean(body.up_next),
          created_at: '2026-08-30T02:00:00Z',
          updated_at: '2026-08-30T02:00:00Z',
        };
      authoritative = [created, ...authoritative];
      while (polls.length === 0) await new Promise((resolve) => setTimeout(resolve, 5));
      cursor += 1;
      await polls.shift()!.fulfill({
        json: {
          cursor,
          events: [{ store: 'git-local', kind: 'created', id: created.id, slug: created.slug }],
          overflow: false,
        },
      });
      eventDelivered = true;
      await createResponse;
      return route.fulfill({
        status: 201,
        json: { ...created, details: body.details ?? '', notes: [], attachments: [] },
      });
    }
    return route.fallback();
  });
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  const readsBefore = ticketGets;
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Created while watch event arrives');
  await page.getByRole('button', { name: 'Create ticket' }).click();
  await expect.poll(() => eventDelivered).toBe(true);
  await expect(page.locator('[data-ticket-slug="HS2-NEW001"]')).toHaveCount(0);
  expect(ticketGets).toBe(readsBefore);
  releaseCreate();
  const incoming = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEW001"]').locator('..'),
    ghost = page.locator('[data-ticket-motion-ghost="incoming"][data-ticket-motion-slug="HS2-NEW001"]');
  await expect(ghost).toBeAttached();
  await expect(incoming).toHaveCSS('visibility', 'hidden');
  await page.waitForTimeout(285);
  expect(ticketGets).toBe(readsBefore);
  const opacity = Number.parseFloat(await ghost.evaluate((element) => getComputedStyle(element).opacity));
  expect(opacity).toBeGreaterThan(0);
  expect(opacity).toBeLessThan(1);
  await page.screenshot({ path: '/private/tmp/hs2-jgwtjj-create-watch-race-after.png', fullPage: true });
  await expect(ghost).toHaveCount(0);
  await expect(incoming).toHaveCSS('visibility', 'visible');
  await page.waitForTimeout(250);
  expect(ticketGets).toBe(readsBefore);
  await expect(incoming).toHaveCount(1);
});

test('runs grouped local commands, confirms stop, exposes history, and saves settings', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const command = page.getByRole('button', { name: 'Run checks' });
  await expect(page.getByText('Quality', { exact: true })).toBeVisible();
  await expect(command).toHaveAttribute('title', 'Press and hold for command history.');
  await command.click();
  await expect(page.getByRole('button', { name: 'Running Run checks' })).toBeVisible();
  await page.getByRole('button', { name: 'Running Run checks' }).click();
  const stop = page.locator('[data-component="command-cancellation-dialog"]');
  await expect(stop).toBeVisible();
  await stop.getByRole('button', { name: 'Stop command' }).click();
  await expect(page.getByRole('button', { name: 'Run checks' })).toHaveAttribute('title', /Last run: cancelled/);
  await page.getByRole('button', { name: 'Run checks' }).dispatchEvent('pointerdown');
  await page.waitForTimeout(600);
  await page.getByRole('button', { name: 'Run checks' }).dispatchEvent('pointerup');
  const history = page.locator('[data-component="command-run-dialog"]');
  await expect(history).toContainText('Stopped by user');
  await page.screenshot({ path: '/private/tmp/hs2-jn3x4w-commands-wide.png', fullPage: true });
  await history.getByRole('button', { name: 'Close' }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  const hiddenNarrowCommand = page.locator('[data-action="run-command"]');
  await hiddenNarrowCommand.dispatchEvent('pointerdown');
  await page.waitForTimeout(600);
  await hiddenNarrowCommand.dispatchEvent('pointerup');
  await expect(history).toContainText('Stopped by user');
  await page.screenshot({ path: '/private/tmp/hs2-jn3x4w-commands-narrow.png', fullPage: true });
  await history.getByRole('button', { name: 'Close' }).click();
  await page.setViewportSize({ width: 1280, height: 720 });
  const settingsView = page.getByLabel('Settings view');
  await settingsView.dispatchEvent('click');
  await expect(settingsView).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  const editor = page.locator('[data-component="command-settings-editor"]');
  const firstRow = editor.locator('.command-settings-editor__row').first();
  await firstRow.locator('.command-settings-editor__row-menu-trigger').click();
  await firstRow.locator('[data-action="edit-command-setting"]').dispatchEvent('click');
  const commandDialog = page.locator('#command-editor-dialog');
  await commandDialog.getByLabel('Button label').fill('Review');
  await commandDialog.getByRole('textbox', { name: 'Program' }).fill('/usr/bin/true');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await expect(editor.getByRole('status')).toContainText('Saved.');
  await page.getByLabel('List view').click();
  await expect(page.getByRole('button', { name: 'Review' })).toBeVisible();
});

test('aligns project sidebar highlights, content, and icon hit targets to shared rails', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 760 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const sidebar = page.locator('.project-sidebar'),
    queue = sidebar.locator('[data-action="select-view"][data-item-id="all"]'),
    queueIcon = queue.locator('.kui-list-item__icon'),
    queueLabel = queue.locator('.kui-list-item__label'),
    viewsTitle = sidebar.locator('.view-navigation > .kui-list-header h2'),
    viewActionLayer = sidebar.getByRole('button', { name: 'Add view' }),
    hideLayer = sidebar.locator(':scope > .kui-pane__header > .kui-toolbar .kui-toolbar-control-group'),
    chat = sidebar.getByRole('button', { name: 'Open Codex conversation' });
  const boxes = await Promise.all(
    [sidebar, queue, queueIcon, queueLabel, viewsTitle, viewActionLayer, hideLayer, chat].map((locator) =>
      locator.boundingBox(),
    ),
  );
  expect(boxes.every(Boolean)).toBe(true);
  const [side, rowBox, iconBox, labelBox, titleBox, viewActionBox, hideBox, chatBox] = boxes as NonNullable<
      (typeof boxes)[number]
    >[],
    rail = (value: number) => {
      expect(value).toBeGreaterThanOrEqual(7);
      expect(value).toBeLessThanOrEqual(9);
    };
  // Shared 8px rail: rows, the summary, and the right-aligned action controls all inset by kerf's
  // --kui-layout-inline-margin (= --kui-space-xs = 8px). Updated from the pre-kerf-upgrade 10-11px rail
  // (HS2-YSHZQK); this is CSS-driven, not a headless font-metric tolerance.
  rail(rowBox.x - side.x);
  rail(side.x + side.width - rowBox.x - rowBox.width);
  rail(side.x + side.width - chatBox.x - chatBox.width);
  rail(side.x + side.width - hideBox.x - hideBox.width);
  rail(side.x + side.width - viewActionBox.x - viewActionBox.width);
  // Beta.24 owns the 18px icon slot, with the same content inset and label gap.
  expect(iconBox.width).toBeCloseTo(18, 0);
  expect(iconBox.x - rowBox.x).toBeGreaterThanOrEqual(8);
  expect(iconBox.x - rowBox.x).toBeLessThanOrEqual(10);
  expect(labelBox.x - iconBox.x - iconBox.width).toBeGreaterThanOrEqual(7);
  expect(labelBox.x - iconBox.x - iconBox.width).toBeLessThanOrEqual(9);
  // The Views section header title indents deeper than the row rail.
  expect(titleBox.x - side.x).toBeGreaterThan(rowBox.x - side.x);
  expect(titleBox.x - side.x).toBeGreaterThanOrEqual(16);
  expect(titleBox.x - side.x).toBeLessThanOrEqual(18);
  // The Codex chat and Hide-terminals controls are 44px touch targets; the kerf ListHeader Add-view
  // action is a smaller icon button — its hit-target size is questioned in HS2-EWF7TM.
  expect(chatBox.width).toBeCloseTo(44, 0);
  expect(hideBox.width).toBeCloseTo(44, 0);
  expect(viewActionBox.width).toBeCloseTo(36, 0);
  await sidebar.screenshot({ path: '/private/tmp/hs2-2p8n8d-sidebar-rails-wide.png' });
  await page.setViewportSize({ width: 1024, height: 651 });
  await expect(sidebar.getByRole('button', { name: 'Run checks' })).toBeInViewport();
  await sidebar.screenshot({ path: '/private/tmp/hs2-2p8n8d-sidebar-rails-narrow.png' });
});

test('runs a portable shell command in a named terminal and opens the bottom drawer', async ({ page }) => {
  const terminalRequests: Array<Record<string, unknown>> = [];
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.route('**/__hotsheet/project-api/demo-checkout/commands', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            {
              id: 'lint',
              title: 'Lint project',
              kind: 'shell',
              command: 'npm run lint',
              cwd: '/work/demo/client',
              group: 'Quality',
            },
          ],
        })
      : route.fallback(),
  );
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/terminals'))
      terminalRequests.push(request.postDataJSON());
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  let refreshStarted = false,
    releaseRefresh!: () => void;
  const refreshGate = new Promise<void>((resolve) => {
    releaseRefresh = resolve;
  });
  await page.route('**/__hotsheet/project-api/demo-checkout/terminals', async (route) => {
    if (route.request().method() === 'GET' && !refreshStarted) {
      refreshStarted = true;
      await refreshGate;
      return route.fulfill({ json: [{ id: 'codex-main', alive: true, busy: true, cwd: '/work/demo' }] });
    }
    return route.fallback();
  });
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  await expect.poll(() => refreshStarted).toBe(true);
  await page.getByRole('button', { name: 'Lint project' }).click();
  await expect.poll(() => terminalRequests).toEqual([{ shell_command: 'npm run lint', cwd: '/work/demo/client' }]);
  releaseRefresh();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toBeVisible();
  await expect(drawer.getByRole('tab', { name: 'Lint project' })).toHaveAttribute('aria-selected', 'true');
  await expect(
    drawer.locator('[data-component="terminal-session"]:not([hidden]) [data-component="terminal-viewport"]'),
  ).toHaveAttribute('data-connection', 'connected');
  await page.screenshot({ path: '/private/tmp/hs2-2bkgpk-shell-command-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1180, height: 651 });
  await expect(drawer.getByRole('tab', { name: 'Lint project' })).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hs2-2bkgpk-shell-command-narrow.png', fullPage: true });
});

test('queues a custom AI command as an urgent Up Next ticket and signals an available AI', async ({ page }) => {
  const ticketCreates: Array<Record<string, unknown>> = [],
    commandRuns: string[] = [],
    turns: Array<{ path: string; body: Record<string, unknown> }> = [];
  await mockProject(page);
  await page.route('**/__hotsheet/project-api/demo-checkout/commands', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            {
              id: 'review',
              title: 'Review current changes',
              kind: 'ai',
              prompt: 'Inspect the current diff and resolve any problems.',
              tool: 'codex',
              model: 'gpt-5.6-sol',
              effort: 'high',
              group: 'Quality',
            },
          ],
        })
      : route.fallback(),
  );
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.endsWith('/tickets')) ticketCreates.push(request.postDataJSON());
    if (request.method() === 'POST' && path.endsWith('/commands/review/run')) commandRuns.push(path);
    if (request.method() === 'POST' && path.endsWith('/turns')) turns.push({ path, body: request.postDataJSON() });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Open Codex conversation' }).click();
  const conversation = page.locator('[data-component="ai-conversation"]'),
    dialog = conversation.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await page.mouse.click(1, 1);
  await expect(dialog).toBeHidden();
  await page.getByRole('button', { name: 'Review current changes' }).click();
  await expect
    .poll(() => ticketCreates)
    .toEqual([
      {
        title: 'Review current changes',
        details: 'Inspect the current diff and resolve any problems.',
        category: 'task',
        priority: 'highest',
        up_next: true,
      },
    ]);
  expect(commandRuns).toEqual([]);
  await expect
    .poll(() => turns)
    .toEqual([
      {
        path: '/__hotsheet/project-api/demo-checkout/drive/connections/hotsheet-project-chat-codex-demo-checkout/turns',
        body: { content: '$hotsheet', model: 'gpt-5.6-sol', effort: 'high' },
      },
    ]);
  const created = page.locator('[data-ticket-slug="HS2-NEW001"]');
  await expect(created).toContainText('Review current changes');
  await expect(page.locator('.app-toast')).toContainText('Queued HS2-NEW001 and notified Codex.');
  await created.screenshot({ path: '/private/tmp/hs2-yxgh0c-ai-command-ticket.png' });
});

test('keeps a custom AI command ticket when no AI connection is available', async ({ page }) => {
  const requests: Array<{ path: string; body: Record<string, unknown> }> = [];
  await mockProject(page);
  await page.route('**/__hotsheet/project-api/demo-checkout/commands', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            {
              id: 'review',
              title: 'Review without a live AI',
              kind: 'ai',
              prompt: 'Inspect the current diff.',
              tool: 'claude',
            },
          ],
        })
      : route.fallback(),
  );
  page.on('request', (request) => {
    if (request.method() === 'POST')
      requests.push({ path: new URL(request.url()).pathname, body: request.postDataJSON() ?? {} });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Review without a live AI' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-NEW001"]')).toContainText('Review without a live AI');
  await expect(page.locator('.app-toast')).toContainText('Queued HS2-NEW001.');
  expect(requests.filter((request) => request.path.endsWith('/tickets'))).toHaveLength(1);
  expect(
    requests.some((request) => request.path.endsWith('/turns') || request.path.endsWith('/commands/review/run')),
  ).toBe(false);
});

test('creates, edits, reorders, deletes, and saves typed custom commands', async ({ page }) => {
  const writes: Array<Array<Record<string, unknown>>> = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'PUT' && new URL(request.url()).pathname.endsWith('/commands'))
      writes.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  const editor = page.locator('[data-component="command-settings-editor"]');
  const commandDialog = page.locator('#command-editor-dialog');
  await editor.getByRole('button', { name: 'Add command' }).click();
  await commandDialog.getByLabel('Button label').fill('Lint');
  await commandDialog.getByLabel('Shell command').fill('npm run lint');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await editor.getByRole('button', { name: 'Add command' }).click();
  await commandDialog.getByLabel('Button label').fill('Review changes');
  await commandDialog.getByLabel('Type').selectOption('ai');
  await expect(commandDialog.getByLabel('Prompt')).toBeVisible();
  await commandDialog.getByLabel('Prompt').fill('Review the current changes');
  await commandDialog.getByRole('button', { name: 'AI configuration: Project Default' }).click();
  await commandDialog.locator('[data-action="select-command-ai-tool"][data-value="claude"]').dispatchEvent('click');
  await commandDialog.locator('[data-action="open-command-manual-model"]').dispatchEvent('click');
  const manualModel = page.locator('[data-component="manual-model-dialog"]');
  await expect(manualModel.locator('dialog')).toBeVisible();
  await manualModel.getByLabel('Model identifier').fill('claude-preview');
  await manualModel.getByRole('button', { name: 'Use model' }).click();
  await expect(commandDialog.getByRole('button', { name: /AI configuration: Claude · claude-preview/ })).toBeVisible();
  await commandDialog.locator('[data-action="select-command-ai-default"]').dispatchEvent('click');
  await expect(commandDialog.getByRole('button', { name: 'AI configuration: Project Default' })).toBeVisible();
  await commandDialog.locator('[data-action="select-command-ai-tool"][data-value="claude"]').dispatchEvent('click');
  await commandDialog.locator('[data-action="select-command-ai-model"][data-value="opus"]').dispatchEvent('click');
  await commandDialog.locator('[data-action="select-command-ai-effort"][data-value="high"]').dispatchEvent('click');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  // Drag "Review changes" above "Lint" (native HTML5 drag-and-drop), then delete "Run checks" via its overflow menu.
  const reviewRow = editor.locator('.command-settings-editor__row', { hasText: 'Review changes' }),
    lintRow = editor.locator('.command-settings-editor__row', { hasText: 'Lint' });
  const lintBox = (await lintRow.boundingBox())!;
  const dataTransfer = await page.evaluateHandle(() => new DataTransfer());
  await reviewRow.dispatchEvent('dragstart', { dataTransfer });
  await lintRow.dispatchEvent('dragover', { dataTransfer, clientX: lintBox.x + 24, clientY: lintBox.y + 3 });
  await lintRow.dispatchEvent('drop', { dataTransfer, clientX: lintBox.x + 24, clientY: lintBox.y + 3 });
  await reviewRow.dispatchEvent('dragend', { dataTransfer });
  const runChecksRow = editor.locator('.command-settings-editor__row', { hasText: 'Run checks' });
  await runChecksRow.locator('.command-settings-editor__row-menu-trigger').click();
  await runChecksRow.locator('[data-action="delete-command-setting"]').dispatchEvent('click');
  await expect(editor.locator('.command-settings-editor__row', { hasText: 'Run checks' })).toHaveCount(0);
  // Autosave persists the final set (Review then Lint) with no explicit Save button; identifiers are auto-generated, so assert by title/kind.
  await expect.poll(() => writes.at(-1)?.map((command) => command.title)).toEqual(['Review changes', 'Lint']);
  expect(writes.at(-1)).toMatchObject([
    { kind: 'ai', prompt: 'Review the current changes', tool: 'claude', model: 'opus', effort: 'high' },
    { kind: 'shell', command: 'npm run lint' },
  ]);
  await expect(editor.getByRole('status')).toContainText('Saved.');
  await page.locator('.app-shell__workspace').evaluate((node) => {
    node.scrollTop = 0;
  });
  await page.screenshot({ path: '/private/tmp/hs2-656xj2-command-editor-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.locator('.app-shell__workspace').evaluate((node) => {
    node.scrollTop = 0;
  });
  await expect(editor.locator('.command-settings-editor__row', { hasText: 'Review changes' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-656xj2-command-editor-narrow.png', fullPage: true });
});

test('multi-selects command rows and drags the whole selection to reorder (HS2-VJYQHG)', async ({ page }) => {
  const writes: Array<Array<Record<string, unknown>>> = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'PUT' && new URL(request.url()).pathname.endsWith('/commands'))
      writes.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  const editor = page.locator('[data-component="command-settings-editor"]'),
    commandDialog = page.locator('#command-editor-dialog'),
    rowFor = (title: string) => editor.locator('.command-settings-editor__row', { hasText: title });
  for (const label of ['Alpha', 'Beta', 'Gamma']) {
    await editor.getByRole('button', { name: 'Add command' }).click();
    await commandDialog.getByLabel('Button label').fill(label);
    await commandDialog.getByLabel('Shell command').fill(`echo ${label}`);
    await commandDialog.getByRole('button', { name: 'Done' }).click();
    await expect(rowFor(label)).toBeVisible();
  }
  // Select Alpha, then Cmd/Ctrl-click Gamma to add it to the selection (Beta stays unselected).
  await rowFor('Alpha').click();
  await rowFor('Gamma').click({ modifiers: ['ControlOrMeta'] });
  await expect(rowFor('Alpha')).toHaveAttribute('data-selected', 'true');
  await expect(rowFor('Gamma')).toHaveAttribute('data-selected', 'true');
  await expect(rowFor('Beta')).not.toHaveAttribute('data-selected', 'true');
  await page.screenshot({ path: '/private/tmp/hs2-vjyqhg-multi-select.png', fullPage: true });
  // Drag the whole selection (grabbing Alpha) above Beta; Alpha and Gamma move together.
  const betaBox = (await rowFor('Beta').boundingBox())!,
    dataTransfer = await page.evaluateHandle(() => new DataTransfer());
  await rowFor('Alpha').dispatchEvent('dragstart', { dataTransfer });
  await rowFor('Beta').dispatchEvent('dragover', { dataTransfer, clientX: betaBox.x + 24, clientY: betaBox.y + 3 });
  await rowFor('Beta').dispatchEvent('drop', { dataTransfer, clientX: betaBox.x + 24, clientY: betaBox.y + 3 });
  await rowFor('Alpha').dispatchEvent('dragend', { dataTransfer });
  // Autosave persists the flat order with the two selected commands moved as one block before Beta.
  await expect
    .poll(() => writes.at(-1)?.map((command) => command.title))
    .toEqual(['Run checks', 'Alpha', 'Gamma', 'Beta']);
  await expect(editor.getByRole('status')).toContainText('Saved.');
});

test('collapses individual project command groups independently and remembers each project state', async ({ page }) => {
  const grouped = [
    {
      id: 'commit',
      title: 'Commit Changes',
      kind: 'ai',
      prompt: 'Commit',
      group: null,
      icon: 'send',
      color: '#3b82f6',
    },
    {
      id: 'requirements',
      title: 'Update Requirements',
      kind: 'ai',
      prompt: 'Update docs',
      group: null,
      icon: 'file-text',
      color: '#6b7280',
    },
    {
      id: 'release',
      title: 'Prep Major Release',
      kind: 'ai',
      prompt: 'Prepare release',
      group: null,
      icon: 'wand',
      color: '#14b8a6',
    },
    { id: 'site', title: 'Site', program: '/usr/bin/true', args: [], group: null, icon: 'globe', color: '#3b82f6' },
    {
      id: 'requirements-code',
      title: 'Reqs ↔ Code',
      kind: 'ai',
      prompt: 'Check requirements',
      group: 'Quality',
      icon: 'arrow-left-right',
      color: '#ec4899',
    },
    {
      id: 'hygiene',
      title: 'Check Code Hygiene',
      kind: 'ai',
      prompt: 'Check hygiene',
      group: 'Quality',
      icon: 'soap-dispenser-droplet',
      color: '#f97316',
    },
    {
      id: 'quality',
      title: 'Analyze Code Quality',
      kind: 'ai',
      prompt: 'Analyze quality',
      group: 'Quality',
      icon: 'circle-check-big',
      color: '#8b5cf6',
    },
    {
      id: 'everything',
      title: 'Everything',
      kind: 'ai',
      prompt: 'Run all checks',
      group: 'Quality',
      icon: 'balloon',
      color: '#e5e7eb',
    },
    {
      id: 'status',
      title: 'git status',
      program: '/usr/bin/true',
      args: [],
      group: 'Git',
      icon: 'git-compare',
      color: '#e5e7eb',
    },
    {
      id: 'push',
      title: 'git push',
      program: '/usr/bin/true',
      args: [],
      group: 'Git',
      icon: 'git-compare-arrows',
      color: '#e5e7eb',
    },
  ];
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.route('**/__hotsheet/project-api/demo-checkout/commands', (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: grouped });
    return route.fallback();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  let commands = page.locator('[data-component="command-navigation"]'),
    quality = commands.getByRole('button', { name: 'Quality', exact: true }),
    git = commands.getByRole('button', { name: 'Git', exact: true });
  await expect(quality).toHaveAttribute('aria-expanded', 'true');
  await expect(git).toHaveAttribute('aria-expanded', 'true');
  for (const [title, icon] of [
    ['Commit Changes', 'send'],
    ['Update Requirements', 'file-text'],
    ['Prep Major Release', 'wand'],
    ['Site', 'globe'],
    ['Reqs ↔ Code', 'arrow-left-right'],
    ['Check Code Hygiene', 'soap-dispenser-droplet'],
    ['Analyze Code Quality', 'circle-check-big'],
    ['Everything', 'balloon'],
    ['git status', 'git-compare'],
    ['git push', 'git-compare-arrows'],
  ] as const)
    await expect(commands.getByRole('button', { name: title }).locator(`[data-lucide="${icon}"]`)).toBeVisible();
  await page.waitForTimeout(220);
  await commands.screenshot({ path: '/private/tmp/hs2-cpz79w-command-icons-after.png' });
  await quality.click();
  await expect(quality).toHaveAttribute('aria-expanded', 'false');
  await expect(commands.getByRole('button', { name: 'Reqs ↔ Code' })).toHaveCount(0);
  await expect(commands.getByRole('button', { name: 'git status' })).toBeVisible();
  await page.reload();
  commands = page.locator('[data-component="command-navigation"]');
  quality = commands.getByRole('button', { name: 'Quality', exact: true });
  git = commands.getByRole('button', { name: 'Git', exact: true });
  await expect(quality).toHaveAttribute('aria-expanded', 'false');
  await expect(git).toHaveAttribute('aria-expanded', 'true');
  await quality.click();
  await git.click();
  await expect(commands.getByRole('button', { name: 'Reqs ↔ Code' })).toBeVisible();
  await expect(commands.getByRole('button', { name: 'git status' })).toHaveCount(0);
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.waitForTimeout(220);
  await commands.screenshot({ path: '/private/tmp/hs2-j963jf-command-groups-narrow.png' });
  await page.reload();
  commands = page.locator('[data-component="command-navigation"]');
  await expect(commands.getByRole('button', { name: 'Quality', exact: true })).toHaveAttribute('aria-expanded', 'true');
  await expect(commands.getByRole('button', { name: 'Git', exact: true })).toHaveAttribute('aria-expanded', 'false');
});

test('switches settings categories from the project sidebar', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const selected = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await selected.click();
  await expect(selected).toHaveAttribute('data-selected', 'true');
  await page.getByLabel('Settings view').click();
  const navigation = page.getByRole('complementary', { name: 'Settings categories' }),
    placeholder = page.locator('.ticket-inspector-placeholder'),
    placeholderToolbar = placeholder.locator(':scope > .kui-toolbar');
  await expect(navigation).toBeVisible();
  await expect(page.getByRole('region', { name: 'Ticket sources settings' })).toBeVisible();
  await expect(page.locator('.project-settings > h2')).toHaveCount(0);
  await expect(page.locator('.project-sidebar')).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: 'Ticket inspector' })).toBeVisible();
  await expect(placeholder).toContainText('Select a ticket to see and edit its details');
  await expect(placeholderToolbar).not.toHaveAttribute('divider-sides');
  await expect(placeholderToolbar).toHaveCSS(
    'box-shadow',
    /^(rgba\(0, 0, 0, 0\) [^,]+)(, rgba\(0, 0, 0, 0\) [^,]+){3}$/,
  );
  await navigation.getByRole('button', { name: 'Commands', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Commands settings' })).toBeVisible();
  await expect(page.locator('[data-component="command-settings-editor"]')).toBeVisible();
  await expect(navigation.getByRole('button', { name: 'Commands', exact: true })).toHaveAttribute(
    'aria-current',
    'page',
  );
  await navigation.getByRole('button', { name: 'Permissions' }).click();
  await expect(page.getByRole('region', { name: 'Permissions settings' })).toBeVisible();
  await expect(page.locator('[name="permission-automation-action"]')).toBeVisible();
  await expect(page.locator('[data-component="command-settings-editor"]')).toHaveCount(0);
  await navigation.getByRole('button', { name: 'Column view' }).click();
  await expect(page.getByRole('region', { name: 'Column view settings' })).toBeVisible();
  await expect(page.getByText('Hide Verified column')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-wsmx7c-settings-sidebar-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(page.getByRole('region', { name: 'Column view settings' })).toBeVisible();
  await expect(placeholderToolbar).toHaveCSS(
    'box-shadow',
    /^(rgba\(0, 0, 0, 0\) [^,]+)(, rgba\(0, 0, 0, 0\) [^,]+){3}$/,
  );
  await page.screenshot({ path: '/private/tmp/hs2-wsmx7c-settings-sidebar-floor.png', fullPage: true });
  await page.getByLabel('List view').click();
  await expect(selected).toHaveAttribute('data-selected', 'true');
});

test('renders exactly once when the long poll announces a permission request', async ({ page }) => {
  await mockProject(page);
  let pending: Array<{
    id: number;
    connection: string;
    tool: string;
    action: string;
    always_allow_supported: boolean;
  }> = [];
  await page.route('**/permissions', (route) => route.fulfill({ json: pending }));
  const polls: Array<import('@playwright/test').Route> = [];
  let cursor = 0;
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await expect(page.locator('.app-loading')).toHaveCount(0);
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  await resetRenderMetrics(page);
  await page.waitForTimeout(100);
  expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
  pending = [{ id: 77, connection: 'codex-session', tool: 'Bash', action: 'cargo test', always_allow_supported: true }];
  cursor += 1;
  await polls.shift()!.fulfill({
    json: { cursor, events: [{ store: '', kind: 'permission_asked', id: '77', slug: 'Bash' }], overflow: false },
  });
  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  const metrics = await renderMetrics(page);
  expect(metrics?.passes).toBe(1);
  expect(metrics?.mutations).toBeGreaterThan(0);
});

test('records externally resolved empty-action permissions in notification history', async ({ page }) => {
  await mockProject(page);
  let pending = [
      { id: 81, connection: 'claude-tool-search', tool: 'ToolSearch', action: '', always_allow_supported: true },
    ],
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [];
  await page.route('**/permissions', (route) => route.fulfill({ json: pending }));
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const popup = page.locator('[data-component="permission-request-popup"]');
  await expect(popup).toContainText('Wants permission to use ToolSearch');
  await expect(popup.locator('.permission-request-card__details')).toHaveCount(0);
  pending = [];
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  cursor += 1;
  await polls.shift()!.fulfill({
    json: {
      cursor,
      events: [{ store: '', kind: 'permission_resolved', id: '81', slug: '', message: 'allow:once' }],
      overflow: false,
    },
  });
  await expect(popup).toHaveCount(0);
  await page.getByRole('button', { name: /Notifications view/ }).click();
  await page.getByRole('button', { name: /Last 24 Hours/ }).click();
  const previous = page
    .locator('[data-component="notification-center"] .permission-request-card--list')
    .filter({ hasText: 'ToolSearch' });
  await expect(previous).toContainText('allowed permission');
  await expect(previous.locator('.permission-request-card__details')).toHaveCount(0);
  await expect(previous.locator('.permission-request-card__footer')).toHaveCount(0);
  await expect(previous).toHaveCSS('padding-bottom', '16px');
  expect(
    await previous.evaluate((node) => {
      const card = node.getBoundingClientRect(),
        summary = node.querySelector('.permission-request-card__summary')!.getBoundingClientRect(),
        border = Number.parseFloat(getComputedStyle(node).borderBottomWidth);
      return Math.round(card.bottom - summary.bottom - border);
    }),
  ).toBe(16);
  await page.screenshot({ path: '/private/tmp/hs2-f058ae-responded-notification-wide.png', fullPage: true });
  await page.getByRole('button', { name: 'Hide notification inspector' }).click();
  await page.setViewportSize({ width: 760, height: 844 });
  await expect(previous).toBeInViewport();
  await expect(previous).toHaveCSS('padding-bottom', '16px');
  await page.screenshot({ path: '/private/tmp/hs2-f058ae-responded-notification-narrow.png', fullPage: true });
});

test('keeps healthy tickets usable and offers safe reveal plus AI repair recovery', async ({ page }) => {
  await mockProject(page);
  const recoveryRequests: { reveal?: unknown; repair?: unknown } = {};
  const diagnostic = {
    store: 'git-local',
    store_path: '/work/demo.hs2',
    path: '/work/demo.hs2/tickets/01/01M1DNB977BK0NG7YJ77RVZXTV.md',
    id: '01M1DNB977BK0NG7YJ77RVZXTV',
    slug: 'HS2-QQRY00',
    error: 'unsupported content follows the bounded Notes section',
  };
  let corruptTickets: (typeof diagnostic)[] = [],
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [];
  await page.route('**/__hotsheet/projects/*/corrupt-tickets/reveal', async (route) => {
    recoveryRequests.reveal = route.request().postDataJSON();
    await route.fulfill({ json: { revealed: true } });
  });
  await page.route('**/corrupt-tickets/repair', async (route) => {
    recoveryRequests.repair = route.request().postDataJSON();
    await route.fulfill({
      status: 201,
      json: {
        ...full,
        id: 'repair-01',
        native_id: 'repair-01',
        qualified_id: 'git-local:repair-01',
        slug: 'HS2-REPAIR',
        title: 'Repair corrupt ticket HS2-QQRY00',
        category: 'bug',
        priority: 'high',
        up_next: true,
      },
    });
  });
  await page.route('**/corrupt-tickets', (route) => {
    void route.fulfill({ json: corruptTickets });
  });
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();

  const stale = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-QQRY00"]');
  await expect(stale).toBeVisible();
  corruptTickets = [diagnostic];
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  cursor += 1;
  await polls.shift()!.fulfill({
    json: {
      cursor,
      events: [{ store: 'git-local', kind: 'changed', id: diagnostic.id, slug: diagnostic.slug }],
      overflow: false,
    },
  });
  const corrupt = page.locator('[data-component="corrupt-ticket-row"]');
  await expect(
    page
      .getByRole('button', { name: /Ticket errors/ })
      .locator('xpath=..')
      .locator('.kui-list-item__count'),
  ).toHaveText('1');
  await expect(corrupt).toHaveCount(0);
  await page.getByRole('button', { name: /Ticket errors/ }).click();
  await expect(corrupt).toContainText('HS2-QQRY00');
  await expect(stale).toHaveCount(0);
  await expect(corrupt).toContainText('Ticket file could not be read');
  await expect(corrupt).toHaveAttribute('role', 'group');
  await expect(corrupt).toHaveCSS('padding', '8px 16px');
  await expect(corrupt).toHaveCSS('gap', '8px');
  await expect(corrupt.locator('[data-lucide="file-warning"]')).toBeVisible();
  const errorRowBefore = await corrupt.evaluate((node) => {
    const icon = node.querySelector('[data-lucide="file-warning"]')!.getBoundingClientRect(),
      rail = getComputedStyle(node, '::before');
    return { iconX: icon.x, railWidth: rail.width, railColor: rail.backgroundColor };
  });
  await page.evaluate(() => {
    (window as typeof window & { __corruptSelectionAnimations?: string[] }).__corruptSelectionAnimations = [];
    document.addEventListener('animationstart', (event) => {
      if ((event.target as HTMLElement).matches('[data-component="corrupt-ticket-row"]'))
        (window as typeof window & { __corruptSelectionAnimations: string[] }).__corruptSelectionAnimations.push(
          event.animationName,
        );
    });
  });
  await corrupt.getByRole('button', { name: 'Open recovery for HS2-QQRY00' }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as typeof window & { __corruptSelectionAnimations?: string[] }).__corruptSelectionAnimations,
      ),
    )
    .toContain('corrupt-ticket-selected-wiggle');
  await corrupt.evaluate(async (node) => {
    await Promise.all(node.getAnimations().map((animation) => animation.finished));
  });
  const selectedBorder = await corrupt.evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      widths: [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth],
      colors: [style.borderTopColor, style.borderRightColor, style.borderBottomColor, style.borderLeftColor],
      radii: [
        style.borderTopLeftRadius,
        style.borderTopRightRadius,
        style.borderBottomRightRadius,
        style.borderBottomLeftRadius,
      ],
    };
  });
  expect(selectedBorder.widths).toEqual(['1px', '1px', '1px', '1px']);
  expect(new Set(selectedBorder.colors).size).toBe(1);
  expect(selectedBorder.radii.every((radius) => Number.parseFloat(radius) > 0)).toBe(true);
  const errorRowAfter = await corrupt.evaluate((node) => {
    const icon = node.querySelector('[data-lucide="file-warning"]')!.getBoundingClientRect(),
      rail = getComputedStyle(node, '::before');
    return { iconX: icon.x, railWidth: rail.width, railColor: rail.backgroundColor };
  });
  expect(errorRowAfter).toEqual(errorRowBefore);
  const inspector = page.locator('[data-component="corrupt-ticket-inspector"]');
  await expect(inspector.locator('.corrupt-ticket-inspector__body')).toHaveCSS('gap', '24px');
  await expect(inspector).toContainText('Ticket parsing error');
  await expect(inspector).toContainText('unsupported content follows the bounded Notes section');
  await expect(inspector).toContainText('01M1DNB977BK0NG7YJ77RVZXTV.md');
  await inspector.getByRole('button', { name: 'Reveal in Finder' }).click();
  await expect(page.locator('.app-toast')).toContainText('Opened the file location.');
  await expect(inspector).not.toContainText('Opened the file location.');
  expect(recoveryRequests.reveal).toEqual({ path: '/work/demo.hs2/tickets/01/01M1DNB977BK0NG7YJ77RVZXTV.md' });
  await inspector.getByRole('button', { name: 'Attempt AI repair' }).click();
  await expect(page.locator('.app-toast')).toContainText('Queued HS2-REPAIR for AI repair.');
  await expect(inspector).not.toContainText('Queued HS2-REPAIR for AI repair.');
  expect(recoveryRequests.repair).toEqual({ path: '/work/demo.hs2/tickets/01/01M1DNB977BK0NG7YJ77RVZXTV.md' });
  await page.screenshot({ path: '/private/tmp/hs2-j1f744-corrupt-recovery-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 844 });
  await expect(inspector).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-j1f744-corrupt-recovery-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 720 });

  corruptTickets = [];
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  cursor += 1;
  await polls.shift()!.fulfill({
    json: {
      cursor,
      events: [{ store: 'git-local', kind: 'changed', id: diagnostic.id, slug: diagnostic.slug }],
      overflow: false,
    },
  });
  await expect(page.getByRole('button', { name: /Ticket errors/ })).toHaveCount(0);
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Queue' })).toBeVisible();
  await expect(corrupt).toHaveCount(0);

  await page.getByText('Use real project tickets').click();
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Use real project tickets');
});

test('identifies a ticket from newer HS2 as upgrade-required instead of corrupt', async ({ page }) => {
  await mockProject(page);
  await page.route('**/corrupt-tickets', (route) =>
    route.fulfill({
      json: [
        {
          store: 'git-local',
          store_path: '/work/demo.hs2',
          path: '/work/demo.hs2/tickets/01/new.md',
          slug: 'HS2-NEWER',
          error_code: 'upgrade_required',
          error:
            'This ticket was created by a newer version of Hot Sheet 2 and cannot be opened by this version. Update Hot Sheet 2 to open it.',
        },
      ],
    }),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /Ticket errors/ }).click();
  const newer = page.locator('[data-component="corrupt-ticket-row"]');
  await expect(newer).toContainText('Hot Sheet 2 update required');
  await expect(newer.locator('[data-lucide="refresh-cw"]')).toBeVisible();
  await expect(newer).not.toContainText('Ticket file could not be read');
  await newer.getByRole('button', { name: 'Open recovery for HS2-NEWER' }).click();
  const inspector = page.locator('[data-component="corrupt-ticket-inspector"]');
  await expect(inspector).toContainText('created by a newer version');
  await expect(inspector.getByRole('button', { name: 'Reveal in Finder' })).toBeVisible();
  await expect(inspector.getByRole('button', { name: 'Attempt AI repair' })).toHaveCount(0);
});

test('updates the open project after external ticket additions edits and deletion', async ({ page }) => {
  await mockProject(page);
  let liveRows = [row, backlogRow, archiveRow, notStartedRow, completedRow, verifiedRow, startedRow2, startedRow3],
    liveFull = { ...full },
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [];
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: liveRows }) : route.fallback(),
  );
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { store: 'git-local', ...liveFull } })
      : route.fallback(),
  );
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  const emit = async (kind: string, id: string, slug: string) => {
    await expect.poll(() => polls.length).toBeGreaterThan(0);
    cursor += 1;
    await polls
      .shift()!
      .fulfill({ json: { cursor, events: [{ store: 'git-local', kind, id, slug }], overflow: false } });
  };

  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const original = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await original.click();
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Use real project tickets');

  const openSelect = page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"]');
  await openSelect.click();
  await expect(openSelect).toHaveJSProperty('open', true);
  liveRows = liveRows.map((item) => (item.id === '01' ? { ...item, title: 'Externally edited ticket' } : item));
  liveFull = { ...liveFull, title: 'Externally edited ticket' };
  await emit('changed', '01', 'HS2-DEMO01');
  await page.waitForTimeout(200);
  await expect(original).toContainText('Use real project tickets');
  await expect(page.locator('.app-loading')).toBeHidden();
  await expect(openSelect).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(original).toContainText('Externally edited ticket');
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Externally edited ticket');

  const external = {
    ...row,
    id: '10',
    native_id: '10',
    qualified_id: 'git-local:10',
    slug: 'HS2-EXTERNAL',
    title: 'Externally added ticket',
    status: 'not_started',
    up_next: false,
  };
  liveRows = [external, ...liveRows];
  await emit('changed', '10', 'HS2-EXTERNAL');
  await expect(page.locator('[data-ticket-slug="HS2-EXTERNAL"]')).toContainText('Externally added ticket');
  await page.screenshot({ path: '/private/tmp/hs2-9d4hcq-live-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: '/private/tmp/hs2-9d4hcq-live-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 720 });

  liveRows = liveRows.filter((item) => item.id !== '01');
  await emit('deleted', '01', '');
  await expect(original).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: 'Ticket inspector' })).toContainText(
    'Select a ticket to see and edit its details',
  );
});

test('merges unrelated external ticket fields and offers an editable merge for the active field', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  let liveRows = [row, backlogRow, archiveRow, notStartedRow, completedRow, verifiedRow, startedRow2, startedRow3],
    liveFull = { ...full },
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [],
    patches: Record<string, unknown>[] = [];
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: liveRows }) : route.fallback(),
  );
  await page.route('**/tickets/01', (route) => {
    const request = route.request();
    if (request.method() === 'GET') return route.fulfill({ json: { store: 'git-local', ...liveFull } });
    if (request.method() !== 'PATCH') return route.fallback();
    const patch = request.postDataJSON() as Record<string, unknown>;
    patches.push(patch);
    if (patch.expected_token !== liveFull.concurrency_token)
      return route.fulfill({ status: 409, json: { error: 'ticket changed since it was read' } });
    liveFull = { ...liveFull, ...patch, concurrency_token: `committed-${patches.length}` };
    liveRows = liveRows.map((item) =>
      item.id === '01' ? { ...item, ...patch, updated_at: '2026-09-02T03:00:00Z' } : item,
    );
    return route.fulfill({ json: { store: 'git-local', ...liveFull } });
  });
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  const emit = async () => {
    await expect.poll(() => polls.length).toBeGreaterThan(0);
    cursor += 1;
    await polls.shift()!.fulfill({
      json: {
        cursor,
        events: [{ store: 'git-local', kind: 'changed', id: '01', slug: 'HS2-DEMO01' }],
        overflow: false,
      },
    });
  };

  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  let editor = inspector.getByRole('textbox', { name: 'Ticket details' });

  liveFull = { ...liveFull, details: 'Remote-only details', concurrency_token: 'remote-details' };
  await emit();
  await expect(editor).toHaveValue('Remote-only details');
  await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);

  liveFull = { ...liveFull, status: 'completed', concurrency_token: 'remote-status' };
  liveRows = liveRows.map((item) => (item.id === '01' ? { ...item, status: 'completed' } : item));
  await editor.fill('Local text after remote status');
  await expect
    .poll(() =>
      patches.some(
        (patch) => patch.details === 'Local text after remote status' && patch.expected_token === 'remote-status',
      ),
    )
    .toBe(true);
  await expect(inspector.locator('wa-select[name="inspector-status"]')).toHaveJSProperty('value', 'completed');
  await expect(editor).toHaveValue('Local text after remote status');
  await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  await emit();

  await inspector.getByRole('button', { name: 'Open ticket reader' }).evaluate((node) => {
    (node as HTMLElement).click();
  });
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ });
  await expect(reader).toBeVisible();
  await reader.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  editor = reader.getByRole('textbox', { name: 'Ticket details' });
  await editor.fill('My local wording');
  await expect.poll(() => patches.some((patch) => patch.details === 'My local wording')).toBe(true);
  liveFull = { ...liveFull, details: 'Their newer wording', concurrency_token: 'remote-conflict' };
  await editor.fill('My revised local wording');
  const conflict = reader.locator('[data-component="ticket-field-conflict"]');
  await expect(conflict).toBeVisible();
  await expect(conflict).toContainText('Their newer wording');
  await expect(conflict).toContainText('My revised local wording');
  await expect(page.locator('.app-error')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-0bp930-field-conflict-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 940 });
  await expect(conflict).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-0bp930-field-conflict-narrow.png', fullPage: true });

  const resolution = conflict.getByRole('textbox', { name: 'Merged details' });
  await resolution.fill('My local wording\n\nTheir newer wording');
  await conflict.getByRole('button', { name: 'Apply merged value' }).click();
  await expect(conflict).toHaveCount(0);
  await expect(editor).toHaveValue('My local wording\n\nTheir newer wording');
  await expect
    .poll(() =>
      patches.some(
        (patch) =>
          patch.details === 'My local wording\n\nTheir newer wording' && patch.expected_token === 'remote-conflict',
      ),
    )
    .toBe(true);
});

test('does not report this clients own in-flight autosave as a merge conflict', async ({ page }) => {
  await mockProject(page);
  let liveFull = { ...full },
    liveRows = [row],
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [],
    writes: Array<import('@playwright/test').Route> = [];
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: liveRows }) : route.fallback(),
  );
  await page.route('**/tickets/01', (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: { store: 'git-local', ...liveFull } });
    if (route.request().method() === 'PATCH') {
      writes.push(route);
      return;
    }
    return route.fallback();
  });
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  const emit = async () => {
    await expect.poll(() => polls.length).toBeGreaterThan(0);
    cursor += 1;
    await polls.shift()!.fulfill({
      json: {
        cursor,
        events: [{ store: 'git-local', kind: 'changed', id: '01', slug: 'HS2-DEMO01' }],
        overflow: false,
      },
    });
  };
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const editor = inspector.getByRole('textbox', { name: 'Ticket details' });
  const partial = 'they should have round borders and outl',
    complete = 'they should have round borders and outlines';
  await editor.fill(partial);
  await expect.poll(() => writes.length).toBe(1);
  await editor.fill(complete);
  await page.waitForTimeout(300);
  expect(writes).toHaveLength(1);
  const first = writes.shift()!;
  liveFull = { ...liveFull, details: partial, concurrency_token: 'partial-token' };
  liveRows = liveRows.map((item) => ({ ...item, details: partial }));
  await first.fulfill({ json: { store: 'git-local', ...liveFull } });
  await expect.poll(() => writes.length).toBe(1);
  const second = writes.shift()!;
  expect(second.request().postDataJSON().expected_token).toBe('partial-token');
  await emit();
  await expect(editor).toHaveValue(complete);
  await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  liveFull = { ...liveFull, details: complete, concurrency_token: 'complete-token' };
  await second.fulfill({ json: { store: 'git-local', ...liveFull } });
  await expect(editor).toHaveValue(complete);
  await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
});

test('translates urgent priority through the canonical server contract', async ({ page }) => {
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  await page.locator('wa-select[name="inspector-priority"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'urgent';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect.poll(() => patches.at(-1)?.priority).toBe('highest');
  await expect(
    page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"]'),
  ).toHaveJSProperty('value', 'urgent');
  await expect(
    page.locator(
      '[data-component="ticket-inspector"] wa-select[name="inspector-priority"] .kui-select__icon--selected [data-lucide="chevrons-up"]',
    ),
  ).toBeVisible();
});

test('moves tickets to Backlog and Archive from every shipped status menu', async ({ page }) => {
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /Archive/ }).click();
  const archived = page.locator('[data-ticket-slug="HS2-ARCH01"]');
  await archived.click();
  const inspector = page.locator('[data-component="ticket-inspector"]'),
    status = inspector.locator('wa-select[name="inspector-status"]');
  await expect(status).toHaveJSProperty('value', 'archive');
  await expect(status).toHaveAttribute('aria-label', 'Change status, Archive');
  await expect(status.locator('.kui-select__custom-selected [data-lucide="archive"]')).toBeVisible();
  await status.click();
  await expect(status.locator('wa-option')).toHaveCount(6);
  await expect(status.locator('wa-divider')).toHaveCount(1);
  await status.locator('wa-option[value="backlog"]').click();
  await expect.poll(() => patches.some((patch) => patch.status === 'backlog')).toBe(true);
  await page.getByRole('button', { name: /Backlog/ }).click();
  const backlogged = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"]');
  await expect(backlogged).toBeVisible();
  await backlogged.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Ticket actions' });
  await menu.locator('wa-dropdown-item:not([slot="submenu"])', { hasText: 'Change status' }).hover();
  const choices = menu.locator('[data-context-field="status"]');
  await expect(choices).toHaveCount(6);
  await expect(menu.locator('wa-divider[slot="submenu"]')).toHaveCount(1);
  await choices.filter({ hasText: 'Archive' }).click();
  await expect.poll(() => patches.some((patch) => patch.status === 'archive')).toBe(true);
  await page.getByRole('button', { name: /Archive/ }).click();
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"]')).toBeVisible();
});

test('projects Up Next immediately and reconciles without a full project refresh', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const row = page.locator('[data-component="ticket-list-row"]', { hasText: 'Not started ticket' }),
    projectTab = page.locator('[data-tab-kind="project"]', { hasText: 'demo' });
  await expect(projectTab.locator('.project-tab__work-count')).toHaveText('1');
  const requests: string[] = [];
  page.on('request', (request) => {
    requests.push(`${request.method()} ${new URL(request.url()).pathname}`);
  });
  await page.route('**/tickets/05', async (route) => {
    if (route.request().method() !== 'PATCH') return route.fallback();
    await new Promise((resolve) => setTimeout(resolve, 250));
    const body = route.request().postDataJSON();
    return route.fulfill({
      json: { store: 'git-local', ...notStartedRow, ...body, details: '', notes: [], attachments: [] },
    });
  });
  await page.evaluate(() => {
    document.addEventListener(
      'click',
      () => {
        const started = performance.now();
        requestAnimationFrame(() => {
          (window as typeof window & { mutationRenderMs?: number }).mutationRenderMs = performance.now() - started;
        });
      },
      { once: true, capture: true },
    );
  });
  await row.getByRole('button', { name: 'Add to Up Next' }).click();
  await expect(row.getByRole('button', { name: 'Remove from Up Next' })).toBeVisible();
  await expect(projectTab.locator('.project-tab__work-count')).toHaveText('2');
  await expect
    .poll(() => page.evaluate(() => (window as typeof window & { mutationRenderMs?: number }).mutationRenderMs))
    .toBeLessThan(100);
  await expect
    .poll(() => requests.some((value) => value.startsWith('PATCH ') && !value.endsWith('/tickets')))
    .toBe(true);
  await page.waitForTimeout(300);
  const afterPatch = requests.slice(requests.findIndex((value) => value.startsWith('PATCH ')) + 1);
  expect(afterPatch.filter((value) => value.includes('/tickets') || value.includes('/repository/status'))).toEqual([]);
});

test('keeps primary 138-ticket interactions within the painted UI budget', async ({ page }, testInfo) => {
  const largeRows = [
    row,
    ...Array.from({ length: 137 }, (_, index) => ({
      ...notStartedRow,
      id: `perf-${index}`,
      native_id: `perf-${index}`,
      qualified_id: `git-local:perf-${index}`,
      slug: `HS2-PERF${String(index).padStart(3, '0')}`,
      title: `Performance ticket ${index + 1}`,
      status: index < 18 ? 'backlog' : 'not_started',
      up_next: false,
    })),
  ];
  await page.addInitScript(() => {
    const state = window as typeof window & {
      __interactionTimings?: Array<{ name: string; update_ms: number; paint_ms: number; over_budget: boolean }>;
    };
    state.__interactionTimings = [];
    document.addEventListener('hotsheet:interaction-timing', (event) => {
      state.__interactionTimings!.push((event as CustomEvent).detail);
    });
  });
  await mockProject(page);
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? { ...project, id: 'other-checkout', root, name: 'other', apiPath: '/__hotsheet/project-api/other-checkout' }
          : project,
    });
  });
  await page.route(/\/tickets(?:\?.*)?$/, (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    return route.fulfill({
      json: new URL(route.request().url()).pathname.includes('/other-checkout/')
        ? [
            {
              ...row,
              id: 'other',
              native_id: 'other',
              qualified_id: 'git-local:other',
              slug: 'HS2-OTHER1',
              title: 'Other ticket',
            },
          ]
        : largeRows,
    });
  });
  await page.route('**/connections', (route) =>
    route.fulfill({ json: [{ id: 'perf-session', tool: 'Codex', project: '/work/demo', role: 'main', busy: true }] }),
  );
  await page.route('**/permissions', (route) =>
    route.fulfill({
      json: new URL(route.request().url()).pathname.includes('/other-checkout/')
        ? []
        : [
            {
              id: 91,
              project: '/work/demo',
              connection: 'perf-session',
              tool: 'Bash',
              action: 'profile command',
              always_allow_supported: true,
            },
          ],
    }),
  );
  await page.route('**/permissions/91', (route) =>
    route.fulfill({ json: { connection: 'perf-session', decision: 'allow', persisted: false } }),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-component="permission-request-popup"]').getByRole('button', { name: 'Ignore' }).click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  await page.evaluate(() => {
    (window as typeof window & { __interactionTimings?: unknown[] }).__interactionTimings = [];
  });
  const timing = async (name: string, action: () => Promise<unknown>) => {
    const before = await page.evaluate(
      () => ((window as typeof window & { __interactionTimings?: unknown[] }).__interactionTimings ?? []).length,
    );
    await action();
    await expect
      .poll(() =>
        page.evaluate(
          ({ name, before }) =>
            ((window as typeof window & { __interactionTimings?: Array<{ name: string }> }).__interactionTimings ?? [])
              .slice(before)
              .find((item) => item.name === name),
          { name, before },
        ),
      )
      .toBeTruthy();
    return page.evaluate(
      ({ name, before }) =>
        (
          (
            window as typeof window & {
              __interactionTimings?: Array<{ name: string; update_ms: number; paint_ms: number; over_budget: boolean }>;
            }
          ).__interactionTimings ?? []
        )
          .slice(before)
          .find((item) => item.name === name)!,
      { name, before },
    );
  };
  const samples = [];
  samples.push(await timing('project-change', () => page.getByRole('tab', { name: 'demo' }).click()));
  samples.push(await timing('ticket-view-change', () => page.getByRole('button', { name: /Backlog/ }).click()));
  samples.push(await timing('ticket-view-change', () => page.getByRole('button', { name: /Queue/ }).click()));
  samples.push(await timing('workspace-mode-change', () => page.getByRole('button', { name: 'Columns view' }).click()));
  samples.push(await timing('workspace-mode-change', () => page.getByRole('button', { name: 'List view' }).click()));
  const first = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  samples.push(await timing('ticket-selection', () => first.click()));
  samples.push(
    await timing('ticket-up-next-change', () => first.getByRole('button', { name: 'Remove from Up Next' }).click()),
  );
  samples.push(
    await timing('ticket-status-change', () =>
      page.locator('wa-select[name="inspector-status"]').evaluate((node: HTMLElement & { value: string }) => {
        node.value = 'completed';
        node.dispatchEvent(new Event('change', { bubbles: true }));
      }),
    ),
  );
  await page.getByRole('button', { name: /Notifications view/ }).click();
  samples.push(
    await timing('permission-decision', () =>
      page.locator('[data-component="notification-center"]').getByRole('button', { name: 'Allow Once' }).click(),
    ),
  );
  await testInfo.attach('138-ticket-interaction-profile.json', {
    body: Buffer.from(JSON.stringify(samples, null, 2)),
    contentType: 'application/json',
  });
  const profile = JSON.stringify(samples);
  for (const sample of samples) {
    expect(sample.update_ms, `${sample.name} state update; ${profile}`).toBeLessThan(100);
    expect(sample.paint_ms, `${sample.name} painted UI; ${profile}`).toBeLessThan(100);
    expect(sample.over_budget, `${sample.name} budget flag; ${profile}`).toBe(false);
  }
});

test('shows Trash below Archive and restores deleted tickets through the real ticket menu (HS2-MWDR19)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  const restoreRequests: string[] = [];
  page.on('request', (request) => {
    if (request.method() === 'POST' && request.url().endsWith('/restore'))
      restoreRequests.push(new URL(request.url()).pathname);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const navigation = page.locator('[data-component="view-navigation"]'),
    labels = () =>
      navigation.locator('li').evaluateAll((items) => items.map((item) => item.textContent.replace(/\d+/g, '').trim()));
  await expect.poll(labels).toEqual(expect.arrayContaining(['Archive', 'Trash']));
  const order = await labels();
  expect(order.indexOf('Trash')).toBe(order.indexOf('Archive') + 1);
  const trashItem = navigation.locator('li').filter({ hasText: 'Trash' });
  await expect(trashItem.locator('[data-lucide="trash-2"]')).toBeVisible();
  await expect(trashItem).toContainText('1');
  await navigation.locator('li').filter({ hasText: 'Archive' }).getByRole('button').first().click();
  await expect(page.locator('[data-ticket-slug="HS2-DEL001"]')).toHaveCount(0);
  await trashItem.getByRole('button').first().click();
  const deleted = page.locator('[data-ticket-slug="HS2-DEL001"]');
  await expect(deleted).toBeVisible();
  await expect(deleted).toHaveAttribute('data-status', 'deleted');
  await expect(deleted.getByText('Deleted', { exact: true })).toBeVisible();
  await expect(page.locator('[data-ticket-slug]')).toHaveCount(1);
  await page.screenshot({ path: '/private/tmp/hs2-mwdr19-trash-view-wide.png' });
  await deleted.click({ button: 'right' });
  await expect(page.locator('[data-component="ticket-inspector"] [data-status="deleted"]')).toContainText('Deleted');
  const restore = page.locator('[data-context-action="Restore ticket"]');
  await expect(restore).toBeVisible();
  await expect(restore.locator('[data-lucide="archive-restore"]')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-mwdr19-restore-menu-wide.png' });
  await restore.click();
  await expect
    .poll(() => restoreRequests)
    .toEqual(['/__hotsheet/project-api/demo-checkout/checkouts/demo-checkout/tickets/12/restore']);
  await expect(deleted).toHaveCount(0);
  // Trash stays while selected so restoring the last ticket does not yank the view away; leaving it hides the empty Trash.
  await expect(trashItem).toBeVisible();
  await navigation.locator('li').filter({ hasText: 'Queue' }).getByRole('button').first().click();
  await expect(page.locator('[data-ticket-slug="HS2-DEL001"]')).toBeVisible();
  await expect(navigation.locator('li').filter({ hasText: 'Trash' })).toHaveCount(0);
  await page.setViewportSize({ width: 760, height: 640 });
  await page.screenshot({ path: '/private/tmp/hs2-mwdr19-restored-queue-narrow.png' });
});

test('projects a newly created ticket within one frame without a collection refresh', async ({ page }, testInfo) => {
  await mockProject(page);
  await page.route(/\/tickets$/, async (route) => {
    if (route.request().method() === 'POST') await new Promise((resolve) => setTimeout(resolve, 200));
    return route.fallback();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const requests: string[] = [];
  page.on('request', (request) => requests.push(`${request.method()} ${new URL(request.url()).pathname}`));
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Immediate ticket');
  await page.evaluate(() => {
    const state = window as typeof window & {
      newTicketResponseAt?: number;
      newTicketTransportMs?: number;
      newTicketProjectionMs?: number;
      newTicketProjectionFrames?: number;
    };
    let frames = 0;
    let animationFrame = 0;
    const countFrame = () => {
      frames += 1;
      animationFrame = requestAnimationFrame(countFrame);
    };
    const nativeFetch = window.fetch.bind(window);
    window.fetch = async (...args) => {
      const started = performance.now();
      const response = await nativeFetch(...args);
      const [input, init] = args;
      const url = input instanceof Request ? input.url : String(input);
      const method = init?.method ?? (input instanceof Request ? input.method : 'GET');
      if (method === 'POST' && new URL(url, location.href).pathname.endsWith('/tickets')) {
        const readJson = response.json.bind(response);
        response.json = async () => {
          const ticket: unknown = await readJson();
          state.newTicketResponseAt = performance.now();
          state.newTicketTransportMs = state.newTicketResponseAt - started;
          animationFrame = requestAnimationFrame(countFrame);
          return ticket;
        };
      }
      return response;
    };
    const observer = new MutationObserver(() => {
      if (document.querySelector('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]')) {
        state.newTicketProjectionMs = performance.now() - state.newTicketResponseAt!;
        state.newTicketProjectionFrames = frames;
        cancelAnimationFrame(animationFrame);
        observer.disconnect();
      }
    });
    observer.observe(document.body, { attributes: true, childList: true, subtree: true });
  });
  await page.getByRole('button', { name: 'Create ticket' }).click();
  const row = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]');
  await expect(row).toBeVisible();
  const timings = await page.evaluate(() => {
    const state = window as typeof window & {
      newTicketProjectionMs: number;
      newTicketTransportMs: number;
      newTicketProjectionFrames: number;
    };
    return {
      projection: state.newTicketProjectionMs,
      transport: state.newTicketTransportMs,
      frames: state.newTicketProjectionFrames,
    };
  });
  await testInfo.attach('new-ticket-projection-timings', {
    body: JSON.stringify(timings),
    contentType: 'application/json',
  });
  expect(timings.transport).toBeGreaterThanOrEqual(150);
  expect(timings.projection).toBeGreaterThanOrEqual(0);
  expect(timings.frames).toBeLessThanOrEqual(1);
  if (process.env.HOTSHEET_WEB_PERFORMANCE_GATE === '1') expect(timings.projection).toBeLessThan(100);
  await page.waitForTimeout(250);
  const createIndex = requests.findIndex((value) => value.startsWith('POST ') && value.endsWith('/tickets'));
  expect(createIndex).toBeGreaterThanOrEqual(0);
  expect(
    requests
      .slice(createIndex + 1)
      .filter(
        (value) => value.startsWith('GET ') && (value.endsWith('/tickets') || value.endsWith('/repository/status')),
      ),
  ).toEqual([]);
});

test('aligns Status controls without duplicating badge insets and preserves selection (HS2-AHADNK)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const patches = await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]'),
    status = inspector.locator('wa-select[name="inspector-status"]'),
    badge = status.locator('.kui-select__custom-selected [data-component="status-badge"]');
  const expectStatusGeometry = async (surface: Locator) => {
    const field = surface.locator('.ticket-inspector__status-field');
    await expect(field.getByRole('heading', { name: 'Status' })).toHaveCSS('text-transform', 'uppercase');
    await expect(field.locator('.kui-list-inset-control')).toHaveCount(1);
    await expect(async () => {
      const geometry = await surface.evaluate((node) => {
        const category = node.querySelector('wa-select[name="inspector-category"]')!,
          control = category.shadowRoot!.querySelector('[part~="combobox"]')!.getBoundingClientRect(),
          categoryLabelNode = category.shadowRoot!.querySelector('[part~="form-control-label"]')!,
          categoryLabel = categoryLabelNode.getBoundingClientRect(),
          statusField = node.querySelector('.ticket-inspector__status-field')!,
          label = statusField.querySelector('h2')!.getBoundingClientRect(),
          row = statusField.querySelector('.kui-list-inset-control')!,
          rowStyle = getComputedStyle(row),
          selected = row.querySelector('[data-component="status-badge"]')!.getBoundingClientRect();
        return {
          badgeInset: selected.left - control.left,
          labelInset:
            label.left - categoryLabel.left - Number.parseFloat(getComputedStyle(categoryLabelNode).paddingLeft),
          fieldGap: row.getBoundingClientRect().top - label.bottom,
          categoryGap: control.top - categoryLabel.bottom,
          rowPadding: rowStyle.padding,
          rowBorder: rowStyle.borderWidth,
        };
      });
      expect(geometry.badgeInset).toBeCloseTo(0, 1);
      expect(geometry.labelInset).toBeCloseTo(0, 1);
      expect(geometry.fieldGap).toBeCloseTo(geometry.categoryGap, 1);
      expect(geometry.rowPadding).toBe('0px');
      expect(geometry.rowBorder).toBe('0px');
    }).toPass({ timeout: 5_000 });
  };
  for (const [value, label, icon, fill] of [
    ['not_started', 'Not started', 'circle', 'neutral-fill-normal'],
    ['completed', 'Completed', 'circle-check', 'success-fill-quiet'],
    ['started', 'Started', 'clock', 'warning-fill-normal'],
  ] as const) {
    await status.click();
    await status.locator(`wa-option[value="${value}"]`).click();
    await expect.poll(() => patches.some((patch) => patch.status === value)).toBe(true);
    await expect(status).toHaveJSProperty('value', value);
    await expect(status).toHaveAttribute('aria-label', `Change status, ${label}`);
    await expect(badge).toHaveAttribute('data-status', value);
    await expect(badge).toHaveText(label);
    await expect(badge.locator(`[data-lucide="${icon}"]`)).toBeVisible();
    await expect(badge.locator('svg')).toHaveCount(1);
    await expect(badge).toHaveCSS('background-color', await resolvedColor(badge, `var(--wa-color-${fill})`));
    await expectStatusGeometry(inspector);
    if (value === 'not_started') await captureInspectorStatus(inspector, '/private/tmp/hs2-ahadnk-status-wide.png');
  }
  await inspector.getByRole('button', { name: 'Block ticket' }).click();
  await inspector.getByRole('textbox', { name: 'Blocked reason' }).fill('Waiting for review');
  await inspector.getByRole('textbox', { name: 'Blocked reason' }).blur();
  await expect(inspector.locator('.ticket-inspector__status-line [data-component="blocked-badge"]')).toBeVisible();
  await page.setViewportSize({ width: 1024, height: 700 });
  await expectStatusGeometry(inspector);
  await captureInspectorStatus(inspector, '/private/tmp/hs2-ahadnk-status-blocked-narrow.png');
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expectStatusGeometry(reader);
  await captureInspectorStatus(reader, '/private/tmp/hs2-ahadnk-status-reader-mobile.png');
});

test('matches Details label spacing to Category before and after editing (HS2-S6S709)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const patches = await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const sidebar = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  const expectDetailsSpacing = async (surface: Locator) => {
    await expect(async () => {
      const spacing = await surface.evaluate((node) => {
        const category = node.querySelector('wa-select[name="inspector-category"]')!.shadowRoot!,
          categoryLabel = category.querySelector('[part~="form-control-label"]')!.getBoundingClientRect(),
          categoryField = category.querySelector('[part~="combobox"]')!.getBoundingClientRect(),
          detailsLabel = node.querySelector('.ticket-inspector__details-section h2')!.getBoundingClientRect(),
          detailsField = node.querySelector('.ticket-inspector__details-surface')!.getBoundingClientRect();
        return {
          categoryGap: categoryField.top - categoryLabel.bottom,
          detailsGap: detailsField.top - detailsLabel.bottom,
        };
      });
      expect(spacing.categoryGap).toBeGreaterThan(0);
      expect(spacing.detailsGap).toBeCloseTo(spacing.categoryGap, 1);
    }).toPass({ timeout: 5_000 });
  };
  const exerciseDetails = async (surface: Locator, name: string) => {
    const section = surface.locator('.ticket-inspector__details-section');
    await expectDetailsSpacing(surface);
    await section.screenshot({ path: `/private/tmp/hs2-s6s709-${name}-preview.png` });
    await surface.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
    const editor = surface.getByRole('textbox', { name: 'Ticket details' });
    await expect(editor).toBeFocused();
    await expectDetailsSpacing(surface);
    await section.screenshot({ path: `/private/tmp/hs2-s6s709-${name}-editing.png` });
    const next = `Details edited in ${name}.`;
    await editor.fill(next);
    await editor.blur();
    await expect.poll(() => patches.some((patch) => patch.details === next)).toBe(true);
    await expect(surface.getByRole('button', { name: 'Edit Ticket details' })).toContainText(next);
    await expectDetailsSpacing(surface);
  };
  await exerciseDetails(sidebar, 'sidebar-wide');
  await page.setViewportSize({ width: 1024, height: 700 });
  await exerciseDetails(sidebar, 'sidebar-narrow');
  await sidebar.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await page.setViewportSize({ width: 390, height: 844 });
  await exerciseDetails(reader, 'reader-mobile');
});

test('autosaves ticket text fields without explicit save or cancel controls', async ({ page }) => {
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const details = inspector.getByRole('textbox', { name: 'Ticket details' });
  await details.fill('Autosaved details');
  await expect.poll(() => patches.some((patch) => patch.details === 'Autosaved details')).toBe(true);
  await expect(inspector.getByRole('button', { name: /Save|Cancel/ })).toHaveCount(0);

  const note = inspector.locator('[data-component="note-card"][data-note-id="N3"]');
  await note.locator('.note-card__body p').first().dblclick();
  const noteEditor = note.getByRole('textbox', { name: 'Note body' });
  await noteEditor.fill('Autosaved note');
  await expect
    .poll(() => patches.some((patch) => patch.note_id === 'N3' && patch.note === 'Autosaved note'))
    .toBe(true);

  await inspector.locator('.ticket-inspector__content').evaluate((node) => {
    node.scrollTop = 0;
  });
  await inspector.screenshot({ path: '/private/tmp/hs2-qbscn2-block-ticket-empty.png' });
  await inspector.getByRole('button', { name: 'Block ticket' }).click();
  const blocked = inspector.getByRole('textbox', { name: 'Blocked reason' });
  await blocked.fill('Waiting for review');
  await expect.poll(() => patches.some((patch) => patch.blocked_reason === 'Waiting for review')).toBe(true);
  await blocked.blur();
  await expect(inspector.getByText('Waiting for review', { exact: true })).toBeVisible();
  await expect(inspector.getByRole('heading', { name: 'Blocked reason' })).toBeVisible();
  await inspector.screenshot({ path: '/private/tmp/hs2-72kryh-blocked-reason.png' });
  await inspector.locator('.ticket-inspector__blocked-surface').dblclick();
  const clearReason = inspector.getByRole('textbox', { name: 'Blocked reason' });
  await expect(clearReason).toBeFocused();
  await clearReason.fill('   ');
  await clearReason.blur();
  await expect.poll(() => patches.some((patch) => patch.blocked_reason === null)).toBe(true);
  await expect(inspector.getByRole('button', { name: 'Block ticket' })).toBeVisible();
});

test('honors the first control click while ticket details is focused', async ({ page }) => {
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const details = inspector.getByRole('textbox', { name: 'Ticket details' });
  await details.fill('Saved before the first action');
  await inspector.locator('[data-action="add-ticket-note"]').first().click();
  await expect(inspector.getByRole('textbox', { name: 'New note' })).toBeFocused();
  await expect.poll(() => patches.some((patch) => patch.details === 'Saved before the first action')).toBe(true);
  await inspector.locator('[data-action="cancel-new-note"]').click();
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  await inspector.getByRole('textbox', { name: 'Ticket details' }).fill('Saved before changing tabs');
  await inspector.getByRole('tab', { name: 'Timeline' }).click();
  await expect(inspector.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await expect.poll(() => patches.some((patch) => patch.details === 'Saved before changing tabs')).toBe(true);
});

test('creates, cancels, edits, and deletes notes through the shared inspector and reader', async ({ page }) => {
  const patches = await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Add note' }).first().click();
  const composer = inspector.locator('[data-component="note-composer"]');
  await expect(composer.getByRole('textbox', { name: 'New note' })).toBeFocused();
  await composer.getByRole('textbox', { name: 'New note' }).fill('Discard me');
  await composer.getByRole('button', { name: 'Cancel' }).click();
  await expect(composer).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Add note' }).first().click();
  await inspector.getByRole('textbox', { name: 'New note' }).fill('Created note');
  await inspector.getByRole('button', { name: 'Add note', exact: true }).last().click();
  await expect
    .poll(() => patches.some((patch) => patch.note === 'Created note' && patch.note_kind === 'regular'))
    .toBe(true);
  const feedbackSurface = inspector.locator('[data-note-id="N2"] .note-card__body');
  await feedbackSurface.dblclick();
  const feedbackEditor = inspector.getByRole('textbox', { name: 'Note body' });
  await expect(feedbackEditor).toHaveValue('Should this reader preserve the current draft?');
  await feedbackEditor.fill('Revised feedback question');
  await expect
    .poll(() => patches.some((patch) => patch.note_id === 'N2' && patch.note === 'Revised feedback question'))
    .toBe(true);
  await feedbackEditor.blur();
  const noteSurface = inspector.locator('[data-note-id="N3"] .note-card__body');
  await expect(noteSurface).toHaveAttribute('aria-label', 'Edit note');
  await noteSurface.locator('p').first().dblclick();
  const editor = inspector.getByRole('textbox', { name: 'Note body' });
  await editor.fill('Edited lifecycle note');
  await expect
    .poll(() => patches.some((patch) => patch.note_id === 'N3' && patch.note === 'Edited lifecycle note'))
    .toBe(true);
  await editor.blur();
  await expect(editor).toHaveCount(0);
  await inspector.locator('[data-note-id="N3"]').getByRole('button', { name: 'Delete note' }).click();
  await expect(inspector.locator('[data-note-id="N3"]')).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog');
  await expect(reader.getByRole('button', { name: 'Edit ticket', exact: true })).toHaveCount(0);
  const readerDetails = reader.getByRole('button', { name: 'Edit Ticket details' });
  await expect(readerDetails).toBeVisible();
  await readerDetails.dblclick();
  const readerDetailsEditor = reader.getByRole('textbox', { name: 'Ticket details' });
  await expect(readerDetailsEditor).toBeFocused();
  await readerDetailsEditor.blur();
  await reader.getByRole('button', { name: 'Add note' }).first().click();
  const readerComposer = reader.locator('[data-component="note-composer"]');
  await expect(readerComposer).toBeVisible();
  await expect(readerComposer.getByRole('textbox', { name: 'New note' })).toBeFocused();
  const firstCard = await reader.locator('[data-component="note-card"]').first().elementHandle();
  expect(
    await readerComposer.evaluate(
      (composer, list) => Boolean(list.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING),
      firstCard,
    ),
  ).toBe(true);
});

test('aligns the empty Notes text and preserves Add note before the first note exists (HS2-D4VEE8)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await mockProject(page);
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { store: 'git-local', ...full, notes: [] } })
      : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  const add = inspector.locator('.ticket-notes__add');
  const expectEmptyNotes = async (surface: Locator, name: string) => {
    const notes = surface.locator('[data-component="ticket-notes"]');
    await notes.scrollIntoViewIfNeeded();
    await expect(notes.locator('[data-component="list-inset-text"]')).toHaveClass(/kui-list-inset-text--horizontal/);
    await expect(notes.locator('.ticket-notes__empty')).toHaveText('No notes added.');
    const geometry = await notes.evaluate((node) => {
      const label = node.querySelector('h2')!.getBoundingClientRect(),
        empty = node.querySelector('.ticket-notes__empty')!.getBoundingClientRect(),
        inset = getComputedStyle(node.querySelector('[data-component="list-inset-text"]')!);
      return { textInset: empty.left - label.left, paddingBlock: [inset.paddingTop, inset.paddingBottom] };
    });
    expect(geometry.textInset).toBeCloseTo(0, 1);
    expect(geometry.paddingBlock).toEqual(['0px', '0px']);
    const clip = await notes.evaluate((node) => {
      const notes = node.getBoundingClientRect(),
        inspector = node.closest('[data-component="ticket-inspector"]')!.getBoundingClientRect();
      return { x: inspector.left, y: notes.top, width: inspector.width, height: notes.height };
    });
    await page.screenshot({ path: `/private/tmp/hs2-d4vee8-empty-notes-${name}.png`, clip });
  };
  await expect(add).toBeVisible();
  await expect(add).toHaveText(/Add note/);
  await expectEmptyNotes(inspector, 'wide');
  await page.screenshot({ path: '/private/tmp/hs2-yn3x2j-empty-notes-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 844 });
  await expect(add).toBeVisible();
  await expectEmptyNotes(inspector, 'narrow');
  await page.screenshot({ path: '/private/tmp/hs2-yn3x2j-empty-notes-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 844 });
  await add.click();
  await expect(inspector.getByRole('textbox', { name: 'New note' })).toBeFocused();
  await expect(inspector.locator('.ticket-notes__empty-inset')).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(inspector.getByRole('textbox', { name: 'New note' })).toHaveCount(0);
  await expectEmptyNotes(inspector, 'after-cancel');
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expectEmptyNotes(reader, 'reader-mobile');
  await reader.getByRole('button', { name: 'Add note', exact: true }).last().click();
  await expect(reader.getByRole('textbox', { name: 'New note' })).toBeFocused();
  await expect(reader.locator('.ticket-notes__empty-inset')).toHaveCount(0);
});

test('aligns the right inspector on shared menu primitives and its shared content gutter', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]'),
    info = inspector.locator('[data-component="ticket-info-panel"]');
  await expect(info).toBeVisible();
  await expect(info.locator('[data-component="list-header"]')).toHaveCount(4);
  await expect(info.locator('[data-component="list-item"]')).toHaveCount(2);
  await expect(info.locator('.ticket-inspector__details-section [data-component="list-header"]')).toHaveText('Details');
  const notesHeader = info.locator('[data-component="ticket-notes"] [data-component="list-header"]'),
    notesCount = notesHeader.locator('.kui-list-header__count');
  await expect(notesHeader).toContainText('Notes');
  await expect(notesHeader.locator('h2')).toHaveAttribute('aria-label', 'Notes, 3 notes');
  await expect(notesCount).toHaveText('3');
  await expect(notesCount).toHaveAttribute('aria-hidden', 'true');
  await expect(notesCount).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await notesHeader.screenshot({ path: '/private/tmp/hs2-fycazc-notes-count-wide.png' });
  const geometry = await info.evaluate((node) => {
    const style = getComputedStyle(node),
      detailsHeading = node
        .querySelector<HTMLElement>('.ticket-inspector__details-section [data-component="list-header"] h2')!
        .getBoundingClientRect(),
      detailsText = node
        .querySelector<HTMLElement>('.ticket-inspector__details-section .markdown-preview p')!
        .getBoundingClientRect(),
      blockIcon = node
        .querySelector<HTMLElement>('.ticket-inspector__block-action .kui-list-item__icon')!
        .getBoundingClientRect(),
      notesHeading = node
        .querySelector<HTMLElement>('[data-component="ticket-notes"] [data-component="list-header"] h2')!
        .getBoundingClientRect(),
      noteText = node
        .querySelector<HTMLElement>('[data-component="note-card"] .markdown-preview p')!
        .getBoundingClientRect();
    return {
      leftGutter: style.paddingLeft,
      rightGutter: style.paddingRight,
      lefts: [detailsHeading.left, detailsText.left, blockIcon.left, notesHeading.left, noteText.left],
    };
  });
  expect(geometry.leftGutter).toBe('0px');
  expect(geometry.rightGutter).toBe('0px');
  expect(Math.max(...geometry.lefts) - Math.min(...geometry.lefts)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: '/private/tmp/hs2-h5cx3g-right-sidebar-wide-after.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 700 });
  await expect(info).toBeVisible();
  await expect.poll(() => info.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await notesHeader.screenshot({ path: '/private/tmp/hs2-fycazc-notes-count-narrow.png' });
  await page.screenshot({ path: '/private/tmp/hs2-h5cx3g-right-sidebar-narrow-after.png', fullPage: true });
});

test('edits title and tags through controlled capability-aware inspector state', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  const title = inspector.getByRole('heading', { name: 'Use real project tickets' });
  await title.dblclick();
  const titleInput = inspector.getByRole('textbox', { name: 'Ticket title' });
  await titleInput.fill('Renamed ticket');
  await expect.poll(() => patches.some((patch) => patch.title === 'Renamed ticket')).toBe(true);
  await titleInput.blur();
  await expect(inspector.getByRole('heading', { name: 'Renamed ticket' })).toBeVisible();
  const tagsHeader = inspector.locator('[data-component="list-header"]').filter({ hasText: 'Tags' }),
    addTag = tagsHeader.getByRole('button', { name: 'Add tag' });
  await expect(tagsHeader).toHaveCount(1);
  await expect(addTag.locator('[data-lucide="plus"]')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-9zpyr8-tags-production-wide.png', fullPage: true });
  await addTag.click();
  const tagInput = inspector.getByRole('combobox', { name: 'Tag name' });
  await tagInput.fill('regression');
  await tagInput.press('Enter');
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="regression"]')).toBeVisible();
  await expect
    .poll(() => patches.some((patch) => Array.isArray(patch.tags) && patch.tags.includes('regression')))
    .toBe(true);
  await inspector
    .locator('[data-component="tag-chip"][data-tag-id="client"]')
    .evaluate((node) => node.dispatchEvent(new CustomEvent('wa-remove', { bubbles: true })));
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="client"]')).toHaveCount(0);
  await expect
    .poll(() => patches.some((patch) => Array.isArray(patch.tags) && !patch.tags.includes('client')))
    .toBe(true);
  await page.keyboard.press('Escape');
  await addTag.evaluate((element) => {
    element.blur();
  });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.mouse.move(10, 10);
  await expect(tagsHeader).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-9zpyr8-tags-production-narrow.png', fullPage: true });
});

test('hides title and tag mutation affordances when the provider cannot update', async ({ page }) => {
  await mockProject(page, false);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('heading', { name: 'Use real project tickets' }).dblclick();
  await expect(inspector.getByRole('textbox', { name: 'Ticket title' })).toHaveCount(0);
  await expect(inspector.getByRole('combobox', { name: 'Add tag' })).toHaveCount(0);
  await expect(inspector.locator('[data-component="tag-chip"]')).not.toHaveAttribute('with-remove', '');
  await expect(inspector.getByRole('button', { name: 'Edit note' })).toHaveCount(0);
  await expect(inspector.getByRole('button', { name: 'Delete note' })).toHaveCount(0);
  const statusField = inspector.locator('.ticket-inspector__status-field'),
    status = statusField.locator('wa-select[name="inspector-status"]');
  await expect(statusField.getByRole('heading', { name: 'Status' })).toBeVisible();
  await expect(statusField.locator('.kui-list-inset-control')).toHaveCount(1);
  await expect(status).toHaveJSProperty('disabled', true);
  await expect(status).toHaveAttribute('aria-label', 'Status, Started');
  await expect(status.locator('.kui-select__custom-selected [data-lucide="clock"]')).toBeVisible();
  await captureInspectorStatus(inspector, '/private/tmp/hs2-ahadnk-status-readonly.png');
});

test('opens a checkout, discovers its source, and drives real shell ticket flows', async ({ page }) => {
  await mockProject(page);
  let submitted: Record<string, unknown> = {};
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/tickets'))
      submitted = request.postDataJSON();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await expect(page.locator('wa-input[name="project-root"]')).toHaveJSProperty('value', '.');
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
  const featureRow = page.locator('[data-component="ticket-list-row"]', { hasText: 'Use real project tickets' });
  await expect(featureRow.locator('[data-lucide="sparkles"]')).toBeVisible();
  await expect(featureRow.locator('.ticket-list-row__category--label')).toHaveCount(0);
  await page.getByText('Use real project tickets').click();
  await expect(page.getByText('The real ticket body.')).toBeVisible();
  const activityNote = page.locator('article[data-note-id="N1"]');
  await expect(activityNote).toHaveAttribute('data-kind', 'activity');
  await expect(activityNote).toContainText('Loaded checkout-scoped tickets.');
  await page.screenshot({ path: '/private/tmp/hs2-a32eak-activity-note-wide.png', fullPage: true });
  await page.locator('wa-select[name="inspector-status"]').click();
  await page.locator('wa-select[name="inspector-status"] wa-option[value="completed"]').click();
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText(
    'Completed',
  );
  await page.getByRole('tab', { name: 'Timeline' }).click();
  await expect(page.getByText('Ticket created')).toBeVisible();
  await expect(page.getByText('Connected project client', { exact: true })).toBeVisible();
  await expect(page.getByText('Loaded checkout-scoped tickets.')).toHaveCount(0);
  const timeline = page.locator('[data-component="ticket-timeline"]');
  await expect(timeline.getByText('Completed', { exact: true })).toBeVisible();
  await expect(timeline).not.toContainText('Status changed from Started to Completed');
  await page.getByRole('tab', { name: 'Info' }).click();
  await page.locator('wa-select[name="inspector-status"]').click();
  await page.locator('wa-select[name="inspector-status"] wa-option[value="backlog"]').click();
  await page.getByRole('tab', { name: 'Timeline' }).click();
  await expect(timeline.getByText('Moved to backlog', { exact: true })).toBeVisible();
  await expect(timeline).not.toContainText('Status changed from Completed to Backlog');
  await page.screenshot({ path: '/private/tmp/hs2-22gcky-timeline-wide.png' });
  await page.setViewportSize({ width: 940, height: 900 });
  await expect(page.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-22gcky-timeline-narrow.png' });
  await page.getByRole('button', { name: 'Show ticket inspector' }).click();
  await page.getByRole('tab', { name: 'Info' }).click();
  await expect(activityNote).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-a32eak-activity-note-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.locator('wa-input[name="new-ticket-title"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = '[client] [Needs Review] Created from the real shell';
    node.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await page.getByRole('textbox', { name: 'Details' }).fill('Created with **Markdown** details.');
  await page.getByRole('button', { name: 'Add new ticket to Up Next' }).click();
  await expect(page.getByRole('button', { name: 'Remove new ticket from Up Next' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page
    .locator('[data-action="create-ticket-form"]')
    .screenshot({ path: '/private/tmp/hs2-new-ticket-composer-production.png' });
  await page.getByRole('button', { name: 'Create ticket' }).click();
  const createdRow = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]');
  await expect(createdRow).toContainText('Created from the real shell');
  await expect(createdRow).not.toContainText('[client]');
  expect(submitted).toMatchObject({
    title: '[client] [Needs Review] Created from the real shell',
    details: 'Created with **Markdown** details.',
    status: 'not_started',
    up_next: true,
  });
  await expect(createdRow.getByRole('button', { name: 'Remove from Up Next' })).toBeVisible();
  await expect(createdRow).toHaveAttribute('data-selected', 'true');
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('HS2-NEW001');
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="markdown-editor"]')).toHaveAttribute(
    'data-mode',
    'write',
  );
  await expect(page.getByRole('textbox', { name: 'Details' })).toHaveValue('Created with **Markdown** details.');
  await expect(page.getByRole('textbox', { name: 'Details' })).toBeFocused();
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(1);
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('client');
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Needs-Review');
  await page.screenshot({ path: '/private/tmp/hs2-chzkr5-create-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await page.screenshot({ path: '/private/tmp/hs2-chzkr5-create-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', 'false');
  await page.getByLabel('Settings view').click();
  await expect(page.getByText('/work/demo.hs2')).toBeVisible();
});

test('adapts the production AppShell below the former 1024 by 600 desktop floor', async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 500 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const shell = page.locator('[data-component="app-shell"]'),
    bounds = await shell.boundingBox();
  expect(bounds?.width).toBe(800);
  expect(bounds?.height).toBeGreaterThanOrEqual(600);
  await expect(shell).toHaveAttribute('data-mobile', 'true');
  await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]')).toHaveAttribute(
    'data-collapsed',
    'true',
  );
  await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-inspector"]')).toHaveAttribute(
    'data-collapsed',
    'true',
  );
  await expect(page.locator('wa-select[name="mobile-project"]')).toBeVisible();
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.waitForTimeout(300);
  await page.screenshot({ path: '/private/tmp/hs2-501eph-production-shell-floor.png', fullPage: true });
});

test('shows reactive open and Up Next counts immediately above Drive', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.waitForTimeout(400);
  const sidebar = page.locator('.project-sidebar'),
    summary = sidebar.locator('[data-component="project-work-summary"]'),
    drive = sidebar.locator('[data-component="drive-control"]');
  await expect(summary).toHaveText('6 open, 1 up next, 0 active');
  const geometry = await sidebar.evaluate((node) => {
    const sidebarBox = node.getBoundingClientRect(),
      summaryBox = node.querySelector('[data-component="project-work-summary"]')!.getBoundingClientRect(),
      driveBox = node.querySelector('[data-component="drive-control"]')!.getBoundingClientRect();
    return {
      centerDelta: Math.abs(summaryBox.left + summaryBox.width / 2 - (sidebarBox.left + sidebarBox.width / 2)),
      orderGap: driveBox.top - summaryBox.bottom,
    };
  });
  expect(geometry.centerDelta).toBeLessThan(1);
  expect(geometry.orderGap).toBeGreaterThanOrEqual(0);
  await page.screenshot({ path: '/private/tmp/hs2-a94d3h-project-work-summary-wide.png', fullPage: true });
  await page.locator('[data-ticket-slug="HS2-NEXT01"]').click();
  const statusSelect = page.locator('wa-select[name="inspector-status"]');
  await statusSelect.click();
  await statusSelect.locator('wa-option[value="completed"]').click();
  await expect(summary).toHaveText('5 open, 1 up next, 0 active');
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 940, height: 844 });
  await expect(summary).toBeVisible();
  await expect(drive).toBeVisible();
  expect(await summary.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-a94d3h-project-work-summary-narrow.png', fullPage: true });
});

test('overrides the default provider and reuses its dedicated drawer Drive chat', async ({ page }) => {
  const driveRequests: Array<{ path: string; body: unknown }> = [];
  await mockProject(page);
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.includes('/drive/connections'))
      driveRequests.push({ path, body: request.postDataJSON() });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const sidebar = page.locator('.project-sidebar'),
    drive = sidebar.locator('[data-action="toggle-drive"]'),
    addView = sidebar.getByRole('button', { name: 'Add view' });
  await expect(addView).toBeEnabled();
  await expect(addView).toHaveAttribute('title', 'Add view');
  await expect(sidebar.locator('wa-select[name="drive-tool"]')).toHaveCount(0);
  await sidebar.getByRole('button', { name: 'Choose Drive provider, model, and effort' }).click();
  await sidebar.locator('wa-dropdown-item').filter({ hasText: 'Provider' }).first().hover();
  await sidebar.locator('[data-action="select-drive-tool"][data-value="claude"]').click();
  await expect(drive).toHaveAccessibleName('Drive with Claude');
  await sidebar
    .locator('[data-component="drive-control"]')
    .screenshot({ path: '/private/tmp/hs2-0fr30w-drive-control-wide.png' });
  await page.setViewportSize({ width: 1024, height: 600 });
  await sidebar
    .locator('[data-component="drive-control"]')
    .screenshot({ path: '/private/tmp/hs2-0fr30w-drive-control-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await drive.click();
  await expect(drive).toHaveAccessibleName('Claude workflow is running');
  await expect(sidebar.locator('[data-component="drive-control"]')).toHaveAttribute('data-running', 'true');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toBeVisible();
  await expect(drawer.getByRole('tab', { name: 'Claude Drive' })).toHaveAttribute('aria-selected', 'true');
  await expect
    .poll(() => driveRequests)
    .toEqual([
      {
        path: '/__hotsheet/project-api/demo-checkout/drive/connections',
        body: {
          tool: 'claude',
          checkout: 'demo-checkout',
          connection_id: 'hotsheet-sidebar-claude-demo-checkout',
          model: 'sonnet',
          effort: 'medium',
        },
      },
      {
        path: '/__hotsheet/project-api/demo-checkout/drive/connections/hotsheet-sidebar-claude-demo-checkout/turns',
        body: { content: '$hotsheet', model: 'sonnet', effort: 'medium' },
      },
    ]);
  await page.screenshot({ path: '/private/tmp/hs2-ndda7p-project-drive-claude-wide.png', fullPage: true });
  page.once('dialog', (dialog) => dialog.accept());
  await drawer.getByRole('button', { name: 'Stop Claude' }).click();
  await expect(drive).toHaveAccessibleName('Drive with Claude');
  expect(driveRequests.at(-1)).toEqual({
    path: '/__hotsheet/project-api/demo-checkout/drive/connections/hotsheet-sidebar-claude-demo-checkout/interrupt',
    body: null,
  });
  await drive.click();
  expect(driveRequests.filter((item) => item.path.endsWith('/drive/connections'))).toHaveLength(1);
  expect(driveRequests.at(-1)).toEqual({
    path: '/__hotsheet/project-api/demo-checkout/drive/connections/hotsheet-sidebar-claude-demo-checkout/turns',
    body: { content: '$hotsheet', session_id: 'thread-1', model: 'sonnet', effort: 'medium' },
  });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(drive).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-ndda7p-project-drive-claude-narrow.png', fullPage: true });
});

test('keeps the Drive menu open while choosing provider, model, and effort', async ({ page }) => {
  await mockProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const sidebar = page.locator('.project-sidebar'),
    options = sidebar.getByRole('button', { name: 'Choose Drive provider, model, and effort' }),
    menu = sidebar.locator('[data-component="drive-options-menu"]');
  await options.click();
  await menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Provider' }).hover();
  await menu.locator('[data-action="select-drive-tool"][data-value="claude"]').click();
  const model = menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Model' });
  await expect(model).toBeVisible();
  await model.hover();
  await expect(model.locator('[data-action="select-drive-model"][data-value="fable"]')).toBeVisible();
  await model.locator('[data-action="select-drive-model"][data-value="fable"]').click();
  const effort = menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Effort' });
  await expect(effort).toBeVisible();
  await effort.hover();
  for (const level of ['low', 'medium', 'high', 'xhigh', 'max'])
    await expect(effort.locator(`[data-action="select-drive-effort"][data-value="${level}"]`)).toBeVisible();
  await effort.locator('[data-action="select-drive-effort"][data-value="high"]').click();
  await expect(effort).toBeVisible();
  await expect(sidebar.locator('[data-action="select-drive-model"][data-value="fable"]')).toHaveAttribute(
    'aria-current',
    'true',
  );
  await expect(sidebar.locator('[data-action="select-drive-effort"][data-value="high"]')).toHaveAttribute(
    'aria-current',
    'true',
  );
  await page.screenshot({ path: '/private/tmp/hs2-s010qf-drive-options-open-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.screenshot({ path: '/private/tmp/hs2-s010qf-drive-options-open-narrow.png', fullPage: true });
  await page.mouse.click(1, 1);
  await expect(effort).toBeHidden();
});

test('opens a Codex chat without implicitly starting Drive through the production ProjectSidebar composition', async ({
  page,
}) => {
  const driveRequests: Array<{ path: string; body: unknown }> = [],
    patches = await mockProject(page);
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.includes('/drive/connections'))
      driveRequests.push({ path, body: request.postDataJSON() });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const sidebar = page.locator('.project-sidebar'),
    drive = sidebar.locator('[data-action="toggle-drive"]'),
    conversationAction = sidebar.getByRole('button', { name: 'Open Codex conversation' });
  await expect(conversationAction).toBeEnabled();
  await expect(drive).toHaveAccessibleName('Drive with Codex');
  await conversationAction.click();
  const conversationHost = page.locator('[data-component="ai-conversation"]'),
    dialog = conversationHost.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(conversationHost).not.toContainText('$hotsheet');
  await expect(conversationHost.getByRole('button', { name: 'Close conversation' })).toHaveCount(0);
  await expect(drive).toHaveAccessibleName('Drive with Codex');
  await expect
    .poll(() => driveRequests)
    .toEqual([
      {
        path: '/__hotsheet/project-api/demo-checkout/drive/connections',
        body: {
          tool: 'codex',
          checkout: 'demo-checkout',
          connection_id: 'hotsheet-project-chat-codex-demo-checkout',
          model: 'gpt-6-astra',
          effort: 'medium',
        },
      },
    ]);
  await expect(conversationHost.locator('[data-component="conversation-model-control"]')).toBeVisible();
  await expect(conversationHost.locator('.ai-conversation__model-name')).toHaveAttribute('title', 'gpt-6-astra');
  await expect(conversationHost.locator('.ai-conversation__model-effort')).toHaveText('medium');
  await dialog.screenshot({ path: '/private/tmp/hs2-ndda7p-empty-codex-chat-wide.png' });
  const composer = conversationHost.getByLabel('Message Codex');
  await composer.fill('Explain the client event boundary.');
  await composer.press('Enter');
  await expect(conversationHost).toContainText('The event stream remains authoritative.');
  await expect(conversationHost).toContainText('12.8K tokens');
  await expect(conversationHost).toContainText('≈$0.04');
  await expect(conversationHost).toContainText('Edited the conversation projection');
  await expect(conversationHost).toContainText('AI-generated · may contain errors');
  await expect(conversationHost.locator('[data-component="permission-request-card"]')).toBeVisible();
  await expect(
    conversationHost.locator('.ai-conversation__foreground [data-component="permission-request-popup"]'),
  ).toBeVisible();
  await expect(conversationHost.getByRole('button', { name: 'Save conversation' })).toHaveJSProperty(
    'tagName',
    'BUTTON',
  );
  await expect(conversationHost.getByRole('button', { name: 'Save conversation' })).toBeDisabled();
  await expect(conversationHost.getByRole('button', { name: 'Stop Codex' })).toHaveJSProperty('tagName', 'BUTTON');
  await expect(conversationHost.getByRole('button', { name: 'Stop Codex' })).toBeVisible();
  page.once('dialog', (nativeDialog) => nativeDialog.accept('Keep the concise summaries.'));
  await conversationHost.getByRole('button', { name: 'Helpful — keep suggestions like this' }).last().click();
  await expect
    .poll(() =>
      patches.some(
        (patch) =>
          typeof patch.note === 'string' &&
          patch.note.includes('AI feedback for activity:activity-1: Helpful') &&
          patch.note.includes('Keep the concise summaries.'),
      ),
    )
    .toBe(true);
  await expect(page.locator('.app-toast')).toContainText('AI feedback saved as a ticket note.');
  await page.screenshot({ path: '/private/tmp/hs2-t6a83t-permission-over-chat-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await page.screenshot({ path: '/private/tmp/hs2-t6a83t-permission-over-chat-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 800 });
  await dialog.screenshot({ path: '/private/tmp/hs2-bxspmn-conversation-usage-wide.png' });
  await conversationHost
    .locator('[data-component="permission-request-card"]')
    .getByRole('button', { name: 'Allow Once' })
    .click();
  await expect(conversationHost.locator('[data-component="permission-request-card"]')).toHaveCount(0);
  await page.setViewportSize({ width: 760, height: 640 });
  await dialog.screenshot({ path: '/private/tmp/hs2-bxspmn-conversation-usage-narrow.png' });
  page.once('dialog', (nativeDialog) => nativeDialog.accept());
  await conversationHost.getByRole('button', { name: 'Stop Codex' }).click();
  await expect(conversationHost.getByRole('button', { name: 'Stop Codex' })).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-293gfj-light-dismiss-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({ path: '/private/tmp/hs2-293gfj-light-dismiss-wide.png', fullPage: true });
  await page.mouse.click(1, 1);
  await expect(dialog).toBeHidden();
});

test('keeps activity before the result that resumes after a permission pause', async ({ page }) => {
  await mockProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Open Codex conversation' }).click();
  const conversation = page.locator('[data-component="ai-conversation"]'),
    dialog = conversation.getByRole('dialog'),
    transcript = conversation.locator('.ai-conversation__transcript');
  await conversation.getByLabel('Message Codex').fill('Check message ordering across a permission pause.');
  await conversation.getByLabel('Message Codex').press('Enter');
  const activity = transcript.locator('[data-activity-id="activity-ordering"]');
  await expect(activity).toContainText('codex ran date');
  await expect(activity.locator('code')).toHaveText('date');
  expect(
    (await activity.locator('code').evaluate((node) => getComputedStyle(node).fontFamily)).toLowerCase(),
  ).toContain('mono');
  const permission = conversation.locator('[data-component="permission-request-card"]');
  await expect(permission).toBeVisible();
  await permission.getByRole('button', { name: 'Allow Once' }).click();
  await expect(permission).toHaveCount(0);
  await expect(transcript.getByText("It's 3:23 PM according to your system clock.", { exact: true })).toBeVisible();
  const order = await transcript.evaluate((node) =>
    [...node.children].flatMap((child) =>
      child.matches('.ai-conversation__message--user')
        ? ['user']
        : child.matches('.ai-conversation__activity')
          ? ['activity']
          : child.matches('.ai-conversation__message--assistant')
            ? ['assistant']
            : [],
    ),
  );
  expect(order).toEqual(['user', 'activity', 'assistant']);
  await dialog.screenshot({ path: '/private/tmp/hs2-kv6sad-ordered-conversation-wide.png' });
  await page.setViewportSize({ width: 760, height: 760 });
  await expect
    .poll(async () =>
      transcript.evaluate((node) =>
        [...node.children].every(
          (child) => child.getBoundingClientRect().right <= node.getBoundingClientRect().right + 1,
        ),
      ),
    )
    .toBe(true);
  await transcript.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await dialog.screenshot({ path: '/private/tmp/hs2-kv6sad-ordered-conversation-narrow.png' });
});

test('clears a foreground permission from its authoritative event while the Allow response is delayed', async ({
  page,
}) => {
  await mockProject(page);
  let pending = [
      { id: 42, connection: 'codex-session', tool: 'Bash', action: 'npm run test', always_allow_supported: true },
    ],
    answerRequests = 0,
    cursor = 0,
    releaseAnswer!: () => void;
  const answerGate = new Promise<void>((resolve) => {
      releaseAnswer = resolve;
    }),
    polls: Array<import('@playwright/test').Route> = [];
  await page.route('**/permissions', (route) => route.fulfill({ json: pending }));
  await page.route('**/permissions/42', async (route) => {
    answerRequests += 1;
    pending = [];
    await answerGate;
    return route.fulfill({ json: { connection: 'codex-session', decision: 'allow', persisted: false } });
  });
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open Codex conversation' }).click();
  const conversation = page.locator('[data-component="ai-conversation"]'),
    dialog = conversation.getByRole('dialog'),
    popup = conversation.locator('.ai-conversation__foreground [data-component="permission-request-popup"]');
  await expect(dialog).toBeVisible();
  await expect(popup).toBeVisible();
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  await popup.getByRole('button', { name: 'Allow Once' }).click();
  await expect(popup).toHaveCount(0);
  await expect.poll(() => answerRequests).toBe(1);
  cursor += 1;
  await polls.shift()!.fulfill({
    json: {
      cursor,
      events: [{ store: '', kind: 'permission_resolved', id: '42', slug: 'Bash', message: 'allow:once' }],
      overflow: false,
    },
  });
  await expect(popup).toHaveCount(0);
  await expect(dialog).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          JSON.parse(localStorage.getItem('hotsheet.permission-history') || '[]').filter(
            (item: { key: string }) => item.key === 'demo-checkout:42',
          ).length,
      ),
    )
    .toBe(1);
  await page.screenshot({
    path: '/private/tmp/hs2-t6a83t-authoritative-permission-resolution-wide.png',
    fullPage: true,
  });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(dialog).toBeVisible();
  await expect(popup).toHaveCount(0);
  await page.screenshot({
    path: '/private/tmp/hs2-t6a83t-authoritative-permission-resolution-narrow.png',
    fullPage: true,
  });
  releaseAnswer();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          JSON.parse(localStorage.getItem('hotsheet.permission-history') || '[]').filter(
            (item: { key: string }) => item.key === 'demo-checkout:42',
          ).length,
      ),
    )
    .toBe(1);
  await expect(popup).toHaveCount(0);
});

test('restores an optimistically dismissed permission only when communication fails', async ({ page }) => {
  await mockProject(page);
  const pending = [
    { id: 43, connection: 'codex-session', tool: 'Bash', action: 'npm run test', always_allow_supported: true },
  ];
  let releaseAnswer!: () => void;
  const answerGate = new Promise<void>((resolve) => {
    releaseAnswer = resolve;
  });
  await page.route('**/permissions', (route) => route.fulfill({ json: pending }));
  await page.route('**/permissions/43', async (route) => {
    await answerGate;
    return route.fulfill({ status: 503, json: { error: 'Permission bridge unavailable' } });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const popup = page.locator('[data-component="permission-request-popup"]');
  await expect(popup).toBeVisible();
  await popup.getByRole('button', { name: 'Deny' }).click();
  await expect(popup).toHaveCount(0);
  releaseAnswer();
  await expect(popup).toBeVisible();
  await expect(popup.getByRole('alert')).toContainText(
    'Could not send the permission decision. Permission bridge unavailable',
  );
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-66tbwx-permission-failure-restored-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(popup).toBeInViewport();
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-66tbwx-permission-failure-restored-narrow.png', fullPage: true });
});

test('presents a legible, aligned AI chat without exposing its session id', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('button', { name: 'Open Codex conversation' }).click();
  const host = page.locator('[data-component="ai-conversation"]'),
    dialog = host.getByRole('dialog'),
    composer = host.getByLabel('Message Codex');
  await expect(dialog).toBeVisible();
  await expect(host).toContainText('Ready for your first message');
  await expect(host).not.toContainText('Session thread-1');
  const modelControl = host.locator('[data-component="conversation-model-control"]');
  await expect(modelControl).toBeVisible();
  await expect(modelControl.locator('.ai-conversation__model-effort')).toBeVisible();
  await host.locator('.ai-conversation__model-trigger').click();
  const modelSubmenu = host.locator('.ai-conversation__model-menu > wa-dropdown-item').filter({ hasText: 'Model' });
  await modelSubmenu.hover();
  await modelSubmenu.locator('[data-action="select-conversation-model"][data-value="gpt-5.6-sol"]').click();
  await expect(modelControl.locator('.ai-conversation__model-name')).toHaveAttribute('title', 'gpt-5.6-sol');
  await expect(dialog).toBeVisible();
  await dialog.screenshot({ path: '/private/tmp/hs2-jvwhq8-model-selection-open-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(dialog).toBeVisible();
  await dialog.screenshot({ path: '/private/tmp/hs2-jvwhq8-model-selection-open-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await composer.fill('What time is it in California?');
  await composer.press('Enter');
  await expect(
    host.getByText('I found the relevant client boundary. The event stream remains authoritative.', { exact: true }),
  ).toBeVisible();
  await expect(host).toContainText('2 messages');
  const userCopy = host.locator('.ai-conversation__message--user .markdown-preview p');
  await expect(userCopy).toHaveCSS('color', 'rgb(255, 255, 255)');
  const contained = () =>
    dialog.evaluate((element) => {
      const box = element.getBoundingClientRect();
      return [...element.querySelectorAll<HTMLElement>('.ai-conversation__message')].every((item) => {
        const message = item.getBoundingClientRect();
        return message.left >= box.left && message.right <= box.right;
      });
    });
  await expect.poll(contained).toBe(true);
  await dialog.screenshot({ path: '/private/tmp/hs2-wj3yr2-ai-chat-polish-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect.poll(contained).toBe(true);
  await expect(userCopy).toHaveCSS('color', 'rgb(255, 255, 255)');
  await dialog.screenshot({ path: '/private/tmp/hs2-wj3yr2-ai-chat-polish-narrow.png' });
});

test('omits effort after selecting a model that does not support it', async ({ page }) => {
  const turns: Array<Record<string, unknown>> = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/turns'))
      turns.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: /AI tools/ }).click();
  const settings = page.locator('[data-component="ai-tool-settings"]'),
    tool = settings.locator('wa-select[name="ai-default-tool"]');
  await tool.click();
  await tool.locator('wa-option[value="claude"]').click();
  await expect(settings).toContainText('Saved locally.');
  await page.getByLabel('List view').click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.locator('[data-action="toggle-terminal-drawer-maximize"]').dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'true');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat').click();
  const host = drawer.locator('[data-component="ai-conversation"]');
  await expect(host).toBeVisible();
  await expect(host.locator('.ai-conversation__model-effort')).toHaveText('medium');
  await host.locator('.ai-conversation__model-trigger').click();
  const modelSubmenu = host.locator('.ai-conversation__model-menu > wa-dropdown-item').filter({ hasText: 'Model' });
  await modelSubmenu.hover();
  await modelSubmenu.locator('[data-action="select-conversation-model"][data-value="haiku"]').click();
  await expect(host.locator('.ai-conversation__model-name')).toHaveAttribute('title', 'haiku');
  await expect(host.locator('.ai-conversation__model-effort')).toHaveCount(0);
  await host.getByLabel('Message Claude').fill('What time is it in California?');
  await host.getByLabel('Message Claude').press('Enter');
  await expect.poll(() => turns).toEqual([{ content: 'What time is it in California?', model: 'haiku' }]);
  await expect(
    host.getByText('I found the relevant client boundary. The event stream remains authoritative.', { exact: true }),
  ).toBeVisible();
  await expect(host.getByRole('alert')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-8xrcyx-haiku-without-effort-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 800 });
  await expect(host).toBeVisible();
  await expect(host.locator('.ai-conversation__model-effort')).toHaveCount(0);
  await host.locator('.ai-conversation__transcript').evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(host.locator('.ai-conversation__message--user')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-8xrcyx-haiku-without-effort-narrow.png', fullPage: true });
});

test('configures plugin-discovered machine-local AI defaults and exposes Claude Fable effort', async ({ page }) => {
  const saves: unknown[] = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'PUT' && new URL(request.url()).pathname.endsWith('/ai-settings'))
      saves.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: /AI tools/ }).click();
  const settings = page.locator('[data-component="ai-tool-settings"]'),
    tool = settings.locator('wa-select[name="ai-default-tool"]'),
    model = settings.locator('wa-select[name="ai-default-model"]'),
    effort = settings.locator('wa-select[name="ai-default-effort"]'),
    modelOption = (id: string) => model.locator(`wa-option[value="${id}"]`);
  await expect(settings).toBeVisible();
  await expect(tool).toHaveAttribute('value', 'codex');
  await expect(model).toHaveAttribute('value', 'gpt-6-astra');
  for (const id of ['gpt-6-astra', 'gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6-luna', 'gpt-5.5', 'gpt-5.3-codex-spark'])
    await expect(modelOption(id)).toBeAttached();
  await expect(model.getByText('Other…')).toBeAttached();
  const settingsGeometry = await settings.evaluate((node) => {
    const description = node.querySelector<HTMLElement>(':scope > p')!.getBoundingClientRect(),
      grid = node.querySelector<HTMLElement>('.ai-tool-settings__grid')!,
      gridBox = grid.getBoundingClientRect(),
      controls = [...grid.children].map((control) => control.getBoundingClientRect());
    return {
      sectionGap: gridBox.top - description.bottom,
      fieldGaps: [controls[1].top - controls[0].bottom, controls[2].top - controls[1].bottom],
    };
  });
  expect(settingsGeometry.sectionGap).toBeCloseTo(24, 0);
  expect(settingsGeometry.fieldGaps).toEqual([16, 16]);
  await page.screenshot({ path: '/private/tmp/hs2-4y6sm9-ai-tool-settings-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.waitForTimeout(250);
  await expect(modelOption('gpt-5.3-codex-spark')).toBeAttached();
  await page.screenshot({ path: '/private/tmp/hs2-4y6sm9-ai-tool-settings-narrow.png', fullPage: true });
  await tool.click();
  await tool.locator('wa-option[value="claude"]').click();
  await expect.poll(() => saves).toEqual([{ tool: 'claude', model: 'sonnet', effort: 'medium' }]);
  for (const id of ['fable', 'opus', 'sonnet', 'haiku']) await expect(modelOption(id)).toBeAttached();
  await model.click();
  await modelOption('fable').click();
  await expect(effort).toBeEnabled();
  await effort.click();
  for (const level of ['low', 'medium', 'high', 'xhigh', 'max'])
    await expect(effort.locator(`wa-option[value="${level}"]`)).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-r9bss0-claude-fable-effort.png', fullPage: true });
  await expect(settings).toContainText('Saved locally.');
});

test('chooses Other for a literal manual model and forgets it after a catalog selection', async ({ page }) => {
  const saves: unknown[] = [],
    driveRequests: Array<Record<string, unknown>> = [];
  await mockProject(page);
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'PUT' && path.endsWith('/ai-settings')) saves.push(request.postDataJSON());
    if (request.method() === 'POST' && path.includes('/drive/connections')) driveRequests.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: /AI tools/ }).click();
  const settings = page.locator('[data-component="ai-tool-settings"]'),
    model = settings.locator('wa-select[name="ai-default-model"]'),
    custom = 'claude legacy "beta"';
  await model.click();
  await model.getByText('Other…').click();
  const dialogHost = page.locator('[data-component="manual-model-dialog"]'),
    dialog = dialogHost.locator('dialog');
  await expect(dialog).toBeVisible();
  await expect(dialogHost).toContainText('accepted by Codex');
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-p28pv9-other-model-dialog-wide.png', fullPage: true });
  await page.getByRole('textbox', { name: /Model identifier/ }).fill(custom);
  await dialogHost.getByRole('button', { name: 'Use model' }).click();
  await expect.poll(() => saves.at(-1)).toEqual({ tool: 'codex', model: custom });
  await expect(model).toHaveAttribute('value', custom);
  await expect(model.locator(`wa-option[value='${custom}']`)).toBeAttached();
  await expect(settings.locator('wa-select[name="ai-default-effort"]')).toHaveJSProperty('disabled', true);
  await page.setViewportSize({ width: 760, height: 640 });
  await model.click();
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-p28pv9-selected-manual-model-narrow.png', fullPage: true });
  await model.locator('wa-option[value="gpt-6-astra"]').click();
  await expect.poll(() => saves.at(-1)).toEqual({ tool: 'codex', model: 'gpt-6-astra', effort: 'low' });
  await expect(model.locator(`wa-option[value='${custom}']`)).toHaveCount(0);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('List view').click();
  const sidebar = page.locator('.project-sidebar'),
    options = sidebar.getByRole('button', { name: 'Choose Drive provider, model, and effort' }),
    menu = sidebar.locator('[data-component="drive-options-menu"]'),
    driveCustom = "drive legacy 'one'";
  await options.click();
  const modelMenu = menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Model' });
  await modelMenu.hover();
  await modelMenu.locator('[data-action="open-drive-manual-model"]').click();
  await expect(dialog).toBeVisible();
  await page.getByRole('textbox', { name: /Model identifier/ }).fill(driveCustom);
  await page.setViewportSize({ width: 760, height: 640 });
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-p28pv9-drive-other-model-dialog-narrow.png', fullPage: true });
  await dialogHost.getByRole('button', { name: 'Use model' }).click();
  await page.setViewportSize({ width: 1280, height: 800 });
  await options.click();
  const selectedCustom = modelMenu.locator(`[data-action="select-drive-model"][data-value="${driveCustom}"]`);
  await modelMenu.hover();
  await expect(selectedCustom).toHaveAttribute('aria-current', 'true');
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-p28pv9-drive-selected-manual-model-wide.png', fullPage: true });
  await selectedCustom.click();
  await sidebar.locator('[data-action="toggle-drive"]').click();
  await expect
    .poll(() => driveRequests.slice(0, 2))
    .toEqual([
      {
        tool: 'codex',
        checkout: 'demo-checkout',
        connection_id: 'hotsheet-sidebar-codex-demo-checkout',
        model: driveCustom,
      },
      { content: '$hotsheet', model: driveCustom },
    ]);
});

test('sends a manually entered live-conversation model literally', async ({ page }) => {
  const turns: Array<Record<string, unknown>> = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/turns'))
      turns.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Open Codex conversation' }).click();
  const chat = page.locator('[data-component="ai-conversation"]'),
    custom = 'legacy model "chat"';
  await chat.locator('.ai-conversation__model-trigger').click();
  const modelSubmenu = chat.locator('.ai-conversation__model-menu > wa-dropdown-item').filter({ hasText: 'Model' });
  await modelSubmenu.hover();
  await modelSubmenu.locator('[data-action="open-conversation-manual-model"]').click();
  const manualDialog = page.locator('[data-component="manual-model-dialog"]');
  await expect(manualDialog.locator('dialog')).toBeVisible();
  await page.getByRole('textbox', { name: /Model identifier/ }).fill(custom);
  await manualDialog.getByRole('button', { name: 'Use model' }).click();
  await expect(chat.locator('.ai-conversation__model-name')).toHaveAttribute('title', custom);
  await expect(manualDialog.locator('dialog')).toBeHidden();
  await expect(chat.locator('.ai-conversation__model-menu')).not.toHaveAttribute('open');
  const composer = chat.getByLabel('Message Codex');
  await composer.click();
  await composer.fill('Use the requested model.');
  await composer.press('Enter');
  await expect.poll(() => turns.at(-1)).toMatchObject({ content: 'Use the requested model.', model: custom });
  expect(turns.at(-1)).not.toHaveProperty('effort');
});

test('shows runtime-discovered Antigravity and OpenCode model catalogs', async ({ page }) => {
  await mockProject(page);
  await page.route('**/ai-tools?refresh=true', (route) =>
    route.fulfill({
      json: [
        {
          id: 'antigravity',
          display_name: 'Antigravity',
          models: [
            { id: 'gemini-3.8-flash-high', label: 'Gemini 3.8 Flash (High)', effort_levels: ['low', 'medium', 'high'] },
            {
              id: 'claude-sonnet-4-6',
              label: 'Claude Sonnet 4.6 (Thinking)',
              effort_levels: ['low', 'medium', 'high'],
            },
          ],
          default_model: 'gemini-3.8-flash-high',
          default_effort: 'medium',
          actions: ['change_model', 'change_effort'],
        },
        {
          id: 'opencode',
          display_name: 'OpenCode',
          models: [
            { id: 'opencode/big-pickle', label: 'opencode/big-pickle' },
            { id: 'openai/gpt-5.6', label: 'openai/gpt-5.6' },
          ],
          default_model: 'opencode/big-pickle',
          actions: ['change_model'],
        },
      ],
    }),
  );
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: /AI tools/ }).click();
  const settings = page.locator('[data-component="ai-tool-settings"]'),
    tool = settings.locator('wa-select[name="ai-default-tool"]'),
    model = settings.locator('wa-select[name="ai-default-model"]');
  await expect(tool).toHaveAttribute('value', 'antigravity');
  await expect(model).toHaveAttribute('value', 'gemini-3.8-flash-high');
  await expect(model.locator('wa-option[value="claude-sonnet-4-6"]')).toBeAttached();
  await page.screenshot({ path: '/private/tmp/hs2-cx4xzr-antigravity-models-wide.png', fullPage: true });
  await tool.click();
  await tool.locator('wa-option[value="opencode"]').click();
  await expect(model).toHaveAttribute('value', 'opencode/big-pickle');
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(model.locator('wa-option[value="openai/gpt-5.6"]')).toBeAttached();
  await page.screenshot({ path: '/private/tmp/hs2-cx4xzr-opencode-models-narrow.png', fullPage: true });
});

test('retries a failed AI-tool discovery from Drive without showing a false empty state', async ({ page }) => {
  await mockProject(page);
  let baseAttempts = 0,
    refreshAttempts = 0;
  const discovered = [
    {
      id: 'opencode',
      display_name: 'OpenCode',
      models: [{ id: 'openai/gpt-5.6', label: 'openai/gpt-5.6' }],
      default_model: 'openai/gpt-5.6',
      actions: ['change_model'],
    },
  ];
  // The fast startup path asks for the server-cached catalog (no forced refresh); model a server whose first discovery fails.
  await page.route('**/ai-tools', (route) => {
    baseAttempts += 1;
    return route.fulfill({ status: 503, json: { error: 'discovery temporarily unavailable' } });
  });
  // Opening Drive options forces a fresh discovery (HS2-10R4VV), which succeeds on retry.
  await page.route('**/ai-tools?refresh=true', async (route) => {
    refreshAttempts += 1;
    await new Promise((resolve) => setTimeout(resolve, 350));
    return route.fulfill({ json: discovered });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await expect.poll(() => baseAttempts).toBeGreaterThan(0);
  const sidebar = page.locator('.project-sidebar'),
    options = sidebar.getByRole('button', { name: 'Choose Drive provider, model, and effort' });
  await options.click();
  const menu = sidebar.locator('[data-component="drive-options-menu"]');
  await expect(menu).toContainText('Detecting AI tools…');
  await expect(menu).not.toContainText('No AI tools detected');
  await page.screenshot({ path: '/private/tmp/hs2-cx4xzr-discovery-retry-wide.png', fullPage: true });
  await expect.poll(() => refreshAttempts).toBe(1);
  await expect(menu.locator('[data-action="select-drive-tool"][data-value="opencode"]')).toBeAttached();
  await expect(menu).not.toContainText('No AI tools detected');
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(menu.locator('[data-action="select-drive-model"][data-value="openai/gpt-5.6"]')).toBeAttached();
  await page.screenshot({ path: '/private/tmp/hs2-cx4xzr-discovery-retry-narrow.png', fullPage: true });
});

test('uses the exact seven-day completion chart beyond retained rows and opens project-scoped statistics', async ({
  page,
}) => {
  const exactTrend = [1, 2, 3, 4, 5, 6, 7];
  let summaryDays: string[] = [];
  await mockProject(page);
  await page.route(/\/tickets(?:\?.*)?$/, (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    summaryDays = new URL(route.request().url()).searchParams.get('summary_days')?.split(',') ?? [];
    return route.fulfill({
      json: {
        items: [row, startedRow2, startedRow3],
        counts: {
          total: 138,
          queued: 97,
          backlog: 13,
          archive: 28,
          open: 103,
          up_next: 15,
          active: 5,
          started: 5,
          completed_today: 7,
          completion_trend: exactTrend,
        },
      },
    });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const summary = page.locator('[data-component="project-summary"]'),
    chart = summary.locator('[role="img"]');
  await expect.poll(() => summaryDays).toHaveLength(8);
  expect(summaryDays).toEqual([...summaryDays].sort());
  await expect(summary).toHaveAccessibleName('Open project statistics: 7 completed today, 5 in progress');
  await expect(chart).toHaveAttribute('aria-label', 'Tickets completed over the last 7 days: 1, 2, 3, 4, 5, 6, 7');
  await expect(summary.locator('[data-zero="false"]')).toHaveCount(7);
  await page.screenshot({ path: '/private/tmp/hs2-q1y8z7-completion-chart-wide.png', fullPage: true });
  await summary.click();
  const shell = page.locator('[data-component="app-shell"]');
  await expect(shell).toHaveAttribute('data-mode', 'stats');
  await expect(page.getByRole('region', { name: 'demo project statistics' })).toContainText(
    'Detailed ticket-flow and usage charts are coming',
  );
  await page.screenshot({ path: '/private/tmp/hs2-y51ehn-project-stats-wide.png', fullPage: true });
  await page.getByRole('tab', { name: 'demo' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'project');
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(summary).toBeVisible();
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.mouse.move(1000, 580);
  await expect(summary).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await page.screenshot({ path: '/private/tmp/hs2-q1y8z7-completion-chart-narrow.png', fullPage: true });
  await summary.click();
  await expect(page.getByRole('region', { name: 'demo project statistics' })).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-y51ehn-project-stats-narrow.png', fullPage: true });
  await page.getByRole('button', { name: 'Cross-project stats' }).click();
  await expect(page.getByRole('heading', { name: 'Cross-project stats' })).toBeVisible();
});

test('keeps the priority select open when opened right after creating a ticket (HS2-43F14D)', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const launcher = page.getByRole('button', { name: 'New ticket…' });
  await launcher.click();
  const form = page.locator('[data-action="create-ticket-form"]');
  await form.locator('wa-input[name="new-ticket-title"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'Race check';
    node.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await form.getByRole('button', { name: 'Create ticket' }).click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]'),
    priority = inspector.locator('wa-select[name="inspector-priority"]');
  await priority.click();
  // The post-creation auto-focus of the details editor must not steal focus and close a popup the
  // user opened within its ~300ms window (HS2-43F14D). Wait past that window and assert it stays open.
  await page.waitForTimeout(450);
  await expect(priority).toHaveJSProperty('open', true);
  await page.screenshot({ path: '/private/tmp/hs2-43f14d-priority-open-after-create.png' });
});

test('focuses the ticket title every time the real composer expands', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const launcher = page.getByRole('button', { name: 'New ticket…' });
  await launcher.click();
  const title = page.locator('wa-input[name="new-ticket-title"]');
  await expect(title).toBeFocused();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await launcher.click();
  await expect(title).toBeFocused();
});

test('stages safe attachment drops from the collapsed and expanded new-ticket composer', async ({ page }) => {
  await mockProject(page);
  const uploads: string[] = [];
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.match(/\/tickets\/02\/attachments$/))
      uploads.push(decodeURIComponent(request.headers()['x-hotsheet-filename'] ?? ''));
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const launcher = page.getByRole('button', { name: 'New ticket…' });
  await launcher.evaluate((node) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['first proof'], 'first-proof.txt', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  await expect(launcher).toHaveAttribute('data-dragging', 'true');
  await launcher.evaluate((node) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['first proof'], 'first-proof.txt', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  const form = page.locator('[data-action="create-ticket-form"]');
  await expect(form.getByText('first-proof.txt')).toBeVisible();
  await form.screenshot({ path: '/private/tmp/hs2-v1xn4t-new-ticket-drop-wide.png' });
  await form.evaluate((node) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['second proof'], 'second-proof.txt', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  await expect(form.getByText('second-proof.txt')).toBeVisible();
  await form.getByRole('button', { name: 'Remove first-proof.txt' }).click();
  await expect(form.getByText('first-proof.txt')).toHaveCount(0);
  await page.setViewportSize({ width: 940, height: 844 });
  await form.screenshot({ path: '/private/tmp/hs2-v1xn4t-new-ticket-drop-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 844 });
  await form.getByRole('button', { name: 'Cancel' }).click();
  await launcher.click();
  await expect(form.getByText('second-proof.txt')).toHaveCount(0);
  await form
    .getByLabel('Browse attachments for new ticket', { exact: true })
    .setInputFiles({ name: 'final-proof.txt', mimeType: 'text/plain', buffer: Buffer.from('final proof') });
  await expect(form.getByText('final-proof.txt')).toBeVisible();
  await form.locator('wa-input[name="new-ticket-title"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'Created with dropped evidence';
    node.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await form.getByRole('button', { name: 'Create ticket' }).click();
  await expect.poll(() => uploads).toEqual(['final-proof.txt']);
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('HS2-NEW001');
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('final-proof.txt');
});

test('generates video posters in the browser for uploads and lazy backfills without blocking unsupported codecs', async ({
  page,
}) => {
  await mockProject(page);
  let attachments = [
    { id: 'VIDEO1', filename: 'legacy.webm', created_at: '2026-08-30T00:40:00Z' },
    { id: 'VIDEO2', filename: 'unsupported.mov', created_at: '2026-08-30T00:41:00Z' },
  ];
  const posters = new Map<string, Buffer>(),
    videoBodies = new Map<string, Buffer>();
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname,
      attachment = path.match(/\/tickets\/01\/attachments\/([^/]+)$/),
      thumbnail = path.match(/\/tickets\/01\/attachments\/([^/]+)\/thumbnail$/);
    if (path.endsWith('/tickets/01') && request.method() === 'GET')
      return route.fulfill({ json: { store: 'git-local', ...full, attachments } });
    if (path.endsWith('/tickets/01/attachments') && request.method() === 'POST') {
      const filename = decodeURIComponent(request.headers()['x-hotsheet-filename'] ?? 'video.webm'),
        id = `VIDEO${attachments.length + 1}`;
      attachments = [...attachments, { id, filename, created_at: '2026-08-30T01:10:00Z' }];
      videoBodies.set(id, request.postDataBuffer() ?? Buffer.alloc(0));
      return route.fulfill({ status: 201, json: { store: 'git-local', ...full, attachments } });
    }
    if (thumbnail) {
      const id = thumbnail[1];
      if (request.method() === 'GET') {
        const poster = posters.get(id);
        return poster
          ? route.fulfill({ contentType: 'image/jpeg', body: poster })
          : route.fulfill({ status: 404, json: { error: 'missing' } });
      }
      if (request.method() === 'PUT') {
        expect(request.headers()['content-type']).toBe('image/jpeg');
        posters.set(id, request.postDataBuffer() ?? Buffer.alloc(0));
        return route.fulfill({ status: 204 });
      }
    }
    if (attachment && request.method() === 'GET') {
      const id = attachment[1],
        body = videoBodies.get(id) ?? Buffer.from('unsupported codec');
      return route.fulfill({ contentType: id === 'VIDEO2' ? 'video/quicktime' : 'video/webm', body });
    }
    return route.fallback();
  });
  await page.goto('/');
  const portableVideo = await page.evaluate(async () => {
    const canvas = document.createElement('canvas');
    canvas.width = 320;
    canvas.height = 180;
    const context = canvas.getContext('2d')!;
    context.fillStyle = '#7857a4';
    context.fillRect(0, 0, 320, 180);
    context.fillStyle = 'white';
    context.font = '24px sans-serif';
    context.fillText('Portable poster', 70, 98);
    const recorder = new MediaRecorder(canvas.captureStream(8), { mimeType: 'video/webm' }),
      chunks: Blob[] = [];
    recorder.ondataavailable = (event) => chunks.push(event.data);
    recorder.start();
    await new Promise((resolve) => setTimeout(resolve, 180));
    const stopped = new Promise((resolve) => {
      recorder.onstop = resolve;
    });
    recorder.stop();
    await stopped;
    return Array.from(new Uint8Array(await new Blob(chunks, { type: 'video/webm' }).arrayBuffer()));
  });
  videoBodies.set('VIDEO1', Buffer.from(portableVideo));
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await expect.poll(() => posters.has('VIDEO1')).toBe(true);
  expect(posters.get('VIDEO1')!.subarray(0, 2)).toEqual(Buffer.from([0xff, 0xd8]));
  const legacy = page.getByRole('button', { name: 'Open legacy.webm in media gallery' }).locator('video');
  await expect.poll(() => legacy.getAttribute('poster')).toMatch(/^blob:/);
  await expect(legacy).not.toHaveAttribute('src', /.+/);
  await page.getByRole('button', { name: 'Open legacy.webm in media gallery' }).click();
  const playableGallery = page.getByRole('dialog', { name: /Video 1 of 3: legacy.webm/ }),
    playableVideo = playableGallery.locator('video');
  await expect(playableVideo).not.toHaveAttribute('poster', /.+/);
  await playableGallery.screenshot({ path: '/private/tmp/hs2-rkhdn8-initial-video-frame.png' });
  await resetRenderMetrics(page);
  const playbackUpdateMs = await playableVideo.evaluate((node) => {
    const started = performance.now();
    for (let index = 0; index < 600; index++) node.dispatchEvent(new Event('timeupdate', { bubbles: false }));
    return performance.now() - started;
  });
  expect(playbackUpdateMs).toBeLessThan(1000);
  const scrubber = playableGallery.getByRole('slider', { name: 'Video position' }),
    scrubUpdateMs = await scrubber.evaluate((node) => {
      const input = node as HTMLInputElement,
        started = performance.now();
      for (let index = 0; index < 300; index++) {
        input.value = String(index % Math.max(1, Number(input.max)));
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }
      return performance.now() - started;
    });
  expect(scrubUpdateMs).toBeLessThan(1000);
  expect((await renderMetrics(page))?.passes).toBe(0);
  await playableGallery.getByRole('button', { name: 'Close video gallery' }).click();
  await expect(playableGallery).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Browse and add attachments' })).toBeEnabled();
  await page.getByRole('button', { name: 'Open unsupported.mov in media gallery' }).click();
  const unsupportedGallery = page.getByRole('dialog', { name: /Video 2 of 3: unsupported.mov/ });
  await expect(unsupportedGallery.locator('video')).not.toHaveAttribute('controls', /.+/);
  await expect(unsupportedGallery.getByRole('button', { name: 'Play' })).toBeVisible();
  await unsupportedGallery.getByRole('button', { name: 'Volume controls' }).click();
  await expect(unsupportedGallery.getByRole('button', { name: 'Mute video' })).toBeVisible();
  await expect(page.locator('.app-error')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(unsupportedGallery).toHaveCount(0);
  await page
    .getByLabel('Browse and add attachments')
    .setInputFiles({ name: 'uploaded.webm', mimeType: 'video/webm', buffer: Buffer.from(portableVideo) });
  await expect.poll(() => posters.has('VIDEO3')).toBe(true);
  expect(posters.get('VIDEO3')!.subarray(0, 2)).toEqual(Buffer.from([0xff, 0xd8]));
  await expect(
    page.getByRole('button', { name: 'Open uploaded.webm in media gallery' }).locator('video'),
  ).toHaveAttribute('poster', /VIDEO3\/thumbnail$/);
});

test('releases video resources and event work after repeated gallery playback cycles', async ({ page }) => {
  test.setTimeout(60_000);
  await mockProject(page);
  const requests: string[] = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.endsWith('/tickets/01/attachments/V1')) requests.push(path);
  });
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: {
            store: 'git-local',
            ...full,
            attachments: [{ id: 'V1', filename: 'walkthrough.mp4', created_at: '2026-08-30T00:40:00Z' }],
          },
        })
      : route.fallback(),
  );
  await page.route('**/tickets/01/attachments/V1', (route) =>
    route.fulfill({ contentType: 'video/mp4', body: 'video fixture' }),
  );
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  const opener = page.getByRole('button', { name: 'Open walkthrough.mp4 in media gallery' });
  for (let cycle = 0; cycle < 3; cycle += 1) {
    await opener.click();
    const gallery = page.getByRole('dialog', { name: /Video 1 of \d+: walkthrough\.mp4/ }),
      video = gallery.locator('video'),
      retained = await video.elementHandle();
    await video.evaluate((element) => {
      const node = element as HTMLVideoElement & { __releaseState?: { loadCalls: number; pauseCalls: number } },
        state = { loadCalls: 0, pauseCalls: 0 };
      let paused = true,
        currentTime = 0,
        frame = 0;
      node.__releaseState = state;
      Object.defineProperties(node, {
        duration: { configurable: true, get: () => 3 },
        paused: { configurable: true, get: () => paused },
        currentTime: {
          configurable: true,
          get: () => currentTime,
          set: (value) => {
            currentTime = Number(value);
            queueMicrotask(() => node.dispatchEvent(new Event('seeked')));
          },
        },
        videoWidth: { configurable: true, get: () => 1280 },
        videoHeight: { configurable: true, get: () => 720 },
        readyState: { configurable: true, get: () => 2 },
      });
      node.requestVideoFrameCallback = (callback) => {
        const id = ++frame;
        queueMicrotask(() => {
          callback(performance.now(), { mediaTime: currentTime } as VideoFrameCallbackMetadata);
        });
        return id;
      };
      node.cancelVideoFrameCallback = () => {};
      node.play = async () => {
        paused = false;
        node.dispatchEvent(new Event('play'));
      };
      node.pause = () => {
        state.pauseCalls += 1;
        paused = true;
        node.dispatchEvent(new Event('pause'));
      };
      node.load = () => {
        state.loadCalls += 1;
      };
      node.dispatchEvent(new Event('loadedmetadata'));
    });
    await gallery.getByRole('button', { name: 'Play' }).click();
    await expect(gallery.getByRole('button', { name: 'Pause' })).toBeVisible();
    const scrubber = gallery.getByRole('slider', { name: 'Video position' });
    await scrubber.fill(String(700 + cycle * 600));
    await resetRenderMetrics(page);
    const updateMs = await video.evaluate((node) => {
      const started = performance.now();
      for (let index = 0; index < 600; index += 1) node.dispatchEvent(new Event('timeupdate'));
      return performance.now() - started;
    });
    expect(updateMs).toBeLessThan(1000);
    expect((await renderMetrics(page))?.passes).toBe(0);
    if (cycle === 2) await gallery.screenshot({ path: '/private/tmp/hs2-3f7n4p-third-playback-cycle.png' });
    await gallery.getByRole('button', { name: 'Close video gallery' }).click();
    await expect(gallery).toHaveCount(0);
    expect(
      await retained.evaluate((node) => {
        const video = node as HTMLVideoElement & { __releaseState?: { loadCalls: number; pauseCalls: number } };
        return {
          connected: video.isConnected,
          paused: video.paused,
          hasSource: video.hasAttribute('src'),
          srcObject: video.srcObject,
          loadCalls: video.__releaseState?.loadCalls,
          pauseCalls: video.__releaseState?.pauseCalls,
        };
      }),
    ).toEqual({ connected: false, paused: true, hasSource: false, srcObject: null, loadCalls: 1, pauseCalls: 1 });
    await resetRenderMetrics(page);
    await retained.evaluate((node) => {
      for (let index = 0; index < 600; index += 1) node.dispatchEvent(new Event('timeupdate', { bubbles: true }));
    });
    await page.waitForTimeout(250);
    expect(await renderMetrics(page)).toEqual({ passes: 0, mutations: 0 });
    const settledRequests = requests.length;
    await page.waitForTimeout(250);
    expect(requests).toHaveLength(settledRequests);
  }
  await page.screenshot({ path: '/private/tmp/hs2-3f7n4p-after-three-cycles-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.screenshot({ path: '/private/tmp/hs2-3f7n4p-after-three-cycles-narrow.png', fullPage: true });
});

test('keeps decoded video seeks responsive across repeated real playback sessions', async ({ page }) => {
  test.setTimeout(90_000);
  await mockProject(page);
  await page.goto('/?dev-review=false');
  const bytes = Buffer.from(
    await page.evaluate(async () => {
      const canvas = document.createElement('canvas');
      canvas.width = 640;
      canvas.height = 360;
      const context = canvas.getContext('2d')!,
        stream = canvas.captureStream(20),
        recorder = new MediaRecorder(stream, { mimeType: 'video/webm' }),
        chunks: Blob[] = [];
      recorder.ondataavailable = (event) => chunks.push(event.data);
      recorder.start();
      for (let frame = 0; frame < 40; frame += 1) {
        context.fillStyle = `hsl(${frame * 9} 70% 38%)`;
        context.fillRect(0, 0, 640, 360);
        context.fillStyle = 'white';
        context.font = 'bold 72px sans-serif';
        context.fillText(String(frame), 250, 210);
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      const stopped = new Promise((resolve) => {
        recorder.onstop = resolve;
      });
      recorder.stop();
      await stopped;
      for (const track of stream.getTracks()) track.stop();
      return Array.from(new Uint8Array(await new Blob(chunks, { type: 'video/webm' }).arrayBuffer()));
    }),
  );
  const requests: Array<{ range?: string }> = [],
    ticket = {
      ...full,
      feedback_needed: false,
      attachments: [{ id: 'V1', filename: 'reliability.webm', created_at: '2026-08-30T00:40:00Z' }],
    };
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { store: 'git-local', ...ticket } }) : route.fallback(),
  );
  await page.route('**/tickets/01/attachments/V1/thumbnail', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="#355070"/><text x="320" y="190" text-anchor="middle" fill="white" font-size="38">Video ready</text></svg>',
    }),
  );
  await page.route('**/tickets/01/attachments/V1', (route) => {
    const rangeHeader = route.request().headers().range;
    requests.push({ range: rangeHeader });
    const headers = { 'accept-ranges': 'bytes', 'content-type': 'video/webm' };
    if (!rangeHeader) return route.fulfill({ body: bytes, headers });
    const match = rangeHeader.match(/^bytes=(\d+)-(\d*)$/);
    if (!match)
      return route.fulfill({ status: 416, headers: { ...headers, 'content-range': `bytes */${bytes.length}` } });
    const start = Number(match[1]),
      end = match[2] ? Math.min(bytes.length - 1, Number(match[2])) : bytes.length - 1;
    return route.fulfill({
      status: 206,
      body: bytes.subarray(start, end + 1),
      headers: {
        ...headers,
        'content-length': String(end - start + 1),
        'content-range': `bytes ${start}-${end}/${bytes.length}`,
      },
    });
  });
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  const opener = page.getByRole('button', { name: 'Open reliability.webm in media gallery' }),
    preview = opener.locator('video');
  await expect(preview).not.toHaveAttribute('src', /.+/);
  for (let cycle = 0; cycle < 3; cycle += 1) {
    await opener.click();
    const gallery = page.getByRole('dialog', { name: /Video 1 of \d+: reliability\.webm/ });
    await expect(gallery).toBeVisible();
    const video = gallery.locator('video'),
      scrubber = gallery.getByRole('slider', { name: 'Video position' });
    await video.evaluate((node) => {
      Object.defineProperty(node, 'duration', { configurable: true, value: 2 });
      node.dispatchEvent(new Event('loadedmetadata'));
    });
    await expect.poll(async () => Number(await scrubber.getAttribute('max'))).toBeGreaterThan(1_500);
    const duration = Number(await scrubber.getAttribute('max')),
      target = Math.round(duration * (cycle % 2 === 0 ? 0.78 : 0.28)),
      presented = video.evaluate(
        (node, targetMs) =>
          new Promise<number>((resolve, reject) => {
            const timeout = window.setTimeout(() => {
                reject(new Error('latest seek frame was not decoded'));
              }, 8_000),
              watch = () => {
                (node as HTMLVideoElement).requestVideoFrameCallback((_now, metadata) => {
                  if (Math.abs(metadata.mediaTime * 1000 - targetMs) < 180) {
                    window.clearTimeout(timeout);
                    resolve(metadata.mediaTime);
                  } else watch();
                });
              };
            watch();
          }),
        target,
      );
    await scrubber.evaluate(
      (node, values) => {
        const input = node as HTMLInputElement;
        for (const value of values) {
          input.value = String(value);
          input.dispatchEvent(new Event('input', { bubbles: true }));
        }
        input.dispatchEvent(new Event('change', { bubbles: true }));
      },
      [Math.round(duration * 0.12), Math.round(duration * 0.55), target],
    );
    expect(Math.abs((await presented) * 1000 - target)).toBeLessThan(180);
    await expect
      .poll(() =>
        video.evaluate((node, value) => Math.abs((node as HTMLVideoElement).currentTime * 1000 - value), target),
      )
      .toBeLessThan(180);
    await gallery.getByRole('button', { name: 'Play' }).click();
    await page.waitForTimeout(120);
    await gallery.getByRole('button', { name: 'Pause' }).click();
    if (cycle === 2) {
      await gallery.screenshot({ path: '/private/tmp/hs2-video-reliability-gallery-wide.png' });
      await page.setViewportSize({ width: 760, height: 640 });
      await gallery.screenshot({ path: '/private/tmp/hs2-video-reliability-gallery-narrow.png' });
    }
    const retained = await video.elementHandle();
    await gallery.getByRole('button', { name: 'Close video gallery' }).click();
    await expect(gallery).toHaveCount(0);
    expect(
      await retained.evaluate((node) => ({
        connected: node.isConnected,
        src: node.hasAttribute('src'),
        source: (node as HTMLVideoElement).srcObject,
      })),
    ).toEqual({ connected: false, src: false, source: null });
    await expect(preview).not.toHaveAttribute('src', /.+/);
  }
  const settled = requests.length;
  await page.waitForTimeout(300);
  expect(requests).toHaveLength(settled);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.screenshot({ path: '/private/tmp/hs2-video-reliability-after-cycles-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await page.screenshot({ path: '/private/tmp/hs2-video-reliability-after-cycles-narrow.png', fullPage: true });
});

test('restores project-scoped ticket creation and staged files after reload', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Create ticket' });
  await dialog.getByRole('textbox', { name: 'Ticket title' }).fill('A ticket draft that survives refresh');
  await dialog.getByRole('textbox', { name: 'Details' }).fill('Draft **Markdown** details');
  await dialog
    .getByLabel('Browse attachments for new ticket', { exact: true })
    .setInputFiles({ name: 'restored-proof.txt', mimeType: 'text/plain', buffer: Buffer.from('persisted evidence') });
  await expect(dialog.getByText('restored-proof.txt')).toBeVisible();
  await page.waitForTimeout(900);
  await page.reload();
  const host = page.locator('[data-component="quick-ticket-composer"]');
  await expect(host).toBeHidden();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const restoredForm = page.locator('[data-action="create-ticket-form"]');
  await expect(restoredForm).toBeVisible();
  await expect(dialog.getByRole('textbox', { name: 'Ticket title' })).toHaveValue(
    'A ticket draft that survives refresh',
  );
  await expect(dialog.getByRole('textbox', { name: 'Details' })).toHaveValue('Draft **Markdown** details');
  await expect(dialog.getByText('restored-proof.txt')).toBeVisible();
  await restoredForm.screenshot({ path: '/private/tmp/hs2-3qxhf3-restored-ticket-draft.png' });
  await dialog.getByRole('button', { name: 'Cancel' }).click();
});

test('keeps Markdown attachment images in flow and reports reference copies with a toast', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: () => Promise.resolve() },
    });
  });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.evaluate((node: HTMLElement) => {
    node.style.width = '250px';
  });
  const markdownImage = inspector.locator('.markdown-preview__attachment-image'),
    followingText = inspector.getByText('Following content after image.');
  await expect(markdownImage).toBeVisible();
  await expect(markdownImage.locator('img')).toHaveAttribute('src', /\/attachments\/A1$/);
  expect(
    await markdownImage.locator('img').evaluate((node) => (node as HTMLImageElement).naturalWidth),
  ).toBeGreaterThan(0);
  await expect(followingText).toBeVisible();
  const imageBox = (await markdownImage.boundingBox())!,
    followingBox = (await followingText.boundingBox())!;
  expect(followingBox.y).toBeGreaterThanOrEqual(imageBox.y + imageBox.height);
  await markdownImage
    .locator('xpath=ancestor::article')
    .screenshot({ path: '/private/tmp/hs2-xane1p-direct-attachment-reference.png' });
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await page.getByRole('button', { name: 'More actions for proof.png' }).click();
  await page
    .getByRole('menu', { name: 'Attachment actions' })
    .getByRole('menuitem', { name: 'Copy reference' })
    .click();
  await expect(page.locator('.app-toast')).toContainText('Attachment reference copied to clipboard.');
  await expect(page.locator('.ticket-attachments__status')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-859hq1-attachment-copy-toast.png', fullPage: true });
});

test('shows the attachment actions menu above the modal ticket reader (HS2-EZ10RS)', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: () => Promise.resolve() },
    });
  });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').dblclick();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ });
  await expect(reader).toBeVisible();
  expect(
    await reader.evaluate((host) => (host.shadowRoot!.querySelector('dialog') as HTMLDialogElement).matches(':modal')),
  ).toBe(true);
  await reader.getByRole('tab', { name: /Attachments/ }).click();
  await reader.getByRole('button', { name: 'More actions for proof.png' }).click();
  const menu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(menu).toBeVisible();
  // The menu renders as a descendant of the modal reader dialog, so it is on top AND interactive
  // (the reader's modality would otherwise leave an out-of-dialog menu inert/covered, HS2-EZ10RS).
  await expect(menu).toHaveCount(1);
  const dialog = page.locator('[data-component="ticket-reader"][data-reader-active="true"]');
  expect(await menu.evaluate((el, d) => d.contains(el), await dialog.elementHandle())).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-ez10rs-reader-attachment-menu.png', fullPage: true });
  // A menu item is clickable and performs its action above the modal reader.
  await menu.getByRole('menuitem', { name: 'Copy reference' }).click();
  await expect(menu).toBeHidden();
  await expect(page.locator('.app-toast')).toContainText('Attachment reference copied to clipboard.');
});

test('suppresses background app shortcuts while a modal dialog is open (HS2-FW4PYZ)', async ({ page }) => {
  await mockProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const searchField = page.getByRole('searchbox', { name: 'Search tickets' });
  // Open the modal ticket reader, then the Open-search shortcut must NOT reach the background search.
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').dblclick();
  const reader = page.locator('[data-component="ticket-reader"][data-reader-active="true"]');
  await expect(reader).toBeVisible();
  await page.keyboard.press('ControlOrMeta+k');
  await expect(searchField).toHaveCount(0);
  await expect(reader).toBeVisible();
  // Close the modal; once no modal remains the same shortcut opens search (it is only suppressed under a modal).
  await page.keyboard.press('Escape');
  await expect(reader).toBeHidden();
  await expect.poll(() => page.evaluate(() => !document.querySelector('wa-dialog[open], dialog:modal'))).toBe(true);
  await page.keyboard.press('ControlOrMeta+k');
  await expect(searchField).toBeVisible();
});

test('renders canonical attachment references for filenames containing backticks', async ({ page }) => {
  await mockProject(page);
  const filename = 'proof`quote.txt',
    ticket = {
      ...full,
      details: 'Backtick evidence is attached below.',
      notes: [
        {
          id: 'N-BACKTICK',
          kind: 'regular' as const,
          created_at: '2026-08-30T00:39:00Z',
          edited_at: '2026-08-30T00:39:00Z',
          text: 'Canonical reference: ``attachment:proof`quote.txt``',
        },
      ],
      attachments: [{ id: 'A-BACKTICK', filename, created_at: '2026-08-30T00:40:00Z' }],
    };
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { store: 'git-local', ...ticket } }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]'),
    reference = inspector.getByRole('link', { name: filename });
  await expect(reference).toBeVisible();
  await expect(reference).toHaveAttribute('href', /\/attachments\/A-BACKTICK$/);
  await inspector.screenshot({ path: '/private/tmp/hs2-h2ptvz-backtick-reference-wide.png' });
  await page.setViewportSize({ width: 760, height: 700 });
  await expect(reference).toBeVisible();
  await inspector.screenshot({ path: '/private/tmp/hs2-h2ptvz-backtick-reference-narrow.png' });
});

test('draws, edits, resizes, and deletes durable image annotations in the full-screen gallery', async ({ page }) => {
  const writes: unknown[] = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'PUT' && new URL(request.url()).pathname.includes('/attachments/'))
      writes.push(request.postDataJSON());
  });
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await page.getByRole('button', { name: 'Open proof.png in media gallery' }).click();
  const gallery = page.getByRole('dialog', { name: /Image 1 of 1: proof.png/ });
  await gallery.getByRole('button', { name: 'Annotate media' }).click();
  await gallery.getByRole('button', { name: 'Add rectangle' }).click();
  const surface = gallery.locator('[data-gallery-annotation-surface="true"]'),
    box = (await surface.boundingBox())!;
  page.once('dialog', (dialog) => dialog.accept('Check the selected region'));
  await page.mouse.move(box.x + box.width * 0.15, box.y + box.height * 0.2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height * 0.55);
  await page.mouse.up();
  const annotation = gallery.getByRole('button', { name: /Annotation 1: Check the selected region/ });
  await expect(annotation).toBeVisible();
  await expect.poll(() => writes.length).toBe(0);
  const before = await annotation.boundingBox(),
    handle = annotation.locator('[data-annotation-handle="se"]');
  await handle.dragTo(surface, { targetPosition: { x: box.width * 0.65, y: box.height * 0.7 } });
  const after = await annotation.boundingBox();
  expect(after!.width).toBeGreaterThan(before!.width);
  page.once('dialog', (dialog) => dialog.accept('Updated annotation'));
  await annotation.locator('.attachment-gallery__annotation-label').dblclick();
  const updatedAnnotation = gallery.getByRole('button', { name: /Annotation 1: Updated annotation/ });
  await expect(updatedAnnotation).toBeVisible();
  await expect.poll(() => writes.length).toBe(0);
  await gallery.screenshot({ path: '/private/tmp/hs2-grgdze-annotation-batch-wide.png' });
  await gallery.getByRole('button', { name: 'Finish markup' }).click();
  await expect.poll(() => writes.length).toBe(1);
  await expect(page.locator('.app-toast')).toContainText('Annotations saved.');
  await gallery.getByRole('button', { name: 'Annotate media' }).click();
  await updatedAnnotation.click();
  page.once('dialog', (dialog) => dialog.accept());
  await gallery.getByRole('button', { name: 'Erase selected annotation' }).click();
  await expect(gallery.locator('.attachment-gallery__annotation')).toHaveCount(0);
  await expect.poll(() => writes.length).toBe(1);
  await gallery.getByRole('button', { name: 'Close image gallery' }).click();
  await expect.poll(() => writes.length).toBe(2);
  await page.getByRole('tab', { name: 'Info' }).click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]'),
    activity = inspector.locator('article').filter({ hasText: 'Annotations changed for' }).last();
  await expect(activity).toBeVisible();
  await expect(activity.getByRole('link', { name: 'attachment:proof.png' })).toBeVisible();
  await expect(activity.getByRole('listitem')).toContainText('Removed');
  await activity.scrollIntoViewIfNeeded();
  await inspector.screenshot({ path: '/private/tmp/hs2-grgdze-annotation-note-wide.png' });
  await page.setViewportSize({ width: 940, height: 844 });
  await page.getByRole('button', { name: 'Show ticket inspector' }).click();
  await activity.scrollIntoViewIfNeeded();
  await inspector.screenshot({ path: '/private/tmp/hs2-grgdze-annotation-note-narrow.png' });
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await page.getByRole('button', { name: 'Open proof.png in media gallery' }).click();
  await gallery.getByRole('button', { name: 'Annotate media' }).click();
  await gallery.getByRole('button', { name: 'Finish markup' }).click();
  await page.waitForTimeout(200);
  expect(writes).toHaveLength(2);
});

test('scrubs video without swiping and persists timed-annotation interaction boundaries', async ({ page }) => {
  test.setTimeout(60_000);
  await mockProject(page);
  const reportedVideoPath = process.env.HOTSHEET_MEDIA_TEST_VIDEO,
    reportedVideo = reportedVideoPath && existsSync(reportedVideoPath) ? readFileSync(reportedVideoPath) : undefined,
    videoName = reportedVideo && reportedVideoPath ? basename(reportedVideoPath) : 'walkthrough.mp4',
    annotationWrites: Array<{ annotations: MediaAnnotation[] }> = [],
    initialAnnotations: MediaAnnotation[] = [
      { id: 'first', x: 1200, y: 6000, width: 2800, height: 2200, start_ms: 400, end_ms: 600, text: 'First range' },
      { id: 'second', x: 6000, y: 6000, width: 2400, height: 2200, start_ms: 2300, end_ms: 2500, text: 'Second range' },
    ];
  let videoTicket = {
    ...full,
    feedback_needed: false,
    attachments: [
      { id: 'V1', filename: videoName, created_at: '2026-08-30T00:40:00Z', annotations: initialAnnotations },
      { id: 'A1', filename: 'proof.png', created_at: '2026-08-30T00:41:00Z' },
    ],
  };
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { store: 'git-local', ...videoTicket } })
      : route.fallback(),
  );
  await page.route('**/tickets/01/attachments/V1', (route) => {
    if (route.request().method() === 'GET') {
      const headers = {
          'accept-ranges': 'bytes',
          'content-type': reportedVideo ? 'video/quicktime' : 'video/mp4',
          'x-hotsheet-filename': videoName,
        },
        rangeHeader = route.request().headers().range,
        range = reportedVideo && rangeHeader ? rangeHeader.match(/^bytes=(\d+)-(\d*)$/) : undefined;
      if (reportedVideo && range) {
        const start = Number(range[1]),
          end = range[2] ? Math.min(reportedVideo.length - 1, Number(range[2])) : reportedVideo.length - 1;
        return route.fulfill({
          status: 206,
          body: reportedVideo.subarray(start, end + 1),
          headers: {
            ...headers,
            'content-length': String(end - start + 1),
            'content-range': `bytes ${start}-${end}/${reportedVideo.length}`,
          },
        });
      }
      return route.fulfill({ body: reportedVideo ?? 'video fixture', headers });
    }
    if (route.request().method() === 'PUT') {
      const body = route.request().postDataJSON() as { annotations: MediaAnnotation[] };
      annotationWrites.push(body);
      videoTicket = {
        ...videoTicket,
        attachments: videoTicket.attachments.map((item) =>
          item.id === 'V1' ? { ...item, annotations: body.annotations } : item,
        ),
      };
      return route.fulfill({ json: { store: 'git-local', ...videoTicket } });
    }
    return route.fallback();
  });
  await page.route('**/tickets/01/attachments/V1/thumbnail', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><defs><linearGradient id="g" x2="1" y2="1"><stop stop-color="#211d35"/><stop offset="1" stop-color="#59407c"/></linearGradient></defs><rect width="1280" height="720" fill="url(#g)"/><rect x="96" y="82" width="1088" height="556" rx="24" fill="#ffffff" opacity=".08"/><text x="640" y="330" text-anchor="middle" font-family="sans-serif" font-size="58" fill="white">Video annotation review</text><text x="640" y="400" text-anchor="middle" font-family="sans-serif" font-size="30" fill="#d7c9ef">Real pointer scrub · selected five-percent range</text></svg>',
    }),
  );
  const prepareVideo = async (locator: Locator) => {
    if (reportedVideo) {
      await expect
        .poll(
          () =>
            locator.evaluate((element) =>
              Number.isFinite((element as HTMLVideoElement).duration) ? (element as HTMLVideoElement).duration : 0,
            ),
          { timeout: 15_000 },
        )
        .toBeGreaterThan(3);
      return;
    }
    await locator.evaluate((element) => {
      const node = element as HTMLVideoElement;
      let paused = true,
        currentTime = 0,
        frame = 0;
      Object.defineProperties(node, {
        duration: { configurable: true, get: () => 3 },
        paused: { configurable: true, get: () => paused },
        currentTime: {
          configurable: true,
          get: () => currentTime,
          set: (value) => {
            currentTime = Number(value);
            queueMicrotask(() => node.dispatchEvent(new Event('seeked')));
          },
        },
        videoWidth: { configurable: true, get: () => 1280 },
        videoHeight: { configurable: true, get: () => 720 },
        readyState: { configurable: true, get: () => 2 },
      });
      node.requestVideoFrameCallback = (callback) => {
        const id = ++frame;
        queueMicrotask(() => {
          callback(performance.now(), { mediaTime: currentTime } as VideoFrameCallbackMetadata);
        });
        return id;
      };
      node.cancelVideoFrameCallback = () => {};
      node.play = async () => {
        paused = false;
        node.dispatchEvent(new Event('play'));
      };
      node.pause = () => {
        paused = true;
        node.dispatchEvent(new Event('pause'));
      };
      node.dispatchEvent(new Event('loadedmetadata'));
    });
  };
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await page.getByRole('button', { name: `Open ${videoName} in media gallery, 2 annotations` }).click();
  let gallery = page.getByRole('dialog', { name: new RegExp(`Video 1 of 2: ${videoName.replace('.', '\\.')}`) }),
    video = gallery.locator('video');
  await prepareVideo(video);
  await expect(video).not.toHaveAttribute('poster', /.+/);
  await expect(video).toHaveAttribute('preload', 'auto');
  await expect(video).not.toHaveAttribute('src', /#t=/);
  await expect.poll(() => video.evaluate((node) => (node as HTMLVideoElement).paused)).toBe(true);
  const scrubber = gallery.getByRole('slider', { name: 'Video position' });
  await expect.poll(async () => Number(await scrubber.getAttribute('max'))).toBeGreaterThan(2500);
  const durationMs = Number(await scrubber.getAttribute('max'));
  if (reportedVideo) {
    await expect.poll(() => video.evaluate((node) => (node as HTMLVideoElement).readyState)).toBeGreaterThanOrEqual(2);
    const firstFrame = await video.screenshot();
    for (const fraction of [0.9, 0.2, 0.72, 0.35]) {
      const targetMs = Math.round(durationMs * fraction),
        presented = video.evaluate(
          (node, target) =>
            new Promise<number>((resolve, reject) => {
              const seen: number[] = [],
                timeout = window.setTimeout(() => {
                  reject(new Error(`paused seek did not present the requested decoded frame; saw ${seen.join(', ')}`));
                }, 5000),
                watch = () => {
                  (node as HTMLVideoElement).requestVideoFrameCallback((_now, metadata) => {
                    seen.push(metadata.mediaTime);
                    if (Math.abs(metadata.mediaTime * 1000 - target) < 150) {
                      window.clearTimeout(timeout);
                      resolve(metadata.mediaTime);
                    } else watch();
                  });
                };
              watch();
            }),
          targetMs,
        );
      await scrubber.fill(String(targetMs));
      expect(Math.abs((await presented) * 1000 - targetMs)).toBeLessThan(150);
    }
    const soughtFrame = await video.screenshot();
    expect(Buffer.compare(firstFrame, soughtFrame)).not.toBe(0);
  }
  const scrubberBox = (await scrubber.boundingBox())!;
  await page.mouse.move(scrubberBox.x + 8, scrubberBox.y + scrubberBox.height / 2);
  await page.mouse.down();
  let heldFrameA: Buffer | undefined;
  if (reportedVideo) {
    const decoded = video.evaluate(
      (node) =>
        new Promise<number>((resolve) => {
          (node as HTMLVideoElement).requestVideoFrameCallback((_now, metadata) => {
            resolve(metadata.mediaTime);
          });
        }),
    );
    await page.mouse.move(scrubberBox.x + scrubberBox.width * 0.2, scrubberBox.y + scrubberBox.height / 2, {
      steps: 8,
    });
    const playhead = Number(await scrubber.inputValue());
    expect(Math.abs((await decoded) * 1000 - playhead)).toBeLessThan(180);
    heldFrameA = await video.screenshot();
    const nextDecoded = video.evaluate(
      (node) =>
        new Promise<number>((resolve) => {
          (node as HTMLVideoElement).requestVideoFrameCallback((_now, metadata) => {
            resolve(metadata.mediaTime);
          });
        }),
    );
    await page.mouse.move(scrubberBox.x + scrubberBox.width * 0.8, scrubberBox.y + scrubberBox.height / 2, {
      steps: 8,
    });
    const nextPlayhead = Number(await scrubber.inputValue());
    expect(Math.abs((await nextDecoded) * 1000 - nextPlayhead)).toBeLessThan(180);
    expect(Buffer.compare(heldFrameA, await video.screenshot())).not.toBe(0);
    await gallery.screenshot({ path: '/private/tmp/hs2-ewztq9-native-held-scrub-wide.png' });
  }
  await page.mouse.move(scrubberBox.x + scrubberBox.width * 0.5, scrubberBox.y + scrubberBox.height / 2, { steps: 8 });
  await page.mouse.up();
  await expect(gallery).toHaveAccessibleName(new RegExp(`Video 1 of 2: ${videoName.replace('.', '\\.')}`));
  const scrubbedPlayhead = Number(await scrubber.inputValue());
  expect(scrubbedPlayhead).toBeGreaterThan(durationMs * 0.35);
  expect(scrubbedPlayhead).toBeLessThan(durationMs * 0.65);
  await expect
    .poll(() =>
      video.evaluate(
        (element, value) => Math.abs((element as HTMLVideoElement).currentTime * 1000 - value),
        scrubbedPlayhead,
      ),
    )
    .toBeLessThan(150);
  await expect.poll(() => video.evaluate((element) => (element as HTMLVideoElement).paused)).toBe(true);
  const stage = gallery.getByLabel(/Video canvas/);
  await stage.focus();
  await page.keyboard.press('Space');
  await expect(gallery.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.waitForTimeout(300);
  await page.keyboard.press('Space');
  await expect(gallery.getByRole('button', { name: 'Play' })).toBeVisible();
  const beforeFrame = Number(await scrubber.inputValue());
  await page.keyboard.press('ArrowRight');
  await expect(scrubber).toHaveValue(String(Math.min(durationMs, beforeFrame + 33)));
  await page.keyboard.press('Shift+ArrowRight');
  const coarsePlayhead = Math.min(durationMs, beforeFrame + 1033);
  await expect(scrubber).toHaveValue(String(coarsePlayhead));
  await page.keyboard.press('j');
  await expect(scrubber).toHaveValue(String(Math.max(0, coarsePlayhead - 1000)));
  await scrubber.fill('1500');
  await gallery.getByRole('button', { name: 'Annotate media, 2 annotations' }).click();
  const first = gallery.getByRole('button', { name: 'Annotation 1: First range' }),
    second = gallery.locator('.attachment-gallery__annotation[data-annotation-id="second"]'),
    firstTimeline = gallery.locator('.attachment-gallery__timeline-annotation[data-annotation-id="first"]');
  expect(
    await firstTimeline.evaluate((node) => Number.parseFloat(getComputedStyle(node).width)),
  ).toBeGreaterThanOrEqual(24);
  await first.click();
  await expect(gallery.locator('[data-gallery-range-handle]')).toHaveCount(2);
  await gallery.getByRole('button', { name: /Annotation range start/ }).press('ArrowRight');
  await expect(first).toHaveAttribute('data-annotation-start', '500');
  await expect(second).toHaveAttribute('data-annotation-start', '2300');
  const surface = gallery.locator('[data-gallery-annotation-surface="true"]'),
    surfaceBox = (await surface.boundingBox())!;
  await surface.click({ position: { x: surfaceBox.width * 0.05, y: surfaceBox.height * 0.05 } });
  await expect(gallery.locator('[data-gallery-range-handle]')).toHaveCount(0);
  await expect(firstTimeline).toHaveAttribute('data-selected', 'false');
  expect(
    await firstTimeline.evaluate((node) => Number.parseFloat(getComputedStyle(node).width)),
  ).toBeGreaterThanOrEqual(24);
  await scrubber.fill('1601');
  await expect(first).toBeHidden();
  await scrubber.fill('1600');
  await expect(first).toBeVisible();
  const creationPlayhead = Math.round(durationMs / 2);
  await scrubber.fill(String(creationPlayhead));
  await gallery.getByRole('button', { name: 'Add rectangle' }).click();
  page.once('dialog', (dialog) => dialog.accept('Real pointer annotation'));
  await page.mouse.move(surfaceBox.x + surfaceBox.width * 0.1, surfaceBox.y + surfaceBox.height * 0.1);
  await page.mouse.down();
  await page.mouse.move(surfaceBox.x + surfaceBox.width * 0.4, surfaceBox.y + surfaceBox.height * 0.3, { steps: 4 });
  await page.mouse.up();
  let created = gallery.locator('.attachment-gallery__annotation').filter({ hasText: 'Real pointer annotation' });
  const expectedStart = Math.round(creationPlayhead - durationMs * 0.05);
  let expectedEnd = Math.round(creationPlayhead + durationMs * 0.05);
  await expect(created).toHaveAttribute('data-annotation-start', String(expectedStart));
  await expect
    .poll(() => created.getAttribute('data-annotation-end').then((value) => Math.abs(Number(value) - expectedEnd)))
    .toBeLessThanOrEqual(3);
  const timelineBox = (await gallery.locator('.attachment-gallery__timeline-track').boundingBox())!,
    endHandle = gallery.getByRole('button', { name: /Annotation range end/ });
  await endHandle.hover();
  await page.mouse.down();
  await page.mouse.move(timelineBox.x + timelineBox.width * 0.75, timelineBox.y + timelineBox.height / 2, { steps: 4 });
  await page.mouse.up();
  expectedEnd = Math.round(durationMs * 0.75);
  await expect
    .poll(() => created.getAttribute('data-annotation-end').then((value) => Math.abs(Number(value) - expectedEnd)))
    .toBeLessThanOrEqual(3);
  await gallery.getByRole('button', { name: 'Finish markup, 3 annotations' }).click();
  await expect.poll(() => annotationWrites.length).toBe(1);
  expect(annotationWrites[0].annotations).toHaveLength(3);
  await gallery.getByRole('button', { name: 'Close video gallery' }).click();
  await page.getByRole('button', { name: `Open ${videoName} in media gallery, 3 annotations` }).click();
  gallery = page.getByRole('dialog', { name: new RegExp(`Video 1 of 2: ${videoName.replace('.', '\\.')}`) });
  video = gallery.locator('video');
  await prepareVideo(video);
  await gallery.getByRole('button', { name: 'Annotate media, 3 annotations' }).click();
  await gallery.getByRole('button', { name: /Annotation 3 at/ }).click();
  created = gallery.locator('.attachment-gallery__annotation').filter({ hasText: 'Real pointer annotation' });
  await expect(created).toBeVisible();
  await expect(gallery.locator('[data-gallery-range-handle]')).toHaveCount(2);
  await gallery.screenshot({ path: '/private/tmp/hs2-ewztq9-ppjape-regression-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(created).toBeVisible();
  await gallery.screenshot({ path: '/private/tmp/hs2-ewztq9-ppjape-regression-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await gallery.getByRole('button', { name: 'Finish markup, 3 annotations' }).click();
  const swipeStage = gallery.getByLabel(/Video canvas/),
    stageBox = (await swipeStage.boundingBox())!;
  await page.mouse.move(stageBox.x + stageBox.width * 0.75, stageBox.y + stageBox.height * 0.15);
  await page.mouse.down();
  await page.mouse.move(stageBox.x + stageBox.width * 0.55, stageBox.y + stageBox.height * 0.15, { steps: 5 });
  await page.mouse.up();
  await expect(page.getByRole('dialog', { name: /Image 2 of 2: proof\.png/ })).toBeVisible();
});

test('links ticket references in details and notes and layers the referenced ticket', async ({ page }) => {
  await mockProject(page);
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: {
            store: 'git-local',
            ...full,
            details: 'Continue with HS2-BACK01 after this ticket.',
            notes: [
              ...full.notes,
              {
                id: 'N-link',
                kind: 'regular',
                created_at: '2026-08-30T00:38:00Z',
                edited_at: '2026-08-30T00:38:00Z',
                text: 'Archive context lives in HS2-ARCH01; keep `HS2-CODE12` literal.',
              },
            ],
          },
        })
      : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  const backlogLink = inspector.getByRole('link', { name: 'HS2-BACK01' }),
    archiveLink = inspector.getByRole('link', { name: 'HS2-ARCH01' });
  await expect(backlogLink).toBeVisible();
  await expect(archiveLink).toBeVisible();
  await expect(inspector.getByText('HS2-CODE12')).toHaveCount(1);
  await inspector.screenshot({ path: '/private/tmp/hs2-bd09b6-ticket-links.png' });
  await backlogLink.click();
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-BACK01 in demo' })).toBeVisible();
  await expect(inspector).toHaveAttribute('data-ticket-slug', 'HS2-DEMO01');
});

test('renders attachment identity from a selected real ticket', async ({ page }) => {
  const hostActions: string[] = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/action'))
      hostActions.push(request.postData() ?? '');
  });
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Use real project tickets');
  await page.locator('[data-component="ticket-inspector"]').evaluate((node: HTMLElement) => {
    node.style.width = '250px';
  });
  await expect(
    page.getByRole('tab', { name: /Attachments/ }).locator('.ticket-inspector__tab-count [aria-hidden="true"]'),
  ).toHaveText('1');
  await page.getByRole('tab', { name: /Attachments/ }).click();
  const item = page.locator('[data-attachment-id="A1"]');
  await expect(item).toContainText('proof.png');
  const more = item.getByRole('button', { name: 'More actions for proof.png' });
  await expect(more).toHaveAttribute('title', 'More actions for proof.png');
  await expect(more.locator('[data-lucide="more-horizontal"]')).toBeVisible();
  await expect(item.getByRole('button')).toHaveCount(1);
  await more.click();
  let attachmentMenu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(attachmentMenu.getByRole('menuitem')).toHaveCount(6);
  await expect(attachmentMenu.getByRole('menuitem').allTextContents()).resolves.toEqual([
    'Open',
    'Download',
    'Copy reference',
    'Rename',
    'Show in Finder',
    'Remove',
  ]);
  await attachmentMenu.getByRole('menuitem', { name: 'Show in Finder' }).click();
  await expect.poll(() => hostActions.filter((action) => action.includes('"reveal"')).length).toBe(1);
  await more.click();
  await attachmentMenu.getByRole('menuitem', { name: 'Open' }).click();
  await item.dblclick({ position: { x: 40, y: 20 } });
  await expect.poll(() => hostActions.filter((action) => action.includes('"open"')).length).toBe(2);
  const thumbnail = page.getByRole('button', { name: 'Open proof.png in media gallery' });
  await expect(thumbnail).toBeVisible();
  const thumbnailBox = await thumbnail.boundingBox();
  expect(thumbnailBox?.width).toBe(thumbnailBox?.height);
  await thumbnail.click();
  const gallery = page.getByRole('dialog', { name: /Image 1 of 1: proof.png/ });
  await expect(gallery).toBeVisible();
  expect(
    await gallery.locator('[data-gallery-image="true"]').evaluate((node) => (node as HTMLImageElement).naturalWidth),
  ).toBeGreaterThan(0);
  await gallery.screenshot({ path: '/private/tmp/hs2-64651d-gallery-wide.png' });
  await page.keyboard.press('Escape');
  await expect(gallery).toHaveCount(0);
  await page.locator('[data-component="ticket-inspector"]').getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ });
  await expect(reader.locator('dialog:modal')).toHaveCount(1);
  await reader.locator('.markdown-preview__attachment-image').click();
  await expect(gallery).toBeVisible();
  await gallery.getByRole('button', { name: 'More image actions' }).click();
  await expect(page.getByRole('menu')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-wqe7br-reader-gallery-after.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(gallery).toHaveCount(0);
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(reader).toHaveCount(0);
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await thumbnail.evaluate((node) =>
    node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 1278, clientY: 718 })),
  );
  attachmentMenu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(attachmentMenu.getByRole('menuitem', { name: 'Show in Finder' })).toBeVisible();
  const menuBounds = (await attachmentMenu.boundingBox())!;
  expect(menuBounds.x).toBeGreaterThanOrEqual(8);
  expect(menuBounds.y).toBeGreaterThanOrEqual(8);
  expect(menuBounds.x + menuBounds.width).toBeLessThanOrEqual(1272);
  expect(menuBounds.y + menuBounds.height).toBeLessThanOrEqual(716);
  await page.screenshot({ path: '/private/tmp/hs2-attachment-context-menu-viewport-safe.png', fullPage: true });
  await page.keyboard.press('Escape');
  await page.getByLabel('Browse and add attachments').setInputFiles({
    name: 'second.svg',
    mimeType: 'image/svg+xml',
    buffer: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>'),
  });
  await expect(page.locator('[data-attachment-id="A2"]')).toContainText('second.svg');
  await thumbnail.click();
  const twoImageGallery = page.locator('[data-component="attachment-gallery"]');
  await expect(twoImageGallery).toHaveAccessibleName(/Image 1 of 2: proof.png/);
  await page.keyboard.press('ArrowRight');
  await expect(twoImageGallery).toHaveAccessibleName(/Image 2 of 2: second.svg/);
  await expect(twoImageGallery.locator('[data-gallery-image="true"]')).toHaveAttribute(
    'data-gallery-attachment-id',
    'A2',
  );
  await twoImageGallery.getByRole('button', { name: 'Previous image' }).click();
  await expect(twoImageGallery).toHaveAccessibleName(/Image 1 of 2: proof.png/);
  await expect(twoImageGallery.locator('[data-gallery-image="true"]')).toHaveAttribute(
    'data-gallery-attachment-id',
    'A1',
  );
  await twoImageGallery.getByRole('button', { name: 'Next image' }).click();
  await expect(twoImageGallery).toHaveAccessibleName(/Image 2 of 2: second.svg/);
  await expect(twoImageGallery.locator('[data-gallery-image="true"]')).toHaveAttribute(
    'data-gallery-attachment-id',
    'A2',
  );
  await twoImageGallery.screenshot({ path: '/private/tmp/hs2-4r31nb-gallery-navigation-wide.png' });
  await page.setViewportSize({ width: 940, height: 844 });
  await page.keyboard.press('ArrowLeft');
  await expect(twoImageGallery).toHaveAccessibleName(/Image 1 of 2: proof.png/);
  await expect(twoImageGallery.locator('[data-gallery-image="true"]')).toHaveAttribute(
    'data-gallery-attachment-id',
    'A1',
  );
  await twoImageGallery.screenshot({ path: '/private/tmp/hs2-4r31nb-gallery-navigation-narrow.png' });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.getByRole('tab', { name: 'Info' }).click();
  const noteGalleryImage = page.locator('.markdown-preview__attachment-image');
  await noteGalleryImage.click();
  await expect(twoImageGallery).toHaveAccessibleName(/Image 1 of 2: proof.png/);
  await page.screenshot({ path: '/private/tmp/hs2-note-reference-gallery-navigation.png', fullPage: true });
  await page.keyboard.press('Escape');
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await page
    .locator('[data-component="ticket-inspector"]')
    .getByRole('heading', { name: 'Attachments' })
    .scrollIntoViewIfNeeded();
  await item.click({ button: 'right' });
  attachmentMenu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(attachmentMenu.getByRole('menuitem')).toHaveCount(6);
  await expect(attachmentMenu.getByRole('menuitem', { name: 'Show in Finder' })).toBeVisible();
  await page
    .locator('[data-component="ticket-inspector"]')
    .screenshot({ path: '/private/tmp/hs2-vqvf5b-attachment-menu-wide.png' });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 940, height: 844 });
  await page.getByRole('button', { name: 'Show ticket inspector' }).click();
  await item.scrollIntoViewIfNeeded();
  await more.click();
  await expect(attachmentMenu.getByRole('menuitem', { name: 'Show in Finder' })).toBeVisible();
  await page
    .locator('[data-component="ticket-inspector"]')
    .screenshot({ path: '/private/tmp/hs2-vqvf5b-attachment-menu-narrow.png' });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.locator('[data-component="ticket-inspector"]').evaluate((node: HTMLElement) => {
    node.style.width = '250px';
  });
  const inspector = await page.locator('[data-component="ticket-inspector"]').elementHandle();
  const attachmentLayout = await item.evaluate((node, container) => {
    const filename = node.querySelector<HTMLElement>(':scope > span')!;
    filename.textContent = 'an-extremely-long-attachment-filename-that-must-ellipsis-before-the-actions.png';
    return {
      contained: node.getBoundingClientRect().right <= container.getBoundingClientRect().right,
      filenameOverflows: filename.scrollWidth > filename.clientWidth,
    };
  }, inspector);
  expect(attachmentLayout).toEqual({ contained: true, filenameOverflows: true });
  await expect(item.getByRole('button')).toHaveCount(1);
  await page
    .getByLabel('Browse and add attachments')
    .setInputFiles({ name: 'new-proof.txt', mimeType: 'text/plain', buffer: Buffer.from('proof') });
  await expect(page.locator('[data-attachment-id="A3"]')).toContainText('new-proof.txt');
  await page.locator('[data-component="ticket-inspector"]').evaluate((node) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['drop proof'], 'dropped-proof.txt', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: transfer }));
  });
  await expect(page.locator('[data-attachment-id="A4"]')).toContainText('dropped-proof.txt');
  await page.locator('[data-component="ticket-inspector"]').evaluate((node) => {
    class PromisedFile extends File {
      override arrayBuffer() {
        return Promise.reject(new TypeError('backing file is unavailable'));
      }
    }
    const transfer = new DataTransfer();
    transfer.items.add(new PromisedFile(['pending'], 'floating-capture.png', { type: 'image/png' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: transfer }));
  });
  await expect(page.getByRole('alert')).toContainText('floating-capture.png');
  await expect(page.getByRole('alert')).toContainText('Wait for it to appear on the desktop');
  await expect(page.locator('[data-attachment-id="A5"]')).toHaveCount(0);
  await page.getByRole('button', { name: 'More actions for new-proof.txt' }).click();
  await page.getByRole('menu', { name: 'Attachment actions' }).getByRole('menuitem', { name: 'Remove' }).click();
  await expect(page.locator('[data-attachment-id="A3"]')).toHaveCount(0);
  await expect(page.locator('.app-toast')).toContainText('Attachment removed.');
  await expect(page.locator('.ticket-attachments__status')).toHaveCount(0);
  await page.getByRole('tab', { name: 'Info' }).click();
  const referencedNote = page.locator('article[data-note-id="N3"]'),
    referencedImage = referencedNote.locator('.markdown-preview__attachment-image');
  await expect(referencedImage).toBeVisible();
  await expect
    .poll(() => referencedImage.locator('img').evaluate((image: HTMLImageElement) => image.naturalWidth))
    .toBeGreaterThan(0);
  await referencedNote.screenshot({ path: '/private/tmp/hs2-b6937s-inline-image-reference.png' });
});

test('edits non-empty details on double click and empty details on one click', async ({ page }) => {
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByText('Use real project tickets').click();
  const preview = page.getByRole('button', { name: 'Edit Ticket details' });
  await preview.dblclick();
  const source = page.getByRole('textbox', { name: 'Ticket details' });
  await expect(source).toBeFocused();
  await source.fill('Carried into the larger editor');
  const detailsSurface = page.locator('.ticket-inspector__details-surface');
  const editorGeometry = await detailsSurface.evaluate((surface) => {
    const editor = surface.querySelector<HTMLTextAreaElement>('textarea[name="markdown-source"]')!,
      outer = surface.getBoundingClientRect(),
      inner = editor.getBoundingClientRect(),
      style = getComputedStyle(editor);
    return {
      left: inner.left - outer.left,
      top: inner.top - outer.top,
      right: outer.right - inner.right,
      paddingBlock: style.paddingBlock,
      paddingInline: style.paddingInline,
      resize: style.resize,
    };
  });
  expect(editorGeometry).toEqual({
    left: 9,
    top: 9,
    right: 9,
    paddingBlock: '0px',
    paddingInline: '0px',
    resize: 'vertical',
  });
  await detailsSurface.screenshot({ path: '/private/tmp/hs2-7nzkyc-details-editor-wide.png' });
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ });
  await expect(reader).toBeVisible();
  const readerPreview = reader.getByRole('button', { name: 'Edit Ticket details' });
  await expect(readerPreview).toContainText('Carried into the larger editor');
  await readerPreview.dblclick();
  await expect(reader.getByRole('textbox', { name: 'Ticket details' })).toHaveValue('Carried into the larger editor');
  await expect(reader.getByRole('textbox', { name: 'Feedback response' })).toHaveCount(0);
  await expect(reader.locator('article[data-note-id="N2"]')).toHaveAttribute('data-kind', 'regular');
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(reader).toHaveCount(0);
  await expect(source).toHaveCount(0);
  await page.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  await source.fill('');
  await source.blur();
  await expect(page.getByText('Click to add Markdown.')).toBeVisible();
  await page.getByRole('button', { name: 'Edit Ticket details' }).click();
  await expect(source).toBeFocused();
  await source.fill('Added from an empty ticket');
  await source.blur();
  await expect(page.locator('.ticket-inspector__details-surface [data-component="markdown-preview"]')).toContainText(
    'Added from an empty ticket',
  );
  await page.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  await page.setViewportSize({ width: 1024, height: 600 });
  await detailsSurface.screenshot({ path: '/private/tmp/hs2-7nzkyc-details-editor-narrow.png' });
});

test('keeps reader details, blocked reason, and note edit state independent from the sidebar', async ({ page }) => {
  const patches = await mockProject(page, true, false, 0, 0, 300);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const row = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await row.dblclick();
  let reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' }),
    preview = reader.getByRole('button', { name: 'Edit Ticket details' });
  const sidebar = page.locator('[data-region-id="app-inspector"]');
  await preview.dblclick();
  let source = reader.getByRole('textbox', { name: 'Ticket details' });
  await expect(source).toBeFocused();
  await expect(sidebar.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);
  await source.blur();
  await expect(reader.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);
  await expect(preview).toContainText('The real ticket body.');
  await reader.getByRole('button', { name: 'Block ticket' }).click();
  await expect(reader.getByRole('textbox', { name: 'Blocked reason' })).toBeVisible();
  await expect(sidebar.getByRole('textbox', { name: 'Blocked reason' })).toHaveCount(0);
  await reader.getByRole('textbox', { name: 'Blocked reason' }).blur();
  await reader.getByText('Editable note with').dblclick();
  await expect(reader.getByRole('textbox', { name: 'Note body' })).toBeVisible();
  await expect(sidebar.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  await reader.getByRole('textbox', { name: 'Note body' }).blur();
  await preview.dblclick();
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(reader).toHaveCount(0);
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);
  await expect(inspector.getByRole('button', { name: 'Edit Ticket details' })).toContainText('The real ticket body.');
  await page.getByRole('button', { name: 'Open ticket reader' }).click();
  reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  preview = reader.getByRole('button', { name: 'Edit Ticket details' });
  await preview.dblclick();
  source = reader.getByRole('textbox', { name: 'Ticket details' });
  await source.fill('Saved while the reader closes');
  await page.screenshot({ path: '/private/tmp/hs2-x3gx39-reader-edit-wide.png', fullPage: true });
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(reader).toHaveCount(0);
  await expect.poll(() => patches.some((patch) => patch.details === 'Saved while the reader closes')).toBe(true);
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);
  await expect(inspector.getByRole('button', { name: 'Edit Ticket details' })).toContainText(
    'Saved while the reader closes',
  );
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.screenshot({ path: '/private/tmp/hs2-x3gx39-reader-close-floor.png', fullPage: true });
});

test('persists separate sidebar, reader, and new-ticket heights through rerenders', async ({ page }) => {
  await mockProject(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-region-id="app-inspector"]'),
    resize = async (locator: Locator, height: number) => {
      await locator.evaluate((node, value) => {
        const textarea = node as HTMLTextAreaElement;
        textarea.style.height = `${value}px`;
        textarea.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
        textarea.dispatchEvent(new Event('input', { bubbles: true }));
      }, height);
      await expect.poll(() => locator.evaluate((node) => Math.round(node.getBoundingClientRect().height))).toBe(height);
    };
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  await resize(inspector.getByRole('textbox', { name: 'Ticket details' }), 132);
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ });
  await reader.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  await resize(reader.getByRole('textbox', { name: 'Ticket details' }), 222);
  await reader.getByRole('button', { name: 'Block ticket' }).click();
  await resize(reader.getByRole('textbox', { name: 'Blocked reason' }), 144);
  await reader.getByText('Editable note with').dblclick();
  await resize(reader.getByRole('textbox', { name: 'Note body' }), 188);
  await expect
    .poll(() =>
      page.evaluate(() =>
        Object.fromEntries(
          ['details.sidebar', 'details.reader', 'blocked-reason.reader', 'note.reader'].map((key) => [
            key,
            localStorage.getItem(`hotsheet.ticket-editor-height.${key}`),
          ]),
        ),
      ),
    )
    .toEqual({
      'details.sidebar': '132',
      'details.reader': '222',
      'blocked-reason.reader': '144',
      'note.reader': '188',
    });
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  const composer = page.locator('[data-action="create-ticket-form"]'),
    composerDetails = composer.getByRole('textbox', { name: 'Details' });
  await resize(composerDetails, 157);
  await composerDetails.fill('A rerender must preserve this height');
  await expect
    .poll(() => composerDetails.evaluate((node) => Math.round(node.getBoundingClientRect().height)), {
      message: 'composer height after value rerender',
    })
    .toBe(157);
  await composer.getByRole('button', { name: 'Cancel' }).click();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await expect
    .poll(
      () =>
        composer.getByRole('textbox', { name: 'Details' }).evaluate((node) => {
          const style = getComputedStyle(node);
          return {
            height: Math.round(node.getBoundingClientRect().height),
            inline: (node as HTMLTextAreaElement).style.height,
            computed: style.height,
            minHeight: style.minHeight,
            variable: getComputedStyle(document.documentElement).getPropertyValue(
              '--hs-new-ticket-details-sidebar-height',
            ),
            stored: localStorage.getItem('hotsheet.ticket-editor-height.new-ticket-details.sidebar'),
          };
        }),
      { message: 'composer height after reopen' },
    )
    .toEqual({ height: 157, inline: '', computed: '157px', minHeight: '157px', variable: '157px', stored: '157' });
  await composer.screenshot({ path: '/private/tmp/hs2-eaha4e-composer-height.png' });
  await expect(
    page.evaluate(() => localStorage.getItem('hotsheet.ticket-editor-height.new-ticket-details.sidebar')),
  ).resolves.toBe('157');
});

test('keeps backlog and archived tickets out of the active Queue', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Queue' })).toBeVisible();
  await expect(page.locator('[data-project-dialog]')).not.toBeVisible();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
  await expect(page.getByText('Deferred backlog ticket')).toHaveCount(0);
  await expect(page.getByText('Archived ticket')).toHaveCount(0);
  await page.getByRole('button', { name: /Backlog/ }).click();
  const backlog = page.locator('[data-ticket-slug="HS2-BACK01"]'),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  await expect(backlog).toBeVisible();
  await backlog.click({ button: 'right' });
  await expect(menu.locator('[data-context-action="Move to Backlog"]')).toHaveAttribute('disabled', '');
  await expect(menu.locator('[data-context-action="Archive ticket"]')).not.toHaveAttribute('disabled', '');
  await page.keyboard.press('Escape');
  await expect(page.getByText('Use real project tickets')).toHaveCount(0);
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Created directly in backlog');
  await page.getByRole('button', { name: 'Create ticket' }).click();
  const created = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]');
  await expect(created).toBeVisible();
  await expect(created).toHaveAttribute('data-status', 'backlog');
  await page.getByRole('button', { name: /Archive/ }).click();
  for (const slug of ['HS2-ARCH01', 'HS2-MOVED1']) {
    const archived = page.locator(`[data-ticket-slug="${slug}"]`);
    await expect(archived).toBeVisible();
    await archived.click({ button: 'right' });
    await expect(menu.locator('[data-context-action="Archive ticket"]')).toHaveAttribute('disabled', '');
    await expect(menu.locator('[data-context-action="Move to Backlog"]')).not.toHaveAttribute('disabled', '');
    await page.keyboard.press('Escape');
  }
  // Soft-deleted tickets live in Trash, not Archive (HS2-MWDR19).
  await expect(page.locator('[data-ticket-slug="HS2-DEL001"]')).toHaveCount(0);
  await expect(page.getByText('Deferred backlog ticket')).toHaveCount(0);
  const persistentComposer = page.locator('[data-component="quick-ticket-composer"]');
  await expect(persistentComposer).not.toHaveAttribute('open', '');
  await expect(persistentComposer.locator('[data-action="create-ticket-form"]')).toHaveCount(0);
});

test('loads all 138 Queue tickets before mixed-status pagination is applied', async ({ page }) => {
  await mockProject(page);
  const queued = Array.from({ length: 138 }, (_, index) => ({
    ...row,
    id: `queue-${index}`,
    native_id: `queue-${index}`,
    qualified_id: `git-local:queue-${index}`,
    slug: `HS2-Q${String(index).padStart(5, '0')}`,
    title: `Queue ticket ${index + 1}`,
    status: 'not_started',
    up_next: false,
  }));
  const mixed = [
    ...Array.from({ length: 240 }, (_, index) => ({
      ...backlogRow,
      id: `backlog-${index}`,
      native_id: `backlog-${index}`,
      qualified_id: `git-local:backlog-${index}`,
      slug: `HS2-B${String(index).padStart(5, '0')}`,
    })),
    ...queued,
  ];
  const collectionRequests: string[] = [];
  await page.route('**/tickets*', async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() !== 'GET' || !url.pathname.endsWith('/tickets')) return route.fallback();
    collectionRequests.push(url.searchParams.get('collection') ?? '');
    const items = url.searchParams.get('collection') === 'queue' ? queued : mixed.slice(0, 200);
    return route.fulfill({
      json: {
        items,
        counts: {
          total: mixed.length,
          queued: queued.length,
          backlog: 240,
          archive: 0,
          open: queued.length,
          up_next: 0,
          active: 0,
          started: 0,
          completed_today: 0,
        },
      },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(138);
  expect(collectionRequests).toContain('queue');
  await page.screenshot({ path: '/private/tmp/hs2-cs8agd-queue-138.png', fullPage: true });
});

test('loads ticket rows beyond the first 200 from the visible queue continuation', async ({ page }) => {
  await mockProject(page);
  const firstPage = Array.from({ length: 200 }, (_, index) => ({
      ...row,
      id: `page-${index}`,
      native_id: `page-${index}`,
      qualified_id: `git-local:page-${index}`,
      slug: `HS2-P${String(index).padStart(5, '0')}`,
      title: `Paged ticket ${index + 1}`,
      status: 'not_started',
      up_next: false,
    })),
    later = Array.from({ length: 5 }, (_, index) => ({
      ...row,
      id: `page-${200 + index}`,
      native_id: `page-${200 + index}`,
      qualified_id: `git-local:page-${200 + index}`,
      slug: `HS2-P${String(200 + index).padStart(5, '0')}`,
      title: `Paged ticket ${201 + index}`,
      status: 'not_started',
      up_next: false,
    })),
    requests: URL[] = [];
  await page.route('**/checkouts/demo-checkout/tickets*', (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() !== 'GET') return route.fallback();
    requests.push(url);
    const counts = {
      total: 205,
      queued: 205,
      backlog: 0,
      archive: 0,
      open: 205,
      up_next: 0,
      active: 0,
      started: 0,
      completed_today: 0,
    };
    return route.fulfill({
      json: url.searchParams.get('cursor')
        ? { items: [firstPage.at(-1), ...later], counts }
        : { items: firstPage, next_cursor: 'after-200', counts },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const more = page.getByRole('button', { name: 'Load more tickets' });
  await expect(more).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-P00204"]')).toHaveCount(0);
  await more.scrollIntoViewIfNeeded();
  await page.screenshot({ path: '/private/tmp/hs2-q9yq2b-load-more-before.png', fullPage: true });
  await more.click();
  await expect(page.locator('[data-ticket-slug="HS2-P00204"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(205);
  await expect(more).toHaveCount(0);
  expect(
    requests.some(
      (url) => url.searchParams.get('cursor') === 'after-200' && url.searchParams.get('page_size') === '200',
    ),
  ).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-q9yq2b-load-more.png', fullPage: true });
});

test('paginates each board column independently so a short column is not starved by a long one (HS2-8NBGBX)', async ({
  page,
}) => {
  await mockProject(page);
  // Whole-checkout totals: Not Started 205 (open-started), Started 20, Completed 20 (queued-open-verified), Verified 15.
  const counts = {
    total: 275,
    queued: 260,
    backlog: 10,
    archive: 5,
    open: 225,
    up_next: 0,
    active: 0,
    started: 20,
    verified: 15,
    completed_today: 0,
  };
  const make = (status: string, prefix: string, count: number, from = 0) =>
    Array.from({ length: count }, (_, i) => ({
      ...row,
      id: `${prefix}-${from + i}`,
      native_id: `${prefix}-${from + i}`,
      qualified_id: `git-local:${prefix}-${from + i}`,
      slug: `HS2-${prefix}${String(from + i).padStart(3, '0')}`,
      title: `${status} ticket ${from + i + 1}`,
      status,
      up_next: false,
    }));
  const notStarted = make('not_started', 'NS', 205),
    started = make('started', 'ST', 20),
    completed = make('completed', 'CP', 20),
    verified = make('verified', 'VF', 15);
  const statusRequests: string[] = [];
  await page.route('**/checkouts/demo-checkout/tickets*', (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() !== 'GET') return route.fallback();
    const status = url.searchParams.get('status'),
      cursor = url.searchParams.get('cursor');
    if (status) {
      statusRequests.push(cursor ? `${status}:${cursor}` : status);
      // Per-column status page. Not Started paginates (100 then the rest); the others fit in one page.
      if (status === 'not_started')
        return route.fulfill({
          json:
            cursor === 'ns-100'
              ? { items: notStarted.slice(100), counts }
              : { items: notStarted.slice(0, 100), next_cursor: 'ns-100', counts },
        });
      if (status === 'started') return route.fulfill({ json: { items: started, counts } });
      if (status === 'completed') return route.fulfill({ json: { items: completed, counts } });
      if (status === 'verified') return route.fulfill({ json: { items: verified, counts } });
    }
    // Initial global page arrives starved: mostly Completed, only a couple Not Started (the reported bug).
    return route.fulfill({
      json: {
        items: [...notStarted.slice(0, 2), ...started.slice(0, 4), ...completed.slice(0, 10), ...verified.slice(0, 5)],
        next_cursor: 'after-200',
        counts,
      },
    });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  const board = page.locator('[data-component="ticket-board"]');
  const column = (name: string) => board.getByRole('region', { name: `${name} column`, exact: true });
  const columnMore = (name: string) => column(name).getByRole('button', { name: 'Load more tickets' });
  // Every column shows its absolute lifecycle total even though few rows loaded.
  await expect(column('Not Started').getByLabel('205 tickets')).toBeVisible();
  await expect(column('Started').getByLabel('20 tickets')).toBeVisible();
  await expect(column('Completed').getByLabel('20 tickets')).toBeVisible();
  await expect(column('Verified').getByLabel('15 tickets')).toBeVisible();
  // Each partial column offers its OWN Load more — the short Not Started column is not starved.
  await expect(columnMore('Not Started')).toBeVisible();
  await expect(columnMore('Started')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-8nbgbx-per-column-load-more-wide.png', fullPage: true });

  // Loading Not Started pages ONLY its status; the others are untouched.
  await columnMore('Not Started').scrollIntoViewIfNeeded();
  await columnMore('Not Started').click();
  await expect(page.locator('[data-ticket-slug="HS2-NS050"]')).toBeVisible();
  expect(statusRequests).toContain('not_started');
  expect(statusRequests).not.toContain('started');
  await expect(columnMore('Not Started')).toBeVisible(); // 100 of 205 — still more
  await expect(columnMore('Started')).toBeVisible(); // untouched

  // A different column pages independently.
  await columnMore('Started').scrollIntoViewIfNeeded();
  await columnMore('Started').click();
  await expect(page.locator('[data-ticket-slug="HS2-ST019"]')).toBeVisible();
  expect(statusRequests).toContain('started');
  await expect(columnMore('Started')).toHaveCount(0); // 20 of 20 — fully loaded, no button
  await expect(columnMore('Not Started')).toBeVisible(); // Not Started unaffected by Started paging
  await page.screenshot({ path: '/private/tmp/hs2-8nbgbx-per-column-load-more-after.png', fullPage: true });
});

test('paginates the merged Completed column through completed then verified when Verified is hidden (HS2-F2N4ZN)', async ({
  page,
}) => {
  await mockProject(page);
  // Hide the Verified column so Completed absorbs verified rows.
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.project.demo-checkout.hide-verified-column', 'true');
  });
  // Whole-checkout totals: open 10 (5 not_started + 5 started); merged Completed = queued-open = 110 (80 completed + 30 verified).
  const counts = {
    total: 130,
    queued: 120,
    backlog: 0,
    archive: 0,
    open: 10,
    up_next: 0,
    active: 0,
    started: 5,
    verified: 30,
    completed_today: 0,
  };
  const make = (status: string, prefix: string, count: number, from = 0) =>
    Array.from({ length: count }, (_, i) => ({
      ...row,
      id: `${prefix}-${from + i}`,
      native_id: `${prefix}-${from + i}`,
      qualified_id: `git-local:${prefix}-${from + i}`,
      slug: `HS2-${prefix}${String(from + i).padStart(3, '0')}`,
      title: `${status} ticket ${from + i + 1}`,
      status,
      up_next: false,
    }));
  const notStarted = make('not_started', 'NS', 5),
    started = make('started', 'ST', 5),
    completed = make('completed', 'CP', 80),
    verified = make('verified', 'VF', 30);
  const statusRequests: string[] = [];
  await page.route('**/checkouts/demo-checkout/tickets*', (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() !== 'GET') return route.fallback();
    const status = url.searchParams.get('status'),
      cursor = url.searchParams.get('cursor');
    if (status) {
      statusRequests.push(cursor ? `${status}:${cursor}` : status);
      if (status === 'not_started') return route.fulfill({ json: { items: notStarted, counts } });
      if (status === 'started') return route.fulfill({ json: { items: started, counts } });
      // Both done in one page (no next_cursor), so each stream exhausts and the walk advances.
      if (status === 'completed') return route.fulfill({ json: { items: completed, counts } });
      if (status === 'verified') return route.fulfill({ json: { items: verified, counts } });
    }
    // Starved initial global page: a handful of each, so the merged column starts partial.
    return route.fulfill({
      json: {
        items: [...notStarted.slice(0, 2), ...started.slice(0, 2), ...completed.slice(0, 10), ...verified.slice(0, 5)],
        next_cursor: 'after-200',
        counts,
      },
    });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  const board = page.locator('[data-component="ticket-board"]');
  const column = (name: string) => board.getByRole('region', { name: `${name} column`, exact: true });
  const completedMore = () => column('Completed').getByRole('button', { name: 'Load more tickets' });
  // Verified is merged away; Completed carries the whole done total.
  await expect(column('Verified')).toHaveCount(0);
  await expect(column('Completed').getByLabel('110 tickets')).toBeVisible();
  await expect(completedMore()).toBeVisible();
  // A verified ticket beyond the baseline is not loaded yet.
  await expect(page.locator('[data-ticket-slug="HS2-VF020"]')).toHaveCount(0);

  // First Load more exhausts the `completed` stream. The column must STILL offer more, because
  // verified rows remain — before HS2-F2N4ZN it stopped here, stranding the verified rows.
  await completedMore().scrollIntoViewIfNeeded();
  await completedMore().click();
  await expect(page.locator('[data-ticket-slug="HS2-CP079"]')).toBeVisible();
  expect(statusRequests).toContain('completed');
  expect(statusRequests).not.toContain('verified');
  await expect(page.locator('[data-ticket-slug="HS2-VF020"]')).toHaveCount(0);
  await expect(completedMore()).toBeVisible();

  // Second Load more walks into the `verified` stream and completes the column.
  await completedMore().scrollIntoViewIfNeeded();
  await completedMore().click();
  await expect(page.locator('[data-ticket-slug="HS2-VF020"]')).toBeVisible();
  expect(statusRequests.indexOf('completed')).toBeLessThan(statusRequests.indexOf('verified'));
  await expect(completedMore()).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/claude/hs2-f2n4zn-merged-completed-paginated.png', fullPage: true });
});

test('stores the shell-history inheritance opt-out locally and applies it only to new terminals', async ({ page }) => {
  const writes: Array<{ inherit_global_shell_history: boolean }> = [];
  await mockProject(page);
  page.on('request', (request) => {
    if (request.method() === 'PUT' && new URL(request.url()).pathname.endsWith('/terminal-settings'))
      writes.push(request.postDataJSON());
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Terminals', exact: true }).click();
  const option = page.getByLabel('Use my global shell history');
  await expect(option).not.toBeChecked();
  await expect(page.getByText(/each terminal keeps private/)).toBeVisible();
  await option.check();
  await expect.poll(() => writes).toEqual([{ inherit_global_shell_history: true }]);
  await expect(page.getByText('Saved locally. New terminals will use this setting.', { exact: true })).toContainText(
    'Saved locally. New terminals will use this setting.',
  );
  await page.screenshot({ path: '/private/tmp/hs2-a5v801-terminal-history-setting-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 700 });
  await page.screenshot({ path: '/private/tmp/hs2-a5v801-terminal-history-setting-narrow.png', fullPage: true });
});

test('keeps managed workspace search open and focused through repeated controlled clears (HS2-M4BNX5)', async ({
  page,
}) => {
  await mockProject(page);
  await page.setViewportSize({ width: 1280, height: 844 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const field = page.locator('[data-token-search-id="workspace-search"]'),
    editor = page.getByRole('searchbox', { name: 'Search tickets' }),
    group = page.locator('.workspace-header__search-group').filter({ has: field });
  for (const [width, name] of [
    [1280, 'wide'],
    [390, 'mobile'],
  ] as const) {
    await page.setViewportSize({ width, height: 844 });
    await page.getByRole('button', { name: 'Search tickets', exact: true }).click();
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await editor.fill('has:attachment ');
      await expect(
        field.locator('[data-component="token-search-token"][data-token-value="has:attachment"]'),
      ).toBeVisible();
      await page.getByRole('button', { name: 'Clear search', exact: true }).click();
      await page.keyboard.type('continued');
      await expect(editor).toBeFocused();
      await expect(editor).toHaveText('continued');
      await expect(field.locator('[data-component="token-search-token"]')).toHaveCount(0);
      await expect(group).toHaveAttribute('data-expanded', 'true');
    }
    await page.evaluate(async () => {
      await Promise.all(
        document
          .getAnimations()
          .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
          .map((animation) => animation.finished.catch(() => {})),
      );
    });
    await expect(editor).toBeVisible();
    await page.screenshot({
      path: `/private/tmp/hs2-m4bnx5-context-${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
    await group.screenshot({ path: `/private/tmp/hs2-m4bnx5-adopted-clear-${name}.png`, animations: 'disabled' });
    await page.getByRole('button', { name: 'Clear search', exact: true }).click();
    await expect(editor).toBeFocused();
    await page.getByRole('button', { name: 'Add project', exact: true }).focus();
    await expect(group).toHaveAttribute('data-expanded', 'false');
  }
});

test('searches indexed ticket details and notes without discarding the full project list', async ({ page }) => {
  const searchRequests: string[] = [];
  await mockProject(page);
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (url.pathname.endsWith('/tickets') && url.searchParams.has('text'))
      searchRequests.push(url.searchParams.get('text')!);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('QQRY00');
  await expect(page.locator('[data-ticket-slug="HS2-QQRY00"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-SHG7YS"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toHaveCount(0);
  await expect.poll(() => searchRequests).toContain('QQRY00');
  await page.waitForTimeout(250);
  const settledRequests = searchRequests.length;
  await page.locator('.kui-toolbar-text', { hasText: 'Search results' }).click();
  await page.waitForTimeout(250);
  expect(searchRequests).toHaveLength(settledRequests);
  await page.screenshot({ path: '/private/tmp/hs2-pdze14-search-blur-no-request.png', fullPage: true });
  await page.getByRole('button', { name: 'Clear search' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-SHG7YS"]')).toBeVisible();
  const pending = page.waitForRequest((request) => new URL(request.url()).searchParams.get('text') === 'QQRY00');
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('QQRY00');
  await pending;
  await page.getByRole('button', { name: 'Clear search' }).click();
  await page.waitForTimeout(200);
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
});

test('clips animated ticket search results above the terminal drawer (HS2-29T4D8)', async ({ page }) => {
  await mockProject(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Columns view' }).click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
  await page.evaluate(() => {
    const original = Object.getOwnPropertyDescriptor(Element.prototype, 'animate')!.value as Element['animate'];
    Element.prototype.animate = function (
      keyframes: Keyframe[] | PropertyIndexedKeyframes | null,
      options?: number | KeyframeAnimationOptions,
    ) {
      const animation = original.call(this, keyframes, options);
      if (this instanceof HTMLElement && this.dataset.ticketMotionGhost) animation.pause();
      return animation;
    };
  });
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('QQRY00');
  await expect(page.locator('[data-ticket-slug="HS2-QQRY00"]')).toBeAttached();
  await expect.poll(() => page.locator('[data-ticket-motion-ghost]').count()).toBeGreaterThan(0);
  const geometry = await page.evaluate(() => {
    const bounds = (node: Element) => {
        const rect = node.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right };
      },
      workspace = bounds(document.querySelector('.app-shell__workspace')!),
      drawer = bounds(document.querySelector('[data-component="terminal-drawer"]')!),
      layers = [...document.querySelectorAll<HTMLElement>('[data-ticket-motion-layer]')].map((layer) => ({
        ...bounds(layer),
        overflow: getComputedStyle(layer).overflow,
      }));
    return { workspace, drawer, layers };
  });
  expect(geometry.layers.length).toBeGreaterThan(0);
  for (const layer of geometry.layers) {
    expect(layer.bottom).toBeLessThanOrEqual(geometry.workspace.bottom + 1);
    expect(layer.bottom).toBeLessThanOrEqual(geometry.drawer.top + 1);
    expect(layer.overflow).toBe('hidden');
  }
  await page.screenshot({ path: '/private/tmp/hs2-29t4d8-clipped-ticket-motion.png', fullPage: true });
});

test('searches only the current view before updating scoped sidebar counts', async ({ page }) => {
  const stabilityRows: TicketRow[] = [
      ...Array.from({ length: 2 }, (_, index) => ({
        ...row,
        native_id: `stability-q-${index}`,
        qualified_id: `git-local:stability-q-${index}`,
        id: `stability-q-${index}`,
        slug: `HS2-STABQ${index}`,
        title: `Stability queue ${index + 1}`,
        status: 'started',
        up_next: false,
        feedback_needed: false,
      })),
      ...Array.from({ length: 3 }, (_, index) => ({
        ...backlogRow,
        native_id: `stability-b-${index}`,
        qualified_id: `git-local:stability-b-${index}`,
        id: `stability-b-${index}`,
        slug: `HS2-STABB${index}`,
        title: `Stability backlog ${index + 1}`,
      })),
      ...Array.from({ length: 4 }, (_, index) => ({
        ...archiveRow,
        native_id: `stability-a-${index}`,
        qualified_id: `git-local:stability-a-${index}`,
        id: `stability-a-${index}`,
        slug: `HS2-STABA${index}`,
        title: `Stability archive ${index + 1}`,
      })),
    ],
    requests: string[] = [];
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() === 'GET' && url.searchParams.get('text') === 'stability') {
      const scope =
        url.searchParams.get('status') === 'backlog' ? 'backlog' : (url.searchParams.get('collection') ?? 'all');
      requests.push(scope);
      await new Promise((resolve) => setTimeout(resolve, scope === 'queue' ? 180 : 80));
      const items =
        scope === 'queue'
          ? stabilityRows.slice(0, 2)
          : scope === 'backlog'
            ? stabilityRows.slice(2, 5)
            : scope === 'archive'
              ? stabilityRows.slice(5)
              : stabilityRows;
      return route.fulfill({
        json: {
          items,
          counts: {
            total: 9,
            queued: 2,
            backlog: 3,
            archive: 4,
            open: 2,
            up_next: 0,
            active: 0,
            started: 2,
            completed_today: 0,
          },
        },
      });
    }
    return route.fallback();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('stability');
  const navigation = page.getByRole('navigation', { name: 'Ticket views' });
  await expect(navigation.getByLabel('Searching this view')).toHaveCount(3);
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(2);
  await expect(page.locator('[data-ticket-slug^="HS2-STABB"]')).toHaveCount(0);
  await expect(page.locator('[data-ticket-slug^="HS2-STABA"]')).toHaveCount(0);
  await expect(navigation.getByLabel('2 search results')).toBeVisible();
  await expect(navigation.getByLabel('3 search results')).toBeVisible();
  await expect(navigation.getByLabel('4 search results')).toBeVisible();
  expect(requests[0]).toBe('queue');
  await expect.poll(() => requests.slice(1).sort()).toEqual(['archive', 'backlog']);
  const selectedCount = navigation.getByRole('button', { name: /Queue/ }).locator('.kui-list-item__count');
  await expect(selectedCount).toHaveAttribute('data-search-count', 'true');
  await page.getByLabel('Columns view').click();
  const board = page.locator('[data-component="ticket-board"]');
  await expect(board.getByRole('region', { name: 'Backlog column' })).toHaveCount(0);
  await expect(board.getByRole('region', { name: 'Archive column' })).toHaveCount(0);
  await expect(board.locator('[data-component="ticket-board-column"]')).toHaveCount(4);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(500);
  await expect(board.locator('[data-component="empty-state"]')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-xr7dfr-scoped-search-columns-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: '/private/tmp/hs2-xr7dfr-scoped-search-columns-narrow.png', fullPage: true });
  const settledRequestCount = requests.length;
  await navigation.getByRole('button', { name: /Backlog/ }).click();
  await expect(navigation.getByLabel('Searching this view')).toHaveCount(1);
  await expect(navigation.getByLabel('2 search results')).toBeVisible();
  await expect(navigation.getByLabel('4 search results')).toBeVisible();
  await expect(page.locator('[data-ticket-slug^="HS2-STABB"]')).toHaveCount(3);
  await expect(navigation.getByLabel('3 search results')).toBeVisible();
  await expect.poll(() => requests.slice(settledRequestCount)).toEqual(['backlog']);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: '/private/tmp/hs2-1qqxdh-cached-search-counts-wide.png', fullPage: true });
});

test('filters duplicate tickets on the server before bounded search pagination', async ({ page }) => {
  const duplicateRows: TicketRow[] = Array.from({ length: 24 }, (_, index) => ({
    ...completedRow,
    id: `duplicate-${index}`,
    native_id: `duplicate-${index}`,
    qualified_id: `git-local:duplicate-${index}`,
    slug: `HS2-DUP${String(index).padStart(3, '0')}`,
    title: `Duplicate ticket ${index + 1}`,
    close_reason: 'duplicate',
    duplicate_of: 'git-local:1',
  }));
  const firstUnfilteredPage = Array.from({ length: 200 }, (_, index) => ({
    ...row,
    id: `ordinary-${index}`,
    native_id: `ordinary-${index}`,
    qualified_id: `git-local:ordinary-${index}`,
    slug: `HS2-ORD${String(index).padStart(3, '0')}`,
    title: `Ordinary ticket ${index + 1}`,
  }));
  const queries: URL[] = [];
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() !== 'GET') return route.fallback();
    queries.push(url);
    if (url.searchParams.get('close_reason') === 'duplicate')
      return route.fulfill({
        json: {
          items: duplicateRows,
          counts: {
            total: duplicateRows.length,
            queued: duplicateRows.length,
            backlog: 0,
            archive: 0,
            open: 0,
            up_next: 0,
            active: 0,
            started: 0,
            completed_today: 0,
          },
        },
      });
    return route.fulfill({
      json: {
        items: firstUnfilteredPage,
        counts: {
          total: firstUnfilteredPage.length + duplicateRows.length,
          queued: firstUnfilteredPage.length + duplicateRows.length,
          backlog: 0,
          archive: 0,
          open: firstUnfilteredPage.length,
          up_next: 0,
          active: 0,
          started: firstUnfilteredPage.length,
          completed_today: 0,
        },
      },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('is:duplicate ');
  await expect(page.locator('[data-component="token-search-token"][data-token-value="is:duplicate"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(duplicateRows.length);
  expect(queries.at(-1)?.searchParams.get('close_reason')).toBe('duplicate');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.screenshot({ path: '/private/tmp/hs2-v59atp-duplicate-results.png', fullPage: true });
});

test('evaluates boolean search matches beyond the first ticket page', async ({ page }) => {
  const firstPage: TicketRow[] = Array.from({ length: 200 }, (_, index) => ({
      ...row,
      id: `boolean-${index}`,
      native_id: `boolean-${index}`,
      qualified_id: `git-local:boolean-${index}`,
      slug: `HS2-BOOL${String(index).padStart(3, '0')}`,
      title: `Ordinary ticket ${index + 1}`,
    })),
    late = {
      ...row,
      id: 'boolean-late',
      native_id: 'boolean-late',
      qualified_id: 'git-local:boolean-late',
      slug: 'HS2-BOOL-LATE',
      title: 'Late matching ticket',
    };
  const queries: URL[] = [];
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() !== 'GET') return route.fallback();
    queries.push(url);
    const counts = {
      total: 201,
      queued: 201,
      backlog: 0,
      archive: 0,
      open: 201,
      up_next: 0,
      active: 0,
      started: 201,
      completed_today: 0,
    };
    if (url.searchParams.get('collection') !== 'queue') return route.fulfill({ json: { items: [], counts } });
    const cursor = url.searchParams.get('cursor');
    return route.fulfill({
      json: cursor ? { items: [late], counts } : { items: firstPage, next_cursor: 'after-200', counts },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('late AND (matching OR missing)');
  await expect(page.locator('[data-ticket-slug="HS2-BOOL-LATE"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'Load more tickets' })).toHaveCount(0);
  expect(
    queries.some(
      (url) => url.searchParams.get('cursor') === 'after-200' && url.searchParams.get('page_size') === '500',
    ),
  ).toBe(true);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.screenshot({ path: '/private/tmp/hs2-dwvpcx-boolean-page-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 700 });
  await page.screenshot({ path: '/private/tmp/hs2-dwvpcx-boolean-page-narrow.png', fullPage: true });
});

test('tokenizes inline tag search with autocomplete and no separate advanced-search button', async ({ page }) => {
  await mockProject(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.getByRole('button', { name: 'Advanced search' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const query = page.getByRole('searchbox', { name: 'Search tickets' });
  await query.fill('tag:cl');
  const suggestions = page.getByRole('listbox', { name: 'Matching tags' });
  await expect(suggestions.getByRole('option', { name: 'tag:client' })).toBeVisible();
  await suggestions.getByRole('option', { name: 'tag:client' }).click();
  const chip = page.locator('[data-component="token-search-token"]');
  await expect(chip).toContainText('tag:client');
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(8);
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-BACK01"]')).toHaveCount(0);
  await page.waitForTimeout(400);
  await page.screenshot({ path: '/private/tmp/hs2-383d6k-inline-search-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.waitForTimeout(400);
  await page.screenshot({ path: '/private/tmp/hs2-383d6k-inline-search-narrow.png', fullPage: true });
  await page.getByRole('button', { name: 'Remove tag client' }).click();
  await expect(chip).toHaveCount(0);
});

test('deletes inline search tokens from either adjacent caret boundary', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const query = page.getByRole('searchbox', { name: 'Search tickets' }),
    chip = page.locator('[data-component="token-search-token"]');
  const placeCaret = async (raw: string, side: 'before' | 'after') =>
    query.evaluate(
      (editor, { raw, side }) => {
        const token = editor.querySelector<HTMLElement>(`[data-token-value="${raw}"]`)!,
          text = side === 'before' ? token.previousSibling : token.nextSibling,
          node = text?.firstChild ?? text,
          range = document.createRange(),
          selection = getSelection()!;
        if (!node) throw new Error(`Missing ${side} token boundary`);
        range.setStart(node, side === 'before' ? (node.textContent?.length ?? 0) : 0);
        range.collapse(true);
        selection.removeAllRanges();
        selection.addRange(range);
      },
      { raw, side },
    );
  await query.fill('tag:cl');
  await page.getByRole('option', { name: 'tag:client' }).click();
  await placeCaret('tag:client', 'after');
  await query.press('Backspace');
  await expect(chip).toHaveCount(0);
  await query.fill('before is:active after ');
  await expect(chip).toHaveAttribute('data-token-value', 'is:active');
  await placeCaret('is:active', 'after');
  await query.press('Backspace');
  await expect(chip).toHaveCount(0);
  await expect(query).toHaveText('before  after ');
  await query.fill('before is:active after ');
  await expect(chip).toHaveAttribute('data-token-value', 'is:active');
  await placeCaret('is:active', 'before');
  await query.press('Delete');
  await expect(chip).toHaveCount(0);
  await expect(query).toHaveText('before  after ');
});

test('deletes a trailing token immediately after Space commits it', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const query = page.getByRole('searchbox', { name: 'Search tickets' }),
    chip = page.locator('[data-component="token-search-token"]');
  await query.pressSequentially('tag:hello');
  await query.press('Space');
  await expect(chip).toHaveAttribute('data-token-value', 'tag:hello');
  await query.press('Backspace');
  await expect(chip).toHaveCount(0);
  await expect(query).toHaveText('');
});

for (const width of [1440, 390]) {
  test(`keeps workspace search focused after Select All deletion and yields to outside focus at ${width}px (HS2-GRAQ2K)`, async ({
    page,
  }, testInfo) => {
    await mockProject(page);
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open project' }).click();
    await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
    const trigger = page.getByRole('button', { name: 'Search tickets', exact: true }),
      query = page.getByRole('searchbox', { name: 'Search tickets' }),
      chip = query.locator('[data-component="token-search-token"]'),
      group = page.locator('.workspace-header__search-group').filter({
        has: page.locator('[data-token-search-id="workspace-search"]'),
      });
    await trigger.click();
    for (const [index, key] of ['Backspace', 'Delete', 'Backspace', 'Delete'].entries()) {
      await query.fill(index % 2 ? 'is:active tag:client ' : 'before is:active tag:client after ');
      await expect(chip).toHaveCount(2);
      await query.press('ControlOrMeta+A');
      await page.keyboard.press(key);
      await expect(query).toBeFocused();
      await expect(query).toHaveText('');
      await expect(chip).toHaveCount(0);
      await expect(group).toHaveAttribute('data-expanded', 'true');
      // Repeated deletion while empty must not collapse the editor or require a click.
      await page.keyboard.press(key);
      await expect(query).toBeFocused();
      await page.keyboard.insertText(`refilled ${index}`);
      await expect(query).toHaveText(`refilled ${index}`);
      await expect(chip).toHaveCount(0);
    }
    await page.screenshot({ path: testInfo.outputPath(`select-all-refilled-${width}.png`), animations: 'disabled' });
    await query.press('ControlOrMeta+A');
    await page.keyboard.press('Backspace');
    await expect(query).toBeFocused();
    await group.screenshot({ path: testInfo.outputPath(`select-all-empty-${width}.png`), animations: 'disabled' });
    // A real keyboard focus handoff must still collapse an empty managed field.
    await page.keyboard.press('Shift+Tab');
    await expect(query).toBeHidden();
    await expect(group).toHaveAttribute('data-expanded', 'false');
    await expect(trigger).toBeVisible();
    const outside = await page.evaluateHandle(() => document.activeElement);
    expect(await outside.evaluate((element) => element !== document.body)).toBe(true);
    await page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              resolve();
            });
          });
        }),
    );
    expect(await outside.evaluate((element) => element === document.activeElement)).toBe(true);
    await outside.dispose();
    await trigger.click();
    await expect(query).toBeFocused();
    await page.keyboard.insertText('reopened');
    await expect(query).toHaveText('reopened');
    await query.press('ControlOrMeta+A');
    await page.keyboard.press('Delete');
    await query.press('Escape');
    await expect(trigger).toBeFocused();
    await expect(group).toHaveAttribute('data-expanded', 'false');
  });
}

for (const surface of ['workspace', 'saved-view']) {
  test(`preserves ${surface} replacement input when token-deletion frames resume late (HS2-PR5TNA)`, async ({
    page,
  }) => {
    await mockProject(page);
    await page.route('**/views', (route) => route.fulfill({ json: [] }));
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open project' }).click();
    await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
    await page
      .getByRole('button', { name: surface === 'workspace' ? 'Search tickets' : 'Add view', exact: true })
      .click();
    if (surface === 'saved-view') {
      const name = page.getByRole('textbox', { name: 'View name' });
      await expect(name).toBeFocused();
      await name.fill('Token deletion regression');
    }
    const query = page.getByRole('searchbox', { name: surface === 'workspace' ? 'Search tickets' : 'Search query' }),
      chip = query.locator('[data-component="token-search-token"]');
    await query.fill('before is:active after ');
    await expect(chip).toHaveAttribute('data-token-value', 'is:active');
    for (const key of ['Backspace', 'Delete', 'Backspace', 'Delete']) {
      await query.evaluate((editor, key) => {
        const token = editor.querySelector('[data-component="token-search-token"]')!,
          text = key === 'Backspace' ? token.nextSibling : token.previousSibling,
          node = text!.firstChild ?? text!,
          selection = getSelection()!,
          range = document.createRange();
        range.setStart(node, key === 'Backspace' ? 0 : (node.textContent?.length ?? 0));
        range.collapse(true);
        selection.removeAllRanges();
        selection.addRange(range);
      }, key);
      const frames = await page.evaluateHandle(() => {
        const original = window.requestAnimationFrame.bind(window),
          originalCancel = window.cancelAnimationFrame.bind(window),
          pending = new Map<number, FrameRequestCallback>();
        let next = -1;
        window.requestAnimationFrame = (callback) => {
          const id = next--;
          pending.set(id, callback);
          return id;
        };
        window.cancelAnimationFrame = (id) => {
          if (id < 0) pending.delete(id);
          else originalCancel(id);
        };
        return {
          advance() {
            const callbacks = [...pending.values()];
            pending.clear();
            for (const callback of callbacks) callback(performance.now());
          },
          restore() {
            window.requestAnimationFrame = original;
            window.cancelAnimationFrame = originalCancel;
            for (const callback of pending.values()) original(callback);
            pending.clear();
          },
        };
      });
      try {
        await query.press(key);
        await expect(chip).toHaveCount(0);
        await expect(query).toHaveText('before  after ');
        // A busy browser may deliver the old restoration between selecting replacement
        // text and its insertion. Keep this ordering deterministic instead of adding sleep.
        await query.selectText();
        await frames.evaluate((clock) => {
          clock.advance();
        });
        await frames.evaluate((clock) => {
          clock.advance();
        });
        await page.keyboard.insertText('before is:active after ');
        await expect(chip).toHaveAttribute('data-token-value', 'is:active');
        await expect(query.locator('[data-token-search-text]')).toHaveText(['before ', ' after ']);
      } finally {
        await frames.evaluate((clock) => {
          clock.restore();
        });
        await frames.dispose();
      }
    }
    await page
      .getByRole('button', { name: surface === 'workspace' ? 'Clear search' : 'Clear search query', exact: true })
      .click();
    await expect(chip).toHaveCount(0);
    await expect(query).toHaveText('');
    await expect(query).toBeFocused();
    await query.fill('refilled tag:client after ');
    await expect(chip).toHaveAttribute('data-token-value', 'tag:client');
    await expect(query.locator('[data-token-search-text]')).toHaveText(['refilled ', ' after ']);
    await chip.getByRole('button', { name: 'Remove tag client' }).click();
    await expect(chip).toHaveCount(0);
    await expect(query).toBeFocused();
    await page.keyboard.insertText('more');
    await expect(query).toHaveText('refilled more after ');
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.screenshot({ path: `/private/tmp/hs2-pr5tna-${surface}-${width}.png`, animations: 'disabled' });
    }
  });
}

test('edits inline filters and exposes attachment, lifecycle-date, and syntax helpers', async ({ page }) => {
  const structured: URL[] = [];
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() === 'GET' && [...url.searchParams].some(([key]) => key !== 'text')) {
      structured.push(url);
      return route.fulfill({ json: [row] });
    }
    return route.fallback();
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const query = page.getByRole('searchbox', { name: 'Search tickets' });
  await query.fill('tag:cl');
  const suggestion = page.getByRole('option', { name: 'tag:client' });
  await expect(suggestion).toBeVisible();
  await expect(suggestion).not.toHaveCSS('color', 'rgb(255, 255, 255)');
  await suggestion.click();
  const chip = page.locator('[data-component="token-search-token"]');
  await expect(chip).toContainText('tag:client');
  await chip.dblclick();
  await expect(query).toHaveText('tag:client');
  await query.press('Enter');
  await expect(chip).toContainText('tag:client');
  await page.getByRole('button', { name: 'Clear search' }).click();
  await query.fill('has:attachment ');
  await expect.poll(() => structured.some((url) => url.searchParams.get('has_attachment') === 'true')).toBe(true);
  await expect(chip.filter({ hasText: 'has attachment' })).toBeVisible();
  await page.getByRole('button', { name: 'Clear search' }).click();
  await query.fill('has:media-annotation ');
  await expect(chip.filter({ hasText: 'has media annotation' })).toBeVisible();
  await expect.poll(() => structured.some((url) => url.searchParams.get('has_media_annotation') === 'true')).toBe(true);
  await page.getByRole('button', { name: 'Clear search' }).click();
  await query.fill('has:commit ');
  await expect(chip.filter({ hasText: 'has commit' })).toBeVisible();
  await expect.poll(() => structured.some((url) => url.searchParams.get('has_commit') === 'true')).toBe(true);
  await page.getByRole('button', { name: 'Clear search' }).click();
  await query.fill('attachment:*.png ');
  await expect(chip.filter({ hasText: 'attachment:*.png' })).toBeVisible();
  await expect.poll(() => structured.some((url) => url.searchParams.get('attachment') === '*.png')).toBe(true);
  await page.getByRole('button', { name: 'Clear search' }).click();
  await query.fill('updated-after:4h ago ');
  await expect.poll(() => structured.some((url) => url.searchParams.has('updated_after'))).toBe(true);
  await expect(chip.filter({ hasText: 'updated after 4h ago' })).toBeVisible();
  await page.getByRole('button', { name: 'Clear search' }).click();
  await query.fill('created-after:');
  const helper = page.getByRole('group', { name: 'Date and time helper' });
  await expect(helper).toBeVisible();
  await helper.getByLabel('Date').fill('2026-09-01');
  await helper.getByLabel('Time (optional)').fill('11:05');
  await helper.getByRole('button', { name: 'Apply' }).click();
  const dateChip = chip.filter({
    hasText: new Intl.DateTimeFormat(undefined, { dateStyle: 'short', timeStyle: 'short' }).format(
      new Date(2026, 8, 1, 11, 5),
    ),
  });
  await expect(dateChip).toBeVisible();
  await expect(dateChip).toHaveAttribute('data-token-value', 'created-after:2026-09-01T11:05');
  await expect.poll(() => structured.some((url) => url.searchParams.has('created_after'))).toBe(true);
  const inputBox = (await query.boundingBox())!,
    tokenBox = (await chip.first().boundingBox())!;
  expect(Math.abs(tokenBox.y + tokenBox.height / 2 - (inputBox.y + inputBox.height / 2))).toBeLessThanOrEqual(3);
  for (const item of await chip.all())
    expect(await item.evaluate((node) => node.scrollWidth)).toBeLessThanOrEqual(
      await item.evaluate((node) => node.clientWidth),
    );
  await page.getByRole('button', { name: 'Search syntax help' }).click();
  const help = page.getByRole('dialog', { name: 'Search syntax' });
  await expect(help.locator('dt')).toHaveText(['Tags', 'Content', 'Workflow', 'Dates']);
  await expect(help).toContainText('has:media-annotation');
  await expect(help).toContainText('updated-after:4h ago');
  await expect(help).toContainText('started, verified, archived, and updated');
  await page.screenshot({ path: '/private/tmp/hs2-7efj3e-search-help-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(help).toBeVisible();
  await expect
    .poll(() =>
      help.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth && box.bottom <= innerHeight;
      }),
    )
    .toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-7efj3e-search-help-narrow.png', fullPage: true });
});

test('distinguishes pending and empty ticket search feedback in list and board views', async ({ page }) => {
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', async (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() === 'GET' && url.searchParams.has('text')) {
      await new Promise((resolve) => setTimeout(resolve, 220));
      return route.fulfill({ json: [] });
    }
    return route.fallback();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('missing parser');
  await expect(page.getByRole('status').filter({ hasText: 'Searching tickets' })).toContainText(
    'Looking for “missing parser”…',
  );
  await expect(page.getByRole('status').filter({ hasText: 'No tickets match' })).toContainText(
    'No tickets match “missing parser”',
  );
  await page.screenshot({ path: '/private/tmp/hs2-ydrmad-search-empty-list-wide.png', fullPage: true });
  await page.getByLabel('Columns view').click();
  await expect(page.locator('[data-component="ticket-board"] [data-component="empty-state"]')).toHaveCount(1);
  await expect(page.getByText('No tickets match “missing parser”')).toBeVisible();
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.screenshot({ path: '/private/tmp/hs2-ydrmad-search-empty-board-narrow.png', fullPage: true });
});

test('shows new-project feedback when a project has no tickets', async ({ page }) => {
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [] }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const empty = page.locator('[data-component="empty-state"]');
  await expect(empty).toHaveClass(/ticket-empty-state--project/);
  await expect(empty).toContainText('No tickets yet');
  await expect(empty).toContainText('Create a ticket to start planning this project.');
  await page.screenshot({ path: '/private/tmp/hs2-ydrmad-project-empty-list-wide.png', fullPage: true });
  await page.getByLabel('Columns view').click();
  await expect(page.locator('[data-component="ticket-board"] [data-component="empty-state"]')).toHaveCount(1);
  await expect(empty).toContainText('No tickets yet');
  await page.screenshot({ path: '/private/tmp/hs2-ydrmad-project-empty-board-wide.png', fullPage: true });
});

test('shows view-specific feedback when a populated project has no tickets in the selected view', async ({ page }) => {
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [archiveRow] }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const empty = page.locator('[data-component="empty-state"]');
  await expect(empty).toHaveClass(/ticket-empty-state--view/);
  await expect(empty).toContainText('No tickets in Queue');
  await expect(empty).toContainText('Tickets will appear here when they enter this view.');
  await page.screenshot({ path: '/private/tmp/hs2-ydrmad-empty-queue-list-wide.png', fullPage: true });
  await page.getByLabel('Columns view').click();
  await expect(page.locator('[data-component="ticket-board"] [data-component="empty-state"]')).toHaveCount(1);
  await expect(empty).toContainText('No tickets in Queue');
  await page.screenshot({ path: '/private/tmp/hs2-ydrmad-empty-queue-board-wide.png', fullPage: true });
});

test('shows honest loading feedback while a selected view collection is unresolved', async ({ page }) => {
  let releaseArchive!: () => void;
  const archiveReady = new Promise<void>((resolve) => {
    releaseArchive = resolve;
  });
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() === 'GET' && url.searchParams.get('collection') === 'archive') {
      await archiveReady;
      return route.fulfill({
        json: {
          items: [archiveRow],
          counts: {
            total: 12,
            queued: 8,
            backlog: 1,
            archive: 3,
            open: 6,
            up_next: 1,
            active: 0,
            started: 3,
            completed_today: 0,
          },
        },
      });
    }
    return route.fallback();
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /Archive/ }).click();
  const loading = page.locator('[data-component="empty-state"].ticket-empty-state--view-loading');
  await expect(loading).toContainText('Loading Archive');
  await expect(loading).toContainText('Fetching tickets for this view…');
  await expect(loading).toHaveAttribute('aria-busy', 'true');
  await expect(page.getByText('No tickets in Archive')).toHaveCount(0);
  // The stale cached Archive count must not surface a misleading "0 of N loaded" progress or a Load more button while the view is still loading (HS2-WR2P01).
  await expect(page.getByRole('button', { name: 'Load more tickets' })).toHaveCount(0);
  await page.getByRole('button', { name: /Columns view/ }).click();
  const boardLoading = page.locator(
    '[data-component="ticket-board"] [data-component="empty-state"].ticket-empty-state--view-loading',
  );
  await expect(boardLoading).toContainText('Loading Archive');
  await expect(page.locator('.ticket-board-column__progress')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Load more tickets' })).toHaveCount(0);
  await page.getByRole('button', { name: /List view/ }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: '/private/tmp/hs2-wr2p01-view-loading-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.waitForTimeout(300);
  await page.screenshot({ path: '/private/tmp/hs2-wr2p01-view-loading-narrow.png', fullPage: true });
  releaseArchive();
  await expect(page.locator('[data-ticket-slug="HS2-ARCH01"]')).toBeVisible();
  await expect(loading).toHaveCount(0);
});

test('does not announce an empty project while its initial ticket collection is unresolved', async ({ page }) => {
  let releaseTickets!: () => void;
  const ticketsReady = new Promise<void>((resolve) => {
    releaseTickets = resolve;
  });
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', async (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() !== 'GET' || url.searchParams.has('text')) return route.fallback();
    await ticketsReady;
    return route.fulfill({ json: [] });
  });
  await page.setViewportSize({ width: 1920, height: 1040 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const loading = page.locator('[data-component="empty-state"].ticket-empty-state--loading');
  await expect(loading).toContainText('Loading tickets');
  await expect(loading).toContainText('Opening this project…');
  await expect(loading.locator('[data-component="loading-spinner"]')).toBeVisible();
  await page.getByLabel('Columns view').click();
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(loading).toBeVisible();
  await expect(page.locator('.app-loading')).toHaveCount(0);
  await expect(page.getByText('No tickets yet')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-e2az2n-loading-board-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await page.screenshot({ path: '/private/tmp/hs2-e2az2n-loading-board-narrow.png', fullPage: true });
  releaseTickets();
  await expect(page.getByText('No tickets yet')).toBeVisible();
});

test('leaves individual empty board columns blank when another column has tickets', async ({ page }) => {
  await mockProject(page);
  await page.route('**/checkouts/demo-checkout/tickets*', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [row] }) : route.fallback(),
  );
  await page.setViewportSize({ width: 1920, height: 1040 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  const verified = page.getByRole('region', { name: 'Verified column' });
  await expect(verified.getByLabel('0 tickets')).toBeVisible();
  await expect(verified.locator('[data-component="empty-state"]')).toHaveCount(0);
  await expect(verified.getByText('No tickets in Verified')).toHaveCount(0);
  await page.waitForTimeout(500);
  await page.screenshot({ path: '/private/tmp/hs2-m0frwd-empty-column-wide.png', fullPage: true });
});

test('requires confirmation before permanently emptying Trash', async ({ page }) => {
  const emptyRequests: string[] = [];
  await mockProject(page);
  let releaseEmptyTrash!: () => void;
  const emptyTrashGate = new Promise<void>((resolve) => {
    releaseEmptyTrash = resolve;
  });
  await page.route('**/trash/empty', async (route) => {
    await emptyTrashGate;
    await route.fallback();
  });
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/trash/empty'))
      emptyRequests.push(request.url());
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /Trash/ }).click();
  await expect(page.locator('[data-ticket-slug="HS2-DEL001"]')).toBeVisible();
  const emptyButton = page.getByRole('button', { name: 'Empty Trash' }),
    emptyHost = page.locator('[data-action="open-empty-trash"]'),
    emptyLabel = emptyHost.locator('.workspace-header__text-action-label'),
    emptyIcon = emptyLabel.locator('svg'),
    emptyText = emptyLabel.locator(':scope > span');
  const [iconBox, textBox] = await Promise.all([emptyIcon.boundingBox(), emptyText.boundingBox()]);
  expect(iconBox).not.toBeNull();
  expect(textBox).not.toBeNull();
  expect(Math.abs(iconBox!.y + iconBox!.height / 2 - (textBox!.y + textBox!.height / 2))).toBeLessThan(2);
  await emptyHost.screenshot({ path: '/private/tmp/hs2-rdvwzx-empty-trash-action.png' });
  await emptyButton.click();
  const dialog = page.locator('[data-component="empty-trash-dialog"]');
  await expect(dialog).toContainText('Permanently remove 1 ticket');
  await expect(dialog).toContainText('Git history will still contain the removed files');
  await page.screenshot({ path: '/private/tmp/hs2-rdvwzx-empty-trash-confirmation.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveCount(0);
  expect(emptyRequests).toEqual([]);
  await page.getByRole('button', { name: 'Empty Trash' }).click();
  await page.locator('[data-component="empty-trash-dialog"]').getByRole('button', { name: 'Empty Trash' }).click();
  await expect(page.locator('[data-component="empty-trash-dialog"]')).toHaveCount(0);
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Queue' })).toBeVisible();
  await expect(page.getByRole('button', { name: /Trash/ })).toHaveCount(0);
  await expect(page.locator('[data-component="not-working-dialog"]')).toHaveCount(0);
  await expect(page.getByText('Loading Queue')).toBeVisible();
  expect(emptyRequests).toHaveLength(1);
  await page.screenshot({ path: '/private/tmp/hs2-rdvwzx-empty-trash-optimistic.png', fullPage: true });
  releaseEmptyTrash();
  await expect(page.getByText('Emptied Trash — 1 ticket permanently removed.')).toBeVisible();
  await page.setViewportSize({ width: 780, height: 760 });
  await page.screenshot({ path: '/private/tmp/hs2-rdvwzx-empty-trash-complete-narrow.png', fullPage: true });
});

test('restores Trash and surfaces a persistent error when emptying fails', async ({ page }) => {
  await mockProject(page);
  await page.route('**/trash/empty', (route) =>
    route.fulfill({ status: 500, json: { error: 'Could not update the ticket store' } }),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /Trash/ }).click();
  await page.getByRole('button', { name: 'Empty Trash' }).click();
  await page.locator('[data-component="empty-trash-dialog"]').getByRole('button', { name: 'Empty Trash' }).click();
  await expect(page.locator('[data-component="empty-trash-dialog"]')).toHaveCount(0);
  await expect(page.getByRole('alert')).toContainText('Empty Trash failed: Could not update the ticket store');
  await expect(page.locator('#workspace-page-title')).toContainText('Trash');
  await expect(page.locator('[data-ticket-slug="HS2-DEL001"]')).toBeVisible();
  await page.waitForTimeout(350);
  await page.screenshot({ path: '/private/tmp/hs2-rdvwzx-empty-trash-error.png', fullPage: true });
});

test('derives board columns from the selected view and merges Verified by project setting', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  const board = page.locator('.ticket-board');
  await expect(board.locator('.ticket-board-column')).toHaveCount(4);
  await expect(board.locator('.ticket-board-column__title')).toHaveText([
    'Not Started',
    'Started',
    'Completed',
    'Verified',
  ]);
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Completed ticket');
  await expect(board.locator('[data-column-id="verified"]')).toContainText('Verified ticket');

  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Column view' }).click();
  await page.getByLabel('Hide Verified column').check();
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('hotsheet.project.demo-checkout.hide-verified-column')))
    .toBe('true');
  await page.getByLabel('Columns view').click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Not Started', 'Started', 'Completed']);
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Completed ticket');
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Verified ticket');

  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Column view' }).click();
  await page.getByLabel('Hide Verified column').uncheck();
  await page.getByLabel('Columns view').click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText([
    'Not Started',
    'Started',
    'Completed',
    'Verified',
  ]);

  await page.getByRole('button', { name: /Backlog/ }).click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Backlog']);
  await expect(board.locator('[data-column-id="backlog"]')).toContainText('Deferred backlog ticket');
  const backlogCard = board.locator('[data-column-id="backlog"] [data-component="ticket-list-row"]');
  expect(await backlogCard.evaluate((node) => parseFloat(getComputedStyle(node).borderRadius))).toBeGreaterThan(0);
  await expect(backlogCard).toHaveCSS('border-style', 'solid');
  await page.screenshot({ path: '/private/tmp/hs2-rzd9d4-backlog-column.png', fullPage: true });
  await page.getByRole('button', { name: /Archive/ }).click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Archive']);
  await expect(board.locator('[data-column-id="archive"]')).toContainText('Archived ticket');
  const archiveCards = board.locator('[data-column-id="archive"] [data-component="ticket-list-row"]');
  await expect(archiveCards).toHaveCount(2);
  await expect(board.locator('[data-ticket-slug="HS2-MOVED1"]')).toBeVisible();
  await expect(board.locator('[data-ticket-slug="HS2-VERIFY01"]')).toHaveCount(0);
  await expect(board.locator('[data-ticket-slug="HS2-DEL001"]')).toHaveCount(0);
  const archiveCard = archiveCards.first();
  expect(await archiveCard.evaluate((node) => parseFloat(getComputedStyle(node).borderRadius))).toBeGreaterThan(0);
  await expect(archiveCard).toHaveCSS('border-style', 'solid');
  await page.screenshot({ path: '/private/tmp/hs2-v20ewj-archive-column.png', fullPage: true });
});

test('switches large ticket views without cloning every row into motion ghosts', async ({ page }) => {
  const submissions: unknown[] = [];
  const statuses: Array<'not_started' | 'backlog' | 'archive'> = [
    ...Array.from({ length: 60 }, () => 'not_started' as const),
    ...Array.from({ length: 18 }, () => 'backlog' as const),
    ...Array.from({ length: 2_075 }, () => 'archive' as const),
  ];
  const largeRows = statuses.map((status, index) => ({
    ...row,
    id: `large-${index}`,
    native_id: `large-${index}`,
    qualified_id: `git-local:large-${index}`,
    slug: `HS2-LARGE${index}`,
    title: `Large collection ticket ${index}`,
    status,
    up_next: false,
  }));
  await mockProject(page);
  await page.route('**/__hotsheet/dev-review/tickets', async (route) => {
    submissions.push(route.request().postDataJSON());
    await route.fulfill({ status: 201, json: { slug: 'HS2-SHOULD-NOT-EXIST' } });
  });
  await page.route('**/checkouts/demo-checkout/tickets*', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: largeRows }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(60);
  for (const [view, count] of [
    ['Backlog', 18],
    ['Archive', 2_075],
    ['Queue', 60],
  ] as const) {
    const button = page.getByRole('button', { name: new RegExp(view) }),
      initial = await button.evaluate(
        (element, itemId) => {
          const started = performance.now();
          (element as HTMLElement).click();
          const list = document.querySelector<HTMLElement>('[data-component="ticket-list"]'),
            selected = document.querySelector<HTMLElement>(`[data-action="select-view"][data-item-id="${itemId}"]`);
          return {
            elapsed: performance.now() - started,
            selected: selected?.getAttribute('aria-current'),
            rendered: list?.dataset.renderedCount,
            total: list?.dataset.totalCount,
            loading: Boolean(list?.querySelector('[data-ticket-progressive-loading="true"]')),
          };
        },
        view === 'Queue' ? 'all' : view.toLowerCase(),
      );
    await expect(page.locator('#workspace-page-title')).toContainText(view);
    expect(initial.elapsed).toBeLessThan(250);
    expect(initial.selected).toBe('page');
    await expect(page.locator('[data-ticket-motion-ghost]')).toHaveCount(0);
    if (view === 'Archive') {
      expect(initial).toMatchObject({ rendered: '40', total: '2075', loading: true });
      await page.screenshot({ path: '/private/tmp/hs2-w52rer-progressive-archive.png' });
    }
    await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCount(count, { timeout: 20_000 });
    await expect(page.locator('[data-ticket-progressive-loading="true"]')).toHaveCount(0);
  }
  await page.waitForTimeout(300);
  expect(submissions).toEqual([]);
});

test('matches HS1 multi-selection semantics and selected outlines in list and column views', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const inspector = page.getByRole('complementary', { name: 'Ticket inspector' }),
    first = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START03"]'),
    second = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]');
  await expect(inspector).toContainText('Select a ticket to see and edit its details');
  await inspector.getByRole('button', { name: 'Hide ticket inspector' }).click();
  await page.getByRole('button', { name: 'Show ticket inspector' }).click();
  await expect(inspector).toBeVisible();
  await first.hover();
  await expect(first).toHaveCSS('border-color', 'color(srgb 0.42 0.729333 1)');
  await first.screenshot({ path: '/private/tmp/hs2-77bn46-hover-border.png' });
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await expect(inspector).toContainText('2 items selected — use batch actions to edit them together');
  await page.screenshot({ path: '/private/tmp/hotsheet-multi-selection-placeholder.png' });
  await expect(first).toHaveAttribute('data-selected', 'true');
  await expect(second).toHaveAttribute('data-selected', 'true');
  await expect(first).toHaveCSS('border-color', 'color(srgb 0.42 0.729333 1)');
  const selectedBoxes = await Promise.all([first, second].map((locator) => locator.boundingBox()));
  expect(Math.abs(selectedBoxes[0]!.y + selectedBoxes[0]!.height - selectedBoxes[1]!.y)).toBe(1);
  await page.getByLabel('Columns view').click();
  const started = page.locator('[data-column-id="started"]');
  const firstStarted = started.locator('[data-ticket-slug="HS2-DEMO01"]'),
    rangeEnd = started.locator('[data-ticket-slug="HS2-QQRY00"]');
  await firstStarted.click();
  await rangeEnd.click({ modifiers: ['Shift'] });
  await expect(started.locator('[data-selected="true"]')).toHaveCount(2);
  const startedCount = await started.locator('[data-component="ticket-list-row"]').count();
  await started.getByRole('button', { name: 'Select all Started tickets' }).click();
  await expect(started.locator('[data-selected="true"]')).toHaveCount(startedCount);
  await expect(inspector).toContainText(`${startedCount} items selected`);
  const crossColumn = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]'),
    closedReader = page.locator('[data-component="ticket-reader"][aria-hidden="true"]');
  await crossColumn.click({ modifiers: ['Shift'] });
  await expect(page.locator('.ticket-board [data-selected="true"]')).toHaveCount(1);
  await expect(crossColumn).toHaveAttribute('data-selected', 'true');
  await expect(crossColumn).toHaveCSS('border-color', 'color(srgb 0.42 0.729333 1)');
  await expect(closedReader).toBeHidden();
  expect(await closedReader.boundingBox()).toBeNull();
  await page.screenshot({ path: '/private/tmp/hs2-6thrgf-selected-board-wide.png' });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(closedReader).toBeHidden();
  expect(await closedReader.boundingBox()).toBeNull();
  await page.screenshot({ path: '/private/tmp/hs2-6thrgf-selected-board-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page
    .locator('[data-column-id="not-started"] .ticket-board-column__tickets')
    .click({ position: { x: 240, y: 300 } });
  await expect(page.locator('.ticket-board [data-selected="true"]')).toHaveCount(0);
  await expect(inspector).toContainText('Select a ticket to see and edit its details');
  await page.screenshot({ path: '/private/tmp/hotsheet-zero-selection-placeholder.png' });
});

test('drops ticket selection across ticket views but preserves it across view modes', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const inspector = page.locator('[data-component="ticket-inspector"]'),
    active = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await active.click();
  await expect(active).toHaveAttribute('data-selected', 'true');
  await expect(inspector).toContainText('HS2-DEMO01');
  await page.getByLabel('Columns view').click();
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]')).toHaveAttribute(
    'data-selected',
    'true',
  );
  await expect(inspector).toContainText('HS2-DEMO01');
  await page.locator('[data-action="select-view"][data-item-id="backlog"]').click();
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: 'Ticket inspector' })).toContainText(
    'Select a ticket to see and edit its details',
  );
  const backlog = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-BACK01"]');
  await backlog.click();
  await expect(backlog).toHaveAttribute('data-selected', 'true');
  await page.locator('[data-action="select-view"][data-item-id="archive"]').click();
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: 'Ticket inspector' })).toContainText(
    'Select a ticket to see and edit its details',
  );
});

test('styles completed and verified titles consistently in list and column rows', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const completed = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DONE01"] strong');
  await expect(completed).toHaveCSS('text-decoration-line', 'line-through');
  const quietColor = await completed.evaluate((node) => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--wa-color-neutral-on-quiet)';
    node.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  });
  await expect(completed).toHaveCSS('color', quietColor);
  await page.getByLabel('Columns view').click();
  const verified = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-VERIFY01"] strong');
  await expect(verified).toHaveCSS('text-decoration-line', 'line-through');
  await expect(verified).toHaveCSS('color', quietColor);
});

test('offers Up Next only on active tickets across rows and inspector', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-NEXT01"] [data-action="toggle-row-up-next"]')).toBeVisible();
  await expect(
    page.locator(
      '[data-component="ticket-list-row"][data-ticket-slug="HS2-DONE01"] [data-action="toggle-row-up-next"]',
    ),
  ).toHaveCount(0);
  await page.getByText('Completed ticket', { exact: true }).click();
  await expect(
    page.locator('[data-component="ticket-inspector"] [data-action="toggle-inspector-up-next"]'),
  ).toHaveCount(0);
  await page.locator('[data-action="select-view"][data-item-id="archive"]').click();
  await expect(
    page.locator(
      '[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"] [data-action="toggle-row-up-next"]',
    ),
  ).toHaveCount(0);
});

test('offers completed verification actions only for one completed ticket', async ({ page }) => {
  const patches = await mockProject(page, true, false, 0, 750);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const completed = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DONE01"]'),
    verified = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-VERIFY01"]'),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  await completed.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Verified', exact: true })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Not Working…', exact: true })).toBeVisible();
  await expect(menu.getByText('Toggle Up Next')).toHaveCount(0);
  await menu.getByRole('menuitem', { name: 'Verified', exact: true }).click();
  await expect.poll(() => patches.some((patch) => patch.status === 'verified')).toBe(true);
  await expect(completed).toContainText('Verified');
  await expect(menu).toHaveCount(0);
  await verified.click();
  await expect(verified).toHaveAttribute('data-selected', 'true');
  await completed.click({ modifiers: ['Meta'] });
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(2);
  await expect(completed).toHaveAttribute('data-selected', 'true');
  await verified.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Verified', exact: true })).toHaveCount(0);
  await expect(menu.getByRole('menuitem', { name: 'Not Working…', exact: true })).toHaveCount(0);
  await expect(menu.getByText('Toggle Up Next')).toHaveCount(0);
});

test('reopens Verified and Archive tickets into Not Started plus Up Next', async ({ page }) => {
  const patches = await mockProject(page, true, false, 0, 750);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const menu = page.getByRole('menu', { name: 'Ticket actions' }),
    verified = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-VERIFY01"]');
  await verified.click({ button: 'right' });
  const verifiedReopen = menu.getByRole('menuitem', { name: 'Reopen Ticket', exact: true });
  await expect(verifiedReopen).toBeVisible();
  await expect(verifiedReopen.locator('[data-lucide="rotate-ccw"]')).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Not Working…', exact: true })).toHaveCount(0);
  await verifiedReopen.click();
  let dialog = page.getByRole('dialog', { name: 'Reopen Ticket — HS2-VERIFY01' });
  await expect(dialog.getByRole('textbox', { name: 'What needs another attempt?' })).toBeFocused();
  await dialog.getByRole('textbox', { name: 'What needs another attempt?' }).fill('Verification exposed a regression.');
  await dialog
    .getByLabel('Browse evidence attachments')
    .setInputFiles({ name: 'verified-proof.png', mimeType: 'image/png', buffer: Buffer.from('png') });
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-bmhnsx-reopen-verified-wide.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Reopen Ticket' }).click();
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => patches.filter((patch) => patch.operation === 'not-working').length).toBe(1);
  await expect(verified).toHaveAttribute('data-status', 'not_started');
  await expect(verified.locator('[data-action="toggle-row-up-next"]')).toHaveAttribute(
    'aria-label',
    'Remove from Up Next',
  );
  await page.getByRole('button', { name: /Archive/ }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  const archived = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"]');
  await expect(archived).toBeVisible();
  await archived.click({ button: 'right' });
  const archiveReopen = menu.getByRole('menuitem', { name: 'Reopen Ticket', exact: true });
  await expect(archiveReopen).toBeVisible();
  await archiveReopen.click();
  dialog = page.getByRole('dialog', { name: 'Reopen Ticket — HS2-ARCH01' });
  const archiveNote = dialog.getByRole('textbox', { name: 'What needs another attempt?' });
  await archiveNote.fill('The archived result needs another pass.');
  await expect(archiveNote).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-bmhnsx-reopen-archive-narrow.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Reopen Ticket' }).click();
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => patches.filter((patch) => patch.operation === 'not-working').length).toBe(2);
  const mobileView = page.locator('wa-select[name="mobile-view"]');
  await mobileView.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'all';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const reopenedArchive = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"]');
  await expect(reopenedArchive).toHaveAttribute('data-status', 'not_started');
  await expect(reopenedArchive.locator('[data-action="toggle-row-up-next"]')).toHaveAttribute(
    'aria-label',
    'Remove from Up Next',
  );
});

test('hides Reopen Ticket when the provider cannot update tickets', async ({ page }) => {
  await mockProject(page, false);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const menu = page.getByRole('menu', { name: 'Ticket actions' });
  await page.locator('[data-ticket-slug="HS2-VERIFY01"]').click({ button: 'right' });
  await expect(menu.getByRole('menuitem', { name: 'Reopen Ticket', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: /Archive/ }).click();
  await page.locator('[data-ticket-slug="HS2-ARCH01"]').click({ button: 'right' });
  await expect(menu.getByRole('menuitem', { name: 'Reopen Ticket', exact: true })).toHaveCount(0);
});

test('reports a completed ticket with note and evidence through one atomic provider operation', async ({ page }) => {
  const patches = await mockProject(page);
  const requests: string[] = [];
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/not-working'))
      requests.push(request.postData() ?? '');
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const completed = page.locator('[data-ticket-slug="HS2-DONE01"]');
  await completed.click({ button: 'right' });
  await page.locator('[data-context-action="Report not working"]').click();
  const dialog = page.getByRole('dialog', { name: 'Not Working — HS2-DONE01' }),
    note = dialog.getByRole('textbox', { name: 'What’s wrong?' });
  await expect(note).toBeFocused();
  await note.fill('The fix regressed after restart.');
  const input = dialog.getByLabel('Browse evidence attachments');
  await input.setInputFiles({ name: 'proof ünicode.png', mimeType: 'image/png', buffer: Buffer.from('png') });
  await dialog.getByRole('button', { name: 'Report Not Working' }).click();
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(() =>
      patches.some(
        (patch) => patch.operation === 'not-working' && patch.status === 'not_started' && patch.up_next === true,
      ),
    )
    .toBe(true);
  expect(requests).toHaveLength(1);
  expect(requests[0]).toContain('The fix regressed after restart.');
  expect(requests[0]).toContain('proof ünicode.png');
  await expect(page.locator('[data-ticket-slug="HS2-DONE01"] [data-action="toggle-row-up-next"]')).toHaveAttribute(
    'aria-label',
    'Remove from Up Next',
  );
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('The fix regressed after restart.');
  await page.getByRole('tab', { name: 'Timeline' }).click();
  const timeline = page.locator('[data-component="ticket-timeline"]');
  await expect(timeline).toContainText('Brian reported as not working');
  await expect(timeline).not.toContainText('The fix regressed after restart.');
});

test('hides a pending Not Working report immediately and restores its draft on failure', async ({ page }) => {
  await mockProject(page);
  const deletes: string[] = [];
  let releaseFailure!: () => void,
    requestStarted = false;
  const failureReleased = new Promise<void>((resolve) => {
    releaseFailure = resolve;
  });
  page.on('request', (request) => {
    if (request.method() === 'DELETE') deletes.push(new URL(request.url()).pathname);
  });
  await page.route('**/not-working', async (route) => {
    requestStarted = true;
    await failureReleased;
    return route.fulfill({ status: 500, json: { error: 'atomic report failed' } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DONE01"]').click({ button: 'right' });
  await page
    .getByRole('menu', { name: 'Ticket actions' })
    .getByRole('menuitem', { name: 'Not Working…', exact: true })
    .click();
  const dialog = page.getByRole('dialog', { name: 'Not Working — HS2-DONE01' });
  await dialog
    .getByLabel('Browse evidence attachments')
    .setInputFiles({ name: 'failure.txt', mimeType: 'text/plain', buffer: Buffer.from('failure') });
  await dialog.getByRole('button', { name: 'Report Not Working' }).click();
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => requestStarted).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-8eeyxd-submission-hidden.png', fullPage: true });
  releaseFailure();
  await expect(dialog.getByRole('textbox', { name: 'What’s wrong?' })).toBeVisible();
  await expect(dialog.getByRole('alert')).toContainText('atomic report failed');
  await expect(dialog.getByText('failure.txt')).toBeVisible();
  expect(deletes).toEqual([]);
  await expect(dialog.getByRole('button', { name: 'Report Not Working' })).toBeEnabled();
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-8eeyxd-failure-restored.png', fullPage: true });
});

test('opens the matching ticket reader on row double-click in list and columns', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').dblclick();
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' })).toBeVisible();
  await page.getByRole('button', { name: 'Close ticket reader' }).click();
  await page.getByLabel('Columns view').click();
  await page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]').dblclick();
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-NEXT01' })).toBeVisible();
});

test('opens shared Markdown links safely in new tabs across the real inspector and reader', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]'),
    projectGuide = inspector.getByRole('link', { name: 'Project guide' });
  await expect(projectGuide).toHaveAttribute('target', '_blank');
  await expect(projectGuide).toHaveAttribute('rel', 'noopener noreferrer');
  const runbook = inspector.getByRole('link', { name: 'runbook' });
  await expect(runbook).toHaveAttribute('target', '_blank');
  await expect(runbook).toHaveAttribute('rel', 'noopener noreferrer');
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').dblclick();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-DEMO01' });
  await expect(reader.getByRole('link', { name: 'Project guide' })).toHaveAttribute('target', '_blank');
  await expect(reader.getByRole('link', { name: 'runbook' })).toHaveAttribute('rel', 'noopener noreferrer');
});

test('opens ticket references without leaving column view (HS2-230NY7)', async ({ page }) => {
  await mockProject(page);
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { store: 'git-local', ...full, details: 'Continue with HS2-START02.' } })
      : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Columns view').click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.locator('[data-component="ticket-inspector"]').getByRole('link', { name: 'HS2-START02' }).click();
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(page.getByLabel('Columns view')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]')).toHaveAttribute(
    'data-selected',
    'true',
  );
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-START02 in demo' })).toBeVisible();
});

test('resolves exact ticket links across projects and shows only a compact ambiguity chooser', async ({ page }) => {
  const sharedDemo = {
    ...row,
    id: 'shared-demo',
    native_id: 'shared-demo',
    qualified_id: 'git-local:shared-demo',
    slug: 'HS2-SHARED1',
    title: 'Demo shared ticket',
  };
  const otherUnique = {
    ...row,
    id: 'other-unique',
    native_id: 'other-unique',
    qualified_id: 'git-local:other-unique',
    slug: 'HS2-OTHER1',
    title: 'Other unique ticket',
  };
  const sharedOther = {
    ...row,
    id: 'shared-other',
    native_id: 'shared-other',
    qualified_id: 'git-local:shared-other',
    slug: 'HS2-SHARED1',
    title: 'Other shared ticket',
  };
  await page.setViewportSize({ width: 1180, height: 760 });
  await mockProject(page);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? { ...project, id: 'other-checkout', root, name: 'other', apiPath: '/__hotsheet/project-api/other-checkout' }
          : project,
    });
  });
  await page.route('**/__hotsheet/project-api/*/checkouts/*/tickets*', (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    const other = new URL(route.request().url()).pathname.includes('/other-checkout/');
    return route.fulfill({ json: other ? [otherUnique, sharedOther] : [row, sharedDemo] });
  });
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: {
            store: 'git-local',
            ...full,
            details:
              'Open HS2-OTHER1. Missing: HS2-MISSING. Choose HS2-SHARED1. Explicit: @other-checkout/HS2-SHARED1.',
            notes: [],
          },
        })
      : route.fallback(),
  );
  await page.route('**/tickets/other-unique', (route) =>
    route.fulfill({
      json: { store: 'git-local', ...otherUnique, details: 'Cross-project destination.', notes: [], attachments: [] },
    }),
  );
  await page.route('**/tickets/shared-other', (route) =>
    route.fulfill({
      json: {
        store: 'git-local',
        ...sharedOther,
        details: 'Chosen cross-project destination.',
        notes: [],
        attachments: [],
      },
    }),
  );
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  const otherTab = page.getByRole('tab', { name: 'other' }),
    otherUniqueRow = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-OTHER1"]');
  await expect(otherTab).toHaveAttribute('aria-selected', 'true');
  await expect(otherUniqueRow).toBeVisible();
  const openSource = async () => {
    const demoTab = page.getByRole('tab', { name: 'demo' });
    await demoTab.click();
    await expect(demoTab).toHaveAttribute('aria-selected', 'true');
    const sourceRow = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
    await expect(sourceRow).toBeVisible();
    await sourceRow.click();
    const inspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
    await expect(inspector.getByRole('link', { name: 'HS2-OTHER1', exact: true })).toBeVisible();
    return inspector;
  };
  let inspector = await openSource();
  await inspector.getByRole('link', { name: 'HS2-OTHER1', exact: true }).click();
  let linked = page.getByRole('dialog', { name: 'Read and edit HS2-OTHER1 in other' });
  await expect(linked).toContainText('Other unique ticket');
  await expect(page.getByRole('tab', { name: 'demo' })).toHaveAttribute('aria-selected', 'true');
  await linked.getByRole('button', { name: 'Close ticket reader' }).click();
  inspector = await openSource();
  await inspector.getByRole('link', { name: 'HS2-MISSING', exact: true }).click();
  await expect(page.locator('.app-toast')).toContainText('No exact match for HS2-MISSING.');
  await inspector.getByRole('link', { name: 'HS2-SHARED1', exact: true }).click();
  const choice = page.locator('[data-component="ticket-link-choice-dialog"]');
  await expect(choice).toHaveJSProperty('open', true);
  await expect(choice.getByRole('button').filter({ hasText: 'Demo shared ticket' })).toHaveCount(1);
  await expect(choice.getByRole('button').filter({ hasText: 'Other shared ticket' })).toHaveCount(1);
  await expect(page.locator('[name="global-search-query"]')).toHaveCount(0);
  await expect
    .poll(() =>
      choice.evaluate(
        (node) =>
          node.shadowRoot
            ?.querySelector('dialog')
            ?.getAnimations()
            .filter((animation) => animation.playState === 'running').length ?? -1,
      ),
    )
    .toBe(0);
  const chooserGeometry = await choice.evaluate((node) => {
    const intro = node.querySelector<HTMLElement>('.ticket-link-choice-dialog__body > p')!.getBoundingClientRect(),
      list = node.querySelector<HTMLElement>('.ticket-link-choice-dialog__matches')!.getBoundingClientRect(),
      rows = [...node.querySelectorAll<HTMLElement>('.ticket-link-choice-dialog__matches > li > button')].map((row) => {
        const box = row.getBoundingClientRect(),
          ticket = row.querySelector<HTMLElement>('.ticket-link-choice-dialog__ticket')!.getBoundingClientRect(),
          source = row.querySelector<HTMLElement>('.ticket-link-choice-dialog__source')!.getBoundingClientRect();
        return {
          top: box.top,
          bottom: box.bottom,
          ticketTop: ticket.top,
          ticketBottom: ticket.bottom,
          sourceTop: source.top,
          sourceBottom: source.bottom,
        };
      });
    return { introBottom: intro.bottom, listTop: list.top, rows };
  });
  expect(chooserGeometry.listTop - chooserGeometry.introBottom).toBeCloseTo(16, 0);
  expect(chooserGeometry.rows).toHaveLength(2);
  expect(chooserGeometry.rows[1].top).toBeGreaterThanOrEqual(chooserGeometry.rows[0].bottom - 1);
  for (const row of chooserGeometry.rows) {
    expect(row.ticketTop).toBeGreaterThanOrEqual(row.top);
    expect(row.ticketBottom).toBeLessThanOrEqual(row.bottom);
    expect(row.sourceTop).toBeGreaterThanOrEqual(row.top);
    expect(row.sourceBottom).toBeLessThanOrEqual(row.bottom);
  }
  await page.screenshot({ path: '/private/tmp/hs2-e729wg-ticket-link-choice-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-e729wg-ticket-link-choice-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1180, height: 760 });
  await choice.getByRole('button').filter({ hasText: 'Other shared ticket' }).click();
  linked = page.getByRole('dialog', { name: 'Read and edit HS2-SHARED1 in other' });
  await expect(linked).toContainText('Chosen cross-project destination.');
  await expect(page.getByRole('tab', { name: 'demo' })).toHaveAttribute('aria-selected', 'true');
  await linked.getByRole('button', { name: 'Close ticket reader' }).click();
  inspector = await openSource();
  await inspector.getByRole('link', { name: '@other-checkout/HS2-SHARED1', exact: true }).click();
  await expect(choice).toHaveCount(0);
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-SHARED1 in other' })).toContainText(
    'Chosen cross-project destination.',
  );
  await expect(page.getByRole('tab', { name: 'demo' })).toHaveAttribute('aria-selected', 'true');
});

test('auto-links a single-digit legacy HS-N reference to the imported ticket (HS2-XB5R3Y, HS2-T9TVYT)', async ({
  page,
}) => {
  const imported = {
    ...row,
    id: 'imp1',
    native_id: 'imp1',
    qualified_id: 'git-local:imp1',
    slug: 'HS2-IMPORTED1',
    title: 'Imported from Hot Sheet 1',
    legacy_number: 'HS-7',
  };
  await mockProject(page);
  // The source ticket's rendered details mention the old HS1 number.
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: { store: 'git-local', ...full, details: 'Superseded by legacy HS-7. Ignore AB-1.', notes: [] },
        })
      : route.fallback(),
  );
  // The FTS search the client runs on click surfaces the imported ticket carrying legacy_number.
  await page.route('**/checkouts/*/tickets*', (route) => {
    const url = new URL(route.request().url());
    return route.request().method() === 'GET' && url.searchParams.get('text') === 'HS-7'
      ? route.fulfill({ json: [imported] })
      : route.fallback();
  });
  // Opening the resolved match loads the imported ticket by its ULID.
  await page.route('**/tickets/imp1', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: { store: 'git-local', ...imported, details: 'The imported ticket body.', notes: [], attachments: [] },
        })
      : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  const legacyLink = inspector.getByRole('link', { name: 'HS-7', exact: true });
  await expect(legacyLink).toBeVisible();
  await expect(inspector.getByRole('link', { name: 'AB-1', exact: true })).toHaveCount(0);
  await legacyLink.click();
  const linked = page.getByRole('dialog', { name: /Read and edit HS2-IMPORTED1/ });
  await expect(linked).toBeVisible();
  await expect(linked).toContainText('The imported ticket body.');
  await page.waitForTimeout(200);
  await page.screenshot({ path: '/private/tmp/hs2-xb5r3y-legacy-link.png', fullPage: true });
});

test('ships TicketRow context-menu behavior through real list and board compositions', async ({ page }) => {
  const mutations: string[] = [];
  page.on('request', (request) => {
    if (['PATCH', 'POST'].includes(request.method())) mutations.push(new URL(request.url()).pathname);
  });
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]'),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await first.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(2);
  await menu.getByText('Toggle Up Next').click();
  await expect.poll(() => patches.filter((patch) => patch.up_next === true).length).toBe(2);
  expect(mutations.filter((path) => path.endsWith('/batch'))).toHaveLength(1);
  expect(mutations.filter((path) => path.includes('/tickets/'))).toHaveLength(0);
  const choose = async (field: 'category' | 'priority' | 'status', value: string) => {
    await first.click({ button: 'right' });
    await menu.locator(`wa-dropdown-item:not([slot="submenu"])`, { hasText: `Change ${field}` }).hover();
    const option = menu.locator(`[data-context-field="${field}"][data-context-value="${value}"]`);
    await expect(option).toBeVisible();
    if (field === 'category') {
      await page.waitForTimeout(200);
      await page.screenshot({ path: '/private/tmp/hotsheet-real-context-submenu.png' });
    }
    await option.click();
    await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(2);
    await expect.poll(() => patches.filter((patch) => patch[field] === value).length).toBe(2);
  };
  // "Open ticket" is hidden while multiple tickets are selected (HS2-XRENF2), so collapse to a
  // single selection before opening the reader from the menu.
  await choose('category', 'bug');
  await choose('priority', 'low');
  await choose('status', 'verified');
  expect(
    patches
      .filter((patch) => patch.category === 'bug' || patch.priority === 'normal' || patch.status === 'verified')
      .every((patch) => typeof patch.expected_token === 'string'),
  ).toBe(true);
  await first.click();
  await first.click({ button: 'right' });
  await menu.getByRole('menuitem', { name: 'Open ticket', exact: true }).click();
  await expect(page.getByRole('dialog', { name: /Read and edit HS2-DEMO01/ })).toBeVisible();
  await page.getByRole('button', { name: 'Close ticket reader' }).click();
  await page.getByLabel('Columns view').click();
  const boardRow = page.locator('[data-column-id="verified"] [data-ticket-slug="HS2-DEMO01"]');
  await boardRow.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hotsheet-real-context-menu.png' });
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
});

test('adds and removes tags and confirms deletion for a real multi-selection', async ({ page }) => {
  const patches = await mockProject(page),
    mutationRequests: string[] = [];
  page.on('request', (request) => {
    if (request.method() !== 'GET') mutationRequests.push(`${request.method()} ${new URL(request.url()).pathname}`);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]'),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  const selectBoth = async () => {
    await first.click();
    await second.click({ modifiers: ['Meta'] });
    await expect(page.locator('[data-action="select-ticket-row"][data-selected="true"]')).toHaveCount(2);
  };
  await selectBoth();
  await first.click({ button: 'right' });
  await menu.locator('[data-context-action="Add tag"]').click();
  let dialog = page.locator('[data-component="bulk-tag-dialog"]');
  await expect(dialog).toContainText('Add tag — 2 selected');
  await page.getByRole('textbox', { name: 'Tag to add *' }).fill('regression');
  await dialog.getByRole('button', { name: 'Add tag' }).click();
  await expect
    .poll(
      () =>
        patches.filter((patch) => Array.isArray(patch.tags) && (patch.tags as string[]).includes('regression')).length,
    )
    .toBe(2);
  await selectBoth();
  await first.click({ button: 'right' });
  await menu.locator('[data-context-action="Remove tag"]').click();
  dialog = page.locator('[data-component="bulk-tag-dialog"]');
  await expect(dialog).toContainText('Remove tag — 2 selected');
  await dialog.getByRole('button', { name: 'client' }).click();
  await dialog.getByRole('button', { name: 'Remove tag' }).click();
  await expect
    .poll(
      () => patches.filter((patch) => Array.isArray(patch.tags) && !(patch.tags as string[]).includes('client')).length,
    )
    .toBe(2);
  await selectBoth();
  await first.click({ button: 'right' });
  await menu.locator('[data-context-action="Delete ticket"]').click();
  const deletion = page.locator('[data-component="bulk-delete-dialog"]');
  await expect(deletion).toContainText('Delete 2 tickets?');
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-x7vkyj-bulk-delete-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/hs2-x7vkyj-bulk-delete-narrow.png', fullPage: true });
  await deletion.getByRole('button', { name: 'Delete 2 tickets' }).click();
  await expect.poll(() => patches.filter((patch) => patch.status === 'deleted').length).toBe(2);
  await expect(first).toHaveCount(0);
  expect(
    patches
      .filter((patch) => Array.isArray(patch.tags) || patch.status === 'deleted')
      .every((patch) => typeof patch.expected_token === 'string'),
  ).toBe(true);
  expect(mutationRequests.filter((value) => value.endsWith('/batch'))).toHaveLength(3);
  expect(mutationRequests.filter((value) => value.includes('/tickets/'))).toEqual([]);
});

test('falls back to visible best-effort progress when a provider cannot update atomically (HS2-967BWM)', async ({
  page,
}) => {
  const patches = await mockProject(page, true, false, 0, 0, 400, false, 2, false, false, false),
    mutationRequests: string[] = [];
  page.on('request', (request) => {
    if (request.method() !== 'GET') mutationRequests.push(`${request.method()} ${new URL(request.url()).pathname}`);
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]');
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await page.getByRole('button', { name: 'Toggle Up Next for selected tickets' }).click();
  await expect(page.locator('.app-toast')).toContainText(/Updating tickets… [01] of 2/);
  await page.screenshot({ path: '/private/tmp/hs2-967bwm-best-effort-progress.png', fullPage: true });
  await expect.poll(() => patches.filter((patch) => patch.up_next === true).length).toBe(2);
  expect(mutationRequests.filter((value) => value.endsWith('/batch'))).toEqual([]);
  expect(mutationRequests.filter((value) => value.includes('/tickets/'))).toHaveLength(2);
});

test('keeps successful best-effort writes when another selected ticket fails (HS2-967BWM)', async ({ page }) => {
  const patches = await mockProject(page, true, false, 0, 0, 0, false, 2, false, false, false);
  await page.route('**/tickets/08', (route) =>
    route.request().method() === 'PATCH'
      ? route.fulfill({ status: 409, json: { error: 'stale imported ticket' } })
      : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]');
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await page.getByRole('button', { name: 'Toggle Up Next for selected tickets' }).click();
  await expect(page.getByRole('alert')).toContainText(
    'Updated 1 of 2 tickets. 1 failed; HS2-START02: stale imported ticket',
  );
  await expect.poll(() => patches.filter((patch) => patch.up_next === true).length).toBe(1);
  await expect(first.locator('[data-action="toggle-row-up-next"]')).toHaveAttribute(
    'aria-label',
    'Remove from Up Next',
  );
  await expect(second.locator('[data-action="toggle-row-up-next"]')).toHaveAttribute('aria-label', 'Add to Up Next');
});

test('keeps optimistically archived verified tickets hidden through an intermediate change event', async ({ page }) => {
  const patches = await mockProject(page, true, false, 0, 1_500, 0, false, 2, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]'),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await first.click({ button: 'right' });
  await menu.locator('wa-dropdown-item:not([slot="submenu"])', { hasText: 'Change status' }).hover();
  await menu.locator('[data-context-field="status"][data-context-value="verified"]').click();
  await expect.poll(() => patches.filter((patch) => patch.status === 'verified').length, { timeout: 4_000 }).toBe(2);
  await page.waitForTimeout(1_700);
  await page.getByLabel('Columns view').click();
  const verified = page.locator('[data-column-id="verified"]'),
    verifiedFirst = verified.locator('[data-ticket-slug="HS2-DEMO01"]'),
    verifiedSecond = verified.locator('[data-ticket-slug="HS2-START02"]');
  await verifiedFirst.click();
  await verifiedSecond.click({ modifiers: ['Meta'] });
  await verifiedFirst.click({ button: 'right' });
  await menu.locator('[data-context-action="Archive ticket"]').click();
  await expect(verifiedFirst).toHaveCount(0);
  await expect(verifiedSecond).toHaveCount(0);
  await page.waitForTimeout(1_100);
  await expect(verifiedFirst).toHaveCount(0);
  await expect(verifiedSecond).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-215bdb-archive-stable-through-event.png', fullPage: true });
  await expect.poll(() => patches.filter((patch) => patch.status === 'archive').length, { timeout: 4_000 }).toBe(2);
  await expect(verifiedFirst).toHaveCount(0);
  await expect(verifiedSecond).toHaveCount(0);
});

test('applies selected-ticket toolbar actions and clears tickets moved outside the current view', async ({ page }) => {
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]'),
    star = page.getByRole('button', { name: 'Toggle Up Next for selected tickets' }),
    starHost = page.locator('button[data-action="toggle-selected-up-next"]'),
    more = page.getByRole('button', { name: 'More actions for selected tickets' }),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  await expect(star).toBeDisabled();
  await expect(more).toBeDisabled();
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await expect(star).toBeEnabled();
  await expect(more).toBeEnabled();
  await star.click();
  await expect.poll(() => patches.filter((patch) => patch.up_next === true).length).toBeGreaterThanOrEqual(1);
  await expect(starHost).toHaveAttribute('aria-pressed', 'true');
  await more.click();
  await expect(menu).toBeVisible();
  await expect(menu.getByText('Toggle Up Next')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-z4vzjm-selected-toolbar-actions.png', fullPage: true });
  await page.keyboard.press('Escape');
  await first.click();
  await more.click();
  await menu.getByText('Archive ticket').click();
  await expect.poll(() => patches.filter((patch) => patch.status === 'archive').length).toBe(1);
  await expect(first).toHaveCount(0);
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(0);
  await expect(page.getByText('Select a ticket to see and edit its details')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-czmm6x-hidden-selection-cleared.png', fullPage: true });
});

for (const surface of ['workspace', 'rail'] as const) {
  for (const theme of ['light', 'dark'] as const) {
    test(`projects native tri-state toolbar stars through ${surface} in ${theme} (HS2-WP15AF)`, async ({ page }) => {
      await page.setViewportSize({ width: 1728, height: 971 });
      await page.emulateMedia({ colorScheme: theme });
      await installFakeTerminalSockets(page, true);
      await mockProject(page);
      await page.goto('/');
      await page.getByRole('button', { name: 'Open project' }).click();
      await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
      if (surface === 'rail') await page.getByRole('button', { name: 'Workspace grid' }).click();
      else await page.getByRole('button', { name: 'Columns view', exact: true }).click();
      const rail = page.locator('[data-component="terminal-ticket-rail"]');
      const controls = surface === 'rail' ? rail : page.locator('.app-shell__main > .kui-toolbar');
      const star = controls.getByRole('button', { name: 'Toggle Up Next for selected tickets' });
      const icon = star.locator('.workspace-header__up-next-icon');
      const select = async (slug: string, toggle = false, single = true) => {
        await page
          .locator(`[data-component="ticket-list-row"][data-ticket-slug="${slug}"]`)
          .click({ modifiers: toggle ? ['Meta'] : [] });
        if (surface === 'rail' && single) {
          await expect(rail).toHaveAttribute('data-screen', 'ticket');
          await rail.getByRole('button', { name: 'Back to ticket list' }).click();
          await expect(rail).toHaveAttribute('data-screen', 'root');
        }
      };
      const expectState = async (state: 'none' | 'mixed' | 'all') => {
        await expect(star).toHaveAttribute('aria-pressed', state === 'mixed' ? 'mixed' : String(state === 'all'));
        await expect(icon).toHaveAttribute('data-up-next-state', state);
        await expect(icon.locator('svg')).toHaveCount(state === 'mixed' ? 2 : 1);
        await expect(icon.locator('svg').first()).toHaveCSS(
          'fill',
          state === 'all' ? await icon.evaluate((node) => getComputedStyle(node).color) : 'none',
        );
        if (state === 'mixed') {
          await expect(icon.locator('.workspace-header__up-next-fill')).toHaveCSS(
            'clip-path',
            'inset(0px 50% 0px 0px)',
          );
          await expect(icon.locator('.workspace-header__up-next-fill')).toHaveCSS(
            'fill',
            await icon.evaluate((node) => getComputedStyle(node).color),
          );
        }
        if (state !== 'none') await expect(icon).toHaveCSS('color', 'rgb(255, 204, 0)');
      };
      await expect(star).toBeDisabled();
      await expect(controls.locator('.workspace-header__utility-group wa-button')).toHaveCount(0);
      await select('HS2-DEMO01');
      await expectState('all');
      await star.click();
      await expectState('none');
      await star.click();
      await expectState('all');
      await page.mouse.move(0, 0);
      await star.evaluate((node) => {
        (node as HTMLButtonElement).blur();
      });
      await expect(star).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
      await expect(star).toHaveCSS('box-shadow', 'none');
      const shape = await star.evaluate((node) => ({
        width: node.getBoundingClientRect().width,
        height: node.getBoundingClientRect().height,
        radius: parseFloat(getComputedStyle(node).borderRadius),
      }));
      expect(shape.width).toBe(shape.height);
      expect(shape.radius).toBeGreaterThanOrEqual(shape.width / 2);
      await page.screenshot({ path: `/private/tmp/hs2-wp15af-${surface}-${theme}-all.png`, animations: 'disabled' });
      await select('HS2-START02', true, false);
      await expectState('mixed');
      if (surface === 'rail')
        await rail.locator('.terminal-ticket-rail__content').evaluate((node) => {
          node.scrollTop = 0;
        });
      await controls.screenshot({
        path: `/private/tmp/hs2-wp15af-${surface}-${theme}-mixed.png`,
        animations: 'disabled',
      });
      await page.setViewportSize({ width: surface === 'workspace' ? 390 : 1024, height: 844 });
      if (surface === 'workspace') {
        const overflow = controls.locator('.workspace-header__overflow');
        await overflow.getByRole('button', { name: 'More workspace controls' }).click();
        const item = overflow.locator('[data-workspace-overflow-action="toggle-selected-up-next"]');
        await expect(item).toHaveAccessibleName('Toggle Up Next: some selected tickets are Up Next');
        await expect(item.locator('.workspace-header__up-next-icon')).toHaveAttribute('data-up-next-state', 'mixed');
        await page.screenshot({
          path: `/private/tmp/hs2-wp15af-${surface}-${theme}-narrow.png`,
          animations: 'disabled',
        });
        await item.click();
      } else {
        await rail.screenshot({
          path: `/private/tmp/hs2-wp15af-${surface}-${theme}-narrow.png`,
          animations: 'disabled',
        });
        await star.click();
      }
      await page.setViewportSize({ width: 1728, height: 971 });
      await expectState('all');
      await star.click();
      await expectState('none');
      await select('HS2-START02');
      await expectState('none');
      await star.focus();
      await page.keyboard.press('Tab');
      await page.keyboard.press('Shift+Tab');
      await expect(star).toBeFocused();
      await expect(star).not.toHaveCSS('outline-style', 'none');
      await page.keyboard.press('Space');
      await expectState('all');
      await select('HS2-DONE01');
      await expect(star).toBeDisabled();
      await expectState('none');
      await page.screenshot({
        path: `/private/tmp/hs2-wp15af-${surface}-${theme}-completed-disabled.png`,
        animations: 'disabled',
      });
      await select('HS2-VERIFY01');
      await expect(star).toBeDisabled();
      await select('HS2-DEMO01');
      await expect(star).toBeEnabled();
      await star.click();
      await expectState('all');
      await select('HS2-DEMO01', true, false);
      await expect(star).toBeDisabled();
      await expectState('none');
    });
  }
  test(`preserves provider-disabled toolbar stars in ${surface} (HS2-WP15AF)`, async ({ page }) => {
    await page.setViewportSize({ width: 1728, height: 971 });
    await installFakeTerminalSockets(page, true);
    await mockProject(page, false);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open project' }).click();
    await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
    await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();
    if (surface === 'rail') await page.getByRole('button', { name: 'Workspace grid' }).click();
    const controls =
      surface === 'rail'
        ? page.locator('[data-component="terminal-ticket-rail"]')
        : page.locator('.app-shell__main > .kui-toolbar');
    await expect(controls.getByRole('button', { name: 'Toggle Up Next for selected tickets' })).toBeDisabled();
    await expect(controls.getByRole('button', { name: 'More actions for selected tickets' })).toBeDisabled();
  });
}

test('keeps every responsive-hidden workspace command keyboard and pointer accessible', async ({ page }) => {
  const patches = await mockProject(page);
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const toolbar = page.locator('.app-shell__main > .kui-toolbar'),
    overflow = toolbar.locator('.workspace-header__overflow'),
    trigger = overflow.getByRole('button', { name: 'More workspace controls' }),
    first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]');
  await expect(page.locator('section[data-region-id="app-inspector"]')).toBeVisible();
  await expect(overflow).toBeVisible();
  await expect(toolbar.locator('.workspace-header__utility-group')).toBeHidden();
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(overflow).toHaveJSProperty('open', true);
  const overflowStar = overflow.locator('[data-workspace-overflow-action="toggle-selected-up-next"]');
  await expect(overflowStar).toBeEnabled();
  await expect
    .poll(() => overflowStar.evaluate((item) => Boolean((item as HTMLElement & { active?: boolean }).active)))
    .toBe(true);
  await page.keyboard.press('Home');
  await expect.poll(() => overflowStar.evaluate((item) => item.matches(':focus'))).toBe(true);
  await page.keyboard.press('Enter');
  await expect.poll(() => patches.filter((patch) => patch.up_next === true).length).toBeGreaterThanOrEqual(1);
  await trigger.click();
  const overflowMore = overflow.locator('[data-workspace-overflow-action="open-selected-ticket-actions"]');
  await expect(overflowMore).toBeVisible();
  await overflowMore.click();
  await expect(page.getByRole('menu', { name: 'Ticket actions' })).toBeVisible();
  await page.keyboard.press('Escape');
  await trigger.click();
  const overflowSort = overflow.locator('[data-workspace-sort="priority"]');
  await expect(overflowSort).toBeVisible();
  await overflowSort.click();
  await expect(toolbar.locator('wa-select[name="workspace-sort"]')).toHaveJSProperty('value', 'priority');
  await trigger.click();
  await expect(overflow).toHaveJSProperty('open', true);
  await page.waitForTimeout(250);
  await page.screenshot({ path: '/private/tmp/kf-zfg6z5-toolbar-overflow.png', fullPage: true });
  await trigger.click();
  await expect(overflow).toHaveJSProperty('open', false);
  await toolbar.evaluate((node) => {
    node.style.width = '13rem';
  });
  await expect(toolbar.locator('.workspace-header__search-group')).toBeHidden();
  await expect(toolbar.locator('.workspace-header__identity')).toBeHidden();
  await trigger.click();
  await expect(overflow).toHaveJSProperty('open', true);
  await page.waitForTimeout(250);
  const overflowSearch = overflow.locator('[data-workspace-overflow-action="open-workspace-search"]');
  await expect(overflowSearch).toBeVisible();
  await overflowSearch.click();
  const search = toolbar.getByRole('searchbox', { name: 'Search tickets' });
  await expect(search).toBeVisible();
  await expect(search).toBeFocused();
  await expect(toolbar.locator('.workspace-header__actions')).toHaveAttribute('data-search-open', 'true');
  await expect(overflow).toBeHidden();
  await expect(overflow).toHaveJSProperty('open', false);
  expect(await toolbar.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/kf-zfg6z5-toolbar-search.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-vt4r56-overflow-search-1024.png', fullPage: true });
  await search.fill('');
  await page.locator('.app-shell__work-area').focus();
  await expect(search).toHaveCount(0);
  await toolbar.evaluate((node) => {
    node.style.width = '10rem';
  });
  await expect(toolbar.locator('.view-mode-switcher')).toBeHidden();
  await trigger.focus();
  await page.keyboard.press('Enter');
  const columns = overflow.locator('[data-view-mode="board"]');
  await expect(columns).toBeVisible();
  await columns.click();
  await expect(toolbar.locator('.view-mode-switcher [data-segment-value="board"]')).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  expect(await toolbar.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
});

test('dismisses ticket context menus on every true outside pointerdown', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]'),
    menu = page.getByRole('menu', { name: 'Ticket actions' });
  await first.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await menu.locator('wa-dropdown-item', { hasText: 'Change status' }).dispatchEvent('pointerdown');
  await expect(menu).toBeVisible();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await expect(menu).toHaveCount(0);
  await first.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await second.dispatchEvent('pointerdown');
  await expect(menu).toHaveCount(0);
  await second.click();
  await first.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
});

test('undoes, redoes, copies, pastes, and drags ticket mutations through the real shell', async ({ page }) => {
  const shortcut = process.platform === 'darwin' ? 'Meta' : 'Control';
  const creates: Record<string, unknown>[] = [];
  page.on('request', (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/tickets'))
      creates.push(request.postDataJSON());
  });
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const ticket = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await ticket.click();
  const nextPatch = () =>
    page.waitForResponse(
      (response) => response.request().method() === 'PATCH' && new URL(response.url()).pathname.endsWith('/tickets/01'),
    );
  await page.locator('wa-select[name="inspector-status"]').click();
  let response = nextPatch();
  await page.locator('wa-select[name="inspector-status"] wa-option[value="completed"]').click();
  await response;
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText(
    'Completed',
  );
  await expect.poll(() => patches.filter((patch) => patch.status === 'completed').length).toBe(1);
  await page.locator('.app-shell__work-area').focus();
  response = nextPatch();
  await page.keyboard.press(`${shortcut}+z`);
  await response;
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText(
    'Started',
  );
  await expect.poll(() => patches.filter((patch) => patch.status === 'started').length).toBe(1);
  await page.waitForTimeout(0);
  await page.locator('.app-shell__work-area').focus();
  response = nextPatch();
  await page.keyboard.press(`${shortcut}+Shift+z`);
  await response;
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText(
    'Completed',
  );
  await expect.poll(() => patches.filter((patch) => patch.status === 'completed').length).toBe(2);
  await ticket.focus();
  await page.keyboard.press(`${shortcut}+c`);
  await page.locator('.app-shell__work-area').focus();
  await page.keyboard.press(`${shortcut}+v`);
  await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
  await expect.poll(() => creates.at(-1)?.status).toBe('not_started');
  const copied = page.locator('[data-component="ticket-list-row"]', { hasText: 'Use real project tickets (Copy)' });
  await expect(copied).toHaveAttribute('data-status', 'not_started');
  await page.screenshot({ path: '/private/tmp/hs2-skp8cw-pasted-completed-ticket.png', fullPage: true });
  await ticket.evaluate((node) => {
    const transfer = new DataTransfer();
    node.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: transfer }));
    document
      .querySelector<HTMLElement>('[data-ticket-drop-status="backlog"]')!
      .dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  await expect(page.locator('[data-ticket-drop-status="backlog"]')).toHaveAttribute('data-dragging-ticket', 'true');
  await page.locator('[data-ticket-drop-status="backlog"]').dispatchEvent('drop');
  await page.locator('[data-ticket-drop-status="backlog"]').click();
  await expect(
    page.locator('[data-component="ticket-list-row"]', { hasText: 'Use real project tickets' }),
  ).toBeVisible();
});

test('drags single and selected tickets across columns, views, and the duplicate target', async ({ page }) => {
  const mutations: string[] = [];
  page.on('request', (request) => {
    if (['PATCH', 'POST'].includes(request.method()))
      mutations.push(`${request.method()} ${new URL(request.url()).pathname}`);
  });
  const patches = await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const first = page.locator('[data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-ticket-slug="HS2-START02"]'),
    dragTo = async (source: Locator, target: string) => {
      await source.evaluate((node, selector) => {
        const transfer = new DataTransfer();
        node.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: transfer }));
        document
          .querySelector<HTMLElement>(selector)!
          .dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
      }, target);
      await expect(page.locator(target)).toHaveAttribute('data-dragging-ticket', 'true');
      await page.locator(target).dispatchEvent('drop');
    };
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  const writesBefore = mutations.length;
  await dragTo(first, '[data-ticket-drop-status="not_started"][data-item-id="all"]');
  expect(mutations).toHaveLength(writesBefore);
  await dragTo(first, '[data-ticket-drop-status="backlog"]');
  await expect.poll(() => patches.filter((patch) => patch.status === 'backlog').length).toBe(2);
  expect(mutations.filter((value) => value.endsWith('/batch'))).toHaveLength(1);
  await page.locator('[data-ticket-drop-status="backlog"]').click();
  const backlogFirst = page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(backlogFirst).toBeVisible();
  await dragTo(backlogFirst, '[data-ticket-drop-status="not_started"][data-item-id="all"]');
  await expect.poll(() => patches.filter((patch) => patch.status === 'not_started').length).toBe(1);
  expect(mutations.filter((value) => value.endsWith('/batch'))).toHaveLength(1);
  expect(mutations.filter((value) => value.includes('/tickets/'))).toHaveLength(1);
  await page.locator('[data-ticket-drop-status="not_started"][data-item-id="all"]').click();
  await page.getByLabel('Columns view').click();
  const boardFirst = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-DEMO01"]');
  await boardFirst.click();
  await dragTo(boardFirst, '[data-column-id="completed"]');
  await expect.poll(() => patches.filter((patch) => patch.status === 'completed').length).toBe(1);
  await expect(page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await page.getByLabel('List view').click();
  const duplicateFirst = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),
    duplicateSecond = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEXT01"]');
  await duplicateFirst.click();
  await duplicateSecond.click({ modifiers: ['Meta'] });
  const createsBefore = mutations.filter((value) => value.endsWith('/tickets')).length;
  await dragTo(duplicateFirst, '[data-ticket-drop-action="duplicate"]');
  await expect.poll(() => mutations.filter((value) => value.endsWith('/tickets')).length).toBe(createsBefore + 2);
  await expect(page.locator('.app-toast')).toContainText('2 tickets copied to demo.');
  await page.screenshot({ path: '/private/tmp/hs2-w2743r-ticket-drag-targets.png', fullPage: true });
});

test('previews rows with a border-only outline and keeps column cards borderless until selected', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const row = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),
    background = await row.evaluate((node) => getComputedStyle(node).backgroundColor),
    shadow = await row.evaluate((node) => getComputedStyle(node).boxShadow);
  await row.hover();
  await expect(row).toHaveCSS('background-color', background);
  expect(await row.evaluate((node) => getComputedStyle(node).boxShadow)).toBe(shadow);
  await page.screenshot({ path: '/private/tmp/hs2-rxy39s-row-hover-after.png', fullPage: true });
  await page.getByLabel('Columns view').click();
  const card = page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]');
  await expect(card).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  await card.click();
  await expect(card).toHaveCSS('border-color', 'color(srgb 0.42 0.729333 1)');
  await page.screenshot({ path: '/private/tmp/hs2-2n2tcr-column-border-after.png', fullPage: true });
  const workArea = page.locator('.app-shell__work-area');
  const focusColor = await resolvedColor(workArea, 'var(--wa-color-focus)');
  await expect
    .poll(() => workArea.evaluate((node) => getComputedStyle(node, '::after').borderTopColor))
    .toBe(focusColor);
  expect(await workArea.evaluate((node) => getComputedStyle(node, '::after').zIndex)).toBe('20');
  await page.screenshot({ path: '/private/tmp/hs2-2rn2hh-work-area-focus-after.png', fullPage: true });
});

test('drops selected tickets on another project tab to copy them there', async ({ page }) => {
  const creates: Record<string, unknown>[] = [];
  await mockProject(page);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    if (root === '/work/other')
      return route.fulfill({
        status: 201,
        json: {
          ...project,
          id: 'other-checkout',
          root,
          name: 'other',
          apiPath: '/__hotsheet/project-api/other-checkout',
        },
      });
    return route.fulfill({ status: 201, json: project });
  });
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.includes('/checkouts/other-checkout/tickets'))
      creates.push(request.postDataJSON());
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await page.getByRole('tab', { name: 'demo' }).click();
  const first = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),
    second = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]');
  await expect(first).toBeVisible();
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await first.evaluate((node) => {
    const transfer = new DataTransfer();
    node.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: transfer }));
    document
      .querySelector<HTMLElement>('[data-ticket-drop-project="other-checkout"]')!
      .dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  const destination = page.locator('[data-ticket-drop-project="other-checkout"]');
  await expect(destination).toHaveAttribute('data-dragging-ticket', 'true');
  await destination.dispatchEvent('drop');
  await expect.poll(() => creates.length).toBe(2);
  expect(creates.map((create) => create.status)).toEqual(['not_started', 'not_started']);
  await expect(page.locator('.app-toast')).toContainText('2 tickets copied to other.');
  await expect(page.getByRole('tab', { name: 'demo' })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('tab', { name: 'other' }).click();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('Use real project tickets (Copy)');
  const copied = page.locator('[data-component="ticket-list-row"]', { hasText: 'Use real project tickets (Copy)' });
  await expect(copied).toHaveAttribute('data-status', 'not_started');
  await expect(copied).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-skp8cw-dropped-started-tickets.png', fullPage: true });
});

test('switches already-open projects from cache within one frame and rejects stale refreshes', async ({ page }) => {
  const otherRow = {
    ...row,
    id: 'other-01',
    native_id: 'other-01',
    qualified_id: 'git-local:other-01',
    slug: 'HS2-OTHER1',
    title: 'Other project cached ticket',
    claimed_by: 'worker',
    claim_lease_expires_at: '2099-09-11T12:00:00Z',
  };
  const otherQueuedRow = {
    ...otherRow,
    id: 'other-02',
    native_id: 'other-02',
    qualified_id: 'git-local:other-02',
    slug: 'HS2-OTHER2',
    title: 'Second cached Up Next ticket',
    claimed_by: undefined,
    claim_lease_expires_at: undefined,
  };
  let holdRefreshes = false;
  const pending = new Map<string, import('@playwright/test').Route>();
  await mockProject(page);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    if (root === '/work/other')
      return route.fulfill({
        status: 201,
        json: {
          ...project,
          id: 'other-checkout',
          root,
          name: 'other',
          apiPath: '/__hotsheet/project-api/other-checkout',
        },
      });
    return route.fulfill({ status: 201, json: project });
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route(/\/tickets(?:\?.*)?$/, (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    const path = new URL(route.request().url()).pathname,
      other = path.includes('/other-checkout/'),
      rows = other ? [otherRow, otherQueuedRow] : [row, notStartedRow];
    if (holdRefreshes) {
      pending.set(other ? 'other' : 'demo', route);
      return;
    }
    return route.fulfill({ json: rows });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toBeVisible();
  await page.getByLabel('Columns view').click();
  await page.waitForTimeout(500);
  await page.evaluate(() => {
    const state = window as typeof window & { __projectSwitchTicketGhosts?: string[] };
    state.__projectSwitchTicketGhosts = [];
    new MutationObserver((records) => {
      for (const record of records)
        for (const node of record.addedNodes)
          if (node instanceof Element)
            for (const ghost of [node, ...node.querySelectorAll('[data-ticket-motion-ghost]')])
              if (ghost.matches('[data-ticket-motion-ghost]'))
                state.__projectSwitchTicketGhosts!.push(
                  `${(ghost as HTMLElement).dataset.ticketMotionGhost}:${(ghost as HTMLElement).dataset.ticketMotionSlug}`,
                );
    }).observe(document.body, { childList: true, subtree: true });
  });
  holdRefreshes = true;
  const projectTabs = page.locator('[data-tab-kind="project"]');
  await expect(projectTabs.filter({ hasText: 'demo' }).locator('.project-tab__work')).toHaveAttribute(
    'aria-label',
    '1 Up Next ticket',
  );
  await expect(projectTabs.filter({ hasText: 'other' }).locator('.project-tab__work')).toHaveAttribute(
    'aria-label',
    '2 Up Next tickets, 1 active ticket',
  );
  await expect(projectTabs.filter({ hasText: 'other' }).locator('.project-tab__activity-ring')).toBeVisible();
  const switchWithinFrame = async (name: string) =>
    page.evaluate(async (label) => {
      const tab = [...document.querySelectorAll<HTMLElement>('[role="tab"]')].find(
        (item) => item.querySelector('.kui-app-tab__name')?.textContent === label,
      )!;
      const start = performance.now();
      tab.click();
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => {
          resolve();
        }),
      );
      return performance.now() - start;
    }, name);
  const observeRefreshPaintOrder = () =>
    page.evaluate(() => {
      const state = window as typeof window & {
        __projectSwitchPainted?: boolean;
        __projectRefreshBeforePaint?: boolean;
        __projectRefreshInstrumented?: boolean;
      };
      state.__projectSwitchPainted = false;
      state.__projectRefreshBeforePaint = false;
      if (!state.__projectRefreshInstrumented) {
        const nativeFetch = window.fetch.bind(window);
        window.fetch = (...args) => {
          const input = args[0],
            url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
          if (url.includes('/__hotsheet/project-api/') && !state.__projectSwitchPainted)
            state.__projectRefreshBeforePaint = true;
          return nativeFetch(...args);
        };
        state.__projectRefreshInstrumented = true;
      }
      requestAnimationFrame(() => {
        state.__projectSwitchPainted = true;
      });
    });
  await observeRefreshPaintOrder();
  expect(await switchWithinFrame('demo')).toBeLessThan(100);
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toHaveCount(0);
  await expect(page.locator('.app-loading')).toHaveCount(0);
  await expect.poll(() => pending.has('demo')).toBe(true);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __projectRefreshBeforePaint?: boolean }).__projectRefreshBeforePaint,
    ),
  ).toBe(false);
  await observeRefreshPaintOrder();
  expect(await switchWithinFrame('other')).toBeLessThan(100);
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toHaveCount(0);
  await expect.poll(() => pending.has('other')).toBe(true);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __projectRefreshBeforePaint?: boolean }).__projectRefreshBeforePaint,
    ),
  ).toBe(false);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __projectSwitchTicketGhosts?: string[] }).__projectSwitchTicketGhosts,
    ),
  ).toEqual([]);
  await pending
    .get('other')!
    .fulfill({ json: [{ ...otherRow, title: 'Other project refreshed ticket' }, otherQueuedRow] });
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toContainText('Other project refreshed ticket');
  await pending.get('demo')!.fulfill({ json: [{ ...row, title: 'Stale demo response must stay hidden' }] });
  await page.waitForTimeout(100);
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toContainText('Other project refreshed ticket');
  await expect(page.getByText('Stale demo response must stay hidden')).toHaveCount(0);
  await expect(projectTabs.filter({ hasText: 'demo' }).locator('.project-tab__work-count')).toHaveText('1');
  await expect(projectTabs.filter({ hasText: 'other' }).locator('.project-tab__work-count')).toHaveText('2');
  await page.screenshot({ path: '/private/tmp/hs2-q8n2q4-instant-project-switch-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await page.screenshot({ path: '/private/tmp/hs2-q8n2q4-instant-project-switch-narrow.png', fullPage: true });
});

test('keeps the new-ticket composer open when it is opened right after a project tab switch (HS2-T1F2VT)', async ({
  page,
}) => {
  const otherRow = {
    ...row,
    id: 'other-01',
    native_id: 'other-01',
    qualified_id: 'git-local:other-01',
    slug: 'HS2-OTHER1',
    title: 'Other project ticket',
  };
  let holdRefreshes = false;
  const pending = new Map<string, import('@playwright/test').Route>();
  await mockProject(page);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    if (root === '/work/other')
      return route.fulfill({
        status: 201,
        json: {
          ...project,
          id: 'other-checkout',
          root,
          name: 'other',
          apiPath: '/__hotsheet/project-api/other-checkout',
        },
      });
    return route.fulfill({ status: 201, json: project });
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route(/\/tickets(?:\?.*)?$/, (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    const other = new URL(route.request().url()).pathname.includes('/other-checkout/');
    if (holdRefreshes) {
      pending.set(other ? 'other' : 'demo', route);
      return;
    }
    return route.fulfill({ json: other ? [otherRow] : [row, notStartedRow] });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toBeVisible();
  // Switch back to demo with its activation refresh (and thus the session restore) held, then quickly open the composer.
  holdRefreshes = true;
  await page.getByRole('tab', { name: 'demo' }).click();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toBeVisible();
  // Release the held refresh so the delayed session restore runs; it must not close the composer the user just opened.
  await expect.poll(() => pending.has('demo')).toBe(true);
  await pending.get('demo')!.fulfill({ json: [row, notStartedRow] });
  await page.waitForTimeout(400);
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toBeVisible();
});

test('reorders project and terminal tabs while preserving project order and complete keyboard focus', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 840 });
  await mockProject(page);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? { ...project, id: 'other-checkout', root, name: 'other', apiPath: '/__hotsheet/project-api/other-checkout' }
          : project,
    });
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  const projectTabs = page.locator('[data-tab-kind="project"]'),
    projectNames = () => projectTabs.locator('.kui-app-tab__name').allTextContents();
  await expect.poll(projectNames).toEqual(['demo', 'other']);
  await projectTabs.nth(0).dragTo(projectTabs.nth(1), { targetPosition: { x: 70, y: 16 } });
  await expect.poll(projectNames).toEqual(['other', 'demo']);
  await expect(
    page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]')),
  ).resolves.toEqual(['/work/other', '/work/demo']);
  await projectTabs.getByRole('tab', { name: 'demo' }).click();
  await projectTabs.getByRole('tab', { name: 'demo' }).focus();
  const focusGeometry = await projectTabs
    .filter({ has: page.getByRole('tab', { name: 'demo' }) })
    .evaluate((element) => {
      const tab = element.getBoundingClientRect(),
        scroller = element.parentElement!.getBoundingClientRect();
      return { leftInset: tab.left - scroller.left, rightInset: scroller.right - tab.right };
    });
  expect(focusGeometry.leftInset).toBeGreaterThanOrEqual(3);
  expect(focusGeometry.rightInset).toBeGreaterThanOrEqual(3);
  await page.screenshot({ path: '/private/tmp/hs2-bkgfy7-q6f4f3-project-tab-focus-wide.png', fullPage: true });
  await page.setViewportSize({ width: 700, height: 720 });
  await expect(page.locator('wa-select[name="mobile-project"]')).toHaveJSProperty('value', 'demo-checkout');
  await page.screenshot({ path: '/private/tmp/hs2-bkgfy7-q6f4f3-project-tab-focus-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1100, height: 840 });
  await page.reload();
  await expect.poll(projectNames).toEqual(['other', 'demo']);
  await expect(projectTabs.getByRole('tab', { name: 'demo' })).toHaveAttribute('aria-selected', 'true');
  await projectTabs.getByRole('tab', { name: 'other' }).focus();
  await page.keyboard.press('Tab');
  await expect(projectTabs.getByRole('tab', { name: 'demo' })).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(projectTabs.getByRole('tab', { name: 'other' })).toBeFocused();
  const list = page.getByRole('button', { name: 'List view' }),
    columns = page.getByRole('button', { name: 'Columns view' });
  await list.focus();
  await page.keyboard.press('Tab');
  await expect(columns).toBeFocused();
  await page.setViewportSize({ width: 1100, height: 840 });
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    terminalTabs = drawer.locator('[data-tab-kind="terminal"]'),
    terminalNames = () => terminalTabs.getByRole('tab').allTextContents();
  await expect.poll(terminalNames).toEqual(['Codex Main', 'Tests']);
  await terminalTabs.nth(0).dragTo(terminalTabs.nth(1), { targetPosition: { x: 90, y: 16 } });
  await expect.poll(terminalNames).toEqual(['Tests', 'Codex Main']);
  await drawer.getByRole('button', { name: 'Hide terminal drawer' }).click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  await expect.poll(terminalNames).toEqual(['Tests', 'Codex Main']);
  await drawer.getByRole('tab', { name: 'Project grid' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(terminalTabs.getByRole('tab', { name: 'Tests' })).toBeFocused();
});

test('scopes ticket clipboard shortcuts to the focused work area and preserves native text copy and paste', async ({
  page,
  context,
}) => {
  const shortcut = process.platform === 'darwin' ? 'Meta' : 'Control';
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  const creates: string[] = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === 'POST' && path.endsWith('/tickets')) creates.push(path);
  });
  await mockProject(page);
  await page.goto('/');
  await page.evaluate(() => {
    (window as typeof window & { shortcutDefaults?: boolean[] }).shortcutDefaults = [];
    document.addEventListener('keydown', (event) => {
      if ((event.ctrlKey || event.metaKey) && ['c', 'v'].includes(event.key.toLowerCase()))
        queueMicrotask(() => {
          (window as typeof window & { shortcutDefaults: boolean[] }).shortcutDefaults.push(event.defaultPrevented);
        });
    });
  });
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const workArea = page.locator('.app-shell__work-area'),
    ticket = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),
    focusColor = await resolvedColor(workArea, 'var(--wa-color-focus)');
  await ticket.click();
  await expect(workArea).toHaveCSS('outline-width', '2px');
  await expect(workArea).toHaveCSS('outline-color', focusColor);
  await page.keyboard.press(`${shortcut}+c`);
  await expect
    .poll(() =>
      page.evaluate(() => (window as typeof window & { shortcutDefaults: boolean[] }).shortcutDefaults.at(-1)),
    )
    .toBe(true);
  const inspectorSlug = page.locator('[data-component="ticket-inspector"] [data-component="toolbar-text"]');
  await inspectorSlug.click();
  await inspectorSlug.evaluate((node) => {
    const range = document.createRange();
    range.selectNodeContents(node);
    const selection = getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
  });
  await expect(workArea).toHaveCSS('outline-width', '0px');
  await page.keyboard.press(`${shortcut}+c`);
  await expect
    .poll(() =>
      page.evaluate(() => (window as typeof window & { shortcutDefaults: boolean[] }).shortcutDefaults.at(-1)),
    )
    .toBe(false);
  expect(creates).toHaveLength(0);
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const search = page.getByRole('searchbox', { name: 'Search tickets' });
  await search.focus();
  await expect(search).toBeFocused();
  await page.evaluate(() => navigator.clipboard.writeText('QQRY00'));
  await page.keyboard.press(`${shortcut}+v`);
  await expect
    .poll(() =>
      page.evaluate(() => (window as typeof window & { shortcutDefaults: boolean[] }).shortcutDefaults.at(-1)),
    )
    .toBe(false);
  expect(creates).toHaveLength(0);
  await page.getByRole('button', { name: 'Clear search' }).click();
  await page.evaluate(() => getSelection()?.removeAllRanges());
  await workArea.focus();
  await expect(workArea).toHaveCSS('outline-width', '2px');
  await page.keyboard.press(`${shortcut}+v`);
  await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
  expect(creates).toHaveLength(1);
  await page.screenshot({ path: '/private/tmp/hs2-rg612c-work-area-focus-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await workArea.focus();
  await expect(workArea).toHaveCSS('outline-width', '2px');
  await page.screenshot({ path: '/private/tmp/hs2-rg612c-work-area-focus-narrow.png', fullPage: true });
});

test('preserves attachments and atomically undoes and redoes cut-paste', async ({ page }) => {
  const shortcut = process.platform === 'darwin' ? 'Meta' : 'Control';
  const copyRequests: unknown[] = [];
  page.on('request', (request) => {
    if (new URL(request.url()).pathname.endsWith('/provider-attachments/copy'))
      copyRequests.push(request.postDataJSON());
  });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const source = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await source.click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await expect(page.locator('[data-attachment-id="A1"]')).toBeVisible();
  await source.focus();
  await page.keyboard.press(`${shortcut}+x`);
  await page.locator('.app-shell__work-area').focus();
  await page.keyboard.press(`${shortcut}+v`);
  await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
  await expect.poll(() => copyRequests.length).toBe(1);
  expect(copyRequests[0]).toEqual({
    source: { connection_id: 'git-local', native_id: '01', attachment_id: 'A1' },
    destination: { connection_id: 'git-local', native_id: '02' },
  });
  await expect(source).toHaveCount(0);
  await page.keyboard.press(`${shortcut}+z`);
  await expect(source).toBeVisible();
  await expect(page.getByText('Use real project tickets (Copy)')).toHaveCount(0);
  await page.keyboard.press(`${shortcut}+Shift+z`);
  await expect(source).toHaveCount(0);
  await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
});

test('shows and resolves cross-project permission notifications with badges and history', async ({ page }) => {
  await mockProject(page);
  let pending = [
      {
        id: 7,
        connection: 'claude-session',
        tool: 'Edit',
        action: '/work/demo/src/main.ts',
        always_allow_supported: true,
      },
    ],
    answer: unknown;
  await page.route('**/connections', (route) =>
    route.fulfill({
      json: [{ id: 'claude-session', tool: 'Claude', project: '/work/demo', role: 'main', busy: true }],
    }),
  );
  await page.route('**/permissions', (route) => route.fulfill({ json: pending }));
  await page.route('**/permissions/7', (route) => {
    answer = route.request().postDataJSON();
    pending = [];
    return route.fulfill({ json: { connection: 'claude-session', decision: 'allow', persisted: true } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const popup = page.locator('[data-component="permission-request-popup"]');
  await expect(popup).toBeVisible();
  await expect(popup).toContainText('demo');
  await expect(popup).toContainText('/work/demo/src/main.ts');
  await expect(page.getByRole('button', { name: /Notifications view, 1 pending/ })).toBeVisible();
  await expect(page.locator('[data-tab-kind="project"] .project-tab__notification')).toContainText('1');
  const notificationsButton = page.getByRole('button', { name: /Notifications view/ });
  await notificationsButton.click();
  await expect(notificationsButton).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('.settings-navigation[aria-label="Notification views"]')).toBeVisible();
  await expect(page.locator('#workspace-page-title')).toContainText('Pending');
  await expect(page.locator('[data-component="notification-center"]')).toBeVisible();
  await expect(popup).toBeVisible();
  await expect(page.locator('.notification-inspector-empty')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hotsheet-permission-shell.png' });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expect(popup).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hotsheet-permission-shell-narrow.png' });
  await popup.getByRole('button', { name: 'Always Allow' }).click();
  await expect.poll(() => answer).toEqual({ decision: 'allow', scope: 'always' });
  await expect(popup).toHaveCount(0);
  await expect(page.getByText('allowed this kind of request')).toHaveCount(0);
  await page.getByRole('button', { name: /Last 24 Hours/ }).click();
  await expect(page.locator('#workspace-page-title')).toContainText('Last 24 Hours');
  await expect(page.getByText('allowed this kind of request')).toBeVisible();
  await page.getByRole('button', { name: /Last 7 Days/ }).click();
  await expect(page.locator('#workspace-page-title')).toContainText('Last 7 Days');
  await expect(page.getByText('allowed this kind of request')).toBeVisible();
});

test('scopes the notification center, navigation counts, and header badge to the selected project', async ({
  page,
}) => {
  const now = Date.now(),
    history = (projectId: string, projectName: string, id: number, action: string) => ({
      key: `${projectId}:${id}`,
      id,
      projectId,
      projectName,
      connection: `${projectId}-session`,
      tool: 'Bash',
      action,
      always_allow_supported: true,
      agent: 'Codex',
      role: 'main worker',
      receivedAt: now - 2_000,
      ignored: false,
      decision: 'allow',
      scope: 'once',
      resolvedAt: now - 1_000,
    });
  await page.addInitScript(
    (items) => {
      localStorage.setItem('hotsheet.permission-history', JSON.stringify(items));
    },
    [
      history('demo-checkout', 'demo', 70, 'demo history action'),
      history('other-checkout', 'other', 80, 'other history action'),
    ],
  );
  await mockProject(page);
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? { ...project, id: 'other-checkout', root, name: 'other', apiPath: '/__hotsheet/project-api/other-checkout' }
          : project,
    });
  });
  await page.route('**/connections', (route) => {
    const other = new URL(route.request().url()).pathname.includes('/other-checkout/'),
      projectRoot = other ? '/work/other' : '/work/demo',
      id = other ? 'other-checkout' : 'demo-checkout';
    return route.fulfill({
      json: [{ id: `${id}-session`, tool: 'Codex', project: projectRoot, role: 'main', busy: true }],
    });
  });
  await page.route('**/permissions', (route) => {
    const other = new URL(route.request().url()).pathname.includes('/other-checkout/'),
      id = other ? 8 : 7,
      projectRoot = other ? '/work/other' : '/work/demo';
    return route.fulfill({
      json: [
        {
          id,
          project: projectRoot,
          connection: `${other ? 'other-checkout' : 'demo-checkout'}-session`,
          tool: 'Bash',
          action: `${other ? 'other' : 'demo'} pending action`,
          always_allow_supported: true,
        },
      ],
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const popup = page.locator('[data-component="permission-request-popup"]');
  await popup.getByRole('button', { name: 'Ignore' }).click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  await popup.getByRole('button', { name: 'Ignore' }).click();
  const notifications = page.getByRole('button', { name: /Notifications view/ });
  await expect(notifications).toHaveAccessibleName('Notifications view, 1 pending');
  await notifications.click();
  const center = page.locator('[data-component="notification-center"]'),
    navigation = page.locator('.settings-navigation[aria-label="Notification views"]');
  await expect(center).toContainText('other pending action');
  await expect(center).not.toContainText('demo pending action');
  await expect(navigation.locator('.kui-list-item__count')).toHaveText(['1', '1', '1']);
  await navigation.getByRole('button', { name: /Last 7 Days/ }).click();
  await expect(center).toContainText('other history action');
  await expect(center).not.toContainText('demo history action');
  await page.getByRole('tab', { name: 'demo' }).click();
  await expect(notifications).toHaveAccessibleName('Notifications view, 1 pending');
  await expect(center).toContainText('demo history action');
  await expect(center).not.toContainText('other history action');
  await navigation.getByRole('button', { name: /Pending/ }).click();
  await expect(center).toContainText('demo pending action');
  await expect(center).not.toContainText('other pending action');
});

test('counts permission automation only while its popup is visible', async ({ page }) => {
  await page.clock.install();
  await mockProject(page);
  let pending = [
      {
        id: 8,
        connection: 'codex-session',
        tool: 'item/commandExecution/requestApproval',
        action: 'npm test',
        always_allow_supported: true,
      },
    ],
    answers = 0;
  await page.route('**/connections', (route) =>
    route.fulfill({
      json: [{ id: 'codex-session', tool: 'Codex', project: '/work/demo', role: 'worker', busy: true }],
    }),
  );
  await page.route('**/permissions', (route) => route.fulfill({ json: pending }));
  await page.route('**/permissions/8', (route) => {
    answers += 1;
    pending = [];
    return route.fulfill({ json: { connection: 'codex-session', decision: 'allow', persisted: false } });
  });
  await page.goto('/');
  await page.evaluate(() => {
    localStorage.setItem(
      'hotsheet.project.demo-checkout.permission-automation',
      JSON.stringify({ action: 'allow', delayMs: 15_000 }),
    );
  });
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const popup = page.locator('[data-component="permission-request-popup"]');
  await expect(popup).toContainText('Auto-allow in');
  await page.clock.fastForward(10_000);
  await expect(popup).toContainText('0:05');
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Permissions' }).click();
  const automation = page.locator('wa-select[name="permission-automation-action"]');
  await automation.click();
  await expect(automation).toHaveJSProperty('open', true);
  await resetRenderMetrics(page);
  await page.clock.fastForward(1_000);
  await expect(popup).toContainText('0:04');
  await expect(automation).toHaveJSProperty('open', true);
  expect((await renderMetrics(page))?.passes).toBe(0);
  await page.keyboard.press('Escape');
  await popup.getByRole('button', { name: 'Stop auto-allow countdown' }).click();
  await expect(popup.locator('.permission-request-card__countdown')).toHaveCount(0);
  await page.clock.fastForward(30_000);
  expect(answers).toBe(0);
  await popup.getByRole('button', { name: 'Ignore' }).click();
  await expect(popup).toHaveCount(0);
  await page.getByRole('button', { name: /Notifications view/ }).click();
  await page.locator('[data-component="notification-center"]').getByRole('button', { name: 'Allow Once' }).click();
  await expect.poll(() => answers).toBe(1);
});

test('persists and restores per-project permission automation settings', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Permissions' }).click();
  const action = page.locator('wa-select[name="permission-automation-action"]'),
    delay = page.locator('wa-select[name="permission-automation-delay"]');
  await expect(action).toHaveJSProperty('value', 'off');
  await expect(delay).toHaveJSProperty('disabled', true);
  await action.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'deny';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(delay).toHaveJSProperty('disabled', false);
  await delay.evaluate((node: HTMLElement & { value: string }) => {
    node.value = '120000';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('hotsheet.project.demo-checkout.permission-automation')))
    .toBe('{"action":"deny","delayMs":120000}');
  await page.getByLabel('List view').click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Permissions' }).click();
  await expect(action).toHaveJSProperty('value', 'deny');
  await expect(delay).toHaveJSProperty('value', '120000');
});

test('shows and persists the shared Trash retention period in Lifecycle settings', async ({ page }) => {
  const writes: unknown[] = [],
    requests: string[] = [];
  await mockProject(page);
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.endsWith('/trash-settings')) requests.push(`${request.method()} ${path}`);
    if (request.method() === 'PUT' && path.endsWith('/trash-settings')) writes.push(request.postDataJSON());
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Lifecycle' }).click();
  const settings = page.locator('[data-component="trash-settings"]'),
    input = settings.locator('wa-input[name="trash-cleanup-days"]');
  await expect(settings).toContainText('Git history keeps every purged ticket file');
  await expect(input).toHaveJSProperty('value', '30');
  await input.evaluate((node: HTMLElement & { value: string }) => {
    node.value = '14';
  });
  await settings.getByRole('button', { name: 'Save retention' }).click();
  await expect(settings.getByRole('status')).toContainText('Saved for this project.');
  expect(writes).toEqual([{ trash_cleanup_days: 14 }]);
  expect(requests).toEqual([
    'GET /__hotsheet/project-api/demo-checkout/checkouts/demo-checkout/trash-settings',
    'PUT /__hotsheet/project-api/demo-checkout/checkouts/demo-checkout/trash-settings',
  ]);
  await expect(page.getByText('Trash retention saved.')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-ew2wxa-trash-retention-wide.png', fullPage: true });
  await page.getByLabel('List view').click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Lifecycle' }).click();
  await expect(input).toHaveJSProperty('value', '14');
  await page.setViewportSize({ width: 760, height: 720 });
  await page.screenshot({ path: '/private/tmp/hs2-ew2wxa-trash-retention-narrow.png', fullPage: true });
});

test('shares the settings category across projects but scopes command drafts to each project (HS2-4J50K3, HS2-G9FMQJ)', async ({
  page,
}) => {
  const demoCommands = [{ id: 'demo-command', title: 'Demo checks', program: 'npm', args: ['test'] }],
    otherCommands = [{ id: 'other-command', title: 'Other checks', program: 'cargo', args: ['test'] }];
  await mockProject(page);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? { ...project, id: 'other-checkout', root, name: 'other', apiPath: '/__hotsheet/project-api/other-checkout' }
          : project,
    });
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route('**/__hotsheet/project-api/*/commands', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: new URL(route.request().url()).pathname.includes('/other-checkout/') ? otherCommands : demoCommands,
        })
      : route.fallback(),
  );
  await page.route('**/__hotsheet/project-api/*/command-runs', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [] }) : route.fallback(),
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  let editor = page.locator('[data-component="command-settings-editor"]');
  const commandDialog = page.locator('#command-editor-dialog');
  await editor
    .locator('.command-settings-editor__row')
    .first()
    .locator('.command-settings-editor__row-menu-trigger')
    .click();
  await editor
    .locator('.command-settings-editor__row')
    .first()
    .locator('[data-action="edit-command-setting"]')
    .dispatchEvent('click');
  await expect(commandDialog.getByLabel('Button label')).toHaveValue('Demo checks');
  await commandDialog.getByLabel('Button label').fill('Unsaved demo draft');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await expect.poll(() => commandDialog.evaluate((node) => node.matches(':popover-open'))).toBe(false);
  await expect(editor).toContainText('Unsaved demo draft');
  // Adding a project keeps the same settings category (shared across projects — HS2-4J50K3), so the
  // new project opens straight onto Commands rather than resetting to Ticket sources.
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('region', { name: 'Commands settings' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Commands', exact: true })).toHaveAttribute('aria-current', 'page');
  editor = page.locator('[data-component="command-settings-editor"]');
  await editor
    .locator('.command-settings-editor__row')
    .first()
    .locator('.command-settings-editor__row-menu-trigger')
    .click();
  await editor
    .locator('.command-settings-editor__row')
    .first()
    .locator('[data-action="edit-command-setting"]')
    .dispatchEvent('click');
  await expect(commandDialog.getByLabel('Button label')).toHaveValue('Other checks');
  await expect(editor).not.toContainText('Unsaved demo draft');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await page.screenshot({ path: '/private/tmp/hs2-g9fmqj-other-project-settings-wide.png', fullPage: true });
  await page.getByRole('tab', { name: 'demo' }).click();
  editor = page.locator('[data-component="command-settings-editor"]');
  await expect(page.getByRole('region', { name: 'Commands settings' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Commands', exact: true })).toHaveAttribute('aria-current', 'page');
  await editor
    .locator('.command-settings-editor__row')
    .first()
    .locator('.command-settings-editor__row-menu-trigger')
    .click();
  await editor
    .locator('.command-settings-editor__row')
    .first()
    .locator('[data-action="edit-command-setting"]')
    .dispatchEvent('click');
  await expect(commandDialog.getByLabel('Button label')).toHaveValue('Unsaved demo draft');
  await expect(editor).not.toContainText('Other checks');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await page.setViewportSize({ width: 1024, height: 640 });
  await page.screenshot({ path: '/private/tmp/hs2-g9fmqj-demo-project-settings-narrow.png', fullPage: true });
});

test('live project visual review', async ({ page }) => {
  test.skip(!process.env.HOTSHEET_LIVE_PROJECT, 'opt-in local visual review');
  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(error.message));
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page
    .locator('wa-input[name="project-root"]')
    .evaluate(
      (node: HTMLElement & { value: string }, value) => (node.value = value),
      process.env.HOTSHEET_LIVE_PROJECT,
    );
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-component="ticket-list-row"]').first()).toBeVisible({ timeout: 15_000 });
  const loaded = page.waitForResponse(
    (response) => response.url().includes('/tickets/') && response.request().method() === 'GET',
  );
  await page.locator('[data-component="ticket-list-row"]').first().click();
  expect((await loaded).status()).toBe(200);
  expect(pageErrors).toEqual([]);
  await expect(page.locator('[data-component="ticket-inspector"]')).toBeVisible();
  if (process.env.HOTSHEET_LIVE_ATTACHMENT) {
    await page.getByRole('tab', { name: /Attachments/ }).click();
    const uploaded = page.waitForResponse(
      (response) => response.url().endsWith('/attachments') && response.request().method() === 'POST',
    );
    await page.getByLabel('Browse and add attachments').setInputFiles(process.env.HOTSHEET_LIVE_ATTACHMENT);
    expect((await uploaded).status()).toBe(201);
    const filename = process.env.HOTSHEET_LIVE_ATTACHMENT.split('/').at(-1)!;
    await expect(page.locator('[data-component="ticket-attachments"]')).toContainText(filename);
    await page.getByRole('button', { name: `Remove ${filename}` }).click();
    await expect(page.locator('[data-component="ticket-attachments"]')).not.toContainText(filename);
  }
  await page.screenshot({ path: '/private/tmp/hotsheet-real-app.png', fullPage: true });
  await page.setViewportSize({ width: 900, height: 760 });
  await expect(page.locator('[data-component="ticket-list-row"]').first()).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hotsheet-real-app-narrow.png', fullPage: true });
});

test('anchors ticket context menus to the pointer while preserving scroller positions (HS2-H4MWDB, HS2-SWC9E4)', async ({
  page,
}) => {
  const base = {
    connection_id: 'git-local',
    native_id: '01',
    qualified_id: 'git-local:01',
    id: '01',
    slug: 'HS2-DEMO01',
    title: 'Ticket',
    category: 'feature',
    priority: 'high',
    status: 'started',
    up_next: false,
    tags: ['client'],
    blocked_by: [],
    claim_count: 0,
    created_at: '2026-08-30T00:00:00Z',
    updated_at: '2026-08-30T01:00:00Z',
  };
  const many = Array.from({ length: 40 }, (_, i) => ({
    ...base,
    id: String(100 + i),
    native_id: String(100 + i),
    qualified_id: `git-local:${100 + i}`,
    slug: `HS2-ROW${String(i).padStart(2, '0')}`,
    title: `Scrollable ticket number ${i}`,
    ...(i === 39 ? { status: 'completed' } : {}),
  }));
  const proj = {
    id: 'demo-checkout',
    root: '/work/demo',
    name: 'demo',
    stores: ['/work/demo.hs2'],
    apiPath: '/__hotsheet/project-api/demo-checkout',
  };
  await page.route('**/*', async (route) => {
    const req = route.request(),
      path = new URL(req.url()).pathname;
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: proj });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities: {
              create: true,
              update: true,
              close: true,
              notes: true,
              note_edit: true,
              note_delete: true,
              attachments: true,
              assignment: true,
              review_requests: true,
              dependencies: true,
              up_next: true,
              close_reasons: true,
              claims: true,
              atomic_batch: true,
              not_working_report: true,
              offline_mutation: true,
              history: true,
              watch: true,
              provider_idempotency: true,
              query_fields: [],
            },
          },
        ],
      });
    if (path.endsWith('/permissions') && req.method() === 'GET') return route.fulfill({ json: [] });
    if (path.endsWith('/connections') && req.method() === 'GET') return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/tickets') && req.method() === 'GET') return route.fulfill({ json: many });
    if (path.includes('/tickets/') && req.method() === 'GET') {
      const id = path.split('/').pop();
      const t = many.find((r) => r.id === id) || many[0];
      return route.fulfill({
        json: { store: 'git-local', ...t, details: 'body', blocked_reason: null, notes: [], attachments: [] },
      });
    }
    return route.continue();
  });
  await page.setViewportSize({ width: 1400, height: 820 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const menu = page.locator('.ticket-context-menu[role="menu"]');
  await expect(page.locator('[data-action="select-ticket-row"]').first()).toBeVisible();
  // List view: scroll the workspace, then open the context menu on a currently-visible row.
  const list = await page.evaluate(() => {
    const ws = document.querySelector('.app-shell__workspace') as HTMLElement;
    ws.scrollTop = 400;
    const before = ws.scrollTop;
    const rows = [...document.querySelectorAll('[data-action="select-ticket-row"]')];
    const r = ws.getBoundingClientRect();
    const v = rows.find((el) => {
      const b = el.getBoundingClientRect();
      return b.top >= r.top + 10 && b.bottom <= r.bottom - 10;
    }) as HTMLElement;
    const b = v.getBoundingClientRect(),
      pointer = { x: b.left + 80, y: b.top + 10 };
    v.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: pointer.x, clientY: pointer.y }),
    );
    return { before, pointer };
  });
  await expect(menu).toBeVisible();
  const listMenu = await menu.locator('wa-dropdown').evaluate((node) => {
    const rect = (node.shadowRoot!.querySelector('[part="menu"]') as HTMLElement).getBoundingClientRect();
    return { x: rect.x, y: rect.y };
  });
  expect(
    Math.abs(listMenu.x - list.pointer.x),
    JSON.stringify({ listMenu, pointer: list.pointer }),
  ).toBeLessThanOrEqual(2);
  expect(Math.abs(listMenu.y - list.pointer.y)).toBeLessThanOrEqual(2);
  expect(await page.evaluate(() => (document.querySelector('.app-shell__workspace') as HTMLElement).scrollTop)).toBe(
    list.before,
  );
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  // Board view: scroll a column's ticket list, then open the context menu on a visible row in it.
  await page.getByRole('button', { name: 'Columns view' }).click();
  await expect(page.locator('.ticket-board-column__tickets').first()).toBeVisible();
  const board = await page.evaluate(() => {
    const col = [...document.querySelectorAll('.ticket-board-column__tickets')].find(
      (c) => c.scrollHeight > c.clientHeight + 50,
    ) as HTMLElement;
    col.scrollTop = 300;
    const before = col.scrollTop;
    const rows = [...col.querySelectorAll('[data-action="select-ticket-row"]')];
    const r = col.getBoundingClientRect();
    const v = rows.find((el) => {
      const b = el.getBoundingClientRect();
      return b.top >= r.top + 10 && b.bottom <= r.bottom - 10;
    }) as HTMLElement;
    const b = v.getBoundingClientRect(),
      pointer = { x: b.left + 80, y: b.top + 10 };
    v.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: pointer.x, clientY: pointer.y }),
    );
    return { before, pointer, colId: col.closest('[data-column-id]')!.getAttribute('data-column-id') };
  });
  await expect(menu).toBeVisible();
  const boardMenu = await menu.locator('wa-dropdown').evaluate((node) => {
    const rect = (node.shadowRoot!.querySelector('[part="menu"]') as HTMLElement).getBoundingClientRect();
    return { x: rect.x, y: rect.y };
  });
  expect(Math.abs(boardMenu.x - board.pointer.x)).toBeLessThanOrEqual(2);
  expect(Math.abs(boardMenu.y - board.pointer.y)).toBeLessThanOrEqual(2);
  expect(
    await page.evaluate((id) => {
      const col = document.querySelector(`[data-column-id="${id}"] .ticket-board-column__tickets`) as HTMLElement;
      return col.scrollTop;
    }, board.colId),
  ).toBe(board.before);
  await page.screenshot({ path: '/private/tmp/hs2-swc9e4-scrolled-context-menu.png', fullPage: true });
  await page.keyboard.press('Escape');
  const edgePointer = { x: 600, y: 812 };
  await page.evaluate((pointer) => {
    document
      .querySelector<HTMLElement>('[data-ticket-slug="HS2-ROW39"]')!
      .dispatchEvent(
        new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: pointer.x, clientY: pointer.y }),
      );
  }, edgePointer);
  await expect(menu).toBeVisible();
  const edgeMenu = await menu.locator('wa-dropdown').evaluate((node) => {
    const rect = (node.shadowRoot!.querySelector('[part="menu"]') as HTMLElement).getBoundingClientRect(),
      host = node.closest('.ticket-context-menu')!.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
      host: { left: host.left, top: host.top },
    };
  });
  expect(
    Math.min(Math.abs(edgeMenu.top - edgePointer.y), Math.abs(edgeMenu.bottom - edgePointer.y)),
    JSON.stringify({ edgeMenu, edgePointer }),
  ).toBeLessThanOrEqual(2);
  await page.screenshot({ path: '/private/tmp/hs2-swc9e4-bottom-edge-context-menu.png', fullPage: true });
});

test('remembers scroll per project, mode and view through delayed loading and shrinking contents (HS2-PDYXYJ)', async ({
  page,
}) => {
  test.setTimeout(60000);
  const makeRows = (prefix: string) =>
    Array.from({ length: 280 }, (_, index) => ({
      ...row,
      id: `${prefix}-${index}`,
      native_id: `${prefix}-${index}`,
      qualified_id: `git-local:${prefix}-${index}`,
      slug: `HS2-${prefix}${String(index).padStart(4, '0')}`,
      title: `${prefix === 'A' ? 'Demo' : 'Other'} scroll ticket ${String(index).padStart(3, '0')}`,
      status: index >= 160 ? 'backlog' : index % 2 ? 'started' : 'not_started',
      up_next: false,
    }));
  let demoRows = makeRows('A');
  const otherRows = makeRows('B');
  await installFakeTerminalSockets(page, true);
  await mockProject(page, true, false, 0, 0, 0, false, 12);
  await page.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json:
        root === '/work/other'
          ? { ...project, id: 'other-checkout', root, name: 'other', apiPath: '/__hotsheet/project-api/other-checkout' }
          : project,
    });
  });
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/other' } }));
  await page.route(/\/tickets(?:\?.*)?$/, async (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    const url = new URL(route.request().url()),
      rows = url.pathname.includes('/other-checkout/') ? otherRows : demoRows,
      backlog = rows.filter((item) => item.status === 'backlog'),
      queue = rows.filter((item) => item.status !== 'backlog'),
      filtered = url.searchParams.get('status') === 'backlog' ? backlog : queue,
      offset = Number(url.searchParams.get('cursor') ?? 0),
      size = Number(url.searchParams.get('page_size') ?? 200),
      items = filtered.slice(offset, offset + size);
    await new Promise((resolve) => setTimeout(resolve, 150));
    return route.fulfill({
      json: {
        items,
        counts: {
          total: rows.length,
          queued: queue.length,
          backlog: backlog.length,
          archive: 0,
          trash: 0,
          open: queue.length,
          up_next: 0,
          active: 0,
          started: queue.filter((item) => item.status === 'started').length,
          completed_today: 0,
        },
        ...(offset + size < filtered.length ? { next_cursor: String(offset + size) } : {}),
      },
    });
  });
  await page.setViewportSize({ width: 1400, height: 760 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const workspace = page.locator('.app-shell__workspace'),
    tickets = workspace.locator('[data-component="ticket-list-row"]'),
    scroll = () => workspace.evaluate((node) => node.scrollTop),
    setScroll = (top: number) =>
      workspace.evaluate((node, value) => {
        node.scrollTop = value;
        return node.scrollTop;
      }, top),
    selectView = (view: string) => page.locator(`[data-action="select-view"][data-item-id="${view}"]`).click();
  await expect(tickets).toHaveCount(160);
  const queueTop = await setScroll(4500);
  expect(queueTop).toBe(4500);
  await selectView('backlog');
  await expect(tickets).toHaveCount(120);
  await expect.poll(scroll).toBe(0);
  const backlogTop = await setScroll(2100);
  await selectView('all');
  await expect(tickets).toHaveCount(160);
  await expect.poll(scroll).toBe(queueTop);
  await page.getByLabel('Columns view').click();
  const columns = workspace.locator('.ticket-board-column__tickets');
  await expect(columns).toHaveCount(4);
  await expect.poll(scroll).toBe(0);
  await columns.evaluateAll((nodes) => {
    nodes.forEach((node, index) => {
      node.scrollTop = 250 + index * 180;
    });
  });
  const boardPositions = await columns.evaluateAll((nodes) => nodes.map((node) => node.scrollTop));
  expect(boardPositions.slice(0, 2)).toEqual([250, 430]);
  await page.getByLabel('List view').click();
  await expect.poll(scroll).toBe(queueTop);
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(tickets).toHaveCount(160);
  await expect(tickets.first()).toContainText('Other scroll ticket');
  await expect.poll(scroll).toBe(0);
  const otherTop = await setScroll(1200);
  await page.getByRole('tab', { name: 'demo', exact: true }).click();
  await expect(tickets.first()).toContainText('Demo scroll ticket');
  await expect.poll(scroll).toBe(queueTop);
  await page.screenshot({
    path: '/private/tmp/hs2-pdyxyj-project-list-restored.png',
    fullPage: true,
    animations: 'disabled',
  });
  await selectView('backlog');
  await expect(tickets).toHaveCount(120);
  await expect.poll(scroll).toBe(backlogTop);
  await selectView('all');
  await page.getByLabel('Columns view').click();
  await expect.poll(() => columns.evaluateAll((nodes) => nodes.map((node) => node.scrollTop))).toEqual(boardPositions);
  await page.screenshot({
    path: '/private/tmp/hs2-pdyxyj-board-columns-restored.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(columns).toHaveCount(0);
  await expect(tickets).toHaveCount(160);
  await expect.poll(scroll).toBe(queueTop);
  await page.screenshot({
    path: '/private/tmp/hs2-pdyxyj-mobile-list-restored.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 1400, height: 760 });
  await expect.poll(() => columns.evaluateAll((nodes) => nodes.map((node) => node.scrollTop))).toEqual(boardPositions);
  await page.getByRole('tab', { name: 'other', exact: true }).click();
  await page.getByLabel('List view').click();
  await expect.poll(scroll).toBe(otherTop);
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const grid = workspace.locator('[data-ticket-scroll-owner="terminal-grid"]');
  await expect(grid).toBeVisible();
  const gridTop = await grid.evaluate((node) => {
    node.scrollTop = 250;
    return node.scrollTop;
  });
  expect(gridTop).toBe(250);
  await page.getByRole('tab', { name: 'other', exact: true }).click();
  await expect.poll(scroll).toBe(otherTop);
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await expect.poll(() => grid.evaluate((node) => node.scrollTop)).toBe(gridTop);
  await page.screenshot({
    path: '/private/tmp/hs2-pdyxyj-terminal-grid-restored.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.getByRole('tab', { name: 'other', exact: true }).click();
  // Change the inactive project's contents, then revisit the saved list destination.
  demoRows = demoRows.filter((item) => item.status === 'backlog').concat(demoRows.slice(0, 20));
  await page.getByRole('tab', { name: 'demo', exact: true }).click();
  await page.getByLabel('List view').click();
  await expect(tickets).toHaveCount(20);
  await expect
    .poll(() => workspace.evaluate((node) => node.scrollTop === Math.max(0, node.scrollHeight - node.clientHeight)))
    .toBe(true);
  const clamped = await scroll();
  expect(clamped).toBeLessThan(queueTop);
  expect(clamped).toBeGreaterThan(0);
  await page.screenshot({
    path: '/private/tmp/hs2-pdyxyj-shrunken-list-clamped.png',
    fullPage: true,
    animations: 'disabled',
  });
  await selectView('backlog');
  await expect(tickets).toHaveCount(120);
  await expect.poll(scroll).toBe(backlogTop);
  await selectView('all');
  await expect(tickets).toHaveCount(20);
  await expect.poll(scroll).toBe(clamped);
});

test('preserves list and every board-column scroll position across ticket mutations (HS2-CEBNAJ)', async ({ page }) => {
  // Keep the real progressive-render callback pending while the user scrolls and mutates.
  // A busy browser can defer idle work even after every board column is already visible.
  await page.addInitScript(() => {
    let nextId = 0;
    const callbacks = new Map<number, IdleRequestCallback>();
    window.requestIdleCallback = (callback) => {
      callbacks.set(++nextId, callback);
      return nextId;
    };
    window.cancelIdleCallback = (id) => callbacks.delete(id);
    Object.assign(window, {
      flushTicketIdleWork() {
        const pending = [...callbacks.values()];
        callbacks.clear();
        for (const callback of pending) callback({ didTimeout: true, timeRemaining: () => 0 });
      },
    });
  });
  const statuses = ['not_started', 'started', 'completed', 'verified'] as const,
    items = Array.from({ length: 120 }, (_, index) => ({
      ...row,
      id: String(200 + index),
      native_id: String(200 + index),
      qualified_id: `git-local:${200 + index}`,
      slug: `HS2-SC${String(index).padStart(4, '0')}`,
      title: `Scrollable mutation ticket ${index}`,
      status: statuses[index % 4],
      up_next: false,
    }));
  let rows = [...items];
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname;
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities: {
              create: true,
              update: true,
              close: true,
              notes: true,
              note_edit: true,
              note_delete: true,
              attachments: true,
              assignment: true,
              review_requests: true,
              dependencies: true,
              up_next: true,
              close_reasons: true,
              claims: true,
              atomic_batch: true,
              not_working_report: true,
              offline_mutation: true,
              history: true,
              watch: true,
              provider_idempotency: true,
              query_fields: [],
            },
          },
        ],
      });
    if (path.endsWith('/permissions') || path.endsWith('/connections')) return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/tickets') && request.method() === 'GET') return route.fulfill({ json: rows });
    if (path.endsWith('/batch') && request.method() === 'POST') {
      const updates = request.postDataJSON().updates as Array<{ id: string; status: (typeof statuses)[number] }>;
      rows = rows.map((item) => {
        const update = updates.find((value) => value.id === item.id);
        return update ? { ...item, status: update.status } : item;
      });
      return route.fulfill({
        json: updates.map((update) => ({
          ...rows.find((item) => item.id === update.id),
          details: '',
          blocked_reason: null,
          notes: [],
          attachments: [],
          concurrency_token: `next-${update.id}`,
        })),
      });
    }
    if (path.includes('/tickets/') && request.method() === 'GET') {
      const id = path.split('/').pop(),
        ticket = rows.find((item) => item.id === id);
      return route.fulfill({
        json: {
          store: 'git-local',
          ...ticket,
          details: '',
          blocked_reason: null,
          notes: [],
          attachments: [],
          concurrency_token: `token-${id}`,
        },
      });
    }
    return route.continue();
  });
  await page.setViewportSize({ width: 1400, height: 760 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const workspace = page.locator('.app-shell__workspace');
  const listTarget = page.locator('[data-component="ticket-list-row"][data-status="completed"]').nth(8),
    listSlug = await listTarget.getAttribute('data-ticket-slug');
  await listTarget.scrollIntoViewIfNeeded();
  const listBefore = await workspace.evaluate((node) => node.scrollTop);
  expect(listBefore).toBeGreaterThan(0);
  await listTarget.click({ button: 'right' });
  await expect.poll(() => workspace.evaluate((node) => node.scrollTop)).toBe(listBefore);
  await page.getByRole('menu', { name: 'Ticket actions' }).locator('[data-context-action="Verify ticket"]').click();
  await expect(page.locator(`[data-component="ticket-list-row"][data-ticket-slug="${listSlug}"]`)).toHaveAttribute(
    'data-status',
    'verified',
  );
  await expect.poll(() => workspace.evaluate((node) => node.scrollTop)).toBe(listBefore);
  await page.getByRole('button', { name: 'Columns view' }).click();
  const columns = page.locator('.ticket-board-column__tickets');
  await expect(columns).toHaveCount(4);
  const positions = () =>
    columns.evaluateAll(
      (nodes) =>
        Object.fromEntries(
          nodes.map((node) => [
            node.closest('[data-column-id]')!.getAttribute('data-column-id'),
            (node as HTMLElement).scrollTop,
          ]),
        ) as Record<string, number>,
    );
  for (let attempt = 0; attempt < 4; attempt += 1) {
    if (attempt === 3) {
      // Settle the previously delayed work, then exercise another edit after reset.
      await page.evaluate(() => {
        (window as typeof window & { flushTicketIdleWork(): void }).flushTicketIdleWork();
      });
    }
    await columns.evaluateAll((nodes) => {
      nodes.forEach((node, index) => {
        (node as HTMLElement).scrollTop = 260 + index * 20;
      });
    });
    const boardTarget = page.locator('[data-column-id="completed"] [data-component="ticket-list-row"]').nth(5),
      boardSlug = await boardTarget.getAttribute('data-ticket-slug'),
      scrolled = await positions();
    expect(Object.values(scrolled).every((top) => top > 0)).toBe(true);
    await boardTarget.click({ button: 'right' });
    const before = await positions();
    // Opening the context menu is already a render and must not lose pending user scrolling.
    expect(Object.values(before).every((top) => top > 0)).toBe(true);
    await page.getByRole('menu', { name: 'Ticket actions' }).locator('[data-context-action="Verify ticket"]').click();
    await expect(page.locator(`[data-column-id="verified"] [data-ticket-slug="${boardSlug}"]`)).toBeVisible();
    const after = await positions();
    for (const [id, scrollTop] of Object.entries(before)) {
      expect(after[id]).toBeGreaterThan(0);
      expect(Math.abs(after[id] - scrollTop)).toBeLessThanOrEqual(2);
    }
  }
  await page.screenshot({
    path: '/private/tmp/hs2-q9z4km-board-scroll-wide.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 900, height: 760 });
  await expect(page.locator('.app-shell')).toHaveAttribute('data-mobile', 'true');
  await expect(page.locator('[data-component="ticket-list-row"]')).not.toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-q9z4km-scroll-narrow.png', fullPage: true, animations: 'disabled' });
});

test('dismisses every remaining dialog through its native close event (HS2-3RQ7V7)', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('tab', { name: /Tests/ }).click({ button: 'right' });
  await page.getByRole('menu', { name: 'Terminal tab actions' }).getByText('Rename…').click();
  let dialog = page.locator('[data-terminal-rename-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveJSProperty('open', false);
  await drawer.getByRole('tab', { name: /Tests/ }).click({ button: 'right' });
  await page.getByRole('menu', { name: 'Terminal tab actions' }).getByText('Rename…').click();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveJSProperty('open', false);
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await page.getByRole('button', { name: 'Manage workspace visibility' }).click();
  const visibility = page.locator('[data-terminal-visibility-dialog]');
  await visibility.getByRole('button', { name: 'Add visibility group' }).click();
  dialog = page.locator('[data-terminal-visibility-name-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveJSProperty('open', false);
  await page.keyboard.press('Escape');
  await page.getByRole('tab', { name: /demo/ }).click();
  const first = page.locator('[data-component="ticket-list-row"]').first(),
    second = page.locator('[data-component="ticket-list-row"]').nth(1);
  await first.click();
  await second.click({ modifiers: ['Meta'] });
  await first.click({ button: 'right' });
  await page.getByRole('menu', { name: 'Ticket actions' }).locator('[data-context-action="Add tag"]').click();
  dialog = page.locator('[data-component="bulk-tag-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await page.locator('[data-ticket-slug="HS2-DONE01"]').click({ button: 'right' });
  await page
    .getByRole('menu', { name: 'Ticket actions' })
    .getByRole('menuitem', { name: 'Not Working…', exact: true })
    .click();
  dialog = page.locator('[data-component="not-working-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  const command = page.getByRole('button', { name: 'Run checks' });
  await command.dispatchEvent('pointerdown');
  await page.waitForTimeout(600);
  await command.dispatchEvent('pointerup');
  dialog = page.locator('[data-component="command-run-dialog"]');
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-3rq7v7-native-dialog-dismissal.png', fullPage: true });
});

test('edits and contains a wrapping attachment group title through the production inspector (HS2-C0R4MX, HS2-YCTQCZ)', async ({
  page,
}) => {
  const writes: Record<string, unknown>[] = [],
    longTitle =
      'After: real MOV pointer scrub and timed annotation state restoration with decoded paused frames and persistent annotation controls';
  let liveFull = {
    ...full,
    attachments: full.attachments.map((item) => ({
      ...item,
      batch_id: undefined as string | undefined,
      batch_label: undefined as string | undefined,
    })),
  };
  await mockProject(page);
  await page.route('**/tickets/01', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: { store: 'git-local', ...liveFull } })
      : route.fallback(),
  );
  await page.route('**/tickets/01/attachments', async (route) => {
    if (route.request().method() !== 'PATCH') return route.fallback();
    const body = route.request().postDataJSON() as Record<string, unknown>;
    writes.push(body);
    liveFull = {
      ...liveFull,
      attachments: [
        { ...liveFull.attachments[0], batch_id: body.batch_id as string, batch_label: body.batch_label as string },
      ],
    };
    return route.fulfill({ json: { store: 'git-local', ...liveFull } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  const batch = page.locator('[data-attachment-group-drop-target]').first(),
    title = batch.getByRole('button', { name: 'Edit batch label Legacy / Uncategorized' });
  await title.dblclick();
  const editor = batch.getByRole('textbox', { name: 'Batch label for Legacy / Uncategorized' });
  await expect(editor).toBeFocused();
  await editor.fill(longTitle);
  await editor.press('Enter');
  await expect.poll(() => writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({ attachment_ids: ['A1'], batch_label: longTitle });
  const wrappedTitle = batch.getByRole('button', { name: `Edit batch label ${longTitle}` });
  await expect(wrappedTitle).toBeFocused();
  const expectWrappedAndContained = async () => {
    const geometry = await wrappedTitle.evaluate((element) => {
      const rect = element.getBoundingClientRect(),
        container = element.closest<HTMLElement>('[data-attachment-group-drop-target]')!.getBoundingClientRect(),
        style = getComputedStyle(element);
      return {
        height: rect.height,
        lineHeight: parseFloat(style.lineHeight),
        clientHeight: element.clientHeight,
        scrollHeight: element.scrollHeight,
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
        left: rect.left,
        right: rect.right,
        containerLeft: container.left,
        containerRight: container.right,
        viewportWidth: innerWidth,
      };
    });
    expect(geometry.height).toBeGreaterThan(geometry.lineHeight * 1.5);
    expect(geometry.scrollHeight).toBeLessThanOrEqual(geometry.clientHeight + 1);
    expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.clientWidth + 1);
    expect(geometry.left).toBeGreaterThanOrEqual(geometry.containerLeft);
    expect(geometry.right).toBeLessThanOrEqual(geometry.containerRight);
    expect(geometry.left).toBeGreaterThanOrEqual(0);
    expect(geometry.right).toBeLessThanOrEqual(geometry.viewportWidth);
  };
  await expectWrappedAndContained();
  await page.screenshot({ path: '/private/tmp/hs2-yctqcz-attachment-group-title-wide.png', fullPage: true });
  await page.setViewportSize({ width: 1024, height: 600 });
  await expectWrappedAndContained();
  await page.screenshot({ path: '/private/tmp/hs2-yctqcz-attachment-group-title-narrow.png', fullPage: true });
});

test('moves a media thumbnail between batches and removes active media from its gallery menu (HS2-9PA2KD, HS2-EDX5J3)', async ({
  page,
}) => {
  const metadataWrites: Record<string, unknown>[] = [],
    uploads: string[] = [];
  let ticket = {
    ...full,
    attachments: [
      { ...full.attachments[0], batch_id: 'before', batch_label: 'Before' },
      { id: 'A2', filename: 'second.svg', created_at: '2026-08-30T00:41:00Z', batch_id: 'after', batch_label: 'After' },
    ],
  };
  await mockProject(page);
  await page.route('**/tickets/01**', async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname;
    if (path.endsWith('/tickets/01') && request.method() === 'GET')
      return route.fulfill({ json: { store: 'git-local', ...ticket } });
    if (path.endsWith('/tickets/01/attachments') && request.method() === 'POST') {
      uploads.push(request.postData() ?? '');
      return route.fallback();
    }
    if (path.endsWith('/tickets/01/attachments') && request.method() === 'PATCH') {
      const body = request.postDataJSON() as Record<string, unknown> & { attachment_ids: string[] };
      metadataWrites.push(body);
      ticket = {
        ...ticket,
        attachments: ticket.attachments.map((item) =>
          body.attachment_ids.includes(item.id)
            ? { ...item, batch_id: String(body.batch_id), batch_label: String(body.batch_label) }
            : item,
        ),
      };
      return route.fulfill({ json: { store: 'git-local', ...ticket } });
    }
    if (path.endsWith('/tickets/01/attachments/A1') && request.method() === 'DELETE') {
      ticket = { ...ticket, attachments: ticket.attachments.filter((item) => item.id !== 'A1') };
      return route.fulfill({ json: { store: 'git-local', ...ticket } });
    }
    return route.fallback();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  const thumbnail = page.getByRole('button', { name: 'Open proof.png in media gallery' }),
    attachments = page.locator('[data-component="ticket-attachments"]');
  await expect(thumbnail).toHaveAttribute('draggable', 'true');
  await thumbnail.evaluate((node, target) => {
    const transfer = new DataTransfer();
    node.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: transfer }));
    document
      .querySelector<HTMLElement>(target)!
      .dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  }, '[data-attachment-batch="after"]');
  await expect.poll(() => metadataWrites).toHaveLength(1);
  expect(metadataWrites[0]).toMatchObject({ attachment_ids: ['A1'], batch_id: 'after', batch_label: 'After' });
  expect(uploads).toHaveLength(0);
  await attachments.screenshot({ path: '/private/tmp/hs2-9pa2kd-media-move-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await attachments.screenshot({ path: '/private/tmp/hs2-9pa2kd-media-move-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 720 });
  await thumbnail.click();
  const gallery = page.getByRole('dialog', { name: /proof.png/ });
  await gallery.getByRole('button', { name: 'More image actions' }).click();
  const menu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(menu.getByRole('menuitem', { name: 'Remove' })).toBeVisible();
  await gallery.screenshot({ path: '/private/tmp/hs2-edx5j3-gallery-remove-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await gallery.getByRole('button', { name: 'More image actions' }).click();
  await expect(menu.getByRole('menuitem', { name: 'Remove' })).toBeVisible();
  await gallery.screenshot({ path: '/private/tmp/hs2-edx5j3-gallery-remove-narrow.png' });
  await menu.getByRole('menuitem', { name: 'Remove' }).click();
  await expect(gallery).toHaveCount(0);
  await expect(page.locator('.app-toast')).toContainText('Attachment removed.');
  await expect(page.getByRole('button', { name: 'Open proof.png in media gallery' })).toHaveCount(0);
});

test('shows the decorative server-busy bars while a server request is in flight (HS2-MW1V3M)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await mockProject(page, true, false, 0, 0, 450);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const bars = page.locator('[data-component="server-busy-bars"]');
  await expect(bars).toBeAttached();
  await expect(bars).toHaveAttribute('aria-hidden', 'true');
  const expected = await page.evaluate(() => Math.floor((window.innerWidth + 2) / 5));
  await expect(bars.locator('.server-busy-bars__bar')).toHaveCount(expected);
  const starYellow = await bars.evaluate((node) => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--hs-ticket-state-up-next)';
    node.append(probe);
    const resolved = getComputedStyle(probe).color;
    probe.remove();
    return resolved;
  });
  await expect(bars.locator('.server-busy-bars__bar').first()).toHaveCSS('background-color', starYellow);
  await expect(bars).toHaveAttribute('data-visible', 'false');
  const row = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');
  await row.click();
  const statusSelect = page.locator('wa-select[name="inspector-status"]');
  await statusSelect.click();
  await statusSelect.locator('wa-option[value="completed"]').click();
  await expect(bars).toHaveAttribute('data-visible', 'true');
  await page.screenshot({ path: '/private/tmp/hs2-mw1v3m-busy-bars-wide.png' });
  await expect(bars).toHaveAttribute('data-visible', 'false');
});

test('rebinds and applies keyboard shortcuts from App Settings (HS2-QT6PGR)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const apple = await page.evaluate(() => /macintosh|mac os|iphone|ipad|ipod/i.test(navigator.userAgent));
  const mod = apple ? 'Meta' : 'Control';
  await page.getByLabel('Settings view').click();
  const nav = page.locator('.settings-navigation[aria-label="Settings categories"]');
  await expect(nav).toContainText('Project Settings');
  await expect(nav).toContainText('App Settings');
  await nav.locator('[data-item-id="keyboard"]').click();
  const screen = page.locator('[data-component="keyboard-settings"]');
  await expect(screen).toBeVisible();
  await expect(page.locator('#workspace-page-title')).toContainText('Keyboard shortcuts');
  await expect(screen.locator('[data-shortcut-id="open-search"] .keyboard-settings__chord')).toHaveText(
    apple ? '⌘K' : 'Ctrl+K',
  );
  // Fixed ARIA navigation stays System; the ticket clipboard chords are now editable (HS2-9PR10F).
  await expect(screen.locator('[data-shortcut-id="move-selection-up"] .keyboard-settings__fixed')).toHaveText('System');
  await expect(screen.locator('[data-shortcut-id="move-selection-up"] [data-action="edit-shortcut"]')).toHaveCount(0);
  await expect(screen.locator('[data-shortcut-id="copy-tickets"] [data-action="edit-shortcut"]')).toHaveCount(1);
  // Rebind Open search to mod+G and confirm it persists device-locally.
  await screen.locator('[data-shortcut-id="open-search"] [data-action="edit-shortcut"]').click();
  await expect(screen.locator('[data-shortcut-capture="open-search"]')).toBeFocused();
  await page.keyboard.press(`${mod}+g`);
  await expect(screen.locator('[data-shortcut-id="open-search"] .keyboard-settings__chord')).toHaveText(
    apple ? '⌘G' : 'Ctrl+G',
  );
  await expect(screen.locator('li.keyboard-settings__row[data-shortcut-id="open-search"]')).toHaveAttribute(
    'data-overridden',
    'true',
  );
  expect(
    await page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.keyboard-shortcuts') || '{}')),
  ).toMatchObject({ 'open-search': { key: 'g', mod: true } });
  await screen.screenshot({ path: '/private/tmp/hs2-qt6pgr-keyboard-settings.png' });
  // Conflict: rebinding Undo onto the same chord flags both shortcuts (still on the screen).
  await screen.locator('[data-shortcut-id="undo"] [data-action="edit-shortcut"]').click();
  await expect(screen.locator('[data-shortcut-capture="undo"]')).toBeFocused();
  await page.keyboard.press(`${mod}+g`);
  await expect(screen.locator('[data-shortcut-id="undo"] .keyboard-settings__conflict')).toContainText('Open search');
  await expect(screen.locator('[data-shortcut-id="open-search"] .keyboard-settings__conflict')).toContainText('Undo');
  // Reset just Undo: its default returns and the conflict clears while Open search stays rebound.
  await screen.locator('[data-shortcut-id="undo"] [data-action="reset-shortcut"]').click();
  await expect(screen.locator('[data-shortcut-id="undo"] .keyboard-settings__chord')).toHaveText(
    apple ? '⌘Z' : 'Ctrl+Z',
  );
  await expect(screen.locator('.keyboard-settings__conflict')).toHaveCount(0);
  // The rebinding takes effect: mod+K no longer opens search, mod+G does.
  await page.getByRole('button', { name: 'List view', exact: true }).click();
  await page.locator('.app-shell__work-area').focus();
  await page.keyboard.press(`${mod}+k`);
  await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);
  await page.keyboard.press(`${mod}+g`);
  await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toBeVisible();
});

test('applies a saved command color and icon to the sidebar command button (HS2-656XJ2)', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const command = page.locator('[data-action="run-command"][data-item-id="check"]');
  await expect(command).toBeVisible();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  const editor = page.locator('[data-component="command-settings-editor"]');
  await expect(editor).toBeVisible();
  const runChecksRow = editor.locator('.command-settings-editor__row', { hasText: 'Run checks' });
  await runChecksRow.locator('.command-settings-editor__row-menu-trigger').click();
  await runChecksRow.locator('[data-action="edit-command-setting"]').dispatchEvent('click');
  const commandDialog = page.locator('#command-editor-dialog');
  await commandDialog.getByTitle('Green', { exact: true }).click();
  await commandDialog.getByTitle('circle-check-big', { exact: true }).click();
  await expect(commandDialog.locator('.command-settings-editor__swatch input:checked')).toHaveValue('#22c55e');
  await expect(
    commandDialog.locator('[data-action="select-command-icon"][data-icon-name="circle-check-big"]'),
  ).toHaveAttribute('aria-pressed', 'true');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await expect(editor.getByRole('status')).toContainText('Saved.');
  await page.getByLabel('List view').click();
  await expect(command).toBeVisible();
  await expect(command).toHaveAttribute('data-command-color', '#22c55e');
  await expect(command.locator('[data-lucide="circle-check-big"]')).toHaveCount(1);
  await page.locator('.project-sidebar').screenshot({ path: '/private/tmp/hs2-656xj2-sidebar-command-color-icon.png' });
});

test('searches the full Lucide catalog to assign an arbitrary command icon (HS2-5VSNV3)', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const command = page.locator('[data-action="run-command"][data-item-id="check"]');
  await expect(command).toBeVisible();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  const editor = page.locator('[data-component="command-settings-editor"]');
  await expect(editor).toBeVisible();
  const runChecksRow = editor.locator('.command-settings-editor__row', { hasText: 'Run checks' });
  await runChecksRow.locator('.command-settings-editor__row-menu-trigger').click();
  await runChecksRow.locator('[data-action="edit-command-setting"]').dispatchEvent('click');
  const commandDialog = page.locator('#command-editor-dialog'),
    picker = commandDialog.locator('[data-component="lucide-icon-picker"]');
  await expect(picker).toBeVisible();
  // The curated popular defaults show before searching; 'compass' is not among them.
  await expect(picker.locator('[data-icon-name="send"]')).toBeVisible();
  await expect(picker.locator('[data-icon-name="compass"]')).toHaveCount(0);
  await picker.scrollIntoViewIfNeeded();
  await picker.screenshot({ path: '/private/tmp/hs2-5vsnv3-icon-picker-popular.png' });
  // Typing a query loads the full catalog and surfaces the matching icon.
  await picker.getByRole('searchbox', { name: 'Search icons' }).fill('compass');
  const compass = picker.locator('[data-action="select-command-icon"][data-icon-name="compass"]').first();
  await expect(compass).toBeVisible();
  await picker.screenshot({ path: '/private/tmp/hs2-5vsnv3-icon-picker-search.png' });
  await compass.click();
  await expect(compass).toHaveAttribute('aria-pressed', 'true');
  await commandDialog.getByRole('button', { name: 'Done' }).click();
  await expect(editor.getByRole('status')).toContainText('Saved.');
  await page.getByLabel('List view').click();
  await expect(command.locator('[data-lucide="compass"]')).toHaveCount(1);
});

test('keeps the open new-ticket composer and its draft through a background ticket refresh (HS2-D4PB9Y)', async ({
  page,
}) => {
  await mockProject(page);
  let liveRows = [row, backlogRow, archiveRow, notStartedRow, completedRow, verifiedRow, startedRow2, startedRow3],
    cursor = 0;
  const polls: Array<import('@playwright/test').Route> = [];
  await page.route(/\/tickets(?:\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: liveRows }) : route.fallback(),
  );
  await page.route('**/ws/poll*', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since');
    if (since === null) return route.fulfill({ json: { cursor, events: [], overflow: false } });
    polls.push(route);
  });
  const emit = async (kind: string, id: string, slug: string) => {
    await expect.poll(() => polls.length).toBeGreaterThan(0);
    cursor += 1;
    await polls
      .shift()!
      .fulfill({ json: { cursor, events: [{ store: 'git-local', kind, id, slug }], overflow: false } });
  };
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /New ticket/ }).click();
  const dialog = page.locator('[data-component="quick-ticket-composer"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await dialog.getByLabel('Ticket title').fill('Draft title I do not want to lose');
  await dialog.locator('[name="new-ticket-details"]').fill('Detailed notes typed before a background refresh.');
  const external = {
    ...row,
    id: '10',
    native_id: '10',
    qualified_id: 'git-local:10',
    slug: 'HS2-EXTERNAL',
    title: 'Externally added ticket',
    status: 'not_started',
    up_next: false,
  };
  liveRows = [external, ...liveRows];
  await emit('changed', '10', 'HS2-EXTERNAL');
  await expect(page.locator('[data-ticket-slug="HS2-EXTERNAL"]')).toContainText('Externally added ticket');
  // The composer must stay open with its typed draft intact (HS2-D4PB9Y).
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.getByLabel('Ticket title')).toHaveValue('Draft title I do not want to lose');
  await expect(dialog.locator('[name="new-ticket-details"]')).toHaveValue(
    'Detailed notes typed before a background refresh.',
  );
});

test('recovers the open new-ticket composer and its draft after a reload/restart (HS2-D4PB9Y)', async ({ page }) => {
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: /New ticket/ }).click();
  const dialog = page.locator('[data-component="quick-ticket-composer"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await dialog.getByLabel('Ticket title').fill('Draft that must survive a restart');
  await dialog.locator('[name="new-ticket-details"]').fill('Notes that must survive a restart.');
  // Reload immediately, before the 700ms persistence debounce — only the pagehide flush can save the draft.
  await page.reload();
  const restored = page.locator('[data-component="quick-ticket-composer"]');
  // Per design the composer is not auto-painted on startup, but the flushed draft is recovered on reopen.
  await expect(restored).toBeHidden();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await expect(restored.getByRole('textbox', { name: 'Ticket title' })).toHaveValue(
    'Draft that must survive a restart',
  );
  await expect(restored.getByRole('textbox', { name: 'Details' })).toHaveValue('Notes that must survive a restart.');
});

test('opens a distinct terminal for each shell command even when a create is still pending (HS2-2BKPGK)', async ({
  page,
}) => {
  const posted: string[] = [];
  let releaseFirst!: () => void;
  const firstGate = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  let firstHeld = false;
  await installFakeTerminalSockets(page, true);
  await mockProject(page);
  await page.route('**/__hotsheet/project-api/demo-checkout/commands', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            { id: 'lint', title: 'Lint project', kind: 'shell', command: 'npm run lint', group: 'Quality' },
            { id: 'test', title: 'Test project', kind: 'shell', command: 'npm test', group: 'Quality' },
          ],
        })
      : route.fallback(),
  );
  await page.route('**/__hotsheet/project-api/demo-checkout/terminals', async (route) => {
    if (route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as { shell_command: string };
      posted.push(body.shell_command);
      if (!firstHeld) {
        firstHeld = true;
        await firstGate;
      }
      return route.fulfill({ json: { id: `term-${posted.length}`, alive: true, busy: true, cwd: '/work/demo' } });
    }
    return route.fallback();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Lint project' }).click();
  await expect.poll(() => posted).toEqual(['npm run lint']); // the first create's POST is held in flight
  await page.getByRole('button', { name: 'Test project' }).click(); // a second click arrives during the pending create
  await page.waitForTimeout(200);
  releaseFirst();
  // The second shell command must still open its own terminal (not be dropped by the in-flight create).
  await expect.poll(() => posted).toEqual(['npm run lint', 'npm test']);
});

test('a quoting feedback reply clears Needs review through the real server (HS2-AVXYCB)', async ({ page }) => {
  test.setTimeout(90_000);
  const server = await realTicketServer();
  try {
    const created = await server.request<FullTicket>('/tickets', 'POST', {
      title: 'Choose the feedback reply behavior',
      category: 'task',
      status: 'started',
      up_next: true,
    });
    const initial = await server.request<FullTicket>(`/tickets/${created.id}`, 'PATCH', {
      note: 'FEEDBACK NEEDED: Keep the current behavior or use the revised flow?',
      note_kind: 'feedback_needed',
    });
    const requestNote = initial.notes.at(-1)!;
    await mockProject(page);
    // Only external project discovery/provider data are fixtures. Ticket reads, mutations,
    // concurrency tokens, classification, and index projections use the actual Rust server.
    await page.route('**/__hotsheet/project-api/demo-checkout/checkouts/demo-checkout/**', async (route) => {
      const incoming = new URL(route.request().url()),
        path = incoming.pathname.replace(
          '/__hotsheet/project-api/demo-checkout/checkouts/demo-checkout',
          `/checkouts/${server.checkoutId}`,
        );
      const response = await route.fetch({
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await route.fulfill({ response });
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/?dev-review=false');
    await page.getByRole('button', { name: 'Open project' }).click();
    await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
    const row = page.locator(`[data-component="ticket-list-row"][data-ticket-slug="${created.slug}"]`);
    await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
    await row.click();
    await page.getByRole('button', { name: 'Open ticket reader' }).click();
    const reader = page.getByRole('dialog').filter({ has: page.locator('[data-component="ticket-inspector"]') }),
      inspector = reader.locator('[data-component="ticket-inspector"]'),
      note = reader.locator(`article[data-note-id="${requestNote.id}"]`);
    await expect(inspector).toHaveAttribute('data-needs-review', 'true');
    await note.scrollIntoViewIfNeeded();
    await reader.screenshot({ path: '/private/tmp/hs2-avxycb-feedback-before-wide.png' });
    // The real inline composer quotes the original request when inserting an answer.
    await note.getByRole('button', { name: 'Add response at a character position' }).click();
    await note.getByRole('textbox', { name: /Response at character/ }).fill('Use the revised flow. This is my answer.');
    await note.getByRole('button', { name: 'Respond' }).click();
    await expect(inspector).toHaveAttribute('data-needs-review', 'false');
    await expect(inspector.locator('.ticket-inspector__feedback')).toHaveCount(0);
    const persisted = await server.request<FullTicket>(`/tickets/${created.id}`),
      reply = persisted.notes.at(-1)!;
    expect(persisted.feedback_needed).toBe(false);
    expect(reply.kind).toBe('regular');
    expect(reply.text).toContain('> FEEDBACK NEEDED');
    expect(reply.text).toContain('Use the revised flow. This is my answer.');
    const replyCard = reader.locator(`article[data-note-id="${reply.id}"]`);
    await expect(replyCard).toContainText('Use the revised flow. This is my answer.');
    await expect(replyCard.locator('.note-card__feedback-block')).toHaveCount(0);
    await expect(note.locator('.note-card__feedback-block')).toHaveCount(0);
    await replyCard.scrollIntoViewIfNeeded();
    await reader.screenshot({ path: '/private/tmp/hs2-avxycb-feedback-answered-wide.png' });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(inspector).toHaveAttribute('data-needs-review', 'false');
    await replyCard.scrollIntoViewIfNeeded();
    await reader.screenshot({ path: '/private/tmp/hs2-avxycb-feedback-answered-mobile.png' });
    await page.getByRole('button', { name: 'Close ticket reader' }).click();
    await expect(row.locator('.ticket-list-row__feedback')).toHaveCount(0);
    await page.reload();
    await expect(row).toBeVisible();
    await expect(row.locator('.ticket-list-row__feedback')).toHaveCount(0);
  } finally {
    await server.stop();
  }
});

test('preserves feature-controller command editing and run-dialog parity across wide and narrow shells (HS2-DHYGXJ)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockProject(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByLabel('Settings view').click();
  await page.getByRole('button', { name: 'Commands', exact: true }).click();
  const editor = page.locator('[data-component="command-settings-editor"]');
  const openEditor = async () => {
    const row = editor.locator('.command-settings-editor__row').first();
    await row.locator('.command-settings-editor__row-menu-trigger').click();
    await row.locator('[data-action="edit-command-setting"]').click();
  };
  const dialog = page.locator('#command-editor-dialog');
  await openEditor();
  await dialog.getByLabel('Button label').fill('Feature parity');
  await dialog.getByRole('textbox', { name: 'Program' }).fill('/usr/bin/true');
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(editor.getByRole('status')).toContainText('Saved.');
  await expect(editor).toContainText('Feature parity');
  await page.screenshot({ path: '/private/tmp/hs2-dhygxj-command-settings-wide.png', animations: 'disabled' });
  await page.getByLabel('List view').click();
  await expect(page.getByRole('button', { name: 'Feature parity', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Feature parity', exact: true }).click();
  await page.getByRole('button', { name: 'Running Feature parity' }).click();
  const stop = page.locator('[data-component="command-cancellation-dialog"]');
  await expect(stop).toBeVisible();
  await stop.getByRole('button', { name: 'Stop command' }).click();
  await expect(page.getByRole('button', { name: 'Feature parity', exact: true })).toHaveAttribute('title', /cancelled/);
  await page.getByLabel('Settings view').click();
  await expect(editor).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', 'true');
  await openEditor();
  await expect(dialog.getByLabel('Button label')).toHaveValue('Feature parity');
  await dialog.getByLabel('Button label').fill('After return');
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(editor.getByRole('status')).toContainText('Saved.');
  await expect(editor).toContainText('After return');
  await page.screenshot({ path: '/private/tmp/hs2-dhygxj-command-settings-narrow.png', animations: 'disabled' });
});
