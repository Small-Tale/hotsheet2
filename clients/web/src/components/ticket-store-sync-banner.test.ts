import { describe, expect, it } from 'vitest';

import type { TicketStoreAiHelp, TicketStoreSyncHealth, TicketStoreSyncReport, TicketStoreSyncState } from '../api';
import {
  AI_REPAIR_GRACE_MS,
  aiHelpSentence,
  failingTicketStores,
  PERSISTENT_OFFLINE_FAILURES,
  plainServerText,
  repairInProgress,
  restatesAiHelp,
  strandedChangeCount,
  strandedTicketStores,
  syncFailureSummary,
  TicketStoreSyncBanner,
} from './ticket-store-sync-banner';

function health(state: TicketStoreSyncState, extra: Partial<TicketStoreSyncHealth> = {}): TicketStoreSyncHealth {
  return {
    state,
    syncing: false,
    unpublished_commits: 0,
    incoming_commits: 0,
    consecutive_failures: 0,
    ...extra,
  };
}

function report(...stores: Array<[string, TicketStoreSyncHealth]>): TicketStoreSyncReport {
  return {
    checkout_id: 'web-1a2b3c',
    stores: stores.map(([label, item]) => ({
      connection_id: `git:${label}`,
      label,
      locator: `/work/${label}`,
      health: item,
    })),
  };
}

const REJECTED = health('rejected', {
  unpublished_commits: 7925,
  consecutive_failures: 4,
  reason: 'remote: error: GH001: Large files detected.',
  oversized: [{ path: 'attachments/01/scan.bin', bytes: 118_971_148, limit_bytes: 104_857_600 }],
  next_action: 'Move the oversized file to Git LFS, then run `hotsheet-cli sync`.',
});

const BLOCKED = health('blocked', {
  unpublished_commits: 2,
  consecutive_failures: PERSISTENT_OFFLINE_FAILURES,
  reason:
    'a Git lock file blocked sync during the pre-sync check: .git/index.lock (created 3 hours ago). Git leaves a lock behind when one of its processes crashes or is killed; the store was not merged.\nNot moved automatically: .git/index.lock may still be in use: process 4242 (git) runs Git in the store.',
  next_action:
    "No ticket merge conflict was found. Sync moves a lock aside by itself once it is over 10 minutes old and no process has the store's Git directory open; `hotsheet-cli sync --recover-stale-locks` runs that check now. Otherwise make sure no Git process is running in the store (an editor's Git integration, a terminal, or another Hot Sheet), then remove the stale lock file and run `hotsheet-cli sync`.",
});

describe('failingTicketStores (HS2-ADHFKZ)', () => {
  it('lists only stores whose publication is failing, across every state', () => {
    const states: Array<[TicketStoreSyncState, boolean]> = [
      ['not_repository', false],
      ['no_remote', false],
      ['pending', false],
      ['up_to_date', false],
      ['synced', false],
      ['offline', false],
      ['rejected', true],
      ['conflict', true],
      ['blocked', false],
      ['rebase_in_progress', true],
      ['history_recovery_required', true],
    ];
    for (const [state, failing] of states)
      expect(failingTicketStores(report(['s', health(state)])).length, state).toBe(failing ? 1 : 0);
    expect(failingTicketStores(undefined)).toEqual([]);
  });

  it('treats offline as failing only once it has persisted', () => {
    const below = health('offline', { consecutive_failures: PERSISTENT_OFFLINE_FAILURES - 1 });
    const at = health('offline', { consecutive_failures: PERSISTENT_OFFLINE_FAILURES });
    expect(failingTicketStores(report(['s', below]))).toEqual([]);
    expect(failingTicketStores(report(['s', at]))).toHaveLength(1);
  });

  it('treats a Git lock as failing once it has persisted, and clears when the lock is removed (HS2-6ACT5Y)', () => {
    const once = health('blocked', { consecutive_failures: 1, reason: BLOCKED.reason });
    expect(failingTicketStores(report(['s', once]))).toEqual([]);
    expect(failingTicketStores(report(['s', BLOCKED]))).toHaveLength(1);
    expect(failingTicketStores(report(['s', health('synced')]))).toEqual([]);
  });

  it('clears when the store recovers (failure, then synced)', () => {
    expect(failingTicketStores(report(['s', REJECTED]))).toHaveLength(1);
    expect(failingTicketStores(report(['s', health('synced')]))).toEqual([]);
  });
});

describe('syncFailureSummary (HS2-ADHFKZ)', () => {
  it('names the oversized file and its size for a rejected push', () => {
    expect(syncFailureSummary(REJECTED)).toBe(
      'The remote rejected the push: attachments/01/scan.bin is 119 MB, over its file size limit.',
    );
    expect(syncFailureSummary(health('rejected'))).toBe('The remote rejected the push.');
  });

  it('distinguishes each failure state', () => {
    expect(syncFailureSummary(health('conflict'))).toContain('could not be merged');
    expect(syncFailureSummary(health('rebase_in_progress'))).toContain('needs a decision');
    expect(syncFailureSummary(health('history_recovery_required'))).toContain('must be recovered');
    expect(syncFailureSummary(health('offline', { consecutive_failures: 5 }))).toContain('5 sync attempts');
  });

  it('names the Git lock from the server reason, never a conflict (HS2-6ACT5Y)', () => {
    const summary = syncFailureSummary(BLOCKED);
    expect(summary).toMatch(/^A Git lock file blocked sync/);
    expect(summary).toContain('.git/index.lock (created 3 hours ago)');
    expect(summary).not.toContain('merged automatically');
    expect(syncFailureSummary(health('blocked'))).toBe('A Git lock file in the store is blocking sync.');
  });
});

describe('TicketStoreSyncBanner (HS2-ADHFKZ)', () => {
  it('renders nothing while every store publishes', () => {
    expect(TicketStoreSyncBanner({ report: undefined })).toBeUndefined();
    expect(TicketStoreSyncBanner({ report: report(['web.hs2', health('up_to_date')]) })).toBeUndefined();
  });

  it('names the store, the cause, the unpublished work, and the next step', () => {
    const html = String(TicketStoreSyncBanner({ report: report(['kerf.hs2', REJECTED]) }));
    expect(html).toContain('data-component="ticket-store-sync-banner"');
    expect(html).toContain('data-tone="warning"');
    expect(html).toContain('Ticket store kerf.hs2 is not publishing changes');
    expect(html).toContain('attachments/01/scan.bin is 119 MB');
    expect(html).toContain('7,925 local changes are not published');
    expect(html).toContain('hotsheet-cli sync');
    expect(html).toContain('cloud-alert');
    // The raw Git output stays out of the strip; the CLI status shows it.
    expect(html).not.toContain('GH001');
  });

  it('shows a persistent Git lock with its file and the safe recovery (HS2-6ACT5Y)', () => {
    const html = String(TicketStoreSyncBanner({ report: report(['web.hs2', BLOCKED]) }));
    expect(html).toContain('Ticket store web.hs2 is not publishing changes');
    expect(html).toContain('.git/index.lock');
    expect(html).toContain('2 local changes are not published');
    expect(html).toContain('no Git process is running');
    expect(html).not.toContain('changed on both sides');
  });

  it('summarizes several failing stores', () => {
    const html = String(
      TicketStoreSyncBanner({
        report: report(['a.hs2', health('conflict', { unpublished_commits: 1 })], ['b.hs2', REJECTED]),
      }),
    );
    expect(html).toContain('2 ticket stores are not publishing changes');
    expect(html).toContain('a.hs2: A ticket changed on both sides');
    expect(html).toContain('1 local change is not published');
    expect(html).toContain('1 more store needs attention');
  });
});

describe('TicketStoreSyncBanner AI repair (HS2-QR1K8R)', () => {
  const help = (outcome: TicketStoreAiHelp['outcome'], tool?: string): TicketStoreAiHelp => ({
    outcome,
    mode: 'start',
    ...(tool ? { tool } : {}),
    ticket: 'HS-ABC123',
    asked_at: '2026-11-13T08:00:00Z',
  });
  const asked = (outcome: TicketStoreAiHelp['outcome'], tool?: string) =>
    health('conflict', {
      unpublished_commits: 3,
      next_action:
        'Hot Sheet started codex to repair the store (ticket HS-ABC123); sync publishes once it is repaired.',
      ai_help: help(outcome, tool),
    });

  it('says whom Hot Sheet asked, for every outcome', () => {
    expect(aiHelpSentence(help('started', 'codex'))).toBe('Hot Sheet started codex to repair it.');
    expect(aiHelpSentence(help('started'))).toBe('Hot Sheet started an AI session to repair it.');
    expect(aiHelpSentence(help('session_asked'))).toContain('asked the running AI session');
    expect(aiHelpSentence(help('finished', 'codex'))).toBe('The codex session Hot Sheet started has ended.');
    expect(aiHelpSentence(help('failed', 'codex'))).toContain('ended with an error');
    expect(aiHelpSentence(help('ticket_only'))).toBe('Hot Sheet could not ask an AI to repair it.');
  });

  it('shows the ask and links the repair ticket through the shared ticket-link action', () => {
    const html = String(TicketStoreSyncBanner({ report: report(['tickets.hs2', asked('started', 'codex')]) }));
    // The server's next step already says who was asked, so it is said once (HS2-GEF1AS).
    expect(html).toContain('Hot Sheet started codex to repair the store');
    expect(html).not.toContain('Hot Sheet started codex to repair it.');
    expect(html).toContain('sync publishes once it is repaired');
    expect(html).toContain('data-ticket-slug="HS-ABC123"');
    expect(html).toContain('data-action="open-linked-ticket"');
    expect(html).toContain('Open HS-ABC123');
    // The action is StateBanner's compact pill Button, not a page link.
    expect(html).toMatch(/<button[^>]*data-action="open-linked-ticket"/);
  });

  it('keeps the outcome sentence only when the next step does not restate it (HS2-GEF1AS)', () => {
    const withNext = (outcome: TicketStoreAiHelp['outcome']) =>
      String(TicketStoreSyncBanner({ report: report(['tickets.hs2', asked(outcome, 'codex')]) }));
    expect(withNext('failed')).toContain('The codex session Hot Sheet started ended with an error.');
    expect(withNext('ticket_only')).toContain('Hot Sheet could not ask an AI to repair it.');
    for (const outcome of ['started', 'session_asked', 'finished'] as const)
      expect(restatesAiHelp(help(outcome), 'next')).toBe(true);
    for (const outcome of ['failed', 'ticket_only'] as const) expect(restatesAiHelp(help(outcome), 'next')).toBe(false);
    // Without a server next step the client sentence is the only account of the ask.
    expect(restatesAiHelp(help('started'), undefined)).toBe(false);
  });

  it('has no ticket link and no ask sentence when no AI was asked', () => {
    const html = String(TicketStoreSyncBanner({ report: report(['tickets.hs2', health('conflict')]) }));
    expect(html).not.toContain('open-linked-ticket');
    expect(html).not.toContain('to repair it');
  });
});

describe('TicketStoreSyncBanner while Hot Sheet repairs the stall (HS2-D4TK0W)', () => {
  const asked = Date.parse('2026-11-13T08:00:00Z');
  const stalled = (state: TicketStoreSyncState, outcome: TicketStoreAiHelp['outcome']) =>
    health(state, {
      unpublished_commits: 2,
      ai_help: { outcome, mode: 'start', tool: 'codex', ticket: 'HS-ABC123', asked_at: '2026-11-13T08:00:00Z' },
    });
  const shown = (item: TicketStoreSyncHealth, nowMs: number) =>
    String(TicketStoreSyncBanner({ report: report(['tickets.hs2', item]), nowMs })) !== 'undefined' &&
    failingTicketStores(report(['tickets.hs2', item]), nowMs).length === 1;

  it('holds the warning back while a session it started or asked is repairing', () => {
    for (const state of ['conflict', 'rebase_in_progress'] as const)
      for (const outcome of ['started', 'session_asked'] as const) {
        const item = stalled(state, outcome);
        expect(repairInProgress(item, asked + 60_000), `${state} ${outcome}`).toBe(true);
        expect(failingTicketStores(report(['tickets.hs2', item]), asked + 60_000)).toEqual([]);
        expect(TicketStoreSyncBanner({ report: report(['tickets.hs2', item]), nowMs: asked + 60_000 })).toBeUndefined();
      }
  });

  it('warns once that repair ended, failed, could not be asked, or ran past the grace period', () => {
    for (const outcome of ['finished', 'failed', 'ticket_only'] as const)
      expect(shown(stalled('rebase_in_progress', outcome), asked + 60_000), outcome).toBe(true);
    expect(shown(stalled('conflict', 'started'), asked + AI_REPAIR_GRACE_MS), 'grace elapsed').toBe(true);
    // A stall nobody was asked about (the ask setting is off) is for a person at once.
    expect(shown(health('rebase_in_progress'), asked)).toBe(true);
    expect(repairInProgress({ ...stalled('conflict', 'started'), ai_help: undefined }, asked)).toBe(false);
  });

  it('never holds back failures an AI repair does not cover', () => {
    for (const state of ['rejected', 'history_recovery_required'] as const)
      expect(shown(stalled(state, 'started'), asked + 60_000), state).toBe(true);
    const offline = { ...stalled('offline', 'started'), consecutive_failures: PERSISTENT_OFFLINE_FAILURES };
    expect(shown(offline, asked + 60_000)).toBe(true);
  });

  it('shows again when a repair in progress stops (started, then failed)', () => {
    const now = asked + 60_000;
    expect(shown(stalled('conflict', 'started'), now)).toBe(false);
    expect(shown(stalled('conflict', 'failed'), now)).toBe(true);
    expect(shown(health('synced'), now)).toBe(false);
  });
});

describe('TicketStoreSyncBanner stranded saved writes (HS2-1HRYD9)', () => {
  const ref = 'refs/hotsheet/sync-backup/1-saved-writes';
  const restore = `Run \`hotsheet-cli sync --restore-saved-writes ${ref}\` to write them back and commit them on the branch.`;
  const STRANDED = health('synced', {
    next_action: restore,
    stranded_saved_writes: [
      { backup_ref: ref, paths: ['tickets/AB/one.md', 'tickets/CD/two.md'] },
      { backup_ref: 'refs/hotsheet/sync-backup/2-saved-writes', paths: ['tickets/EF/three.md'] },
    ],
  });

  it('lists only stores holding stranded changes, in any publication state', () => {
    const one = health('up_to_date', { stranded_saved_writes: [{ backup_ref: ref, paths: ['tickets/AB/one.md'] }] });
    expect(
      strandedTicketStores(report(['a.hs2', STRANDED], ['b.hs2', health('synced')], ['c.hs2', one])).map(
        (store) => store.label,
      ),
    ).toEqual(['a.hs2', 'c.hs2']);
    expect(strandedTicketStores(report(['b.hs2', health('synced', { stranded_saved_writes: [] })]))).toEqual([]);
    expect(strandedChangeCount(STRANDED)).toBe(3);
    expect(strandedChangeCount(health('synced'))).toBe(0);
  });

  it('shows a publishing store with stranded changes, their count, and the restore command', () => {
    const html = String(TicketStoreSyncBanner({ report: report(['hotsheet2.hs2', STRANDED]) }));
    expect(html).toContain('data-component="ticket-store-sync-banner"');
    expect(html).toContain('3 ticket changes in hotsheet2.hs2 were never restored');
    expect(html).toContain('kept only in a Hot Sheet backup');
    expect(html).toContain(`hotsheet-cli sync --restore-saved-writes ${ref}`);
    expect(html).not.toContain('not publishing');
    const single = health('synced', {
      next_action: restore,
      stranded_saved_writes: [{ backup_ref: ref, paths: ['a'] }],
    });
    expect(String(TicketStoreSyncBanner({ report: report(['web.hs2', single]) }))).toContain(
      '1 ticket change in web.hs2 was never restored',
    );
  });

  it('counts other stranded stores, and a failing store keeps the publication strip with the restore in its next step', () => {
    const several = String(
      TicketStoreSyncBanner({ report: report(['a.hs2', STRANDED], ['b.hs2', STRANDED], ['c.hs2', health('synced')]) }),
    );
    expect(several).toContain('1 more store holds changes to restore');
    const failing = health('rejected', {
      consecutive_failures: 1,
      next_action: `Read the remote's reason, then run \`hotsheet-cli sync\`. ${restore}`,
      stranded_saved_writes: STRANDED.stranded_saved_writes,
    });
    const html = String(TicketStoreSyncBanner({ report: report(['a.hs2', failing]) }));
    expect(html).toContain('Ticket store a.hs2 is not publishing changes');
    expect(html).toContain('--restore-saved-writes');
    expect(html).not.toContain('never restored');
  });

  it('clears once the changes are restored (stranded, then none)', () => {
    expect(TicketStoreSyncBanner({ report: report(['hotsheet2.hs2', STRANDED]) })).toBeDefined();
    expect(TicketStoreSyncBanner({ report: report(['hotsheet2.hs2', health('synced')]) })).toBeUndefined();
  });
});

describe('server sentences as plain text (HS2-GN2S3F)', () => {
  it('drops Markdown code-span delimiters and keeps the command', () => {
    expect(plainServerText('Fix it, then run `hotsheet-cli sync`.')).toBe('Fix it, then run hotsheet-cli sync.');
    expect(plainServerText('Run `a` or `b`.')).toBe('Run a or b.');
    expect(plainServerText('No code here.')).toBe('No code here.');
    expect(plainServerText('')).toBe('');
  });

  it('shows no backtick in the failing, blocked, and stranded strips', () => {
    const failing = String(
      TicketStoreSyncBanner({
        report: report([
          'kerf.hs2',
          health('rejected', { unpublished_commits: 1, next_action: 'Fix the cause, then run `hotsheet-cli sync`.' }),
        ]),
      }),
    );
    expect(failing).toContain('then run hotsheet-cli sync.');
    expect(failing).not.toContain('`');
    const blocked = String(
      TicketStoreSyncBanner({
        report: report([
          'web.hs2',
          health('blocked', {
            consecutive_failures: PERSISTENT_OFFLINE_FAILURES,
            reason: '`index.lock` has blocked sync for 5 minutes',
            next_action: 'Run `hotsheet-cli sync --recover-stale-locks`.',
          }),
        ]),
      }),
    );
    expect(blocked).toContain('Index.lock has blocked sync');
    expect(blocked).toContain('Run hotsheet-cli sync --recover-stale-locks.');
    expect(blocked).not.toContain('`');
    const stranded = String(
      TicketStoreSyncBanner({
        report: report([
          'web.hs2',
          health('synced', {
            next_action: 'Run `hotsheet-cli sync --restore-saved-writes refs/x` to write them back.',
            stranded_saved_writes: [{ backup_ref: 'refs/x', paths: ['tickets/AB/one.md'] }],
          }),
        ]),
      }),
    );
    expect(stranded).toContain('Run hotsheet-cli sync --restore-saved-writes refs/x to write them back.');
    expect(stranded).not.toContain('`');
  });
});
