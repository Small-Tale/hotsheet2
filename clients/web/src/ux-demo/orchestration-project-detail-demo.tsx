import { CatalogExample } from '@kerfjs/ui/catalog';

import type { OrchestrationSession } from '../api';
import { projectActionItems } from '../components/orchestration-project-actions';
import {
  OrchestrationProjectDetail,
  type OrchestrationProjectDetailProps,
  orchestrationProjectPanel,
} from '../components/orchestration-project-detail';
import { SidebarPane } from '../components/sidebar-panel';
import type { OrchestrationProjectRow, TargetLimitsDraft } from '../orchestration-settings-model';
import type { DemoModule } from './demo-module';
import {
  DEMO_TARGETS,
  demoBinding,
  demoCapacity,
  demoSessions,
  demoTime,
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
  models: { codex: 'gpt-5-codex', claude: 'claude-opus-*' },
  resources: 'vm/macos-vm',
  roleSessions: ['dispatcher'],
  roleSessionsAuto: false,
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
  models: {},
  resources: '',
  // HS2-R2N33M: unlisted role sessions are Auto, shown as both.
  roleSessions: ['dispatcher', 'integrator'],
  roleSessionsAuto: true,
};
const policyTools = ['codex', 'claude'];
/** HS2-1X1TXC: the models each tool reports, which its Models control offers with their families. */
const toolModels = {
  claude: ['claude-opus-4-1', 'claude-sonnet-4-5'],
  codex: ['gpt-5-codex', 'gpt-5.1-codex-mini'],
};
/** HS2-TK9ADC: the dispatcher the orchestrator started for the project's ready work. */
const sessions: OrchestrationSession[] = [
  ...demoSessions,
  {
    ...demoSessions[0],
    session_id: '01JSYNTHSESSION00000000003',
    role: 'dispatcher',
    tool: 'codex',
    label: 'Launch: dispatcher',
    hosted: undefined,
  },
];

const admissions = [
  { outcome: 'accepted' as const, tool: 'claude', requested: 1, admitted: 1, reason: null },
  { outcome: 'reduced' as const, tool: 'codex', requested: 2, admitted: 1, reason: 'session_cap_reached' },
];

/** HS2-RD16CQ: a project row as Settings › Orchestration lists it, for the panel toolbar's actions. */
const row = (change: Partial<OrchestrationProjectRow>): OrchestrationProjectRow => ({
  targetId: DEMO_TARGETS.web,
  label: 'Web',
  location: 'local',
  where: 'Local on This Computer',
  place: 'Local on This Computer',
  connection: { label: 'Bound', tone: 'neutral' },
  machine: 'This Computer',
  ...change,
});

/**
 * HS2-RD16CQ: the detail as the inspector's panel, with its toolbar's Actions for <name> menu (HS2-M1T6KN): a
 * home project whose tab is open, with Add a Site; and a site, whose project tab is not open, so Show Project
 * Tab is disabled and the menu offers Make <site> the Home Site and Separate from <project>.
 */
function ProjectPanel({ props }: { props: OrchestrationProjectDetailProps }) {
  return <SidebarPane parts={orchestrationProjectPanel(props)} side="right" />;
}

const homeRow = row({ tabProjectId: 'web-0123456789ab' }),
  siteRow = row({
    targetId: DEMO_TARGETS.api,
    label: 'Web second clone',
    location: 'remote',
    where: 'Site of Web · Remote on Build Host',
    place: 'Remote on Build Host',
    machine: 'Build Host',
    siteOf: DEMO_TARGETS.web,
  });

/** HS2-RD16CQ: the panel parts of a home project and of a site, as the app's inspector builds them. */
function ProjectPanelExamples() {
  return (
    <>
      <CatalogExample
        label="Panel Toolbar, Home Project"
        viewport={{ width: 'sidebar', height: 'tall', frame: 'solid', overflow: 'hidden' }}
      >
        <ProjectPanel
          props={{
            targetId: homeRow.targetId,
            label: homeRow.label,
            where: homeRow.where,
            binding: demoBinding('web'),
            detail: { targetId: homeRow.targetId, capacity: demoCapacity, admissions: [], loading: false },
            actions: projectActionItems(homeRow, {
              tabOpen: true,
              busy: false,
              siteChoices: [
                {
                  projectId: 'web-laptop-0123456789ab',
                  name: 'Web laptop',
                  location: 'remote',
                  machine: 'Laptop',
                  sameStore: true,
                },
              ],
            }),
            toolLabel: demoToolLabel,
            formatTime: demoTime,
          }}
        />
      </CatalogExample>
      <CatalogExample
        label="Panel Toolbar, Site"
        viewport={{ width: 'sidebar', height: 'tall', frame: 'solid', overflow: 'hidden' }}
      >
        <ProjectPanel
          props={{
            targetId: siteRow.targetId,
            label: siteRow.label,
            where: siteRow.where,
            binding: demoBinding('api'),
            detail: { targetId: siteRow.targetId, admissions: [], sessions: [], loading: false },
            actions: projectActionItems(siteRow, { tabOpen: false, busy: false, projectLabel: 'Web' }),
            homeLabel: 'Web',
            toolLabel: demoToolLabel,
            formatTime: demoTime,
          }}
        />
      </CatalogExample>
    </>
  );
}

/**
 * One orchestrated project's detail (HS2-W27N1D), as the inspector shows it: a connected project
 * running sessions, an unreachable one whose reads fail in place, and one still loading. Each keeps
 * only its Admission and Open in Systems; the machine facts are on the Systems screen (HS2-JF3EA3).
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
            sessions,
            loading: false,
            limits: listedLimits,
          }}
          sessions={sessions.map((session, index) => ({ session, showable: index === 0, busy: false }))}
          claimNamespaces={['vm']}
          policyTools={policyTools}
          toolModels={toolModels}
          resourceChoices={['vm/*', 'vm/macos-vm', 'vm/windows-vm']}
          systemsMachineId="local"
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
      <CatalogExample label="Another Site of a Project">
        <OrchestrationProjectDetail
          targetId={DEMO_TARGETS.api}
          label="Web second clone"
          where="Site of Web · Remote on Build Host"
          binding={demoBinding('api')}
          detail={{
            targetId: DEMO_TARGETS.api,
            admissions: [],
            admissionTools: [],
            sessions: [],
            loading: false,
            limits: { ...listedLimits, targetId: DEMO_TARGETS.api, roleSessions: ['dispatcher'] },
          }}
          policyTools={policyTools}
          homeLabel="Web"
          toolLabel={demoToolLabel}
          formatTime={demoTime}
        />
      </CatalogExample>
      <CatalogExample label="Several Sites">
        <OrchestrationProjectDetail
          targetId={DEMO_TARGETS.web}
          label="Web"
          where="Local on This Computer"
          binding={demoBinding('web')}
          detail={{ targetId: DEMO_TARGETS.web, capacity: demoCapacity, admissions: [], loading: false }}
          sites={[
            {
              targetId: DEMO_TARGETS.web,
              label: 'Web',
              where: 'Local on This Computer',
              home: true,
            },
            {
              targetId: DEMO_TARGETS.api,
              label: 'Web on the build host',
              where: 'Remote on Build Host',
              home: false,
            },
            {
              targetId: DEMO_TARGETS.docs,
              label: 'Web laptop',
              where: 'Remote on Laptop',
              home: false,
            },
          ]}
          toolLabel={demoToolLabel}
          formatTime={demoTime}
        />
      </CatalogExample>
      <ProjectPanelExamples />
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
