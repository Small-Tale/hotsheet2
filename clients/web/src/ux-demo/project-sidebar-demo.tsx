import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { signal } from 'kerfjs';
import { GripHorizontal } from 'lucide';

import type { CommandDefinition } from '../api';
import { AiToolSettings } from '../components/ai-tool-settings';
import { CommandNavigation, type CommandNavigationItem } from '../components/command-navigation';
import { COMMAND_EDITOR_DIALOG_ID, CommandSettingsEditor } from '../components/command-settings-editor';
import { DriveControl } from '../components/drive-control';
import { type AiToolDescriptor,DriveOptionsMenu } from '../components/drive-options-menu';
import { NotificationNavigation } from '../components/notification-navigation';
import { ProjectSidebar } from '../components/project-sidebar';
import { ProjectSummary } from '../components/project-summary';
import { RepositorySummary } from '../components/repository-summary';
import { SettingsNavigation } from '../components/settings-navigation';
import { TerminalOperationsSidebar } from '../components/terminal-operations-sidebar';
import { ViewNavigation, type ViewNavigationItem } from '../components/view-navigation';

export const selectedViewId = signal('all');
export const commandGroupExpanded = signal(true);
export const collapsedCommandGroups=signal<string[]>([]);
export const runningCommandId = signal<string | undefined>(undefined);
export const driveRunning = signal(false);
export const sidebarEvent = signal('Choose a view or run an action.');
export const projectSidebarHeight = signal(640);
export const PROJECT_SIDEBAR_MIN_HEIGHT = 288;
export const PROJECT_SIDEBAR_MAX_HEIGHT = 768;
export const clampProjectSidebarHeight = (height: number) => Math.min(PROJECT_SIDEBAR_MAX_HEIGHT, Math.max(PROJECT_SIDEBAR_MIN_HEIGHT, Math.round(height)));

export const sidebarViews: ViewNavigationItem[] = [
  { id: 'needs-review', label: 'Needs Review', count: 3, attention: true, icon: 'needs-review' },
  { id: 'all', label: 'Queue', count: 12, icon: 'all' },
  { id: 'backlog', label: 'Backlog', count: 5, icon: 'backlog' },
  { id: 'archive', label: 'Archive', count: 241, icon: 'archive' },
  { id: 'trash', label: 'Trash', count: 4, icon: 'trash' },
];
export const sidebarCommands: CommandNavigationItem[] = [
  { id: 'verify', label: 'Verify project', color: '#14b8a6', icon: 'test',group:'Quality' },
  { id: 'build', label: 'Build clients', color: '#f97316', icon: 'build',kind:'shell',group:'Quality' },
  { id: 'publish', label: 'Publish preview', color: '#8b5cf6', icon: 'send',kind:'ai',group:'Release' },
];

function DemoFrame({ children }: { children: unknown }) {
  return <section class="sidebar-component-demo"><div class="sidebar-component-demo__rail">{children}</div><p class="component-stage__event" aria-live="polite">{sidebarEvent.value}</p></section>;
}
const completionTrend = [3, 0, 2, 5, 4, 7, 6];
export const demoAiTools:AiToolDescriptor[]=[
  {id:'codex',display_name:'Codex',models:[{id:'gpt-5.6',label:'GPT-5.6',effort_levels:['medium','high','xhigh']}],default_model:'gpt-5.6',default_effort:'high'},
  {id:'claude',display_name:'Claude',models:[{id:'sonnet',label:'Sonnet'}],default_model:'sonnet'},
];
export function ProjectSummaryDemo() { return <DemoFrame><div class="project-summary-demo__variants"><ProjectSummary completedToday={6} inProgress={3} trend={completionTrend} /><ProjectSummary completedToday={12} inProgress={5} trend={completionTrend.map(value=>value*2)} chartTone="success" /></div></DemoFrame>; }
export function RepositorySummaryDemo() { return <DemoFrame><RepositorySummary branch="feature/client-sidebar" unpushed={6} uncommitted={2} /></DemoFrame>; }
export function ViewNavigationDemo() { return <DemoFrame><ViewNavigation items={sidebarViews} selectedId={selectedViewId.value} /></DemoFrame>; }
export function CommandNavigationDemo() { return <DemoFrame><CommandNavigation label="Project commands" expanded={commandGroupExpanded.value} collapsedGroups={collapsedCommandGroups.value} commands={sidebarCommands.map(command => ({ ...command, running: command.id === runningCommandId.value }))} /></DemoFrame>; }
export const commandEditorCommands = signal<CommandDefinition[]>([
  { id: 'verify', title: 'Verify project', kind: 'shell', command: 'npm test', group: 'Quality', color: '#22c55e', icon: 'circle-check-big' },
  { id: 'build', title: 'Build clients', kind: 'program', program: 'npm', args: ['run', 'build'], group: 'Quality', color: '#f97316', icon: 'build' },
  { id: 'publish', title: 'Publish preview', kind: 'ai', prompt: 'Publish a preview', color: '#8b5cf6', icon: 'send', group: 'Release' },
]);
export const commandEditorEditingId = signal<string | undefined>(undefined);
export const commandEditorMessage = signal('');
export function openCommandEditorDemo(id: string) {
  commandEditorEditingId.value = id;
  document.querySelector<HTMLElement>(`#${COMMAND_EDITOR_DIALOG_ID}`)?.showPopover();
}
export function closeCommandEditorDemo() {
  document.querySelector<HTMLElement>(`#${COMMAND_EDITOR_DIALOG_ID}`)?.hidePopover();
  commandEditorEditingId.value = undefined;
}
export function updateCommandEditorField(id: string, field: string, value: string) {
  commandEditorCommands.value = commandEditorCommands.value.map(command => {
    if (command.id !== id) return command;
    const updated: CommandDefinition = { ...command };
    if (field === 'args') updated.args = value.split('\n').filter(argument => argument.length > 0);
    else if (field === 'id' || field === 'title') updated[field] = value;
    else if (['program', 'group', 'cwd', 'confirmation', 'command', 'prompt', 'tool', 'color', 'icon', 'kind'].includes(field)) (updated as unknown as Record<string, unknown>)[field] = value || undefined;
    return updated;
  });
  if (field === 'id' && commandEditorEditingId.value === id) commandEditorEditingId.value = value;
}
export function moveCommandEditorSetting(id: string, direction: 'up' | 'down') {
  const commands = [...commandEditorCommands.value], index = commands.findIndex(command => command.id === id), target = index + (direction === 'up' ? -1 : 1);
  if (index < 0 || target < 0 || target >= commands.length) return;
  [commands[index], commands[target]] = [commands[target], commands[index]];
  commandEditorCommands.value = commands;
}
export function deleteCommandEditorSetting(id: string) {
  commandEditorCommands.value = commandEditorCommands.value.filter(command => command.id !== id);
  if (commandEditorEditingId.value === id) closeCommandEditorDemo();
}
export function addCommandEditorSetting() {
  const existing = new Set(commandEditorCommands.value.map(command => command.id));
  let index = 1, id = 'command-1';
  while (existing.has(id)) id = `command-${++index}`;
  commandEditorCommands.value = [...commandEditorCommands.value, { id, title: 'New command', kind: 'shell', command: '' }];
  openCommandEditorDemo(id);
}
export function CommandSettingsEditorDemo() { return <section class="command-settings-editor-demo" aria-label="CommandSettingsEditor demo"><CommandSettingsEditor commands={commandEditorCommands.value} editingId={commandEditorEditingId.value} message={commandEditorMessage.value} /></section>; }
export function DriveControlDemo() { return <DemoFrame><DriveControl running={driveRunning.value} tool="Codex" /></DemoFrame>; }
export function DriveOptionsMenuDemo(){return <DemoFrame><div style="position:relative;margin-top:14rem"><DriveOptionsMenu tools={demoAiTools} selection={{tool:'codex',model:'gpt-5.6',effort:'high'}} defaultSelection={{tool:'codex',model:'gpt-5.6',effort:'high'}}/></div></DemoFrame>}
export function AiToolSettingsDemo(){return <AiToolSettings tools={demoAiTools} selection={{tool:'codex',model:'gpt-5.6',effort:'high'}} message="Saved locally."/>}
export function ProjectSidebarDemo() {
  return <section class="project-sidebar-demo"><div class="project-sidebar-demo__resizer" style={`--project-sidebar-demo-height:${projectSidebarHeight.value}px`}><ProjectSidebar completedToday={6} inProgress={3} completionTrend={completionTrend} branch="feature/client-sidebar" unpushed={6} uncommitted={2} views={sidebarViews} selectedViewId={selectedViewId.value} commandGroupLabel="Project commands" commands={sidebarCommands.map(command => ({ ...command, running: command.id === runningCommandId.value }))} commandGroupExpanded={commandGroupExpanded.value} collapsedCommandGroups={collapsedCommandGroups.value} driveRunning={driveRunning.value} driveTool="codex" driveTools={demoAiTools} driveSelection={{tool:'codex'}} driveDefaultSelection={{tool:'codex',model:'gpt-5.6',effort:'high'}} openCount={17} upNextCount={4} activeCount={2} /><div class="project-sidebar-demo__resize-handle" data-action="resize-project-sidebar" role="separator" aria-label="Resize project sidebar" aria-orientation="horizontal" aria-valuemin={PROJECT_SIDEBAR_MIN_HEIGHT} aria-valuemax={PROJECT_SIDEBAR_MAX_HEIGHT} aria-valuenow={projectSidebarHeight.value} tabindex="0"><LucideIcon icon={GripHorizontal} name="grip-horizontal" /></div></div><p class="component-stage__event" aria-live="polite">{sidebarEvent.value}</p></section>;
}
export function SettingsNavigationDemo() {
  return <section class="sidebar-component-demo"><div class="sidebar-component-demo__rail"><SettingsNavigation selected="ai" collapseControl /></div></section>;
}
export function NotificationNavigationDemo() {
  return <section class="sidebar-component-demo"><div class="sidebar-component-demo__rail"><NotificationNavigation selected="pending" counts={{ pending: 3, day: 5, week: 12 }} collapseControl /></div></section>;
}
export function TerminalOperationsSidebarDemo() {
  return <section class="sidebar-component-demo"><div class="sidebar-component-demo__rail"><TerminalOperationsSidebar projects={[
    { id: 'hotsheet2', name: 'HotSheet2', completedToday: 1, inProgress: 6, trend: [1, 1, 3, 2, 5, 4, 1] },
    { id: 'best-in-manila', name: 'Best-in-Manila', completedToday: 0, inProgress: 0, trend: [0, 0, 0, 0, 0, 0, 0] },
    { id: 'kerf', name: 'Kerf', completedToday: 1, inProgress: 1, trend: [1, 0, 2, 0, 0, 5, 1] },
  ]}/></div></section>;
}
