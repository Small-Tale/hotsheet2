import { describe, expect, it } from 'vitest';

import type { TicketStoreAiHelp, TicketStoreSyncHealth, TicketStoreSyncReport, TicketStoreSyncState } from '../api';
import {
  aiHelpSentence,
  failingTicketStores,
  PERSISTENT_OFFLINE_FAILURES,
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
    expect(html).toContain('Hot Sheet started codex to repair it.');
    expect(html).toContain('sync publishes once it is repaired');
    expect(html).toContain('href="#ticket-HS-ABC123"');
    expect(html).toContain('data-ticket-slug="HS-ABC123"');
    expect(html).toContain('data-action="open-linked-ticket"');
    expect(html).toContain('Open HS-ABC123');
  });

  it('has no ticket link and no ask sentence when no AI was asked', () => {
    const html = String(TicketStoreSyncBanner({ report: report(['tickets.hs2', health('conflict')]) }));
    expect(html).not.toContain('open-linked-ticket');
    expect(html).not.toContain('to repair it');
  });
});
