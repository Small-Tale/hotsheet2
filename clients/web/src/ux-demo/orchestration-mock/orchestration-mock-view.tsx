import { Button } from '@kerfjs/ui/button';
import { px } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SegmentedControl } from '@kerfjs/ui/segmented-control';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Bell, Columns3, List as ListIcon, Plus, Settings, UsersRound, Workflow } from 'lucide';

import type { MockGrouping, MockScenario, MockSection } from './fixtures';
import { mockTargets, mockTickets } from './fixtures';
import { OrchestrationMockApprovals } from './orchestration-mock-approvals';
import { OrchestrationMockScheduler } from './orchestration-mock-scheduler';
import { OrchestrationMockSummary } from './orchestration-mock-summary';
import { OrchestrationMockTargets } from './orchestration-mock-targets';
import { OrchestrationMockWork } from './orchestration-mock-work';

export const ORCH_MOCK_SET_VIEW = 'orch-mock-set-view';
export const ORCH_MOCK_SET_SECTION = 'orch-mock-set-section';
export const ORCH_MOCK_ADD_TARGET = 'orch-mock-add-target';

const VIEW_CHOICES = [
  { value: 'list', label: 'List', icon: ListIcon, iconName: 'list' },
  { value: 'board', label: 'Columns', icon: Columns3, iconName: 'columns-3' },
  { value: 'workers', label: 'Workers', icon: UsersRound, iconName: 'users-round' },
  { value: 'orchestration', label: 'Orchestration', icon: Workflow, iconName: 'workflow' },
  { value: 'notifications', label: 'Notifications', icon: Bell, iconName: 'bell' },
  { value: 'settings', label: 'Settings', icon: Settings, iconName: 'settings' },
] as const;

/**
 * The project header's controls for the proposed sixth view: the production view switcher's Kerf
 * SegmentedControl with an Orchestration choice, and the view's one action. The shipped
 * `WorkspaceControls` mode list is closed, so the mock configures the same component directly.
 */
export function OrchestrationMockHeaderActions({
  view,
  compact = false,
}: {
  view: 'orchestration' | 'settings';
  /** On a phone the view's action leaves the header (an open question in the review notes). */
  compact?: boolean;
}) {
  return (
    <>
      <ToolbarControlGroup shape="pill" visibility="yield-to-expanded-sibling" hideBelow={px(176)}>
        <SegmentedControl
          id="orch-mock-view-mode"
          label="View mode"
          value={view}
          action={ORCH_MOCK_SET_VIEW}
          appearance="toolbar"
          shape="pill"
          layout="content"
          choices={VIEW_CHOICES.map((choice) => ({
            value: choice.value,
            label: `${choice.label} View`,
            title: `${choice.label} view`,
            content: <LucideIcon icon={choice.icon} name={choice.iconName} />,
          }))}
        />
      </ToolbarControlGroup>
      {view === 'orchestration' && !compact && (
        <ToolbarControlGroup shape="pill" single label="Orchestration Actions">
          <button
            type="button"
            data-action={ORCH_MOCK_ADD_TARGET}
            aria-label="Add Target…"
            title="Add a target project"
          >
            <LucideIcon icon={Plus} name="plus" />
          </button>
        </ToolbarControlGroup>
      )}
    </>
  );
}

const SECTION_CHOICES: ReadonlyArray<{ value: MockSection; label: string }> = [
  { value: 'work', label: 'Work' },
  { value: 'targets', label: 'Targets' },
  { value: 'scheduler', label: 'Scheduler' },
  { value: 'summary', label: 'Summary' },
];

function DegradedBanners() {
  return (
    <List gap="xs">
      <StateBanner
        copyLayout="stacked"
        tone="danger"
        title="Target C Is Unreachable"
        detail="Since 14:02. Its runs show the last confirmed state and nothing new starts there."
      />
      <StateBanner
        copyLayout="stacked"
        tone="danger"
        title="CI Failed for TA-44"
        detail="TC-11 is blocked until the gate passes."
      />
      <StateBanner
        copyLayout="stacked"
        tone="warning"
        title="Claude Is Limited Until 14:30"
        detail="Usage limit on the personal account."
      />
    </List>
  );
}

/**
 * The Orchestration project view's work area: approvals waiting on a person, the degraded-state
 * banners, a section switcher, and the selected section. Empty shows the first-run state.
 */
export function OrchestrationMockView({
  scenario,
  grouping,
  section,
  selectedStep,
  selectedTarget,
  collapsed,
}: {
  scenario: MockScenario;
  grouping: MockGrouping;
  section: MockSection;
  selectedStep: string;
  selectedTarget: string;
  collapsed: readonly string[];
}) {
  if (scenario === 'empty')
    return (
      <List fill hAlign="center" vAlign="middle" rootAttributes={{ 'data-orch-mock': 'empty' }}>
        <EmptyState
          icon={<LucideIcon icon={Workflow} name="workflow" />}
          title="No Targets Yet"
          detail="Add the projects this one plans work for. Each target is bound to a checkout on this computer before any work is sent there."
          action={
            <Button emphasis="primary" action={ORCH_MOCK_ADD_TARGET}>
              Add Target…
            </Button>
          }
        />
      </List>
    );
  return (
    <List
      fill
      scrollable
      gap="l"
      inset="document"
      ariaLabel="Orchestration"
      rootAttributes={{ 'data-orch-mock': 'view' }}
    >
      <OrchestrationMockApprovals scenario={scenario} />
      {scenario === 'degraded' && <DegradedBanners />}
      <Row>
        <SegmentedControl
          id="orch-mock-section"
          label="Orchestration section"
          value={section}
          action={ORCH_MOCK_SET_SECTION}
          choices={SECTION_CHOICES}
        />
      </Row>
      {section === 'work' && (
        <OrchestrationMockWork
          tickets={mockTickets(scenario)}
          grouping={grouping}
          selectedStep={selectedStep}
          collapsed={collapsed}
        />
      )}
      {section === 'targets' && (
        <OrchestrationMockTargets targets={mockTargets(scenario)} selectedTarget={selectedTarget} />
      )}
      {section === 'scheduler' && <OrchestrationMockScheduler scenario={scenario} selectedStep={selectedStep} />}
      {section === 'summary' && <OrchestrationMockSummary />}
    </List>
  );
}
