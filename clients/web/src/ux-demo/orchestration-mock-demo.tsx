import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { AppShell } from '../components/app-shell';
import { projectSidebarPanel } from '../components/project-sidebar';
import { WorkspaceIdentity } from '../components/workspace-header';
import { TICKET_GROUP_ACTIONS } from '../interaction-attrs/ticket-groups';
import { projectTabs } from './app-shell-demo';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import { type DemoModule, type FormControl, listen } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import {
  findStep,
  type MockGrouping,
  type MockScenario,
  type MockSection,
  mockState,
  mockTargets,
  resetMockState,
} from './orchestration-mock/fixtures';
import { ORCH_MOCK_ANSWER_APPROVAL } from './orchestration-mock/orchestration-mock-approvals';
import { OrchestrationMockPolicy, orchestrationMockSettingsRail } from './orchestration-mock/orchestration-mock-policy';
import {
  ORCH_MOCK_RUN_CONTROL,
  orchestrationMockStepPanel,
} from './orchestration-mock/orchestration-mock-step-inspector';
import {
  ORCH_MOCK_SELECT_TARGET,
  ORCH_MOCK_SESSION_CONTROL,
  orchestrationMockTargetPanel,
} from './orchestration-mock/orchestration-mock-targets';
import {
  ORCH_MOCK_ADD_TARGET,
  ORCH_MOCK_SET_SECTION,
  ORCH_MOCK_SET_VIEW,
  OrchestrationMockHeaderActions,
  OrchestrationMockView,
} from './orchestration-mock/orchestration-mock-view';
import { ORCH_MOCK_SELECT_STEP } from './orchestration-mock/orchestration-mock-work';
import { sidebarCommands, sidebarViews } from './project-sidebar-demo';
import { syncSettingsControls } from './settings-controls';

/** Groups collapsed in the mock's Work section, by orchestration ticket. */
const collapsedGroups = signal<readonly string[]>([]);
/** Whether the policy mock shows its validation failure. */
const policyInvalid = signal(false);

/** The app's phone breakpoint (1024px of viewport): side panels overlay the main column below it. */
const MOBILE_BELOW = 1024;

function mockSidebar() {
  return projectSidebarPanel({
    completedToday: 4,
    inProgress: 3,
    completionTrend: [2, 1, 3, 4, 2, 5, 4],
    branch: 'main',
    unpushed: 0,
    uncommitted: 0,
    views: sidebarViews,
    // The Orchestration view filters no ticket list, so no ticket view is selected.
    selectedViewId: '',
    commandGroupLabel: 'Project commands',
    commands: sidebarCommands,
    commandGroupExpanded: false,
    driveRunning: false,
    driveTool: 'codex',
    openCount: 9,
    upNextCount: 2,
    activeCount: 3,
  });
}

function mockInspector() {
  const scenario = mockState.scenario.value;
  return mockState.section.value === 'targets'
    ? orchestrationMockTargetPanel({
        target: mockTargets(scenario).find((target) => target.id === mockState.selectedTarget.value),
        scenario,
      })
    : orchestrationMockStepPanel({ step: findStep(scenario, mockState.selectedStep.value), scenario });
}

/** The Orchestration project view inside the real application shell (HS2-HSMH2Q mock). */
function OrchestrationWorkspaceMock() {
  const scenario = mockState.scenario.value,
    mobile = mockState.mobile.value;
  return (
    <CatalogExample
      label="Orchestration View"
      viewport={{ width: 'full', height: 'app' }}
      rootAttributes={{ 'data-orch-mock-root': 'workspace' }}
    >
      <AppShell
        presentation="viewport"
        mobile={mobile}
        tabs={projectTabs.value}
        sidebar={mockSidebar()}
        sidebarVisible={!mobile}
        header={<WorkspaceIdentity projectName="Orchestration" id="orch-mock-page-title" headingLevel={1} />}
        headerActions={<OrchestrationMockHeaderActions view="orchestration" compact={mobile} />}
        workspace={
          <OrchestrationMockView
            scenario={scenario}
            grouping={mockState.grouping.value}
            section={mockState.section.value}
            selectedStep={mockState.selectedStep.value}
            selectedTarget={mockState.selectedTarget.value}
            collapsed={collapsedGroups.value}
          />
        }
        inspector={scenario === 'empty' ? undefined : mockInspector()}
        inspectorVisible={scenario !== 'empty' && mockState.inspectorOpen.value}
      />
      <DemoEvent>{mockState.event.value}</DemoEvent>
    </CatalogExample>
  );
}

/** Each inspector state at full height, for review without the shell's frame. */
function OrchestrationPanelsMock() {
  const scenario = mockState.scenario.value;
  const step = (id: string) => findStep(scenario, id);
  const target = (id: string) => mockTargets(scenario).find((item) => item.id === id);
  return (
    <>
      <CatalogExample
        label="Run in Progress"
        viewport={{ width: 'compact' }}
        rootAttributes={{ 'data-orch-mock-panel': 'run' }}
      >
        {
          orchestrationMockStepPanel({
            step: step(scenario === 'degraded' ? 'step-tc9' : 'step-tb17'),
            scenario,
          }).content
        }
      </CatalogExample>
      <CatalogExample
        label="Not Running: Waits for Approval"
        viewport={{ width: 'compact' }}
        rootAttributes={{ 'data-orch-mock-panel': 'approval' }}
      >
        {orchestrationMockStepPanel({ step: step('step-ta47'), scenario }).content}
      </CatalogExample>
      <CatalogExample
        label="Not Running: Resource Held"
        viewport={{ width: 'compact' }}
        rootAttributes={{ 'data-orch-mock-panel': 'resource' }}
      >
        {orchestrationMockStepPanel({ step: step('step-tb20'), scenario }).content}
      </CatalogExample>
      <CatalogExample
        label="Target"
        viewport={{ width: 'compact' }}
        rootAttributes={{ 'data-orch-mock-panel': 'target' }}
      >
        {
          orchestrationMockTargetPanel({
            target: target(scenario === 'degraded' ? 'target-c' : 'target-b'),
            scenario,
          }).content
        }
      </CatalogExample>
      <CatalogExample
        label="Target Not Bound"
        viewport={{ width: 'compact' }}
        rootAttributes={{ 'data-orch-mock-panel': 'unbound' }}
      >
        {orchestrationMockTargetPanel({ target: target('target-d'), scenario }).content}
      </CatalogExample>
    </>
  );
}

/** Settings → Orchestration inside the real application shell. */
function OrchestrationPolicyMock() {
  const mobile = mockState.mobile.value;
  return (
    <CatalogExample
      label="Settings Category"
      viewport={{ width: 'full', height: 'app' }}
      rootAttributes={{ 'data-orch-mock-root': 'policy' }}
    >
      <AppShell
        presentation="viewport"
        mobile={mobile}
        tabs={projectTabs.value}
        sidebar={orchestrationMockSettingsRail()}
        sidebarVisible={!mobile}
        header={<WorkspaceIdentity projectName="Project Settings" id="orch-mock-policy-title" headingLevel={1} />}
        headerActions={<OrchestrationMockHeaderActions view="settings" compact={mobile} />}
        workspace={<OrchestrationMockPolicy invalid={policyInvalid.value} />}
        workspacePresentation="inset"
      />
    </CatalogExample>
  );
}

function OrchestrationMockSettings() {
  return (
    <DemoSettingsForm settings="orchestration-mock">
      <Select
        name="orch-scenario"
        label="State"
        value={mockState.scenario.value}
        choices={[
          { value: 'normal', label: 'Working' },
          { value: 'degraded', label: 'Degraded' },
          { value: 'empty', label: 'No Targets Yet' },
        ]}
      />
      <Select
        name="orch-grouping"
        label="Work Layout"
        value={mockState.grouping.value}
        choices={[
          { value: 'ticket', label: 'Grouped by Orchestration Ticket' },
          { value: 'flat', label: 'One Flat List' },
        ]}
      />
      <Select
        name="orch-section"
        label="Section"
        value={mockState.section.value}
        choices={[
          { value: 'work', label: 'Work' },
          { value: 'targets', label: 'Targets' },
          { value: 'scheduler', label: 'Scheduler' },
          { value: 'summary', label: 'Summary' },
        ]}
      />
      <Checkbox name="orch-inspector" label="Inspector Open" checked={mockState.inspectorOpen.value} />
      <Checkbox name="orch-policy-invalid" label="Policy Has Problems" checked={policyInvalid.value} />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

function resetOrchestrationMock(root: ParentNode): void {
  resetMockState();
  collapsedGroups.value = [];
  policyInvalid.value = false;
  syncSettingsControls(root, 'orchestration-mock', {
    values: { 'orch-scenario': 'normal', 'orch-grouping': 'ticket', 'orch-section': 'work' },
    checked: { 'orch-inspector': true, 'orch-policy-invalid': false },
  });
}

const within = (target: Element) => target.closest('[data-orch-mock-root], [data-orch-mock-panel]') !== null;

function wireOrchestrationMock(root: HTMLElement): () => void {
  const scope = createScope();
  const syncMobile = () => {
    mockState.mobile.value = window.innerWidth < MOBILE_BELOW;
  };
  syncMobile();
  scope.add(listen(window, 'resize', syncMobile));
  scope.add(
    delegate(root, 'click', `[data-action="${ORCH_MOCK_SELECT_STEP}"]`, (_event, target) => {
      if (!within(target)) return;
      mockState.selectedStep.value = (target as HTMLElement).dataset.stepId ?? mockState.selectedStep.value;
      mockState.inspectorOpen.value = true;
      mockState.event.value = `Selected ${mockState.selectedStep.value}.`;
    }),
  );
  scope.add(
    delegate(root, 'click', `[data-action="${ORCH_MOCK_SELECT_TARGET}"]`, (_event, target) => {
      if (!within(target)) return;
      mockState.selectedTarget.value = (target as HTMLElement).dataset.targetId ?? mockState.selectedTarget.value;
      mockState.inspectorOpen.value = true;
      mockState.event.value = `Selected ${mockState.selectedTarget.value}.`;
    }),
  );
  scope.add(
    delegate(root, 'click', `[data-action="${ORCH_MOCK_SET_SECTION}"]`, (_event, target) => {
      const value = (target as HTMLElement).dataset.segmentValue as MockSection | undefined;
      if (value) mockState.section.value = value;
    }),
  );
  scope.add(
    delegate(root, 'click', `[data-action="${ORCH_MOCK_SET_VIEW}"]`, (_event, target) => {
      mockState.event.value = `${(target as HTMLElement).dataset.segmentValue ?? 'That'} view is outside this mock.`;
    }),
  );
  scope.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.toggleTicketGroup.selector, (_event, target) => {
      if (!within(target)) return;
      const group = (target as HTMLElement).dataset.ticketGroup;
      if (!group) return;
      collapsedGroups.value = collapsedGroups.value.includes(group)
        ? collapsedGroups.value.filter((item) => item !== group)
        : [...collapsedGroups.value, group];
    }),
  );
  scope.add(
    delegate(root, 'click', '[data-orch-mock-root="workspace"] [data-action="toggle-ticket-inspector"]', () => {
      mockState.inspectorOpen.value = !mockState.inspectorOpen.value;
    }),
  );
  for (const [action, verb] of [
    [ORCH_MOCK_RUN_CONTROL, 'Run control'],
    [ORCH_MOCK_SESSION_CONTROL, 'Session control'],
    [ORCH_MOCK_ANSWER_APPROVAL, 'Approval answer'],
    [ORCH_MOCK_ADD_TARGET, 'Add Target'],
  ] as const)
    scope.add(
      delegate(root, 'click', `[data-action="${action}"]`, (_event, target) => {
        const element = target as HTMLElement;
        mockState.event.value = `${verb} ${element.dataset.control ?? element.dataset.answer ?? ''} requested (mock only).`;
      }),
    );
  scope.add(
    delegate(root, 'change', '[data-settings="orchestration-mock"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'orch-scenario') {
        mockState.scenario.value = control.value as MockScenario;
        // The degraded state opens on the run whose target cannot be confirmed.
        mockState.selectedStep.value = control.value === 'degraded' ? 'step-tc9' : 'step-tb17';
      }
      if (name === 'orch-grouping') mockState.grouping.value = control.value as MockGrouping;
      if (name === 'orch-section') mockState.section.value = control.value as MockSection;
      if (name === 'orch-inspector') mockState.inspectorOpen.value = control.checked;
      if (name === 'orch-policy-invalid') policyInvalid.value = control.checked;
    }),
  );
  return () => {
    scope.dispose();
  };
}

/** The orchestration workspace concept entries (HS2-HSMH2Q): a reviewable mock, not the shipped view. */
export const demoModule: DemoModule = {
  entries: {
    'orchestration-mock-workspace': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: OrchestrationWorkspaceMock,
      Settings: OrchestrationMockSettings,
      reset: resetOrchestrationMock,
    },
    'orchestration-mock-panels': {
      catalogExamples: true,
      Demo: OrchestrationPanelsMock,
      Settings: OrchestrationMockSettings,
      reset: resetOrchestrationMock,
    },
    'orchestration-mock-policy': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: OrchestrationPolicyMock,
      Settings: OrchestrationMockSettings,
      reset: resetOrchestrationMock,
    },
  },
  wire: wireOrchestrationMock,
};
