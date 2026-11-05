import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';

import { StatsProjectsTable } from '../components/stats-projects-table';
import { rightNowRows } from '../stats-right-now';
import type { DemoModule } from './demo-module';
import { GLANCE_FIXTURE_NOW, glanceScenario } from './live-state-fixtures';

/** The Right now Projects table with the mock's synthetic projects (HS2-ERTNJ7). */
export function StatsProjectsTableDemo() {
  const rows = rightNowRows(glanceScenario(), GLANCE_FIXTURE_NOW, {
    projectFor: (state) => state.checkouts[0],
    terminalFor: (projectId) => `${projectId}:demo-terminal`,
  });
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Sorted by Need"
        note="May be stuck offers Open terminal; Waiting on you offers Answer; a project stopped as expected recedes. Narrow the stage to see the table scroll inside its own container."
        viewport={{ width: 'full' }}
      >
        {StatsProjectsTable({ rows })}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'stats-projects-table': { catalogExamples: true, stackWidth: 'full', Demo: StatsProjectsTableDemo },
  },
};
