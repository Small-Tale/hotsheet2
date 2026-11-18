import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationSettings } from '../components/orchestration-settings';
import { wireOrchestrationSettings } from '../components/orchestration-settings-interactions';
import { addChoices, policyDraftFrom, projectRows } from '../orchestration-settings-model';
import type { DemoModule } from './demo-module';
import {
  createDemoOrchestrationSettings,
  DEMO_HOSTED_CHECKOUTS,
  DEMO_OPEN_PROJECTS,
  DEMO_OWNER,
  DEMO_REMOTE_LABELS,
  demoToolLabel,
} from './orchestration-settings-fixtures';

const demo = createDemoOrchestrationSettings();

/**
 * Settings → Orchestration (HS2-W27N1D): the production controller and handlers over an in-memory
 * orchestration project. Add Project adds an open project, a row's menu removes it, and Apply Policy
 * stores the limits.
 */
export function OrchestrationSettingsDemo() {
  const state = demo.settings.settings.value[DEMO_OWNER],
    labels = demo.settings.remoteLabels.value;
  return (
    <>
      <CatalogExample label="Interactive" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          rows={projectRows(state?.manifest, state?.bindings ?? {}, labels)}
          choices={addChoices(DEMO_OPEN_PROJECTS, DEMO_OWNER, state?.bindings ?? {}, state?.manifest, labels)}
          openTabs={DEMO_OPEN_PROJECTS.map((project) => project.id)}
          selectedTarget={demo.settings.selected.value[DEMO_OWNER]}
          loading={state?.loading ?? true}
          busy={state?.busy}
          message={state?.message}
          status={state?.status}
          policy={{ draft: state?.draft, toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="No Projects Yet" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          choices={addChoices(DEMO_OPEN_PROJECTS, DEMO_OWNER, {}, undefined, DEMO_REMOTE_LABELS)}
          policy={{ draft: policyDraftFrom(undefined, ['claude', 'codex']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Loading" viewport={{ width: 'full' }}>
        <OrchestrationSettings loading />
      </CatalogExample>
      <CatalogExample label="Open From Another Server" viewport={{ width: 'full' }}>
        {/* HS2-92FJXM: + Add Project offers the projects open on that server. */}
        <OrchestrationSettings
          hostedChoices={DEMO_HOSTED_CHECKOUTS.map((choice, index) => ({
            checkoutId: choice.checkout_id,
            name: choice.name,
            added: index === 0,
          }))}
          hostedServer="Build Host"
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Another Server's Projects Unreadable" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          hostedChoices={[]}
          hostedServer="Build Host"
          unavailableReason="Could not read the projects open on Build Host. The server did not answer."
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'orchestration-settings': {
      catalogExamples: true,
      Demo: OrchestrationSettingsDemo,
      reset: () => {
        void demo.reset();
      },
    },
  },
  wire: (root) => {
    void demo.reset();
    return wireOrchestrationSettings(root, demo.settings, {
      projectId: () => DEMO_OWNER,
      activateProject: () => undefined,
    });
  },
};
