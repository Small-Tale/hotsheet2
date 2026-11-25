import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Switch } from '@kerfjs/ui/switch';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { Eye } from 'lucide';

import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import {
  TerminalVisibilityDialog,
  TerminalVisibilityNameDialog,
  type TerminalVisibilityNamePrompt,
} from '../components/terminal-visibility-dialog';
import { closedPopupMenuRoot } from '../popup-menu-state';
import {
  activeTerminalVisibilityGroup,
  addTerminalVisibilityGroup,
  effectiveTerminalVisibleTypes,
  includedTerminalProjectGroups,
  initialTerminalVisibilityState,
  projectVisibilityToggleTarget,
  removeTerminalVisibilityGroup,
  renameTerminalVisibilityGroup,
  scopeTerminalVisibilityTypes,
  selectTerminalVisibilityGroup,
  setAllTerminalsVisibleInGroup,
  setShowMinimizedInGroup,
  setTerminalVisibleInGroup,
  terminalVisibilityItems,
  type TerminalVisibilityType,
  toggleEffectiveTerminalTypeInGroup,
} from '../terminal-visibility';
import { DEMO_ACTIONS } from './demo-actions';
import { type DemoModule, type FormControl, listen } from './demo-module';

export const terminalVisibilityDemoGroups: TerminalDashboardGroup[] = [
  {
    projectId: 'demo',
    projectName: 'Demo project',
    chats: [{ id: 'ai-chat:demo', projectId: 'demo', projectName: 'Demo project', name: 'Codex chat', tool: 'Codex' }],
    sessions: [
      {
        id: 'ai',
        kind: 'ai',
        projectId: 'demo',
        projectName: 'Demo project',
        title: 'AI',
        alive: true,
        busy: true,
        scrollback: '',
      },
      {
        id: 'server',
        projectId: 'demo',
        projectName: 'Demo project',
        title: 'Development server',
        alive: true,
        busy: false,
        scrollback: '',
      },
    ],
  },
];
const seeded = addTerminalVisibilityGroup(initialTerminalVisibilityState(), 'focus', 'Focus');
export const terminalVisibilityDemoState = signal(
  setTerminalVisibleInGroup(
    selectTerminalVisibilityGroup(seeded.state, 'dashboard', 'focus'),
    'focus',
    'demo:server',
    false,
  ),
);
/** The demo's AI chat feature area; off, the chat-less project shows no AI Chats segment (HS2-4TKASN). */
export const terminalVisibilityDemoChatFeature = signal(true);
/** The demo's projects: the chat closes while the feature is off, as an empty workspace would be. */
/** A minimized project, listed only while the workspace's Show minimized is on (HS2-W1XEH7). */
export const terminalVisibilityDemoMinimizedGroup: TerminalDashboardGroup = {
  projectId: 'archive',
  projectName: 'Archived project',
  sessions: [
    {
      id: 'release',
      projectId: 'archive',
      projectName: 'Archived project',
      title: 'Release notes',
      alive: true,
      busy: false,
      scrollback: '',
    },
  ],
};
export const terminalVisibilityDemoProjects = () =>
  includedTerminalProjectGroups(
    [
      ...(terminalVisibilityDemoChatFeature.value
        ? terminalVisibilityDemoGroups
        : terminalVisibilityDemoGroups.map((group) => ({ ...group, chats: [] }))),
      terminalVisibilityDemoMinimizedGroup,
    ],
    activeTerminalVisibilityGroup(terminalVisibilityDemoState.value, 'dashboard'),
    [terminalVisibilityDemoMinimizedGroup.projectId],
  );
export const terminalVisibilityDemoOpen = signal(false),
  terminalVisibilityDemoContextMenu = signal<{ id: string; x: number; y: number } | undefined>(undefined),
  terminalVisibilityDemoNamePrompt = signal<TerminalVisibilityNamePrompt | undefined>(undefined);
export function TerminalVisibilityDialogDemo() {
  return (
    <CatalogExample label="Interactive" align="inline-control">
      <List gap="xs">
        <Button
          emphasis="primary"
          action={'show-terminal-visibility-demo'}
          aria-haspopup="dialog"
          icon={<LucideIcon size="s" icon={Eye} name="eye" />}
        >
          Manage Workspace Visibility
        </Button>
        <Switch
          action={DEMO_ACTIONS.toggleTerminalVisibilityDemoChatFeature.value}
          checked={terminalVisibilityDemoChatFeature.value}
          label="AI chat feature on"
        />
      </List>
      <TerminalVisibilityDialog
        open={terminalVisibilityDemoOpen.value}
        state={terminalVisibilityDemoState.value}
        scope="dashboard"
        groups={terminalVisibilityDemoProjects()}
        chatFeatureOn={terminalVisibilityDemoChatFeature.value}
        contextMenu={terminalVisibilityDemoContextMenu.value}
      />
      <TerminalVisibilityNameDialog prompt={terminalVisibilityDemoNamePrompt.value} />
    </CatalogExample>
  );
}
export function showTerminalVisibilityDemo() {
  terminalVisibilityDemoOpen.value = true;
}
export function closeTerminalVisibilityDemo() {
  terminalVisibilityDemoOpen.value = false;
  terminalVisibilityDemoContextMenu.value = undefined;
  terminalVisibilityDemoNamePrompt.value = undefined;
}
const active = () => activeTerminalVisibilityGroup(terminalVisibilityDemoState.value, 'dashboard');
export function selectTerminalVisibilityDemoGroup(id: string) {
  terminalVisibilityDemoState.value = selectTerminalVisibilityGroup(terminalVisibilityDemoState.value, 'dashboard', id);
}
export function promptAddTerminalVisibilityDemoGroup() {
  terminalVisibilityDemoContextMenu.value = undefined;
  terminalVisibilityDemoNamePrompt.value = { mode: 'add', value: '' };
}
export function submitTerminalVisibilityDemoName(name: string) {
  const prompt = terminalVisibilityDemoNamePrompt.value;
  if (!prompt || !name.trim()) return;
  if (prompt.mode === 'add') {
    const added = addTerminalVisibilityGroup(
      terminalVisibilityDemoState.value,
      `demo-${terminalVisibilityDemoState.value.groups.length}`,
      name,
    );
    terminalVisibilityDemoState.value = selectTerminalVisibilityGroup(added.state, 'dashboard', added.group.id);
  } else if (prompt.groupId)
    terminalVisibilityDemoState.value = renameTerminalVisibilityGroup(
      terminalVisibilityDemoState.value,
      prompt.groupId,
      name,
    );
  terminalVisibilityDemoNamePrompt.value = undefined;
}
export function showTerminalVisibilityDemoContextMenu(id: string, x: number, y: number) {
  if (id !== 'default') terminalVisibilityDemoContextMenu.value = { id, x, y };
}
export function closeTerminalVisibilityDemoContextMenu() {
  terminalVisibilityDemoContextMenu.value = undefined;
}
export function promptRenameTerminalVisibilityDemoGroup() {
  const id = terminalVisibilityDemoContextMenu.value?.id,
    group = terminalVisibilityDemoState.value.groups.find((item) => item.id === id);
  terminalVisibilityDemoContextMenu.value = undefined;
  if (group) terminalVisibilityDemoNamePrompt.value = { mode: 'rename', groupId: group.id, value: group.name };
}
export function cancelTerminalVisibilityDemoName() {
  terminalVisibilityDemoNamePrompt.value = undefined;
}
export function renameTerminalVisibilityDemoGroup(name: string) {
  terminalVisibilityDemoState.value = renameTerminalVisibilityGroup(
    terminalVisibilityDemoState.value,
    active().id,
    name,
  );
}
export function removeTerminalVisibilityDemoGroup() {
  const id = terminalVisibilityDemoContextMenu.value?.id ?? active().id;
  terminalVisibilityDemoContextMenu.value = undefined;
  terminalVisibilityDemoState.value = removeTerminalVisibilityGroup(terminalVisibilityDemoState.value, id);
}
export function toggleTerminalVisibilityDemo(key: string) {
  terminalVisibilityDemoState.value = setTerminalVisibleInGroup(
    terminalVisibilityDemoState.value,
    active().id,
    key,
    active().hiddenKeys.includes(key),
  );
}
const demoAvailableTypes = () =>
  scopeTerminalVisibilityTypes(terminalVisibilityDemoProjects(), 'dashboard', terminalVisibilityDemoChatFeature.value);
export function toggleTerminalVisibilityDemoType(type: TerminalVisibilityType) {
  terminalVisibilityDemoState.value = toggleEffectiveTerminalTypeInGroup(
    terminalVisibilityDemoState.value,
    active().id,
    type,
    demoAvailableTypes(),
  );
}
export function setTerminalVisibilityDemoShowMinimized(show: boolean) {
  terminalVisibilityDemoState.value = setShowMinimizedInGroup(terminalVisibilityDemoState.value, active().id, show);
}
export function setTerminalVisibilityDemoChatFeature(on: boolean) {
  terminalVisibilityDemoChatFeature.value = on;
}
/** A project heading's toggle (HS2-HC67SP): any hidden listed item shows all, otherwise all hide. */
export function toggleTerminalVisibilityDemoProject(projectId: string) {
  const keys = demoListedKeys().filter((key) => key.startsWith(`${projectId}:`));
  terminalVisibilityDemoState.value = setAllTerminalsVisibleInGroup(
    terminalVisibilityDemoState.value,
    active().id,
    keys,
    projectVisibilityToggleTarget(keys, active().hiddenKeys),
  );
}
const demoListedKeys = () =>
  terminalVisibilityItems(
    terminalVisibilityDemoProjects(),
    'dashboard',
    effectiveTerminalVisibleTypes(active(), demoAvailableTypes()),
  ).flatMap((group) => group.items.map((item) => item.key));
export function setAllTerminalVisibilityDemo(visible: boolean) {
  terminalVisibilityDemoState.value = setAllTerminalsVisibleInGroup(
    terminalVisibilityDemoState.value,
    active().id,
    demoListedKeys(),
    visible,
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTerminalVisibilityDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    // The Kerf menu's own close: its top-level panel's popover `toggle`, which does not bubble.
    listen(
      root,
      'toggle',
      (event) => {
        if (closedPopupMenuRoot(event)?.dataset.contextMenu !== 'terminal-visibility-group') return;
        window.setTimeout(closeTerminalVisibilityDemoContextMenu, 0);
      },
      { capture: true },
    ),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.showTerminalVisibilityDemo.selector, showTerminalVisibilityDemo),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleTerminalVisibilityType.selector, (_event, target) => {
      const type = (target as HTMLElement).dataset.visibilityType;
      if (type === 'shell' || type === 'ai' || type === 'chat') toggleTerminalVisibilityDemoType(type);
    }),
  );
  demoListeners.add(
    delegate(root, 'change', DEMO_ACTIONS.toggleTerminalVisibilityShowMinimized.selector, (_event, target) => {
      setTerminalVisibilityDemoShowMinimized((target as HTMLInputElement).checked);
    }),
  );
  demoListeners.add(
    delegate(root, 'change', DEMO_ACTIONS.toggleTerminalVisibilityDemoChatFeature.selector, (_event, target) => {
      setTerminalVisibilityDemoChatFeature((target as HTMLInputElement).checked);
    }),
  );
  demoListeners.add(
    delegate(root, 'kui-request-close', '[data-terminal-visibility-dialog]', (event, target) => {
      if (event.target === target) closeTerminalVisibilityDemo();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectTerminalVisibilityTab.selector, (_event, target) => {
      selectTerminalVisibilityDemoGroup((target as HTMLElement).dataset.tabId ?? 'default');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.addTerminalVisibilityGroup.selector, () => {
      promptAddTerminalVisibilityDemoGroup();
      requestAnimationFrame(() =>
        root
          .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
          ?.focus(),
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'contextmenu', '[data-visibility-group-id]', (event, target) => {
      event.preventDefault();
      showTerminalVisibilityDemoContextMenu(
        (target as HTMLElement).dataset.visibilityGroupId ?? '',
        (event as MouseEvent).clientX,
        (event as MouseEvent).clientY,
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.renameTerminalVisibilityGroup.selector, () => {
      promptRenameTerminalVisibilityDemoGroup();
      requestAnimationFrame(() =>
        root
          .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
          ?.focus(),
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'submit', DEMO_ACTIONS.submitTerminalVisibilityName.selector, (event, target) => {
      event.preventDefault();
      submitTerminalVisibilityDemoName(
        (target.querySelector('[name="terminal-visibility-group-name"]') as FormControl).value,
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.cancelTerminalVisibilityName.selector, cancelTerminalVisibilityDemoName),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.removeTerminalVisibilityGroup.selector, () => {
      removeTerminalVisibilityDemoGroup();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleTerminalVisibility.selector, (_event, target) => {
      toggleTerminalVisibilityDemo((target as HTMLElement).dataset.itemId ?? '');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleTerminalVisibilityProject.selector, (_event, target) => {
      toggleTerminalVisibilityDemoProject((target as HTMLElement).dataset.projectId ?? '');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.showAllTerminalsInGroup.selector, () => {
      setAllTerminalVisibilityDemo(true);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.hideAllTerminalsInGroup.selector, () => {
      setAllTerminalVisibilityDemo(false);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'terminal-visibility-dialog': { catalogExamples: true, Demo: TerminalVisibilityDialogDemo },
  },
  wire: wireTerminalVisibilityDemo,
};
