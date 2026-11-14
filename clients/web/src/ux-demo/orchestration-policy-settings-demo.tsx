import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationPolicySettings } from '../components/orchestration-policy-settings';
import { policyDraftFrom } from '../orchestration-settings-model';
import type { DemoModule } from './demo-module';
import { demoToolLabel } from './orchestration-settings-fixtures';

const stored = policyDraftFrom(
  {
    enabled: true,
    revision: '01JSYNTHREVISION0000000002',
    policy: {
      machine: { max_sessions: 2, min_free_ram_mb: 4096 },
      tools: [{ tool: 'codex', max_sessions: 3, weight: 60 }],
      placement: { automatic: true, actor: 'synthetic-person' },
      speculation: { allowed: true },
    },
  },
  ['claude', 'codex'],
);

/** The limits-only policy form of Settings → Orchestration (HS2-W27N1D, HS2-XHQ1EH). */
export function OrchestrationPolicySettingsDemo() {
  return (
    <>
      <CatalogExample label="Stored Policy" viewport={{ width: 'full' }}>
        <OrchestrationPolicySettings draft={stored} toolLabel={demoToolLabel} />
      </CatalogExample>
      <CatalogExample label="No Policy Yet (Defaults)" viewport={{ width: 'full' }}>
        <OrchestrationPolicySettings
          draft={policyDraftFrom(undefined, ['claude', 'codex'])}
          toolLabel={demoToolLabel}
        />
      </CatalogExample>
      <CatalogExample label="Saving" viewport={{ width: 'full' }}>
        <OrchestrationPolicySettings draft={stored} saving toolLabel={demoToolLabel} />
      </CatalogExample>
      <CatalogExample label="Loading" viewport={{ width: 'full' }}>
        <OrchestrationPolicySettings />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'orchestration-policy-settings': { catalogExamples: true, Demo: OrchestrationPolicySettingsDemo },
  },
};
