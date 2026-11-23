import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationLimitsHelp } from '../components/orchestration-limits-help';
import type { DemoModule } from './demo-module';

/** HS2-Y737GM: the content of the Limits header's help popup. */
export function OrchestrationLimitsHelpDemo() {
  return (
    <CatalogExample label="About Limits">
      <OrchestrationLimitsHelp />
    </CatalogExample>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'orchestration-limits-help': { catalogExamples: true, Demo: OrchestrationLimitsHelpDemo },
  },
};
