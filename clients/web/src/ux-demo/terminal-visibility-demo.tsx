import { CatalogExample } from '@kerfjs/ui/catalog';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { Eye } from 'lucide';

import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import {
  TerminalVisibilityDialog,
  TerminalVisibilityNameDialog,
  type TerminalVisibilityNamePrompt,
} from '../components/terminal-visibility-dialog';
import {
  activeTerminalVisibilityGroup,
  addTerminalVisibilityGroup,
  initialTerminalVisibilityState,
  removeTerminalVisibilityGroup,
  renameTerminalVisibilityGroup,
  selectTerminalVisibilityGroup,
  setAllTerminalsVisibleInGroup,
  setTerminalVisibleInGroup,
  TERMINAL_VISIBILITY_TYPES,
  terminalVisibilityItems,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import { wireTerminalVisibilityTypeFilter } from '../terminal-visibility-filter';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';

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
export const terminalVisibilityDemoTypes = signal<readonly TerminalVisibilityType[]>(TERMINAL_VISIBILITY_TYPES);
export const terminalVisibilityDemoOpen = signal(false),
  terminalVisibilityDemoContextMenu = signal<{ id: string; x: number; y: number } | undefined>(undefined),
  terminalVisibilityDemoNamePrompt = signal<TerminalVisibilityNamePrompt | undefined>(undefined);
export function TerminalVisibilityDialogDemo() {
  return (
    <CatalogExample label="Interactive" align="inline-control">
      <wa-button appearance="accent" data-action="show-terminal-visibility-demo" aria-haspopup="dialog">
        <LucideIcon size="s" icon={Eye} name="eye" slot="start" />
        Manage Workspace Visibility
      </wa-button>
      <TerminalVisibilityDialog
        open={terminalVisibilityDemoOpen.value}
        state={terminalVisibilityDemoState.value}
        scope="dashboard"
        groups={terminalVisibilityDemoGroups}
        types={terminalVisibilityDemoTypes.value}
        contextMenu={terminalVisibilityDemoContextMenu.value}
      />
      <TerminalVisibilityNameDialog prompt={terminalVisibilityDemoNamePrompt.value} />
    </CatalogExample>
  );
}
export function showTerminalVisibilityDemo() {
  terminalVisibilityDemoTypes.value = TERMINAL_VISIBILITY_TYPES;
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
export function setAllTerminalVisibilityDemo(visible: boolean) {
  terminalVisibilityDemoState.value = setAllTerminalsVisibleInGroup(
    terminalVisibilityDemoState.value,
    active().id,
    terminalVisibilityItems(terminalVisibilityDemoGroups, 'dashboard', terminalVisibilityDemoTypes.value).flatMap(
      (group) => group.items.map((item) => item.key),
    ),
    visible,
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTerminalVisibilityDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'wa-hide', '[data-context-menu]', (event, target) => {
      if (event.target !== target || (target as HTMLElement).dataset.contextMenu !== 'terminal-visibility-group')
        return;
      window.setTimeout(closeTerminalVisibilityDemoContextMenu, 0);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.showTerminalVisibilityDemo.selector, showTerminalVisibilityDemo),
  );
  wireTerminalVisibilityTypeFilter(root, (types) => {
    terminalVisibilityDemoTypes.value = types;
  });
  demoListeners.add(
    delegate(root, 'wa-hide', '[data-terminal-visibility-dialog]', (event, target) => {
      if (event.target === target) closeTerminalVisibilityDemo();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectTerminalVisibilityTab.selector, (_event, target) => {
      selectTerminalVisibilityDemoGroup((target as HTMLElement).dataset.itemId ?? 'default');
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
