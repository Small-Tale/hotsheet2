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

const OFFLINE: TicketStoreSyncHealth = {
  ...BASE,
  state: 'offline',
  unpublished_commits: 1,
  consecutive_failures: 5,
  next_action: 'Check the network and Git credentials; sync retries automatically, or run `hotsheet-cli sync`.',
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
