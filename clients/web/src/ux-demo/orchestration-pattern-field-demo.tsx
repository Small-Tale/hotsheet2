import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationPatternField } from '../components/orchestration-pattern-field';
import { MODELS_HINT, REQUIRED_RESOURCES_HINT } from '../orchestration-settings-model';
import type { DemoModule } from './demo-module';

/**
 * HS2-1X1TXC, HS2-NPAZQW: the Limits form's chosen-values field, as a tool's Models and as Required
 * Resources: empty, with chosen names and patterns, and disabled while the limits save.
 */
export function OrchestrationPatternFieldDemo() {
  return (
    <>
      <CatalogExample label="Models, Empty">
        <OrchestrationPatternField
          field="limits.models.claude"
          label="Claude Models"
          values={[]}
          suggestions={['claude-opus-*', 'claude-sonnet-*', 'claude-opus-4-1', 'claude-sonnet-4-5']}
          placeholder="Any model"
          hint={MODELS_HINT}
        />
      </CatalogExample>
      <CatalogExample label="Resources, Chosen">
        <OrchestrationPatternField
          field="limits.resources"
          label="Required Resources"
          values={['vm/*', 'ports/8080']}
          suggestions={['ports/*', 'vm/*', 'ports/8080', 'vm/macos-vm']}
          placeholder="None"
          hint={REQUIRED_RESOURCES_HINT}
        />
      </CatalogExample>
      <CatalogExample label="Disabled">
        <OrchestrationPatternField
          field="limits.models.codex"
          label="Codex Models"
          values={['gpt-*']}
          suggestions={[]}
          hint={MODELS_HINT}
          disabled
        />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'orchestration-pattern-field': { catalogExamples: true, Demo: OrchestrationPatternFieldDemo },
  },
};
