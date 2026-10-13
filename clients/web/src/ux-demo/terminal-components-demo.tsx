import { CatalogExample } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  TerminalCopyDialog,
  type TerminalCopyState,
  TerminalEditMenu,
  type TerminalEditMenuState,
  TerminalPasteDialog,
  type TerminalPasteState,
} from '../components/terminal-clipboard-dialogs';
import { FixedAspectTerminalCard, TerminalDashboard } from '../components/terminal-dashboard';
import { TerminalDrawer } from '../components/terminal-drawer';
import { TerminalKeyBar } from '../components/terminal-key-bar';
import { TerminalRenameDialog, type TerminalRenameTarget } from '../components/terminal-rename-dialog';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import { nextMobileTerminalColumns } from '../mobile-terminal-columns';
import { terminalCopyMessage, terminalCopySelection } from '../terminal-clipboard';
import {
  consumeTerminalModifiers,
  encodeTerminalKey,
  NO_TERMINAL_MODIFIERS,
  type TerminalModifier,
  type TerminalModifiers,
  type TerminalSpecialKey,
  toggleTerminalModifier,
} from '../terminal-keys';
import { DemoEvent } from './components/demo-event';
import { onDemoSelected } from './components/demo-state';
import { DEMO_ACTIONS, DEMO_COMPONENTS } from './demo-actions';
import type { DemoModule } from './demo-module';
import { listen } from './demo-module';
import { TERMINAL_DRAWER_DEMO_PROVIDERS } from './fixtures';
import { syncTerminalDemoViewports } from './terminal-demo';

export const drawerFocusDemoColumns = signal(60);
export const keyBarDemoModifiers = signal<TerminalModifiers>(NO_TERMINAL_MODIFIERS),
  keyBarDemoFunctionRow = signal(false),
  keyBarDemoOutput = signal('');
// HS2-FRB545 clipboard sheets: deterministic snapshot text; actions report what production would do.
export const CLIPBOARD_DEMO_TEXT = Array.from(
    { length: 40 },
    (_, index) => `line ${index + 1}: build step ${index + 1} ok`,
  )
    .concat('$ npm test', 'All 1504 tests passed.', '$ ')
    .join('\n'),
  clipboardDemoCopy = signal<TerminalCopyState | undefined>(undefined),
  clipboardDemoPaste = signal<TerminalPasteState | undefined>(undefined),
  clipboardDemoOutput = signal(''),
  clipboardDemoGeneration = signal(0);
// HS2-2Q7KTX: the rename dialog opens renamed (offering Reset to default) or default-named.
export const renameDemoTargets = {
    renamed: { projectId: 'demo', terminalId: 'shell', value: 'Development', defaultName: 'Terminal 1' },
    default: { projectId: 'demo', terminalId: 'shell', value: 'Terminal 1' },
  } as const,
  renameDemoSession = signal(1),
  renameDemoTarget = signal<TerminalRenameTarget | undefined>({ ...renameDemoTargets.renamed, session: 1 }),
  renameDemoOutput = signal('');
// HS2-KKP8YJ long-press edit menu: the stage opens it at a point; items report what production would do.
export const editMenuDemo = signal<TerminalEditMenuState | undefined>(undefined),
  editMenuDemoOutput = signal('');
export const terminalDashboardContextMenu = signal<{ key: string; x: number; y: number } | undefined>(undefined);
export const terminalDashboardHaltCleared = signal(false);

export function TerminalDrawerDemo() {
  const shell = {
    id: 'shell',
    projectId: 'demo',
    projectName: 'Demo project',
    title: 'Development',
    alive: true,
    busy: true,
    scrollback: 'npm run dev\nready on http://127.0.0.1',
  };
  // Phone focus mode is a fixed, viewport-sized surface; each stage contains it (and its blackout
  // backdrop) so the variant can be inspected in place (HS2-01D4JP).
  const focusVariant = (label: string, keyboardVisible: boolean, height: number) => (
    <CatalogExample
      label={label}
      viewport={{ width: 'compact', frame: 'solid', overflow: 'hidden' }}
      rootAttributes={{ 'data-demo-scope': 'terminal-drawer-focus-demo' }}
    >
      <div class="terminal-drawer-focus-demo__stage" style={`height:${height}px`}>
        <TerminalDrawer
          projectId="demo"
          projectName="Demo project"
          sessions={[shell]}
          width={390}
          height={height}
          fitAcross={2}
          fitHigh={2}
          selectedId="shell"
          focusMode
          focusViewport={{ left: 0, top: 0, width: 390, height }}
          focusTextSize={{
            viewport: { left: 0, top: 0, width: 390, height },
            keyboardVisible,
            columns: drawerFocusDemoColumns.value,
          }}
        />
      </div>
    </CatalogExample>
  );
  const drawerFrame = { width: 'full', height: 'short', frame: 'solid', overflow: 'hidden' } as const;
  return (
    <>
      <CatalogExample label="Interactive" viewport={{ ...drawerFrame, height: 'medium' }}>
        <TerminalDrawer
          projectId="demo"
          projectName="Demo project"
          sessions={[shell]}
          width={900}
          height={320}
          fitAcross={2}
          fitHigh={2}
          selectedId="shell"
          contextMenu={terminalDashboardContextMenu.value}
          aiProviders={TERMINAL_DRAWER_DEMO_PROVIDERS}
          defaultAiProvider="claude"
        />
      </CatalogExample>
      {/* With one installed AI provider the AI shell entry is named for it instead of a submenu (HS2-3HT4PA). */}
      <CatalogExample label="One AI provider" viewport={drawerFrame}>
        <TerminalDrawer
          projectId="demo-single"
          projectName="Demo project"
          sessions={[]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="grid"
          aiProviders={TERMINAL_DRAWER_DEMO_PROVIDERS.slice(1)}
          defaultAiProvider="claude"
        />
      </CatalogExample>
      {/* Tab status: a halted AI session (HS2-HJ4D1H), and whether an AI session reaches Hot Sheet (HS2-EV1XK3). */}
      <CatalogExample label="Terminal tab states" viewport={drawerFrame}>
        <TerminalDrawer
          projectId="demo-states"
          projectName="Demo project"
          sessions={[
            {
              ...shell,
              id: 'connected',
              projectId: 'demo-states',
              title: 'Connected',
              tool: 'codex',
              ai_connection: { agent: 'codex', at: '2026-10-05T08:00:00Z' },
              last_hook_report: {
                agent: 'codex',
                at: '2026-10-05T08:00:00Z',
                source: 'session_start',
              },
              aiConnection: 'connected',
            },
            {
              ...shell,
              id: 'missing',
              projectId: 'demo-states',
              title: 'Not connected',
              kind: 'ai',
              tool: 'codex',
              busy: false,
              aiConnection: 'missing',
            },
            {
              ...shell,
              id: 'halted',
              projectId: 'demo-states',
              title: 'Halted',
              kind: 'ai',
              tool: 'claude',
              halt: {
                error_type: 'overloaded',
                message: 'Selected model is at capacity. Please try a different model.',
                at: '2026-10-05T08:00:00Z',
              },
            },
          ]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="connected"
        />
      </CatalogExample>
      <CatalogExample label="AI chat tab states" viewport={drawerFrame}>
        <TerminalDrawer
          projectId="demo-chat-states"
          projectName="Demo project"
          sessions={[]}
          chatTabs={[
            {
              id: 'failed-chat',
              name: 'Stopped chat',
              tool: 'Codex',
              error: 'Selected model is at capacity',
              content: <p>Selected model is at capacity</p>,
            },
            {
              id: 'recovered-chat',
              name: 'Recovered chat',
              tool: 'Claude',
              content: <p>Retry completed successfully.</p>,
            },
          ]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="failed-chat"
        />
      </CatalogExample>
      {/* A touch-first desktop (coarse primary pointer) adds rail Copy and Paste (HS2-5DHHPV). */}
      <CatalogExample label="Touch-first desktop" viewport={drawerFrame}>
        <TerminalDrawer
          projectId="demo-touch"
          projectName="Demo project"
          sessions={[{ ...shell, projectId: 'demo-touch' }]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="shell"
          touchClipboard
        />
      </CatalogExample>
      {focusVariant('Phone focus mode', false, 560)}
      {focusVariant('Phone focus mode, keyboard presented', true, 360)}
    </>
  );
}

export function TerminalDashboardDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}>
      <TerminalDashboard
        groups={[
          {
            projectId: 'demo',
            projectName: 'Demo project',
            sessions: [
              {
                id: 'shell',
                projectId: 'demo',
                projectName: 'Demo project',
                title: 'Development',
                tool: 'codex',
                ai_connection: { agent: 'codex', at: '2026-10-05T08:00:00Z' },
                last_hook_report: {
                  agent: 'codex',
                  at: '2026-10-05T08:00:00Z',
                  source: 'session_start',
                },
                aiConnection: 'connected',
                alive: true,
                busy: true,
                cwd: '/work/demo',
                progress: 68,
                scrollback: 'npm run dev\nready on http://127.0.0.1',
              },
              {
                id: 'tests',
                projectId: 'demo',
                projectName: 'Demo project',
                title: 'Tests',
                kind: 'ai',
                tool: 'codex',
                aiConnection: 'missing',
                alive: true,
                busy: false,
                cwd: '/work/demo',
                scrollback: '42 tests passed\nwaiting for changes',
              },
              {
                id: 'halted',
                projectId: 'demo',
                projectName: 'Demo project',
                title: 'Halted AI',
                tool: 'claude',
                alive: true,
                busy: false,
                aiConnection: 'connected',
                halt: terminalDashboardHaltCleared.value
                  ? undefined
                  : {
                      error_type: 'overloaded',
                      message: 'Selected model is at capacity.',
                      at: '2026-10-05T08:00:00Z',
                    },
                scrollback: 'Selected model is at capacity.',
              },
            ],
            chats: [
              {
                id: 'chat:review',
                projectId: 'demo',
                projectName: 'Demo project',
                name: 'Review chat',
                tool: 'Codex',
                busy: true,
                summary: 'Reviewing the latest workspace changes and test results.',
              },
            ],
          },
        ]}
        width={900}
        height={560}
        fitAcross={3}
        fitHigh={3}
        contextMenu={terminalDashboardContextMenu.value}
      />
    </CatalogExample>
  );
}

export function TerminalKeyBarDemo() {
  return (
    <>
      <CatalogExample
        label="Keys"
        viewport={{ width: 'compact', overflow: 'hidden' }}
        rootAttributes={{ 'data-demo-scope': 'terminal-key-bar-demo' }}
      >
        <TerminalKeyBar modifiers={keyBarDemoModifiers.value} functionRow={keyBarDemoFunctionRow.value} />
        <DemoEvent live={false} attributes={{ 'data-key-bar-demo-output': '' }}>
          {keyBarDemoOutput.value || 'Tap a key to see the bytes it sends.'}
        </DemoEvent>
      </CatalogExample>
      <CatalogExample
        label="Function row"
        viewport={{ width: 'compact', overflow: 'hidden' }}
        rootAttributes={{ 'data-demo-scope': 'terminal-key-bar-demo' }}
      >
        <TerminalKeyBar modifiers={{ ctrl: 'once', alt: 'locked', shift: 'off' }} functionRow />
      </CatalogExample>
    </>
  );
}

export function FixedAspectTerminalCardDemo() {
  const session = {
    id: 'shell',
    projectId: 'demo',
    projectName: 'Demo project',
    title: 'Development',
    alive: true,
    busy: true,
    cwd: '/work/demo',
    progress: 68,
    scrollback: 'GNU nano 8.4\n80 columns × 24 rows\n^X Exit',
  };
  return (
    <>
      <CatalogExample label="Grid preview" viewport={{ width: 'text', surface: 'lowered' }}>
        <FixedAspectTerminalCard session={session} fit="aspect" />
      </CatalogExample>
      <CatalogExample label="Magnified interactive" viewport={{ width: 'full', surface: 'lowered' }}>
        <FixedAspectTerminalCard session={session} mode="magnified" fit="aspect" />
      </CatalogExample>
      <CatalogExample label="Magnified, touch-first desktop" viewport={{ width: 'full', surface: 'lowered' }}>
        <FixedAspectTerminalCard session={session} mode="magnified" fit="aspect" touchClipboard />
      </CatalogExample>
      <CatalogExample label="Magnified phone toolbar" viewport={{ width: 'compact', surface: 'lowered' }}>
        <FixedAspectTerminalCard
          session={session}
          mode="magnified"
          fit="aspect"
          mobile={{ viewport: { left: 0, top: 0, width: 390, height: 844 }, keyboardVisible: false, columns: 60 }}
        />
      </CatalogExample>
      <CatalogExample label="Magnified phone, keyboard presented" viewport={{ width: 'compact', surface: 'lowered' }}>
        <FixedAspectTerminalCard
          session={session}
          mode="magnified"
          fit="aspect"
          mobile={{ viewport: { left: 0, top: 0, width: 390, height: 500 }, keyboardVisible: true, columns: 60 }}
        />
      </CatalogExample>
    </>
  );
}

export function TerminalEditMenuDemo() {
  return (
    <CatalogExample label="Interactive" rootAttributes={{ 'data-demo-scope': 'terminal-edit-menu-demo' }}>
      <Row gap="xs">
        <wa-button data-edit-menu-demo-open>Long-press here</wa-button>
        <wa-button data-edit-menu-demo-open="selection">Long-press a word</wa-button>
      </Row>
      <DemoEvent live={false} attributes={{ 'data-edit-menu-demo-output': '' }}>
        {editMenuDemoOutput.value || 'Open the menu, then choose an action.'}
      </DemoEvent>
      <TerminalEditMenu state={editMenuDemo.value} />
    </CatalogExample>
  );
}

export function terminalClipboardDemo(copy: boolean) {
  return (
    <CatalogExample label="Interactive" rootAttributes={{ 'data-demo-scope': 'terminal-clipboard-demo' }}>
      <Row gap="xs">
        {copy ? (
          <wa-button data-clipboard-demo-open="copy">Open copy sheet</wa-button>
        ) : (
          <>
            <wa-button data-clipboard-demo-open="denied">Clipboard denied</wa-button>
            <wa-button data-clipboard-demo-open="unavailable">Clipboard unavailable</wa-button>
          </>
        )}
      </Row>
      <DemoEvent live={false} attributes={{ 'data-clipboard-demo-output': '' }}>
        {clipboardDemoOutput.value || 'Open the sheet to try it.'}
      </DemoEvent>
      {copy ? (
        <TerminalCopyDialog state={clipboardDemoCopy.value} />
      ) : (
        <TerminalPasteDialog state={clipboardDemoPaste.value} />
      )}
    </CatalogExample>
  );
}

export function TerminalRenameDialogDemo() {
  return (
    <CatalogExample label="Interactive" rootAttributes={{ 'data-demo-scope': 'terminal-rename-demo' }}>
      <Row gap="xs">
        <wa-button data-rename-demo-open="renamed">Rename a renamed terminal</wa-button>
        <wa-button data-rename-demo-open="default">Rename a default-named terminal</wa-button>
      </Row>
      <DemoEvent live={false} attributes={{ 'data-rename-demo-output': '' }}>
        {renameDemoOutput.value || 'Reset to default appears only while a rename applies.'}
      </DemoEvent>
      <TerminalRenameDialog target={renameDemoTarget.value} />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTerminalComponentsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // Dashboard and long-press menus close with Web Awesome's own hide, an outside press, or Escape.
  demoListeners.add(
    delegate(root, 'wa-hide', '[data-context-menu]', (event, target) => {
      if (event.target !== target) return;
      const surface = (target as HTMLElement).dataset.contextMenu;
      window.setTimeout(() => {
        if (surface === 'terminal') terminalDashboardContextMenu.value = undefined;
        else if (surface === 'terminal-edit') editMenuDemo.value = undefined;
      }, 0);
    }),
  );
  demoListeners.add(
    listen(
      window,
      'pointerdown',
      (event) => {
        if (
          terminalDashboardContextMenu.value &&
          !(event.target as Element).closest(
            '[data-component="terminal-context-menu"], [data-action="open-terminal-context-menu"]',
          )
        )
          terminalDashboardContextMenu.value = undefined;
      },
      { capture: true },
    ),
  );
  demoListeners.add(
    listen(window, 'keydown', (event) => {
      if (event.key === 'Escape') terminalDashboardContextMenu.value = undefined;
    }),
  );
  demoListeners.add(
    onDemoSelected(() => {
      terminalDashboardContextMenu.value = undefined;
    }),
  );
  const terminalDemoMounts = new Map<HTMLElement, () => void>();
  const syncDemoTerminals = () => {
    syncTerminalDemoViewports(root, terminalDemoMounts);
  };
  new MutationObserver(syncDemoTerminals).observe(root, { childList: true, subtree: true });
  queueMicrotask(syncDemoTerminals);
  function showTerminalDashboardContextMenu(target: HTMLElement, x: number, y: number): void {
    terminalDashboardContextMenu.value = {
      key: target.dataset.terminalKey ?? target.dataset.itemId ?? '',
      ...viewportSafeContextMenuPosition(x, y, innerWidth, innerHeight, { width: 224, height: 104 }),
    };
  }
  demoListeners.add(
    delegate(root, 'contextmenu', DEMO_COMPONENTS.terminalTile.selector, (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent;
      showTerminalDashboardContextMenu(target as HTMLElement, pointer.clientX, pointer.clientY);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', TERMINALS_ACTIONS.clearTerminalHalt.selector, () => {
      terminalDashboardHaltCleared.value = true;
      terminalDashboardContextMenu.value = undefined;
    }),
  );
  // The TerminalKeyBar demo exercises the same modifier and Fn transitions as production and shows the
  // exact bytes each key would send (HS2-CKS78M).
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-key-bar-demo"] [data-action="toggle-terminal-modifier"]',
      (_event, target) => {
        const modifier = (target as HTMLElement).dataset.modifier as TerminalModifier | undefined;
        if (modifier) keyBarDemoModifiers.value = toggleTerminalModifier(keyBarDemoModifiers.value, modifier);
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-key-bar-demo"] [data-action="toggle-terminal-function-row"]',
      (_event, target) => {
        keyBarDemoFunctionRow.value = !keyBarDemoFunctionRow.value;
        target.closest('[data-component="terminal-key-bar"]')?.scrollTo({ left: 0 });
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-key-bar-demo"] [data-action="send-terminal-key"]',
      (_event, target) => {
        const key = (target as HTMLElement).dataset.key as TerminalSpecialKey,
          bytes = encodeTerminalKey(key, keyBarDemoModifiers.value);
        keyBarDemoModifiers.value = consumeTerminalModifiers(keyBarDemoModifiers.value);
        keyBarDemoOutput.value = `${key} → ${JSON.stringify(bytes).slice(1, -1)}`;
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-key-bar-demo"] [data-action="copy-terminal-text"], [data-demo-scope="terminal-key-bar-demo"] [data-action="paste-terminal-text"]',
      (_event, target) => {
        keyBarDemoOutput.value =
          (target as HTMLElement).dataset.action === 'copy-terminal-text'
            ? 'Copy → opens the terminal Copy sheet'
            : 'Paste → sends the clipboard to the terminal';
      },
    ),
  );
  // The rename dialog demo reports what each production action would do (HS2-2Q7KTX).
  demoListeners.add(
    delegate(root, 'click', '[data-rename-demo-open]', (_event, target) => {
      const kind = (target as HTMLElement).dataset.renameDemoOpen === 'default' ? 'default' : 'renamed';
      renameDemoSession.value += 1;
      renameDemoTarget.value = { ...renameDemoTargets[kind], session: renameDemoSession.value };
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-demo-scope="terminal-rename-demo"] [data-action="reset-terminal-rename"]', () => {
      renameDemoOutput.value = `Reset to default → the tab shows ${renameDemoTarget.value?.defaultName ?? ''} again`;
      renameDemoTarget.value = undefined;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-demo-scope="terminal-rename-demo"] [data-action="cancel-terminal-rename"]', () => {
      renameDemoOutput.value = 'Cancel → the name is unchanged';
      renameDemoTarget.value = undefined;
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'submit',
      '[data-demo-scope="terminal-rename-demo"] [data-action="rename-terminal-form"]',
      (event, target) => {
        event.preventDefault();
        const name = target.querySelector<HTMLInputElement>('[name="terminal-name"]')?.value.trim() ?? '';
        renameDemoOutput.value = `Rename → the tab shows ${name}`;
        renameDemoTarget.value = undefined;
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'wa-hide', '[data-demo-scope="terminal-rename-demo"] [data-terminal-rename-dialog]', () => {
      renameDemoTarget.value = undefined;
    }),
  );
  // The clipboard sheet demos drive the same open/close/selection behavior as production (HS2-FRB545).
  demoListeners.add(
    delegate(root, 'click', '[data-clipboard-demo-open]', (_event, target) => {
      const kind = (target as HTMLElement).dataset.clipboardDemoOpen,
        generation = (clipboardDemoGeneration.value += 1);
      if (kind === 'copy')
        clipboardDemoCopy.value = { open: true, title: 'Build', generation, text: CLIPBOARD_DEMO_TEXT };
      else
        clipboardDemoPaste.value = {
          open: true,
          title: 'Build',
          generation,
          reason: kind === 'unavailable' ? 'unavailable' : 'denied',
        };
    }),
  );
  const closeClipboardDemo = () => {
    if (clipboardDemoCopy.value?.open) clipboardDemoCopy.value = { ...clipboardDemoCopy.value, open: false };
    if (clipboardDemoPaste.value?.open) clipboardDemoPaste.value = { ...clipboardDemoPaste.value, open: false };
  };
  demoListeners.add(
    delegate(root, 'click', '[data-demo-scope="terminal-clipboard-demo"] [data-action="confirm-terminal-copy"]', () => {
      const field = root.querySelector<HTMLTextAreaElement>(
        '[data-demo-scope="terminal-clipboard-demo"] textarea[name="terminal-copy-text"]',
      );
      if (!field) return;
      const { text, selection } = terminalCopySelection(field.value, field.selectionStart, field.selectionEnd);
      clipboardDemoOutput.value = terminalCopyMessage(text, selection);
      closeClipboardDemo();
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'submit',
      '[data-demo-scope="terminal-clipboard-demo"] [data-action="submit-terminal-paste"]',
      (event, target) => {
        event.preventDefault();
        const text = target.querySelector<HTMLTextAreaElement>('textarea[name="terminal-paste-text"]')?.value ?? '';
        clipboardDemoOutput.value = `Pasted → ${JSON.stringify(text.replace(/\r?\n/g, '\r')).slice(1, -1)}`;
        closeClipboardDemo();
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-clipboard-demo"] [data-action="close-terminal-copy"], [data-demo-scope="terminal-clipboard-demo"] [data-action="cancel-terminal-paste"]',
      closeClipboardDemo,
    ),
  );
  demoListeners.add(
    delegate(root, 'wa-hide', '[data-demo-scope="terminal-clipboard-demo"] wa-dialog', closeClipboardDemo),
  );
  // The edit menu demo opens where the stage was pressed and reports the production action (HS2-KKP8YJ).
  demoListeners.add(
    delegate(root, 'click', '[data-edit-menu-demo-open]', (_event, target) => {
      const box = target.getBoundingClientRect(),
        // A long-press on a word leaves a selection, so the menu leads with Copy (HS2-EYR96N).
        selection = (target as HTMLElement).dataset.editMenuDemoOpen === 'selection';
      editMenuDemo.value = {
        ...viewportSafeContextMenuPosition(box.left + box.width / 2, box.bottom, innerWidth, innerHeight, {
          width: 192,
          height: selection ? 144 : 96,
        }),
        selection,
      };
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-edit-menu-demo"] [data-action="copy-terminal-selection"], [data-demo-scope="terminal-edit-menu-demo"] [data-action="copy-terminal-text"], [data-demo-scope="terminal-edit-menu-demo"] [data-action="paste-terminal-text"]',
      (_event, target) => {
        const action = (target as HTMLElement).dataset.action;
        editMenuDemoOutput.value =
          action === 'copy-terminal-selection'
            ? 'Copy → copies the selected terminal text'
            : action === 'copy-terminal-text'
              ? 'Copy Text… → opens the terminal Copy sheet'
              : 'Paste → sends the clipboard to the terminal';
        editMenuDemo.value = undefined;
      },
    ),
  );
  // The drawer focus-mode text-size control cycles the demo's column fixture like production (HS2-01D4JP).
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-demo-scope="terminal-drawer-focus-demo"] [data-action="cycle-mobile-terminal-columns"]',
      () => {
        drawerFocusDemoColumns.value = nextMobileTerminalColumns(drawerFocusDemoColumns.value);
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openTerminalContextMenu.selector, (event, target) => {
      event.preventDefault();
      event.stopPropagation();
      const box = target.getBoundingClientRect();
      showTerminalDashboardContextMenu(target as HTMLElement, box.right, box.bottom);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders: terminal drawer, dashboard, keyboard, and clipboard surfaces over deterministic sessions (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'terminal-drawer': { catalogExamples: true, Demo: TerminalDrawerDemo },
    'terminal-dashboard': { catalogExamples: true, Demo: TerminalDashboardDemo },
    'terminal-key-bar': { catalogExamples: true, Demo: TerminalKeyBarDemo },
    'fixed-aspect-terminal-card': { catalogExamples: true, Demo: FixedAspectTerminalCardDemo },
    'terminal-edit-menu': { catalogExamples: true, Demo: TerminalEditMenuDemo },
    'terminal-copy-dialog': { catalogExamples: true, Demo: () => terminalClipboardDemo(true) },
    'terminal-paste-dialog': { catalogExamples: true, Demo: () => terminalClipboardDemo(false) },
    'terminal-rename-dialog': { catalogExamples: true, Demo: TerminalRenameDialogDemo },
  },
  wire: wireTerminalComponentsDemo,
};
