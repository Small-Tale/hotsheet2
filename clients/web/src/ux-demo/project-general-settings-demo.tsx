import { CatalogExample } from '@kerfjs/ui/catalog';

import { wireOrchestrationSettings } from '../components/orchestration-settings-interactions';
import { ProjectGeneralSettings } from '../components/project-general-settings';
import type { DemoModule } from './demo-module';
import { createDemoOrchestrationSettings, DEMO_OWNER } from './orchestration-settings-fixtures';

const demo = createDemoOrchestrationSettings();

/** Project Settings → General (HS2-W27N1D): the production controller over an in-memory project. */
export function ProjectGeneralSettingsDemo() {
  const state = demo.settings.types.value[DEMO_OWNER];
  return (
    <>
      <CatalogExample label="Interactive">
        <ProjectGeneralSettings
          projectType={state?.view?.project_type}
          activeProjects={state?.view?.active_projects}
          loading={state?.loading}
          saving={state?.saving}
          message={state?.message}
        />
      </CatalogExample>
      <CatalogExample label="Software (Default)">
        <ProjectGeneralSettings projectType="software" />
      </CatalogExample>
      <CatalogExample label="Software Keeping Added Projects">
        <ProjectGeneralSettings projectType="software" activeProjects={3} />
      </CatalogExample>
      <CatalogExample label="Loading">
        <ProjectGeneralSettings loading />
      </CatalogExample>
      <CatalogExample label="Save Failure">
        <ProjectGeneralSettings
          projectType="orchestration"
          message="invalid orchestration request: projects are still added"
        />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'project-general-settings': {
      catalogExamples: true,
      Demo: ProjectGeneralSettingsDemo,
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
