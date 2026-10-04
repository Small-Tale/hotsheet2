import './terminal-drawer.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import {
  ALargeSmall,
  Bot,
  ClipboardPaste,
  Copy,
  FolderOpen,
  LayoutGrid,
  MessageSquare,
  Minimize2,
  PanelBottomClose,
  Plus,
  SquareTerminal,
  X,
} from 'lucide';

import { orderedDrawerTabIds } from '../drawer-tab-order';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import type { MobileTerminalViewport } from '../mobile-terminal-focus';
import { terminalGridContentSize } from '../terminal-grid-layout';
import {
  type MobileMagnifiedTerminal,
  type TerminalContextMenuAction,
  TerminalDashboard,
  type TerminalDashboardSession,
  TerminalSession,
} from './terminal-dashboard';
import { TerminalKeyBar } from './terminal-key-bar';

export interface TerminalDrawerChatTab {
  id: string;
  name: string;
  tool: string;
  content: SafeHtml;
  busy?: boolean;
  summary?: string;
}
/** An AI provider the drawer can open an AI shell with (HS2-3HT4PA). */
export interface TerminalDrawerAiProvider {
  id: string;
  name: string;
}
export interface TerminalDrawerProps {
  projectId: string;
  projectName: string;
  sessions: TerminalDashboardSession[];
  chatTabs?: TerminalDrawerChatTab[];
  tabOrder?: string[];
  width: number;
  height: number;
  fitAcross: number;
  fitHigh: number;
  selectedId: string;
  magnifiedKey?: string;
  mobileMagnified?: MobileMagnifiedTerminal;
  loading?: boolean;
  message?: string;
  maximized?: boolean;
  focusMode?: boolean;
  focusViewport?: MobileTerminalViewport;
  /** Phone focus-mode text-size control state (columns + keyboard visibility); undefined off phones (HS2-ZSFAHF). */
  focusTextSize?: MobileMagnifiedTerminal;
  /** The open tile More actions menu, rendered by the drawer grid when it targets one of its tiles (HS2-V2CCN6). */
  contextMenu?: { key: string; x: number; y: number };
  /** Installed AI providers, in catalog order, for the New drawer item menu's AI shell entry (HS2-3HT4PA). */
  aiProviders?: readonly TerminalDrawerAiProvider[];
  /** The provider an AI shell uses when none is picked: the project default or this session's Drive choice. */
  defaultAiProvider?: string;
  /** Phone presentation (the app's mobile layout): dedicated terminals clip their scaled xterm. */
  mobile?: boolean;
  /**
   * Touch-first device at a desktop width (coarse primary pointer, HS2-5DHHPV): the rail offers Copy and
   * Paste for the selected terminal, and a magnified grid tile's toolbar carries them too.
   */
  touchClipboard?: boolean;
}
export const TERMINAL_DRAWER_TAB_BAR_ID = 'terminal-drawer';
// Terminal visibility is scoped to the workspace dashboard, so the drawer grid offers Open only.
const DRAWER_CONTEXT_MENU_ACTIONS: readonly TerminalContextMenuAction[] = ['open'];

/**
 * The New drawer item menu's AI shell entry (HS2-3HT4PA): with one provider it is named for that
 * provider; with several it opens a submenu of the default, then every provider by name, each using
 * that provider's own default model and effort.
 */
export function aiShellMenuItem(
  providers: readonly TerminalDrawerAiProvider[],
  defaultProvider?: string,
): PopupMenuEntry {
  const icon = <LucideIcon icon={Bot} name="bot" />,
    shell = (label: string, provider?: string): PopupMenuEntry => ({
      label,
      action: 'create-terminal-drawer-item',
      icon,
      attributes: { 'data-item-id': 'ai-shell', ...(provider ? { 'data-provider': provider } : {}) },
    });
  if (providers.length < 2) {
    const only = providers.at(0);
    // The only provider is normally the default; name it explicitly when a stale default points elsewhere.
    return only ? shell(`${only.name} shell`, only.id === defaultProvider ? undefined : only.id) : shell('AI shell');
  }
  const fallback = providers.find((provider) => provider.id === defaultProvider) ?? providers[0];
  return {
    label: 'AI shell',
    icon,
    attributes: { 'data-item-id': 'ai-shell-providers' },
    submenu: [
      shell(`Default (${fallback.name})`),
      { kind: 'divider' },
      ...providers.map((provider) => shell(provider.name, provider.id)),
    ],
  };
}

export function TerminalDrawer({
  projectId,
  projectName,
  sessions,
  chatTabs = [],
  tabOrder = [],
  width,
  height,
  fitAcross,
  fitHigh,
  selectedId,
  magnifiedKey,
  mobileMagnified,
  loading = false,
  message = '',
  maximized = false,
  focusMode = false,
  focusViewport,
  focusTextSize,
  contextMenu,
  aiProviders = [],
  defaultAiProvider,
  mobile = false,
  touchClipboard = false,
}: TerminalDrawerProps) {
  const selected =
      sessions.some((session) => session.id === selectedId) || chatTabs.some((chat) => chat.id === selectedId)
        ? selectedId
        : 'grid',
    selectedSession = sessions.find((session) => session.id === selected),
    selectedChat = chatTabs.find((chat) => chat.id === selected),
    // HS2-5DHHPV: a touch-first desktop gets rail Copy/Paste for the selected terminal (phones use focus mode).
    railClipboard = touchClipboard && Boolean(selectedSession) && !focusMode,
    gridSize = terminalGridContentSize(width, height),
    sessionsById = new Map(sessions.map((session) => [session.id, session])),
    chatsById = new Map(chatTabs.map((chat) => [chat.id, chat])),
    orderedIds = orderedDrawerTabIds(
      sessions.map((session) => session.id),
      chatTabs.map((chat) => chat.id),
      tabOrder,
    ),
    gridChats = chatTabs.map((chat) => ({
      id: chat.id,
      name: chat.name,
      tool: chat.tool,
      busy: chat.busy,
      summary: chat.summary,
      projectId,
      projectName,
    })),
    tabs = [
      <AppTab
        id="grid"
        name="Project grid"
        selected={selected === 'grid'}
        closable={false}
        // Kerf keeps the leading tab visible while its peers scroll (beta.62 `pinned`, HS2-WF3W6A).
        pinned
        presentation="icon-only"
        size="compact"
        selectAction="select-drawer-item"
        leading={<LucideIcon icon={LayoutGrid} name="layout-grid" />}
      />,
      ...orderedIds.map((id) => {
        const session = sessionsById.get(id);
        if (session)
          return (
            <AppTab
              id={session.id}
              name={session.title ?? session.id}
              selected={selected === session.id}
              draggable
              className="terminal-tab"
              size="compact"
              // Narrow drawers (phones) truncate terminal names sooner.
              labelMaxWidth={width <= 768 ? 112 : 144}
              selectAction="select-drawer-item"
              closeAction="close-terminal-tab"
              closeIcon={<LucideIcon icon={X} name="x" />}
              rootAttributes={{ 'data-tab-kind': 'terminal', 'data-terminal-id': session.id }}
              leading={<LucideIcon icon={SquareTerminal} name="square-terminal" />}
              trailing={session.busy ? <i class="terminal-drawer__busy-dot" aria-label="Busy" /> : undefined}
            />
          );
        const chat = chatsById.get(id)!;
        return (
          <AppTab
            id={chat.id}
            name={chat.name}
            selected={selected === chat.id}
            draggable
            className="ai-chat-tab"
            size="compact"
            labelMaxWidth={144}
            selectAction="select-drawer-item"
            closeAction="close-ai-chat-tab"
            closeIcon={<LucideIcon icon={X} name="x" />}
            rootAttributes={{ 'data-tab-kind': 'ai-chat', 'data-chat-id': chat.id }}
            leading={<LucideIcon icon={MessageSquare} name="message-square" />}
          />
        );
      }),
    ];
  return (
    <section
      class="terminal-drawer"
      data-component="terminal-drawer"
      data-project-id={projectId}
      data-mode={selectedChat ? 'ai-chat' : selected === 'grid' ? 'grid' : 'dedicated'}
      data-maximized={String(maximized)}
      data-focus-mode={String(focusMode)}
      data-terminal-drawer-measure="true"
      aria-label={`${projectName} terminal drawer`}
      style={
        focusMode && focusViewport
          ? `--terminal-focus-left:${focusViewport.left}px;--terminal-focus-top:${focusViewport.top}px;--terminal-focus-width:${focusViewport.width}px;--terminal-focus-height:${focusViewport.height}px`
          : undefined
      }
    >
      {!focusMode && (
        <header
          class="terminal-drawer__rail"
          {...TERMINALS_ACTIONS.toggleTerminalDrawerMaximize.attrs}
          title={`Double-click to ${maximized ? 'restore' : 'maximize'} terminal drawer`}
        >
          <div class="terminal-drawer__views">
            <TabBar
              id={TERMINAL_DRAWER_TAB_BAR_ID}
              label="Terminal drawer views"
              // Terminal selection replaces the controlled tab nodes and changes a live work surface.
              // Keep arrow-key navigation focus-only; Enter/Space performs the explicit activation.
              activation="manual"
              trailingPlacement="adjacent"
              // The pinned Project grid tab overlaps the scrolling peers; settle the strip at whole-tab
              // starts so a reveal or swipe never leaves a peer as a sliver beside it (HS2-6Y8HSH).
              snapTabs
              trailing={
                <ToolbarControlGroup
                  className="terminal-drawer__create-wrap"
                  size="compact"
                  appearance="borderless"
                  single
                  nestedDropdown
                >
                  <PopupMenu
                    label="New drawer item"
                    icon={<LucideIcon icon={Plus} name="plus" />}
                    caret={false}
                    placement="top-start"
                    rootAttributes={{ 'data-terminal-drawer-create': 'true' }}
                    items={[
                      {
                        label: 'Terminal',
                        action: 'create-terminal-drawer-item',
                        icon: <LucideIcon icon={SquareTerminal} name="square-terminal" />,
                        attributes: { 'data-item-id': 'default-shell' },
                      },
                      aiShellMenuItem(aiProviders, defaultAiProvider),
                      {
                        label: 'AI chat',
                        action: 'create-terminal-drawer-item',
                        icon: <LucideIcon icon={MessageSquare} name="message-square" />,
                        attributes: { 'data-item-id': 'ai-chat' },
                      },
                      {
                        label: 'Saved conversation…',
                        action: 'open-saved-conversation',
                        icon: <LucideIcon icon={FolderOpen} name="folder-open" />,
                      },
                    ]}
                  />
                </ToolbarControlGroup>
              }
              end={
                <ToolbarControlGroup
                  className="terminal-drawer__actions"
                  appearance="borderless"
                  single={!railClipboard}
                >
                  {railClipboard && (
                    <button
                      type="button"
                      {...TERMINALS_ACTIONS.copyTerminalText.attrs}
                      aria-label="Copy terminal text"
                      title="Copy terminal text"
                    >
                      <LucideIcon icon={Copy} name="copy" />
                    </button>
                  )}
                  {railClipboard && (
                    <button
                      type="button"
                      {...TERMINALS_ACTIONS.pasteTerminalText.attrs}
                      aria-label="Paste"
                      title="Paste"
                    >
                      <LucideIcon icon={ClipboardPaste} name="clipboard-paste" />
                    </button>
                  )}
                  <button
                    type="button"
                    {...TERMINALS_ACTIONS.toggleTerminalDrawer.attrs}
                    aria-label="Hide terminal drawer"
                    title="Hide terminal drawer"
                  >
                    <LucideIcon icon={PanelBottomClose} name="panel-bottom-close" />
                  </button>
                </ToolbarControlGroup>
              }
            >
              {tabs}
            </TabBar>
          </div>
        </header>
      )}
      <div class="terminal-drawer__content">
        {selectedChat ? (
          selectedChat.content
        ) : selectedSession ? (
          sessions.map((session) => (
            <TerminalSession
              session={session}
              active={session.id === selectedSession.id}
              mobile={mobile}
              focus={focusMode}
            />
          ))
        ) : (
          <TerminalDashboard
            groups={[{ projectId, projectName, sessions, chats: gridChats, itemOrder: orderedIds }]}
            width={gridSize.width}
            height={gridSize.height}
            fitAcross={fitAcross}
            fitHigh={fitHigh}
            grouping="flow"
            layoutMode="drawer"
            magnifiedKey={magnifiedKey}
            mobileMagnified={mobileMagnified}
            touchClipboard={touchClipboard}
            loading={loading}
            message={message}
            contextMenu={contextMenu}
            contextMenuActions={DRAWER_CONTEXT_MENU_ACTIONS}
          />
        )}
      </div>
      {focusMode && focusTextSize?.keyboardVisible && focusTextSize.keyBar && (
        <TerminalKeyBar {...focusTextSize.keyBar} />
      )}
      {focusMode && focusTextSize && (
        <button
          type="button"
          class="terminal-drawer__focus-text-size"
          {...TERMINALS_ACTIONS.cycleMobileTerminalColumns.attrs}
          data-columns={String(focusTextSize.columns)}
          data-keyboard-visible={String(focusTextSize.keyboardVisible)}
          aria-label={`Text size: ${focusTextSize.columns} columns. Change text size`}
          title="Change text size"
        >
          <LucideIcon size="s" icon={ALargeSmall} name="a-large-small" />
        </button>
      )}
      {focusMode && focusTextSize && (
        <div
          class="terminal-drawer__focus-clipboard"
          role="group"
          aria-label="Clipboard"
          data-keyboard-visible={String(focusTextSize.keyboardVisible)}
        >
          <button
            type="button"
            {...TERMINALS_ACTIONS.copyTerminalText.attrs}
            aria-label="Copy terminal text"
            title="Copy terminal text"
          >
            <LucideIcon size="s" icon={Copy} name="copy" />
          </button>
          <button type="button" {...TERMINALS_ACTIONS.pasteTerminalText.attrs} aria-label="Paste" title="Paste">
            <LucideIcon size="s" icon={ClipboardPaste} name="clipboard-paste" />
          </button>
        </div>
      )}
      {focusMode && (
        <button
          type="button"
          class="terminal-drawer__focus-exit"
          {...TERMINALS_ACTIONS.exitTerminalFocusMode.attrs}
          aria-label="Exit terminal focus"
          title="Exit terminal focus"
        >
          <LucideIcon size="s" icon={Minimize2} name="minimize-2" />
          <span>Exit</span>
        </button>
      )}
    </section>
  );
}
