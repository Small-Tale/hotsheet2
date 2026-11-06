import { CatalogExample } from '@kerfjs/ui/catalog';

import { DemoWorkerColumns } from './worker-columns-demo';

/** Wide and phone specimens use the same production WorkerColumns component. */
export function WorkerColumnsCatalogDemo() {
  return (
    <>
      <CatalogExample label="Wide" viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}>
        <DemoWorkerColumns />
      </CatalogExample>
      <CatalogExample label="Phone" viewport={{ width: 'compact', height: 'app', frame: 'solid', overflow: 'hidden' }}>
        <DemoWorkerColumns layout="paged" />
      </CatalogExample>
      {/* HS2-7WE7CQ: a failed read in the banner; unsupported and unobserved sources in the disclosure. */}
      <CatalogExample
        label="Coverage Gaps"
        viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}
      >
        <DemoWorkerColumns coverage="gaps" />
      </CatalogExample>
    </>
  );
}
