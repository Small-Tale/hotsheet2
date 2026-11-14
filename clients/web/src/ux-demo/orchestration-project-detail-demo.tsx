import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationProjectDetail } from '../components/orchestration-project-detail';
import type { DemoModule } from './demo-module';
import {
  DEMO_TARGETS,
  demoBinding,
  demoCapacity,
  demoSessions,
  demoTime,
  demoToolCapacity,
  demoToolLabel,
} from './orchestration-settings-fixtures';

const admissions = [
  { outcome: 'accepted' as const, tool: 'claude', requested: 1, admitted: 1, reason: null },
  { outcome: 'reduced' as const, tool: 'codex', requested: 2, admitted: 1, reason: 'session_cap_reached' },
];

/**
 * One orchestrated project's detail (HS2-W27N1D), as the inspector shows it: a connected project
 * running sessions, an unreachable one whose reads fail in place, and one still loading.
 */
export function OrchestrationProjectDetailDemo() {
  return (
    <>
      <CatalogExample label="Connected, Running Sessions">
        <OrchestrationProjectDetail
          targetId={DEMO_TARGETS.web}
          label="Web"
          where="Local on This Computer"
          binding={demoBinding('web')}
          detail={{
            targetId: DEMO_TARGETS.web,
            capacity: demoCapacity,
            admissions,
            toolCapacity: demoToolCapacity,
            sessions: demoSessions,
            loading: false,
          }}
          sessions={demoSessions.map((session, index) => ({ session, showable: index === 0, busy: false }))}
          claimNamespaces={['vm']}
          toolLabel={demoToolLabel}
          formatTime={demoTime}
        />
      </CatalogExample>
      <CatalogExample label="Unreachable">
        <OrchestrationProjectDetail
          targetId={DEMO_TARGETS.docs}
          label="Docs"
          where="Remote on Build Host"
          binding={{ ...demoBinding('api'), last_failure: 'unreachable' }}
          detail={{
            targetId: DEMO_TARGETS.docs,
            capacityError: 'orchestration observation failed: unreachable',
            admissions: [],
            admissionError: 'orchestration observation failed: unreachable',
            toolCapacity: demoToolCapacity,
            sessions: [],
            loading: false,
          }}
          toolLabel={demoToolLabel}
          formatTime={demoTime}
        />
      </CatalogExample>
      <CatalogExample label="Loading">
        <OrchestrationProjectDetail
          targetId={DEMO_TARGETS.api}
          label="API"
          where="Remote on Build Host"
          detail={{ targetId: DEMO_TARGETS.api, admissions: [], loading: true }}
          formatTime={demoTime}
        />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'orchestration-project-detail': { catalogExamples: true, Demo: OrchestrationProjectDetailDemo },
  },
};
