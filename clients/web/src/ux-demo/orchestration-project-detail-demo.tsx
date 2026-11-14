import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationProjectDetail } from '../components/orchestration-project-detail';
import type { TargetLimitsDraft } from '../orchestration-settings-model';
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

const machine = { max_sessions: '2', min_free_ram_mb: '2048', max_cpu_percent: '90', min_free_disk_mb: '5120' };
/** HS2-7WF6JD: a listed project with its own machine limits, and one the policy does not list yet. */
const listedLimits: TargetLimitsDraft = {
  targetId: DEMO_TARGETS.web,
  listed: true,
  enabled: true,
  tools: ['codex', 'claude'],
  max_sessions: '2',
  machineOverride: true,
  machine: { ...machine, min_free_ram_mb: '4096' },
  buildAhead: 'off',
};
const unlistedLimits: TargetLimitsDraft = {
  targetId: DEMO_TARGETS.docs,
  listed: false,
  enabled: true,
  tools: [],
  max_sessions: '1',
  machineOverride: false,
  machine,
  buildAhead: 'default',
};
const policyTools = ['codex', 'claude'];

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
            limits: listedLimits,
          }}
          sessions={demoSessions.map((session, index) => ({ session, showable: index === 0, busy: false }))}
          claimNamespaces={['vm']}
          policyTools={policyTools}
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
            limits: unlistedLimits,
            limitsMessage: "Sessions Here must be at most 2, the machine's session limit.",
          }}
          policyTools={policyTools}
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
