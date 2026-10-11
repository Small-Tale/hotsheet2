import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketCounts, CorruptTicket, TicketRow } from '../api';
import { corruptTicketKey } from '../components/corrupt-ticket-row';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import { createClaimClockController } from './claim-clock';
import { createCorruptTicketRecoveryController } from './corrupt-ticket-recovery';
import { createTerminalNamesController } from './terminal-names';

const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const counts = (active: number) => ({ active }) as unknown as CheckoutTicketCounts;
const fetchMock = vi.fn<typeof fetch>();

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-11T00:00:00Z'));
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  const stored = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
  vi.stubGlobal('window', { setTimeout, clearTimeout });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('runtime-extracted feature owners (HS2-K7SYHQ)', () => {
  it('claim clock: wakes at lease expiry, refreshes only the expiring project, then reschedules', async () => {
    const now = Date.now(),
      claimed = {
        status: 'started',
        claimed_by: 'w',
        claim_lease_expires_at: new Date(now + 5_000).toISOString(),
      } as TicketRow,
      rows: Record<string, TicketRow[]> = { a: [claimed], b: [] },
      refreshProjectTab = vi.fn(async (target: Project) => {
        rows[target.id] = [];
      }),
      activeTicketCount = signal(0),
      projectTabClaimClock = signal(0),
      claimEtaClock = signal(0);
    const clock = createClaimClockController({
      projects: signal([project('a'), project('b')]),
      selectedProjectId: signal('a'),
      ticketCountsByProject: signal({}),
      activeTicketCount,
      projectTabClaimClock,
      claimEtaClock,
      projectTabTicketRows: (id) => rows[id] ?? [],
      projectTicketCounts: (id) => counts((rows[id] ?? []).length),
      refreshProjectTab,
    });
    clock.scheduleClaimLeaseExpiry();
    expect(activeTicketCount.value).toBe(1);
    expect(clock.liveClaimNotice(claimed)).toMatchObject({ agentName: 'w' });
    await vi.advanceTimersByTimeAsync(5_100);
    expect(refreshProjectTab).toHaveBeenCalledTimes(1);
    expect(refreshProjectTab.mock.calls[0][0].id).toBe('a');
    expect(activeTicketCount.value).toBe(0);
    expect(clock.liveClaimNotice(claimed)).toBeUndefined();
    // Nothing is claimed any more, so no further wake-up is armed.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(refreshProjectTab).toHaveBeenCalledTimes(1);
  });

  it('corrupt recovery: blocks duplicate actions while pending and records failures', async () => {
    const ticket = { path: 'tickets/x.md', error_code: 'parse_error' } as unknown as CorruptTicket,
      showToast = vi.fn(),
      refreshProject = vi.fn(async () => undefined),
      owner = createCorruptTicketRecoveryController({
        project: () => project('a'),
        corruptTickets: signal([ticket]),
        showToast,
        refreshProject,
      });
    const key = corruptTicketKey(ticket);
    fetchMock.mockResolvedValueOnce(new Response('{"error":"denied"}', { status: 403 }));
    const first = owner.revealCorruptTicket(key);
    expect(owner.corruptRecovery.value[key]).toEqual({ pending: 'reveal' });
    await owner.revealCorruptTicket(key);
    await first;
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(owner.corruptRecovery.value[key]).toEqual({ message: 'denied', failed: true });
    fetchMock.mockResolvedValueOnce(Response.json({ slug: 'HS2-AAAAAA' }));
    await owner.queueCorruptTicketRepair(key);
    expect(owner.corruptRecovery.value[key]).toEqual({});
    expect(showToast).toHaveBeenCalledWith('Queued HS2-AAAAAA for AI repair.');
    expect(refreshProject).toHaveBeenCalledTimes(1);
  });

  it('terminal names: keeps the local copy until the server accepts, and ignores echoes while pending', async () => {
    const terminalNames = signal<Record<string, string>>({}),
      terminalGroups = signal<TerminalDashboardGroup[]>([
        { projectId: 'a', sessions: [{ id: 't1', title: 'shell', defaultTitle: 'shell' }] } as never,
      ]),
      showToast = vi.fn(),
      owner = createTerminalNamesController({
        projects: signal([project('a')]),
        terminalNames,
        terminalGroups,
        showToast,
        terminalGroupLoaded: () => true,
        refreshTerminalDashboard: vi.fn(async () => undefined),
      });
    let accept!: () => void;
    fetchMock.mockReturnValueOnce(
      new Promise((resolve) => {
        accept = () => {
          resolve(Response.json({ id: 't1', name: 'build' }));
        };
      }),
    );
    owner.saveTerminalName('a', 't1', '  build  ');
    expect(Object.values(terminalNames.value)).toEqual(['build']);
    // The server's echo of this rename arrives while the write is in flight: the local copy wins.
    owner.applyTerminalRenamed(project('a'), 't1', 'build');
    expect(Object.values(terminalNames.value)).toEqual(['build']);
    accept();
    await vi.runAllTimersAsync();
    expect(terminalNames.value).toEqual({});
    expect(fetchMock).toHaveBeenCalledTimes(1);
    owner.saveTerminalName('a', 't1', '   ');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(showToast).not.toHaveBeenCalled();
  });
});
