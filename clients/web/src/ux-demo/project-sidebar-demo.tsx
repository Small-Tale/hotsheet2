import { CatalogExample } from '@kerfjs/ui/catalog';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { delegate, type SafeHtml, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { GripHorizontal } from 'lucide';

import type { CommandDefinition } from '../api';
import { type CommandDropTarget, keptCommandGroups, reorderCommandsMultiple } from '../command-order';
import { AiToolSettings } from '../components/ai-tool-settings';
import { CommandNavigation, type CommandNavigationItem } from '../components/command-navigation';
import { COMMAND_EDITOR_DIALOG_ID, CommandSettingsEditor } from '../components/command-settings-editor';
import { wireCommandSettingsEditor } from '../components/command-settings-editor-interactions';
import { DriveControl } from '../components/drive-control';
import { type AiToolDescriptor, DriveOptionsMenu } from '../components/drive-options-menu';
import { NotificationNavigation } from '../components/notification-navigation';
import { ProjectSidebar } from '../components/project-sidebar';
import { ProjectSummary } from '../components/project-summary';
import { RepositorySummary } from '../components/repository-summary';
import { SettingsNavigation } from '../components/settings-navigation';
import { TerminalOperationsSidebar } from '../components/terminal-operations-sidebar';
import { ViewNavigation, type ViewNavigationItem } from '../components/view-navigation';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { loadLucideCatalog } from '../lucide-catalog';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { listen } from './demo-module';
import { syncSettingsControls } from './settings-controls';

export const selectedViewId = signal('all');
export const commandGroupExpanded = signal(true);
export const collapsedCommandGroups = signal<string[]>([]);
export const runningCommandId = signal<string | undefined>(undefined);
export const driveRunning = signal(false);
export const sidebarEvent = signal('Choose a view or run an action.');
/** Conversation-toggle states the ProjectSidebar demo exposes for review (HS2-PK67QK). */
export const projectSidebarSettings = {
  conversationOpen: signal(false),
  conversationDisabled: signal(false),
};
export function syncProjectSidebarSettingsControls(root: ParentNode): void {
  syncSettingsControls(root, 'project-sidebar', {
    checked: {
      'conversation-open': projectSidebarSettings.conversationOpen.value,
      'conversation-disabled': projectSidebarSettings.conversationDisabled.value,
    },
  });
}
export function resetProjectSidebarDemo(controlRoot?: ParentNode): void {
  projectSidebarSettings.conversationOpen.value = false;
  projectSidebarSettings.conversationDisabled.value = false;
  if (controlRoot) syncProjectSidebarSettingsControls(controlRoot);
}
export function ProjectSidebarSettings() {
  return (
    <form class="settings-form" data-settings="project-sidebar">
      <wa-checkbox name="conversation-open" checked={projectSidebarSettings.conversationOpen.value}>
        Conversation open
      </wa-checkbox>
      <wa-checkbox name="conversation-disabled" checked={projectSidebarSettings.conversationDisabled.value}>
        Conversation disabled
      </wa-checkbox>
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}
export const projectSidebarHeight = signal(640);
export const PROJECT_SIDEBAR_MIN_HEIGHT = 288;
export const PROJECT_SIDEBAR_MAX_HEIGHT = 768;
export const clampProjectSidebarHeight = (height: number) =>
  Math.min(PROJECT_SIDEBAR_MAX_HEIGHT, Math.max(PROJECT_SIDEBAR_MIN_HEIGHT, Math.round(height)));

export const sidebarViews: ViewNavigationItem[] = [
  { id: 'needs-review', label: 'Needs Review', count: 3, attention: true, icon: 'needs-review' },
  { id: 'all', label: 'Queue', count: 12, icon: 'all' },
  { id: 'backlog', label: 'Backlog', count: 5, icon: 'backlog' },
  { id: 'archive', label: 'Archive', count: 241, icon: 'archive' },
  { id: 'trash', label: 'Trash', count: 4, icon: 'trash' },
];
export const sidebarCommands: CommandNavigationItem[] = [
  { id: 'verify', label: 'Verify project', color: '#14b8a6', icon: 'test', group: 'Quality' },
  { id: 'build', label: 'Build clients', color: '#f97316', icon: 'build', kind: 'shell', group: 'Quality' },
  // The transparent palette slot (no fill) aligns with the filled rows (HS2-F9JKMJ).
  { id: 'everything', label: 'Run everything', color: 'transparent', icon: 'balloon', group: 'Quality' },
  { id: 'publish', label: 'Publish preview', color: '#8b5cf6', icon: 'send', kind: 'ai', group: 'Release' },
];

/** One sidebar specimen in a compact Kerf frame, with the shared sidebar event log below it. Resizing
 * the sidebar does not write to the log, so no stale height readout lingers (HS2-3HC2A7, HS2-7F6MJ7). */
function DemoFrame({ children, label = 'Interactive' }: { children: SafeHtml; label?: string }) {
  return (
    <CatalogExample label={label} viewport={{ width: 'compact' }}>
      {children}
      <DemoEvent>{sidebarEvent.value}</DemoEvent>
    </CatalogExample>
  );
}
const completionTrend = [3, 0, 2, 5, 4, 7, 6];
export const demoAiTools: AiToolDescriptor[] = [
  {
    id: 'codex',
    display_name: 'Codex',
    models: [{ id: 'gpt-5.6', label: 'GPT-5.6', effort_levels: ['medium', 'high', 'xhigh'] }],
    default_model: 'gpt-5.6',
    default_effort: 'high',
  },
  { id: 'claude', display_name: 'Claude', models: [{ id: 'sonnet', label: 'Sonnet' }], default_model: 'sonnet' },
];
export function ProjectSummaryDemo() {
  return (
    <>
      <DemoFrame label="Default">
        <ProjectSummary completedToday={6} inProgress={3} trend={completionTrend} />
      </DemoFrame>
      <CatalogExample label="Success tone" viewport={{ width: 'compact' }}>
        <ProjectSummary
          completedToday={12}
          inProgress={5}
          trend={completionTrend.map((value) => value * 2)}
          chartTone="success"
        />
      </CatalogExample>
      <CatalogExample label="Compact with background trend" viewport={{ width: 'compact' }}>
        <ProjectSummary
          completedToday={2}
          inProgress={4}
          trend={completionTrend}
          backgroundTrend={completionTrend.map((value) => value * 2)}
          size="compact"
        />
      </CatalogExample>
    </>
  );
}
export function RepositorySummaryDemo() {
  return (
    <DemoFrame>
      <RepositorySummary branch="feature/client-sidebar" unpushed={6} uncommitted={2} />
    </DemoFrame>
  );
}
export function ViewNavigationDemo() {
  return (
    <DemoFrame>
      <ViewNavigation items={sidebarViews} selectedId={selectedViewId.value} />
    </DemoFrame>
  );
}
export function CommandNavigationDemo() {
  return (
    <DemoFrame>
      <CommandNavigation
        label="Project commands"
        expanded={commandGroupExpanded.value}
        collapsedGroups={collapsedCommandGroups.value}
        commands={sidebarCommands.map((command) => ({ ...command, running: command.id === runningCommandId.value }))}
      />
    </DemoFrame>
  );
}
export const commandEditorCommands = signal<CommandDefinition[]>([
  {
    id: 'verify',
    title: 'Verify project',
    kind: 'shell',
    command: 'npm test',
    group: 'Quality',
    color: '#22c55e',
    icon: 'circle-check-big',
  },
  {
    id: 'build',
    title: 'Build clients',
    kind: 'program',
    program: 'npm',
    args: ['run', 'build'],
    group: 'Quality',
    color: '#f97316',
    icon: 'build',
  },
  {
    id: 'publish',
    title: 'Publish preview',
    kind: 'ai',
    prompt: 'Publish a preview',
    color: '#8b5cf6',
    icon: 'send',
    group: 'Release',
  },
]);
export const commandEditorEditingId = signal<string | undefined>(undefined);
export const commandEditorSelection = signal<string[]>([]);
let commandEditorSelectionAnchor: string | undefined;
export const commandEditorExtraGroups = signal<string[]>([]);
export const commandEditorMessage = signal('');
export const commandEditorIconSearch = signal('');
export function openCommandEditorDemo(id: string) {
  commandEditorEditingId.value = id;
  commandEditorIconSearch.value = '';
  void loadLucideCatalog();
  document.querySelector<HTMLElement>(`#${COMMAND_EDITOR_DIALOG_ID}`)?.showPopover();
}
export function closeCommandEditorDemo() {
  document.querySelector<HTMLElement>(`#${COMMAND_EDITOR_DIALOG_ID}`)?.hidePopover();
  commandEditorEditingId.value = undefined;
  commandEditorIconSearch.value = '';
}
export function updateCommandEditorField(id: string, field: string, value: string) {
  commandEditorCommands.value = commandEditorCommands.value.map((command) => {
    if (command.id !== id) return command;
    const updated: CommandDefinition = { ...command };
    if (field === 'args') updated.args = value.split('\n').filter((argument) => argument.length > 0);
    else if (field === 'id' || field === 'title') updated[field] = value;
    else if (
      ['program', 'group', 'cwd', 'confirmation', 'command', 'prompt', 'tool', 'color', 'icon', 'kind'].includes(field)
    )
      (updated as unknown as Record<string, unknown>)[field] = value || undefined;
    return updated;
  });
  if (field === 'id' && commandEditorEditingId.value === id) commandEditorEditingId.value = value;
}
export function reorderCommandEditorSetting(sourceId: string, target: CommandDropTarget) {
  reorderCommandEditorSettings([sourceId], target);
}
export function reorderCommandEditorSettings(sourceIds: readonly string[], target: CommandDropTarget) {
  const before = commandEditorCommands.value,
    next = reorderCommandsMultiple(before, sourceIds, target);
  commandEditorCommands.value = next;
  commandEditorExtraGroups.value = keptCommandGroups(before, next, commandEditorExtraGroups.value);
}
export function selectCommandEditorRow(id: string, intent: { toggle?: boolean; range?: boolean }) {
  const ordered = commandEditorCommands.value.map((command) => command.id),
    current = commandEditorSelection.value;
  if (intent.range && commandEditorSelectionAnchor) {
    const from = ordered.indexOf(commandEditorSelectionAnchor),
      to = ordered.indexOf(id);
    if (from >= 0 && to >= 0) {
      const [lo, hi] = from < to ? [from, to] : [to, from];
      commandEditorSelection.value = ordered.slice(lo, hi + 1);
      return;
    }
  }
  if (intent.toggle) {
    commandEditorSelection.value = current.includes(id) ? current.filter((item) => item !== id) : [...current, id];
    commandEditorSelectionAnchor = id;
    return;
  }
  commandEditorSelection.value = [id];
  commandEditorSelectionAnchor = id;
}
export function addCommandEditorGroup() {
  const name = window.prompt('New group name')?.trim();
  if (name && !commandEditorExtraGroups.value.includes(name))
    commandEditorExtraGroups.value = [...commandEditorExtraGroups.value, name];
}
export function deleteCommandEditorGroup(group: string) {
  commandEditorExtraGroups.value = commandEditorExtraGroups.value.filter((item) => item !== group);
}
export function deleteCommandEditorSetting(id: string) {
  const before = commandEditorCommands.value;
  commandEditorCommands.value = before.filter((command) => command.id !== id);
  commandEditorExtraGroups.value = keptCommandGroups(
    before,
    commandEditorCommands.value,
    commandEditorExtraGroups.value,
  );
  if (commandEditorEditingId.value === id) closeCommandEditorDemo();
}
export function addCommandEditorSetting() {
  const existing = new Set(commandEditorCommands.value.map((command) => command.id));
  let index = 1,
    id = 'command-1';
  while (existing.has(id)) id = `command-${++index}`;
  commandEditorCommands.value = [
    ...commandEditorCommands.value,
    { id, title: 'New command', kind: 'shell', command: '' },
  ];
  openCommandEditorDemo(id);
}
export function CommandSettingsEditorDemo() {
  return (
    <section class="command-settings-editor-demo" aria-label="CommandSettingsEditor demo">
      <CommandSettingsEditor
        commands={commandEditorCommands.value}
        extraGroups={commandEditorExtraGroups.value}
        editingId={commandEditorEditingId.value}
        selectedIds={commandEditorSelection.value}
        iconSearch={commandEditorIconSearch.value}
        message={commandEditorMessage.value}
      />
    </section>
  );
}
export function DriveControlDemo() {
  return (
    <DemoFrame>
      <DriveControl running={driveRunning.value} tool="Codex" />
    </DemoFrame>
  );
}
export function DriveOptionsMenuDemo() {
  return (
    <DemoFrame>
      {/* The stage anchors the context-mode menu below a 14rem spacer; the demo wiring opens it. */}
      <div style="position:relative;margin-top:14rem;height:1px">
        <DriveOptionsMenu
          tools={demoAiTools}
          selection={{ tool: 'codex', model: 'gpt-5.6', effort: 'high' }}
          defaultSelection={{ tool: 'codex', model: 'gpt-5.6', effort: 'high' }}
          x={320}
          y={360}
        />
      </div>
    </DemoFrame>
  );
}
export function AiToolSettingsDemo() {
  return (
    <AiToolSettings
      tools={demoAiTools}
      selection={{ tool: 'codex', model: 'gpt-5.6', effort: 'high' }}
      message="Saved locally."
    />
  );
}
export function ProjectSidebarDemo() {
  return (
    <section class="project-sidebar-demo">
      <div
        class="project-sidebar-demo__resizer"
        style={`--project-sidebar-demo-height:${projectSidebarHeight.value}px`}
      >
        <div class="sidebar-demo-card">
          <ProjectSidebar
            completedToday={6}
            inProgress={3}
            completionTrend={completionTrend}
            branch="feature/client-sidebar"
            unpushed={6}
            uncommitted={2}
            views={sidebarViews}
            selectedViewId={selectedViewId.value}
            commandGroupLabel="Project commands"
            commands={sidebarCommands.map((command) => ({
              ...command,
              running: command.id === runningCommandId.value,
            }))}
            commandGroupExpanded={commandGroupExpanded.value}
            collapsedCommandGroups={collapsedCommandGroups.value}
            driveRunning={driveRunning.value}
            driveTool="codex"
            driveTools={demoAiTools}
            driveSelection={{ tool: 'codex' }}
            driveDefaultSelection={{ tool: 'codex', model: 'gpt-5.6', effort: 'high' }}
            openCount={17}
            upNextCount={4}
            activeCount={2}
            conversationOpen={projectSidebarSettings.conversationOpen.value}
            conversationDisabled={projectSidebarSettings.conversationDisabled.value}
          />
        </div>
        <div
          class="project-sidebar-demo__resize-handle"
          data-action="resize-project-sidebar"
          role="separator"
          aria-label="Resize project sidebar"
          aria-orientation="horizontal"
          aria-valuemin={PROJECT_SIDEBAR_MIN_HEIGHT}
          aria-valuemax={PROJECT_SIDEBAR_MAX_HEIGHT}
          aria-valuenow={projectSidebarHeight.value}
          tabindex="0"
        >
          <LucideIcon size={18.4} icon={GripHorizontal} name="grip-horizontal" />
        </div>
      </div>
      <DemoEvent>{sidebarEvent.value}</DemoEvent>
    </section>
  );
}
export function SettingsNavigationDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'compact' }}>
      <SettingsNavigation selected="ai" collapseControl />
    </CatalogExample>
  );
}
export function NotificationNavigationDemo() {
  return (
    <>
      <CatalogExample label="Active" viewport={{ width: 'compact' }}>
        <NotificationNavigation selected="pending" counts={{ pending: 3, day: 5, week: 12 }} collapseControl />
      </CatalogExample>
      {/* Paused: the footer offers Resume instead of Pause (HS2-QYA9SC). */}
      <CatalogExample label="Paused" viewport={{ width: 'compact' }} rootAttributes={{ 'data-demo-variant': 'paused' }}>
        <NotificationNavigation selected="pending" counts={{ pending: 3, day: 5, week: 12 }} paused collapseControl />
      </CatalogExample>
    </>
  );
}
export function TerminalOperationsSidebarDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'compact' }}>
      <TerminalOperationsSidebar
        projects={[
          { id: 'hotsheet2', name: 'HotSheet2', completedToday: 1, inProgress: 6, trend: [1, 1, 3, 2, 5, 4, 1] },
          {
            id: 'best-in-manila',
            name: 'Best-in-Manila',
            completedToday: 0,
            inProgress: 0,
            trend: [0, 0, 0, 0, 0, 0, 0],
          },
          { id: 'kerf', name: 'Kerf', completedToday: 1, inProgress: 1, trend: [1, 0, 2, 0, 0, 5, 1] },
        ]}
      />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireProjectSidebarDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  let sidebarResizeDrag: { startY: number; startHeight: number } | undefined;
  // The production CommandSettingsEditor handlers over the fixture command list (HS2-RWY78R).
  demoListeners.add(
    wireCommandSettingsEditor(root, {
      ownsCommand: (id) => commandEditorCommands.value.some((command) => command.id === id),
      openEditor: openCommandEditorDemo,
      closeEditor: closeCommandEditorDemo,
      onEditorClosed: () => {
        commandEditorEditingId.value = undefined;
        commandEditorIconSearch.value = '';
      },
      addCommand: addCommandEditorSetting,
      deleteCommand: deleteCommandEditorSetting,
      addGroup: addCommandEditorGroup,
      deleteGroup: deleteCommandEditorGroup,
      selection: () => commandEditorSelection.value,
      orderedIds: () => commandEditorCommands.value.map((command) => command.id),
      selectRow: selectCommandEditorRow,
      selectOnly: (id) => {
        selectCommandEditorRow(id, {});
      },
      reorder: reorderCommandEditorSettings,
      updateField: updateCommandEditorField,
      setIconSearch: (query) => {
        commandEditorIconSearch.value = query;
      },
      chooseAi: (id, choice) => {
        commandEditorMessage.value =
          choice.kind === 'default' ? `${id} uses the default AI.` : `${id} AI ${choice.kind} set to ${choice.value}.`;
      },
      openManualModel: (id) => {
        commandEditorMessage.value = `Manual model entry requested for ${id}.`;
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openRepositoryStatus.selector, () => {
      sidebarEvent.value = 'Repository status requested.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.addView.selector, () => {
      sidebarEvent.value = 'New view editor requested.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectView.selector, (_event, target) => {
      const id = (target as HTMLElement).dataset.itemId!;
      selectedViewId.value = id;
      sidebarEvent.value = `${sidebarViews.find((view) => view.id === id)?.label ?? 'View'} selected.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleCommandGroup.selector, () => {
      commandGroupExpanded.value = !commandGroupExpanded.value;
      sidebarEvent.value = commandGroupExpanded.value ? 'Command group expanded.' : 'Command group collapsed.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleCommandSection.selector, (_event, target) => {
      const group = target.closest<HTMLElement>('[data-command-group]')?.dataset.commandGroup;
      if (!group) return;
      collapsedCommandGroups.value = collapsedCommandGroups.value.includes(group)
        ? collapsedCommandGroups.value.filter((item) => item !== group)
        : [...collapsedCommandGroups.value, group];
      sidebarEvent.value = collapsedCommandGroups.value.includes(group) ? `${group} collapsed.` : `${group} expanded.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.runCommand.selector, (_event, target) => {
      const id = (target as HTMLElement).dataset.itemId!;
      runningCommandId.value = runningCommandId.value === id ? undefined : id;
      sidebarEvent.value = runningCommandId.value
        ? `${sidebarCommands.find((command) => command.id === id)?.label ?? 'Command'} started.`
        : 'Command stopped.';
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="project-sidebar"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'conversation-open')
        projectSidebarSettings.conversationOpen.value = control.checked;
      else if (control.getAttribute('name') === 'conversation-disabled')
        projectSidebarSettings.conversationDisabled.value = control.checked;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', `.project-sidebar ${COMMANDS_AND_AI_ACTIONS.openConversation.selector}`, () => {
      projectSidebarSettings.conversationOpen.value = !projectSidebarSettings.conversationOpen.value;
      syncProjectSidebarSettingsControls(root);
      sidebarEvent.value = projectSidebarSettings.conversationOpen.value
        ? 'Conversation opened.'
        : 'Conversation closed.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleDrive.selector, () => {
      driveRunning.value = !driveRunning.value;
      sidebarEvent.value = driveRunning.value ? 'Codex drive started.' : 'Codex drive stopped.';
    }),
  );
  demoListeners.add(
    delegate(root, 'pointerdown', DEMO_ACTIONS.resizeProjectSidebar.selector, (event) => {
      event.preventDefault();
      sidebarResizeDrag = {
        startY: (event as PointerEvent).clientY,
        startHeight: projectSidebarHeight.value,
      };
      document.body.dataset.resizingProjectSidebar = 'true';
    }),
  );
  demoListeners.add(
    delegate(root, 'keydown', DEMO_ACTIONS.resizeProjectSidebar.selector, (event) => {
      if ((event as KeyboardEvent).key !== 'ArrowUp' && (event as KeyboardEvent).key !== 'ArrowDown') return;
      event.preventDefault();
      projectSidebarHeight.value = clampProjectSidebarHeight(
        projectSidebarHeight.value + ((event as KeyboardEvent).key === 'ArrowDown' ? 24 : -24),
      );
    }),
  );
  demoListeners.add(
    listen(window, 'pointermove', (event) => {
      if (!sidebarResizeDrag) return;
      projectSidebarHeight.value = clampProjectSidebarHeight(
        sidebarResizeDrag.startHeight + event.clientY - sidebarResizeDrag.startY,
      );
    }),
  );
  demoListeners.add(
    listen(window, 'pointerup', () => {
      if (!sidebarResizeDrag) return;
      sidebarResizeDrag = undefined;
      delete document.body.dataset.resizingProjectSidebar;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'project-summary': { catalogExamples: true, Demo: ProjectSummaryDemo },
    'project-sidebar': {
      Demo: ProjectSidebarDemo,
      Settings: ProjectSidebarSettings,
      reset: resetProjectSidebarDemo,
    },
    'repository-summary': { catalogExamples: true, Demo: RepositorySummaryDemo },
    'settings-navigation': { catalogExamples: true, Demo: SettingsNavigationDemo },
    'notification-navigation': { catalogExamples: true, Demo: NotificationNavigationDemo },
    'view-navigation': { catalogExamples: true, Demo: ViewNavigationDemo },
    'command-navigation': { catalogExamples: true, Demo: CommandNavigationDemo },
    'command-settings-editor': { Demo: CommandSettingsEditorDemo },
    'drive-control': { catalogExamples: true, Demo: DriveControlDemo },
    'drive-options-menu': { catalogExamples: true, Demo: DriveOptionsMenuDemo },
    'ai-tool-settings': { Demo: AiToolSettingsDemo },
    'terminal-operations-sidebar': { catalogExamples: true, Demo: TerminalOperationsSidebarDemo },
  },
  wire: wireProjectSidebarDemo,
};
