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
    </>
  );
}
