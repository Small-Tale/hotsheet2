import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { createSearchFieldModel } from '@kerfjs/ui/search-field-model';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { batch, delegate, delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { AIConversationToolbar } from '../components/ai-conversation';
import { wireContextMenuDismissal } from '../components/context-menu-dismissal';
import {
  TerminalCopyDialog,
  type TerminalCopyState,
  TerminalEditMenu,
  type TerminalEditMenuState,
  TerminalPasteDialog,
  type TerminalPasteState,
} from '../components/terminal-clipboard-dialogs';
import {
  wireTerminalClipboardActions,
  wireTerminalClipboardSheets,
  wireTerminalDashboardMenu,
  wireTerminalKeyBar,
  wireTerminalRenameDialog,
  wireTerminalTextSizeControl,
} from '../components/terminal-component-interactions';
import { FixedAspectTerminalCard, TerminalDashboard } from '../components/terminal-dashboard';
import { TerminalDrawer } from '../components/terminal-drawer';
import { TerminalFocusSearch } from '../components/terminal-focus-search';
import { TerminalKeyBar } from '../components/terminal-key-bar';
import { TerminalRenameDialog, type TerminalRenameTarget } from '../components/terminal-rename-dialog';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
import { nextMobileTerminalColumns } from '../mobile-terminal-columns';
import { terminalCopyMessage, terminalCopySelection } from '../terminal-clipboard';
import {
  consumeTerminalModifiers,
  encodeTerminalKey,
  NO_TERMINAL_MODIFIERS,
  type TerminalModifiers,
  toggleTerminalModifier,
} from '../terminal-keys';
import { terminalDrawerSearch, type TerminalSearchDirection, type TerminalSearchResults } from '../terminal-search';
import { wireTerminalSearchCountFit } from '../terminal-search-count-fit';
import { DemoEvent } from './components/demo-event';
import { onDemoSelected } from './components/demo-state';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { TERMINAL_DRAWER_DEMO_PROVIDERS } from './fixtures';
import { syncSettingsControls } from './settings-controls';
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
export const drawerCreateFailureDemo = signal<'failed' | 'retried' | 'closed'>('failed');

// HS2-174S6X: the rail's terminal search (HS2-TWZSZF) and match count (HS2-M5QPED) over a real Kerf
// SearchField model. A demo terminal has no xterm search addon, so each state supplies the results a
// searched terminal would report; production's count text and drawer mapping present them.
const TERMINAL_SEARCH_DEMO_STATES = {
  collapsed: { label: 'Collapsed', open: false, query: '', results: undefined },
  open: { label: 'Open, Empty', open: true, query: '', results: undefined },
  count: { label: 'Open, Match Count', open: true, query: 'root', results: { resultIndex: 2, resultCount: 12 } },
  none: { label: 'Open, No Matches', open: true, query: 'webpack', results: { resultIndex: -1, resultCount: 0 } },
  limit: { label: 'Open, Highlight Limit', open: true, query: 'e', results: { resultIndex: -1, resultCount: 1000 } },
} as const satisfies Record<
  string,
  { label: string; open: boolean; query: string; results: TerminalSearchResults | undefined }
>;
export type TerminalSearchDemoState = keyof typeof TERMINAL_SEARCH_DEMO_STATES;
const TERMINAL_SEARCH_DEMO_CHOICES: SelectChoice[] = Object.entries(TERMINAL_SEARCH_DEMO_STATES).map(
  ([value, { label }]) => ({ value, label }),
);
/** The terminal search demo's field model and open state, registered with the catalog's `wireSearchFields`. */
export const terminalSearchDemoModel = createSearchFieldModel(),
  terminalSearchDemoOpen = signal(false),
  terminalSearchDemoState = signal<TerminalSearchDemoState>('collapsed'),
  terminalSearchDemoResults = signal<TerminalSearchResults | undefined>(undefined),
  // Measured by production's wireTerminalSearchCountFit, not a setting: a narrow catalog compacts the count (HS2-HPDX81).
  terminalSearchDemoCountCompact = signal(false);

/** Put the terminal search demo in one of its states: the field's text, open state, and reported results. */
export function applyTerminalSearchDemoState(state: TerminalSearchDemoState): void {
  const fixture = TERMINAL_SEARCH_DEMO_STATES[state];
  batch(() => {
    terminalSearchDemoState.value = state;
    terminalSearchDemoModel.replace({ query: fixture.query, tokens: [] });
    terminalSearchDemoOpen.value = fixture.open;
    terminalSearchDemoResults.value = fixture.results && { ...fixture.results };
  });
}

/** Step the reported active match the way the terminal's search addon wraps through its matches. */
export function stepTerminalSearchDemo(direction: TerminalSearchDirection): void {
  const results = terminalSearchDemoResults.value;
  if (!results || results.resultCount <= 0) return;
  const total = results.resultCount,
    index =
      direction === 'next'
        ? (results.resultIndex + 1) % total
        : results.resultIndex <= 0
          ? total - 1
          : results.resultIndex - 1;
  terminalSearchDemoResults.value = { resultIndex: index, resultCount: total };
}

export function resetTerminalDrawerDemo(root?: ParentNode): void {
  applyTerminalSearchDemoState('collapsed');
  drawerCreateFailureDemo.value = 'failed';
  if (root) syncSettingsControls(root, 'terminal-drawer', { values: { 'search-state': 'collapsed' } });
}

export function resetTerminalFocusSearchDemo(root?: ParentNode): void {
  applyTerminalSearchDemoState('count');
  if (root) syncSettingsControls(root, 'terminal-focus-search', { values: { 'search-state': 'count' } });
}

/** The phone search bar's states share the rail demo's fixtures (HS2-7HPXCY); Collapsed shows the bar before typing. */
export function TerminalFocusSearchSettings() {
  return (
    <DemoSettingsForm settings="terminal-focus-search">
      <Select
        name="search-state"
        label="Terminal Search"
        value={terminalSearchDemoState.value}
        choices={TERMINAL_SEARCH_DEMO_CHOICES}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** The phone full-screen search bar (HS2-7HPXCY) over the shared demo model, with production's drawer mapping. */
export function TerminalFocusSearchDemo() {
  const search = terminalDrawerSearch(
    terminalSearchDemoModel,
    true,
    terminalSearchDemoModel.state.value.query ? terminalSearchDemoResults.value : undefined,
  );
  return (
    <CatalogExample
      label="Phone Search Bar"
      note="Choose a search state in Settings. Previous and Next step the reported match; the results line shows once there is a query."
      viewport={{ width: 'compact', frame: 'solid', overflow: 'hidden' }}
    >
      <TerminalFocusSearch
        model={search.model}
        count={search.count}
        hasMatches={search.hasMatches}
        hasQuery={search.hasQuery}
      />
    </CatalogExample>
  );
}

export function TerminalDrawerSettings() {
  return (
    <DemoSettingsForm settings="terminal-drawer">
      <Select
        name="search-state"
        label="Terminal Search"
        value={terminalSearchDemoState.value}
        choices={TERMINAL_SEARCH_DEMO_CHOICES}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

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
      <CatalogExample
        label="Failed session creation"
        viewport={{ ...drawerFrame, height: 'medium' }}
        rootAttributes={{ 'data-demo-scope': 'drawer-create-failure-demo' }}
      >
        <TerminalDrawer
          projectId="demo"
          projectName="Demo project"
          sessions={[
            shell,
            ...(drawerCreateFailureDemo.value === 'retried'
              ? [{ ...shell, id: 'demo-retry', title: 'Claude 1', busy: false }]
              : []),
          ]}
          pendingTabs={
            drawerCreateFailureDemo.value === 'failed'
              ? [
                  {
                    id: 'pending:demo',
                    name: 'Claude',
                    kind: 'terminal',
                    failure: 'Demo project did not respond. Check the server and try again.',
                  },
                ]
              : []
          }
          width={900}
          height={320}
          fitAcross={2}
          fitHigh={2}
          selectedId={
            drawerCreateFailureDemo.value === 'failed'
              ? 'pending:demo'
              : drawerCreateFailureDemo.value === 'retried'
                ? 'demo-retry'
                : 'shell'
          }
        />
      </CatalogExample>
      {/* The rail's terminal search beside Hide (HS2-TWZSZF) and its match count (HS2-M5QPED, HS2-174S6X).
          As in production, emptying or closing the field clears the count. */}
      <CatalogExample
        label="Terminal Search"
        note="Choose a search state in Settings. The match count steps to the next match (Shift: previous)."
        viewport={drawerFrame}
      >
        <TerminalDrawer
          projectId="demo-search"
          projectName="Demo project"
          sessions={[{ ...shell, projectId: 'demo-search' }]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="shell"
          search={terminalDrawerSearch(
            terminalSearchDemoModel,
            terminalSearchDemoOpen.value,
            terminalSearchDemoOpen.value && terminalSearchDemoModel.state.value.query
              ? terminalSearchDemoResults.value
              : undefined,
            terminalSearchDemoCountCompact.value,
          )}
        />
      </CatalogExample>
      {/* A phone rail's Search opens the terminal full screen with its search bar (HS2-7HPXCY); it is pressed
          while a kept search has a query. */}
      <CatalogExample label="Phone Search" viewport={{ width: 'compact', frame: 'solid', overflow: 'hidden' }}>
        <TerminalDrawer
          projectId="demo-phone-search"
          projectName="Demo project"
          sessions={[{ ...shell, projectId: 'demo-phone-search' }]}
          width={390}
          height={96}
          fitAcross={1}
          fitHigh={1}
          selectedId="shell"
          mobile
          search={terminalDrawerSearch(
            terminalSearchDemoModel,
            terminalSearchDemoOpen.value,
            terminalSearchDemoResults.value,
          )}
        />
      </CatalogExample>
      {/* With one installed AI provider the AI shell entry is named for it instead of a submenu (HS2-3HT4PA). */}
      <CatalogExample label="One AI Provider" viewport={drawerFrame}>
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
      {/* A remote client disables host-only drawer entries such as Saved conversation… (HS2-NWWF1Z). */}
      <CatalogExample
        label="Remote Device"
        note="Host-only entries in the new-terminal menu, such as Saved conversation…, render disabled with an explanation."
        viewport={drawerFrame}
      >
        <TerminalDrawer
          projectId="demo-remote"
          projectName="Demo project"
          sessions={[]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="grid"
          aiProviders={TERMINAL_DRAWER_DEMO_PROVIDERS}
          defaultAiProvider="claude"
          hostActionsAvailable={false}
        />
      </CatalogExample>
      {/* Tab status: a halted AI session (HS2-HJ4D1H), and whether an AI session reaches Hot Sheet (HS2-EV1XK3). */}
      <CatalogExample label="Terminal Tab States" viewport={drawerFrame}>
        <TerminalDrawer
          projectId="demo-states"
          projectName="Demo project"
          sessions={[
            {
              ...shell,
              id: 'primary',
              projectId: 'demo-states',
              title: 'Primary',
              tool: 'claude',
              ai_connection: { agent: 'claude', at: '2026-10-05T08:00:00Z' },
              mcp_connected: true,
              feedback_primary: true,
              aiConnection: 'connected',
            },
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
              mcp_connected: true,
              aiConnection: 'connected',
            },
            {
              ...shell,
              id: 'mcp-missing',
              projectId: 'demo-states',
              title: 'MCP not connected',
              kind: 'ai',
              tool: 'claude',
              ai_connection: { agent: 'claude', at: '2026-10-05T08:00:00Z' },
              mcp_connected: false,
              aiConnection: 'mcp_missing',
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
      <CatalogExample label="AI Chat Tab States" viewport={drawerFrame}>
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
      {/* The selected chat's model selector, Save, and Stop render in the tab toolbar (HS2-59F7WK). */}
      <CatalogExample label="AI Chat Toolbar Controls" viewport={drawerFrame}>
        <TerminalDrawer
          projectId="demo-chat-toolbar"
          projectName="Demo project"
          sessions={[]}
          chatTabs={[
            {
              id: 'working-chat',
              name: 'Claude chat',
              tool: 'Claude',
              content: <p>Claude is working.</p>,
              toolbar: (
                <AIConversationToolbar
                  open
                  presentation="embedded"
                  tool="Claude"
                  messages={[{ id: 'answer', role: 'assistant', content: 'Done.', status: 'completed' }]}
                  draft=""
                  busy
                  interruptible
                  model="opus"
                  effort="medium"
                  models={[{ id: 'opus', label: 'Opus' }]}
                  efforts={['low', 'medium', 'high']}
                  canChangeModel
                  canChangeEffort
                />
              ),
            },
          ]}
          width={900}
          height={96}
          fitAcross={2}
          fitHigh={2}
          selectedId="working-chat"
        />
      </CatalogExample>
      {/* A requested AI shell still starting: its pending tab is selected and the drawer shows a busy
          placeholder instead of passing through the Project grid (HS2-AQ40G7). */}
      <CatalogExample label="Starting Drawer Item" viewport={{ ...drawerFrame, height: 'medium' }}>
        <TerminalDrawer
          projectId="demo-pending"
          projectName="Demo project"
          sessions={[{ ...shell, projectId: 'demo-pending' }]}
          pendingTabs={[{ id: 'pending:demo', name: 'Claude', kind: 'terminal' }]}
          width={900}
          height={320}
          fitAcross={2}
          fitHigh={2}
          selectedId="pending:demo"
        />
      </CatalogExample>
      {/* A touch-first desktop (coarse primary pointer) adds rail Copy and Paste (HS2-5DHHPV). */}
      <CatalogExample label="Touch-First Desktop" viewport={drawerFrame}>
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
    <>
      <CatalogExample
        label="Interactive"
        viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}
      >
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
                  mcp_connected: true,
                  feedback_primary: true,
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
                  id: 'mcp-missing',
                  projectId: 'demo',
                  projectName: 'Demo project',
                  title: 'Review',
                  kind: 'ai',
                  tool: 'claude',
                  ai_connection: { agent: 'claude', at: '2026-10-05T08:00:00Z' },
                  mcp_connected: false,
                  aiConnection: 'mcp_missing',
                  alive: true,
                  busy: false,
                  cwd: '/work/demo',
                  scrollback: 'Reviewing the diff',
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
      <TerminalDashboardStates />
    </>
  );
}

/** The dashboard's empty, loading, and message states, including a short drawer grid (HS2-1Q6GER). */
function TerminalDashboardStates() {
  const state = (props: Partial<Parameters<typeof TerminalDashboard>[0]>, height: number) => (
    <TerminalDashboard groups={[]} width={640} height={height} fitAcross={3} fitHigh={3} {...props} />
  );
  return (
    <>
      <CatalogExample label="Empty" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <div style="height: 240px">{state({}, 240)}</div>
      </CatalogExample>
      <CatalogExample label="Loading" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <div style="height: 240px">{state({ loading: true }, 240)}</div>
      </CatalogExample>
      <CatalogExample label="Message" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <div style="height: 240px">{state({ message: 'Terminals are unavailable on this device.' }, 240)}</div>
      </CatalogExample>
      <CatalogExample label="Empty, Short Drawer" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <div style="height: 100px">{state({ layoutMode: 'drawer' }, 100)}</div>
      </CatalogExample>
    </>
  );
}

export function TerminalKeyBarDemo() {
  return (
    <>
      <CatalogExample label="Keys" viewport={{ width: 'compact', overflow: 'hidden' }}>
        <TerminalKeyBar modifiers={keyBarDemoModifiers.value} functionRow={keyBarDemoFunctionRow.value} />
        <DemoEvent live={false} attributes={{ 'data-key-bar-demo-output': '' }}>
          {keyBarDemoOutput.value || 'Tap a key to see the bytes it sends.'}
        </DemoEvent>
      </CatalogExample>
      <CatalogExample label="Function Row" viewport={{ width: 'compact', overflow: 'hidden' }}>
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
      <CatalogExample label="Grid Preview" viewport={{ width: 'text', surface: 'lowered' }}>
        <FixedAspectTerminalCard session={session} fit="aspect" />
      </CatalogExample>
      <CatalogExample label="Magnified Interactive" viewport={{ width: 'full', surface: 'lowered' }}>
        <FixedAspectTerminalCard session={session} mode="magnified" fit="aspect" />
      </CatalogExample>
      <CatalogExample label="Magnified, Touch-First Desktop" viewport={{ width: 'full', surface: 'lowered' }}>
        <FixedAspectTerminalCard session={session} mode="magnified" fit="aspect" touchClipboard />
      </CatalogExample>
      <CatalogExample label="Magnified Phone Toolbar" viewport={{ width: 'compact', surface: 'lowered' }}>
        <FixedAspectTerminalCard
          session={session}
          mode="magnified"
          fit="aspect"
          mobile={{ viewport: { left: 0, top: 0, width: 390, height: 844 }, keyboardVisible: false, columns: 60 }}
        />
      </CatalogExample>
      <CatalogExample label="Magnified Phone, Keyboard Presented" viewport={{ width: 'compact', surface: 'lowered' }}>
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
    <CatalogExample label="Interactive">
      <Row gap="xs">
        <Button rootAttributes={{ 'data-edit-menu-demo-open': '' }}>Long-Press Here</Button>
        <Button rootAttributes={{ 'data-edit-menu-demo-open': 'selection' }}>Long-Press a Word</Button>
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
    <CatalogExample label="Interactive">
      <Row gap="xs">
        {copy ? (
          <Button rootAttributes={{ 'data-clipboard-demo-open': 'copy' }}>Open Copy Sheet</Button>
        ) : (
          <>
            <Button rootAttributes={{ 'data-clipboard-demo-open': 'denied' }}>Clipboard Denied</Button>
            <Button rootAttributes={{ 'data-clipboard-demo-open': 'unavailable' }}>Clipboard Unavailable</Button>
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
    <CatalogExample label="Interactive">
      <Row gap="xs">
        <Button rootAttributes={{ 'data-rename-demo-open': 'renamed' }}>Rename a Renamed Terminal</Button>
        <Button rootAttributes={{ 'data-rename-demo-open': 'default' }}>Rename a Default-Named Terminal</Button>
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
  demoListeners.add(
    delegate(
      root,
      'click',
      `${TERMINALS_ACTIONS.retryFailedDrawerTab.selector}, ${TERMINALS_ACTIONS.closeFailedDrawerTab.selector}`,
      (_event, target) => {
        if (target.closest('[data-demo-scope="drawer-create-failure-demo"]'))
          drawerCreateFailureDemo.value = target.matches(TERMINALS_ACTIONS.retryFailedDrawerTab.selector)
            ? 'retried'
            : 'closed';
      },
    ),
  );
  // The search specimen's count yields its room on a narrow rail through production's wiring (HS2-HPDX81).
  demoListeners.add(
    wireTerminalSearchCountFit(root, (compact) => {
      terminalSearchDemoCountCompact.value = compact;
    }),
  );
  // Dashboard and long-press menus close with Web Awesome's own hide, an outside press, or Escape,
  // through the dismissal helper production's shell uses (HS2-AFCWNG).
  demoListeners.add(
    wireContextMenuDismissal(root, [
      {
        surface: 'terminal',
        isOpen: () => Boolean(terminalDashboardContextMenu.value),
        stateToken: () => terminalDashboardContextMenu.value,
        close: () => {
          terminalDashboardContextMenu.value = undefined;
        },
        ignore: '[data-action="open-terminal-context-menu"]',
        hide: true,
      },
      {
        surface: 'terminal-edit',
        isOpen: () => Boolean(editMenuDemo.value),
        stateToken: () => editMenuDemo.value,
        close: () => {
          editMenuDemo.value = undefined;
        },
        hide: true,
      },
    ]),
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
  // The production dashboard context-menu handlers with fixture effects (HS2-05NMW0).
  demoListeners.add(
    wireTerminalDashboardMenu(root, {
      open: (menu) => {
        // Kerf places a context menu at its viewport point even inside the app-height example frame's
        // transform (KF-7PEMC6), so the point passes through unconverted (HS2-HVD74J, was HS2-CY6A6K).
        terminalDashboardContextMenu.value = menu;
      },
      clearHalt: () => {
        terminalDashboardHaltCleared.value = true;
        terminalDashboardContextMenu.value = undefined;
      },
    }),
  );
  // The TerminalKeyBar demo runs production's key-bar wiring and shows the exact bytes each key would
  // send (HS2-CKS78M, HS2-XE048K).
  demoListeners.add(
    wireTerminalKeyBar(root, {
      toggleModifier: (modifier) => {
        keyBarDemoModifiers.value = toggleTerminalModifier(keyBarDemoModifiers.value, modifier);
      },
      toggleFunctionRow: () => {
        keyBarDemoFunctionRow.value = !keyBarDemoFunctionRow.value;
      },
      sendKey: (key) => {
        const bytes = encodeTerminalKey(key, keyBarDemoModifiers.value);
        keyBarDemoModifiers.value = consumeTerminalModifiers(keyBarDemoModifiers.value);
        keyBarDemoOutput.value = `${key} → ${JSON.stringify(bytes).slice(1, -1)}`;
      },
    }),
  );
  // The key bar's More-row clipboard group and the long-press edit menu report what production does
  // (HS2-FRB545, HS2-KKP8YJ).
  const reportClipboardAction = (target: Element, action: 'selection' | 'copy' | 'paste') => {
    if (target.closest('[data-context-menu="terminal-edit"]')) {
      editMenuDemoOutput.value =
        action === 'selection'
          ? 'Copy → copies the selected terminal text'
          : action === 'copy'
            ? 'Copy Text… → opens the terminal Copy sheet'
            : 'Paste → sends the clipboard to the terminal';
      editMenuDemo.value = undefined;
    } else if (target.closest('[data-component="terminal-key-bar"]'))
      keyBarDemoOutput.value =
        action === 'paste' ? 'Paste → sends the clipboard to the terminal' : 'Copy → opens the terminal Copy sheet';
  };
  demoListeners.add(
    wireTerminalClipboardActions(root, {
      copySelection: (target) => {
        reportClipboardAction(target, 'selection');
      },
      copyText: (target) => {
        reportClipboardAction(target, 'copy');
      },
      paste: (target) => {
        reportClipboardAction(target, 'paste');
      },
    }),
  );
  // The terminal search demo (HS2-174S6X): Settings choose a state; the count control and Enter in the
  // field step the reported match as production's do (HS2-M5QPED), keeping the keyboard in the field.
  demoListeners.add(
    delegate(root, 'change', '[data-settings="terminal-drawer"] [name="search-state"]', (_event, target) => {
      const state = (target as FormControl).value;
      if (Object.hasOwn(TERMINAL_SEARCH_DEMO_STATES, state))
        applyTerminalSearchDemoState(state as TerminalSearchDemoState);
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="terminal-focus-search"] [name="search-state"]', (_event, target) => {
      const state = (target as FormControl).value;
      if (Object.hasOwn(TERMINAL_SEARCH_DEMO_STATES, state))
        applyTerminalSearchDemoState(state as TerminalSearchDemoState);
    }),
  );
  // The phone search bar's Previous Match (HS2-7HPXCY) steps back and keeps the keyboard in the field.
  demoListeners.add(
    delegate(root, 'mousedown', TERMINALS_ACTIONS.previousTerminalSearchMatch.selector, (event) => {
      event.preventDefault();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', TERMINALS_ACTIONS.previousTerminalSearchMatch.selector, () => {
      stepTerminalSearchDemo('previous');
    }),
  );
  demoListeners.add(
    delegate(root, 'mousedown', TERMINALS_ACTIONS.nextTerminalSearchMatch.selector, (event) => {
      event.preventDefault();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', TERMINALS_ACTIONS.nextTerminalSearchMatch.selector, (event) => {
      stepTerminalSearchDemo((event as MouseEvent).shiftKey ? 'previous' : 'next');
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'keydown', TERMINALS_TARGETS.terminalSearchEditor.selector, (event) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' || keyboard.isComposing || keyboard.altKey || keyboard.ctrlKey || keyboard.metaKey)
        return;
      keyboard.preventDefault();
      keyboard.stopPropagation();
      stepTerminalSearchDemo(keyboard.shiftKey ? 'previous' : 'next');
    }),
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
    wireTerminalRenameDialog(root, {
      rename: (name) => {
        renameDemoOutput.value = `Rename → the tab shows ${name}`;
        renameDemoTarget.value = undefined;
      },
      reset: () => {
        renameDemoOutput.value = `Reset to default → the tab shows ${renameDemoTarget.value?.defaultName ?? ''} again`;
        renameDemoTarget.value = undefined;
      },
      dismiss: (reason) => {
        if (reason === 'cancel') renameDemoOutput.value = 'Cancel → the name is unchanged';
        renameDemoTarget.value = undefined;
      },
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
    wireTerminalClipboardSheets(root, {
      confirmCopy: (field) => {
        const { text, selection } = terminalCopySelection(field.value, field.selectionStart, field.selectionEnd);
        clipboardDemoOutput.value = terminalCopyMessage(text, selection);
        closeClipboardDemo();
      },
      closeCopy: closeClipboardDemo,
      submitPaste: (text) => {
        clipboardDemoOutput.value = `Pasted → ${JSON.stringify(text.replace(/\r?\n/g, '\r')).slice(1, -1)}`;
        closeClipboardDemo();
      },
      closePaste: closeClipboardDemo,
    }),
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
  // The drawer focus-mode text-size control cycles the demo's column fixture through production's
  // handler (HS2-01D4JP, HS2-05NMW0).
  demoListeners.add(
    wireTerminalTextSizeControl(root, (control) => {
      if (control.closest('[data-demo-scope="terminal-drawer-focus-demo"]'))
        drawerFocusDemoColumns.value = nextMobileTerminalColumns(drawerFocusDemoColumns.value);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders: terminal drawer, dashboard, keyboard, and clipboard surfaces over deterministic sessions (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'terminal-drawer': {
      catalogExamples: true,
      Demo: TerminalDrawerDemo,
      Settings: TerminalDrawerSettings,
      reset: resetTerminalDrawerDemo,
    },
    'terminal-dashboard': { catalogExamples: true, Demo: TerminalDashboardDemo },
    'terminal-key-bar': { catalogExamples: true, Demo: TerminalKeyBarDemo },
    'terminal-focus-search': {
      catalogExamples: true,
      Demo: TerminalFocusSearchDemo,
      Settings: TerminalFocusSearchSettings,
      reset: resetTerminalFocusSearchDemo,
    },
    'fixed-aspect-terminal-card': { catalogExamples: true, Demo: FixedAspectTerminalCardDemo },
    'terminal-edit-menu': { catalogExamples: true, Demo: TerminalEditMenuDemo },
    'terminal-copy-dialog': { catalogExamples: true, Demo: () => terminalClipboardDemo(true) },
    'terminal-paste-dialog': { catalogExamples: true, Demo: () => terminalClipboardDemo(false) },
    'terminal-rename-dialog': { catalogExamples: true, Demo: TerminalRenameDialogDemo },
  },
  wire: wireTerminalComponentsDemo,
};
