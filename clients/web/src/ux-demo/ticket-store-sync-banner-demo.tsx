import { CatalogExample } from '@kerfjs/ui/catalog';

import type { TicketStoreSyncHealth, TicketStoreSyncReport } from '../api';
import { TicketStoreSyncBanner } from '../components/ticket-store-sync-banner';
import type { DemoModule } from './demo-module';

function demoReport(...stores: Array<[string, TicketStoreSyncHealth]>): TicketStoreSyncReport {
  return {
    checkout_id: 'hotsheet2-1a2b3c',
    stores: stores.map(([label, health]) => ({
      connection_id: `git:${label}`,
      label,
      locator: `/work/${label}`,
      health,
    })),
  };
}

const BASE = { syncing: false, incoming_commits: 0, last_success_at: '2026-11-09T18:00:00Z' } as const;

const REJECTED: TicketStoreSyncHealth = {
  ...BASE,
  state: 'rejected',
  unpublished_commits: 7925,
  consecutive_failures: 4,
  oversized: [{ path: 'attachments/01/scan.bin', bytes: 118_971_148, limit_bytes: 104_857_600 }],
  next_action:
    'Local tickets are saved but cannot be published until the oversized file leaves unpublished history (move it to Git LFS or rewrite those commits), then run `hotsheet-cli sync`.',
};

const STALLED: TicketStoreSyncHealth = {
  ...BASE,
  state: 'rebase_in_progress',
  unpublished_commits: 12,
  consecutive_failures: 2,
  next_action:
    'Sync continues an unfinished rebase by itself when nothing conflicts; this one needs a decision. Finish the rebase or merge in the store, or run `hotsheet-cli sync --abort-stalled-rebase` to restore the local branch (a backup ref is kept).',
};

const REPAIR_ASKED: TicketStoreSyncHealth = {
  ...BASE,
  state: 'conflict',
  unpublished_commits: 3,
  consecutive_failures: 1,
  next_action: 'Hot Sheet started codex to repair the store (ticket HS2-4KQ7RT); sync publishes once it is repaired.',
  ai_help: { outcome: 'started', mode: 'start', tool: 'codex', ticket: 'HS2-4KQ7RT', asked_at: '2026-11-13T08:00:00Z' },
};

const BLOCKED: TicketStoreSyncHealth = {
  ...BASE,
  state: 'blocked',
  unpublished_commits: 2,
  consecutive_failures: 3,
  reason:
    'a Git lock file blocked sync during the pre-sync check: .git/index.lock (created 3 hours ago). Git leaves a lock behind when one of its processes crashes or is killed; the store was not merged.\nNot moved automatically: .git/index.lock may still be in use: process 4242 (git) runs Git in the store.',
  next_action:
    "No ticket merge conflict was found. Sync moves a lock aside by itself once it is over 10 minutes old and no process has the store's Git directory open; `hotsheet-cli sync --recover-stale-locks` runs that check now. Otherwise make sure no Git process is running in the store (an editor's Git integration, a terminal, or another Hot Sheet), then remove the stale lock file and run `hotsheet-cli sync`.",
};

const OFFLINE: TicketStoreSyncHealth = {
  ...BASE,
  state: 'offline',
  unpublished_commits: 1,
  consecutive_failures: 5,
  next_action: 'Check the network and Git credentials; sync retries automatically, or run `hotsheet-cli sync`.',
};

const STRANDED: TicketStoreSyncHealth = {
  ...BASE,
  state: 'synced',
  unpublished_commits: 0,
  consecutive_failures: 0,
  reason:
    '2 changes saved during an interrupted rebase or merge recovery were never written back; they are kept only in a Hot Sheet backup ref.',
  next_action:
    'Run `hotsheet-cli sync --restore-saved-writes refs/hotsheet/sync-backup/1794863200000000000-saved-writes` to write them back and commit them on the branch.',
  stranded_saved_writes: [
    {
      backup_ref: 'refs/hotsheet/sync-backup/1794863200000000000-saved-writes',
      paths: ['tickets/AB/01M7EXAMPLEAAAAAAAAAAAAAAAB.md', 'tickets/CD/01M7EXAMPLEBBBBBBBBBBBBBBCD.md'],
    },
  ],
};

/** The project shell strip shown while a ticket store is not publishing (HS2-ADHFKZ). */
export function TicketStoreSyncBannerDemo() {
  return (
    <>
      <CatalogExample
        label="Push Rejected"
        note="The remote refused the push because an unpublished attachment is over its file size limit."
        viewport={{ width: 'full' }}
      >
        {TicketStoreSyncBanner({ report: demoReport(['kerf.hs2', REJECTED]) })}
      </CatalogExample>
      <CatalogExample
        label="Stalled Rebase"
        note="A rebase stopped at a conflict sync cannot settle; it will not start another one."
        viewport={{ width: 'full' }}
      >
        {TicketStoreSyncBanner({ report: demoReport(['hotsheet2.hs2', STALLED]) })}
      </CatalogExample>
      <CatalogExample
        label="Stale Git Lock"
        note="A Git lock file a crashed Git process left behind has blocked several sync attempts (HS2-6ACT5Y)."
        viewport={{ width: 'full' }}
      >
        {TicketStoreSyncBanner({ report: demoReport(['hotsheet2.hs2', BLOCKED]) })}
      </CatalogExample>
      <CatalogExample
        label="AI Repair Started"
        note="Sync could not finish alone, so Hot Sheet started the project's default AI tool and links its repair ticket."
        viewport={{ width: 'full' }}
      >
        {TicketStoreSyncBanner({ report: demoReport(['hotsheet2.hs2', REPAIR_ASKED]) })}
      </CatalogExample>
      <CatalogExample
        label="Changes Never Restored"
        note="A recovery stopped after setting ticket changes aside; the store publishes, but the changes wait in a backup (HS2-1HRYD9)."
        viewport={{ width: 'full' }}
      >
        {TicketStoreSyncBanner({ report: demoReport(['hotsheet2.hs2', STRANDED]) })}
      </CatalogExample>
      <CatalogExample
        label="Several Stores"
        note="Persistent offline counts once the outage has lasted several sync attempts."
        viewport={{ width: 'full' }}
      >
        {TicketStoreSyncBanner({ report: demoReport(['hotsheet2.hs2', OFFLINE], ['kerf.hs2', REJECTED]) })}
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'ticket-store-sync-banner': { catalogExamples: true, Demo: TicketStoreSyncBannerDemo },
  },
};
