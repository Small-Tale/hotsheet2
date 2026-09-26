import './terminal-drawer.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { TabBar } from '@kerfjs/ui/tab-bar';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import {
  Bot,
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
import type { MobileTerminalViewport } from '../mobile-terminal-focus';
import { terminalGridContentSize } from '../terminal-grid-layout';
import {
  type MobileMagnifiedTerminal,
  TerminalDashboard,
  type TerminalDashboardSession,
  TerminalSession,
} from './terminal-dashboard';

export interface TerminalDrawerChatTab {
  id: string;
  name: string;
  tool: string;
  content: SafeHtml;
  busy?: boolean;
  summary?: string;
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
  createMenuOpen?: boolean;
  focusMode?: boolean;
  focusViewport?: MobileTerminalViewport;
}
export const TERMINAL_DRAWER_TAB_BAR_ID = 'terminal-drawer';

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
  createMenuOpen = false,
  focusMode = false,
  focusViewport,
}: TerminalDrawerProps) {
  const selected =
      sessions.some((session) => session.id === selectedId) || chatTabs.some((chat) => chat.id === selectedId)
        ? selectedId
        : 'grid',
    selectedSession = sessions.find((session) => session.id === selected),
    selectedChat = chatTabs.find((chat) => chat.id === selected),
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
        className="terminal-drawer__grid-tab"
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
              labelMaxWidth={144}
              selectAction="select-drawer-item"
              closeAction="close-terminal-tab"
              closeIcon={<LucideIcon icon={X} name="x" />}
              rootAttributes={{ 'data-tab-kind': 'terminal', 'data-terminal-id': session.id }}
              leading={<LucideIcon icon={SquareTerminal} name="square-terminal" />}
              trailing={session.busy ? <i aria-label="Busy"></i> : undefined}
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
          data-action="toggle-terminal-drawer-maximize"
          title={`Double-click to ${maximized ? 'restore' : 'maximize'} terminal drawer`}
        >
          <TabBar
            id={TERMINAL_DRAWER_TAB_BAR_ID}
            label="Terminal drawer views"
            className="terminal-drawer__views"
            // Terminal selection replaces the controlled tab nodes and changes a live work surface.
            // Keep arrow-key navigation focus-only; Enter/Space performs the explicit activation.
            activation="manual"
            trailing={
              <>
                <div class="terminal-drawer__create-wrap">
                  <button
                    type="button"
                    class="terminal-drawer__create"
                    data-action="toggle-terminal-create-menu"
                    aria-label="New drawer item"
                    aria-expanded={String(createMenuOpen)}
                    title="New shell or AI chat"
                  >
                    <LucideIcon icon={Plus} name="plus" />
                  </button>
                  {createMenuOpen && (
                    <div class="terminal-drawer__create-menu" role="menu" aria-label="New drawer item">
                      <wa-dropdown-item data-action="create-terminal-drawer-item" data-item-id="default-shell">
                        <span slot="icon">
                          <LucideIcon icon={SquareTerminal} name="square-terminal" />
                        </span>
                        Default shell
                      </wa-dropdown-item>
                      <wa-dropdown-item data-action="create-terminal-drawer-item" data-item-id="ai-shell">
                        <span slot="icon">
                          <LucideIcon icon={Bot} name="bot" />
                        </span>
                        AI shell
                      </wa-dropdown-item>
                      <wa-dropdown-item data-action="create-terminal-drawer-item" data-item-id="ai-chat">
                        <span slot="icon">
                          <LucideIcon icon={MessageSquare} name="message-square" />
                        </span>
                        AI chat
                      </wa-dropdown-item>
                      <wa-dropdown-item data-action="open-saved-conversation">
                        <span slot="icon">
                          <LucideIcon icon={FolderOpen} name="folder-open" />
                        </span>
                        Saved conversation…
                      </wa-dropdown-item>
                    </div>
                  )}
                </div>
                <div class="terminal-drawer__actions">
                  <button
                    type="button"
                    data-action="toggle-terminal-drawer"
                    aria-label="Hide terminal drawer"
                    title="Hide terminal drawer"
                  >
                    <LucideIcon icon={PanelBottomClose} name="panel-bottom-close" />
                  </button>
                </div>
              </>
            }
          >
            {tabs}
          </TabBar>
        </header>
      )}
      <div class="terminal-drawer__content">
        {selectedChat ? (
          selectedChat.content
        ) : selectedSession ? (
          sessions.map((session) => <TerminalSession session={session} active={session.id === selectedSession.id} />)
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
            loading={loading}
            message={message}
          />
        )}
      </div>
      {focusMode && (
        <button
          type="button"
          class="terminal-drawer__focus-exit"
          data-action="exit-terminal-focus-mode"
          aria-label="Exit terminal focus"
          title="Exit terminal focus"
        >
          <LucideIcon icon={Minimize2} name="minimize-2" />
          <span>Exit</span>
        </button>
      )}
    </section>
  );
}
