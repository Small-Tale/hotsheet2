import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationSettings } from '../components/orchestration-settings';
import { wireOrchestrationSettings } from '../components/orchestration-settings-interactions';
import { addChoices, policyDraftFrom, projectRows } from '../orchestration-settings-model';
import type { DemoModule } from './demo-module';
import {
  createDemoOrchestrationSettings,
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
        <OrchestrationSettings
          unavailableReason="This project is open from another server. Add projects to it on the computer it runs on."
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
