export const project = {
  id: 'terminal-feedback',
  root: '/work/terminal-feedback',
  name: 'Terminal feedback',
  stores: ['/work/terminal-feedback.hs2'],
  apiPath: '/__hotsheet/project-api/terminal-feedback',
};
const capabilities = {
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
};

export async function installTerminalFixture(
  page: import('@playwright/test').Page,
  leadingZshMarker = false,
  ticketLinks = false,
  { nativeSocket = false }: { nativeSocket?: boolean } = {},
) {
  await page.addInitScript(
    ({ leadingZshMarker, ticketLinks, nativeSocket }) => {
      if (nativeSocket) return;
      const nano = (cols = 80, rows = 24) => {
        const bar = (value: string) => `\u001b[7m${value.padEnd(cols).slice(0, cols)}\u001b[0m`,
          references = ticketLinks
            ? '\u001b[3;1HHS2-EXACT HS2-SHARED @terminal-feedback/HS2-QUALIFIED HS2-MISSING'
            : '';
        return `\u001b[2J\u001b[H${bar('  GNU nano 8.4                 terminal-fill-proof.txt')}\u001b[2;1H${bar('File: terminal-fill-proof.txt')}${references}\u001b[${Math.max(4, Math.floor(rows / 2))};20H${cols} columns × ${rows} rows\u001b[${Math.max(2, rows - 1)};1H${bar('^G Help  ^O Write Out  ^W Where Is  ^K Cut  ^T Execute')}\u001b[${rows};1H${bar('^X Exit  ^R Read File  ^\\ Replace  ^U Paste  ^J Justify')}`;
      };
      const sockets: FakeSocket[] = [];
      class FakeSocket extends EventTarget {
        static CONNECTING = 0;
        static OPEN = 1;
        static CLOSING = 2;
        static CLOSED = 3;
        readyState = 0;
        binaryType = 'blob';
        sent: unknown[] = [];
        constructor(public url: string) {
          super();
          sockets.push(this);
          setTimeout(() => {
            if (this.readyState === FakeSocket.CLOSED) return;
            this.readyState = FakeSocket.OPEN;
            this.dispatchEvent(new Event('open'));
            const output = leadingZshMarker ? '\u001b[1m\u001b[7m%\u001b[27m\u001b[1m\u001b[0m\r\nprompt % ' : nano();
            this.dispatchEvent(new MessageEvent('message', { data: new TextEncoder().encode(output).buffer }));
          });
        }
        send(value: unknown) {
          this.sent.push(value);
          if (typeof value !== 'string') return;
          try {
            const resize = JSON.parse(value).resize;
            if (resize) {
              this.dispatchEvent(
                new MessageEvent('message', {
                  data: JSON.stringify({
                    pty_size: { cols: resize.cols, rows: resize.rows },
                    driven_by: resize.viewer_id,
                  }),
                }),
              );
              if (!leadingZshMarker)
                this.dispatchEvent(
                  new MessageEvent('message', {
                    data: new TextEncoder().encode(nano(resize.cols, resize.rows)).buffer,
                  }),
                );
            }
          } catch {
            /* input */
          }
        }
        close() {
          this.readyState = 3;
          this.dispatchEvent(new CloseEvent('close'));
        }
      }
      Object.assign(window, { WebSocket: FakeSocket, __terminalFeedbackSockets: sockets });
    },
    { leadingZshMarker, ticketLinks, nativeSocket },
  );
  let createdTerminal = false;
  const ticketRow = (id: string, slug: string, title: string) => ({
      connection_id: 'git-local',
      native_id: id,
      qualified_id: `git-local:${id}`,
      id,
      slug,
      title,
      category: 'feature',
      priority: 'default',
      status: 'started',
      up_next: true,
      feedback_needed: false,
      tags: [],
      blocked_by: [],
      claim_count: 0,
      created_at: '2026-09-21T00:00:00Z',
      updated_at: '2026-09-21T00:00:00Z',
    }),
    linkTickets = [
      ticketRow('exact', 'HS2-EXACT', 'Exact terminal destination'),
      ticketRow('shared-a', 'HS2-SHARED', 'Shared terminal destination A'),
      ticketRow('shared-b', 'HS2-SHARED', 'Shared terminal destination B'),
      ticketRow('qualified', 'HS2-QUALIFIED', 'Qualified terminal destination'),
    ];
  await page.route('**/*', (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
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
            capabilities,
          },
        ],
      });
    if (ticketLinks && path.endsWith('/tickets')) {
      const text = url.searchParams.get('text')?.toLocaleLowerCase();
      return route.fulfill({
        json: text ? linkTickets.filter((ticket) => ticket.slug.toLocaleLowerCase() === text) : [],
      });
    }
    if (ticketLinks && path.match(/\/tickets\/[^/]+$/)) {
      const id = decodeURIComponent(path.split('/').at(-1)!),
        ticket = linkTickets.find((item) => item.id === id);
      if (ticket)
        return route.fulfill({
          json: {
            store: 'git-local',
            ...ticket,
            details: `Opened ${ticket.slug} from the terminal.`,
            blocked_reason: null,
            concurrency_token: 'token',
            notes: [],
            attachments: [],
          },
        });
    }
    if (
      path.endsWith('/permissions') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/tickets') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(new URL(request.url()).searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminals') && request.method() === 'POST') {
      createdTerminal = true;
      return route.fulfill({ status: 201, json: { id: 'terminal-new', alive: true, busy: false, cwd: project.root } });
    }
    if (path.endsWith('/terminals'))
      return route.fulfill({
        json: [
          { id: 'nano', alive: true, busy: true, cwd: project.root, progress: 50 },
          ...(createdTerminal ? [{ id: 'terminal-new', alive: true, busy: false, cwd: project.root }] : []),
        ],
      });
    if (path.endsWith('/terminals/nano'))
      return route.fulfill({
        json: {
          id: 'nano',
          alive: true,
          busy: true,
          cwd: project.root,
          progress: 50,
          scrollback: 'GNU nano 8.4\n80 columns × 24 rows\n^X Exit',
        },
      });
    return route.fallback();
  });
}
