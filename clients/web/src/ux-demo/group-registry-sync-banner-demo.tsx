import { CatalogExample } from '@kerfjs/ui/catalog';

import { GroupRegistrySyncBanner } from '../components/group-registry-sync-banner';
import type { GroupRegistrySync } from '../server-response-parsers';
import type { DemoModule } from './demo-module';

const FAILED: GroupRegistrySync = {
  state: 'failed',
  detail: 'could not commit the group registry: pre-commit: whitespace check failed / hook exited with status 1',
  committed: false,
  at: '2026-11-13T00:40:00Z',
};

const BLOCKED: GroupRegistrySync = {
  state: 'blocked',
  detail: 'the branch is behind its upstream; pull (with --rebase) to publish the group registry',
  committed: true,
  at: '2026-11-13T00:40:00Z',
};

/** The project shell strip shown while the group registry is not publishing (HS2-VFYSQK). */
export function GroupRegistrySyncBannerDemo() {
  return (
    <>
      <CatalogExample
        label="Commit Failed"
        note="A repository hook refused the registry commit; the server sync pass retries it."
        viewport={{ width: 'full' }}
      >
        {GroupRegistrySyncBanner({ status: FAILED })}
      </CatalogExample>
      <CatalogExample
        label="Behind Upstream"
        note="The registry commit is local; publishing it needs the person to pull first."
        viewport={{ width: 'full' }}
      >
        {GroupRegistrySyncBanner({ status: BLOCKED })}
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'group-registry-sync-banner': { catalogExamples: true, Demo: GroupRegistrySyncBannerDemo },
  },
};
