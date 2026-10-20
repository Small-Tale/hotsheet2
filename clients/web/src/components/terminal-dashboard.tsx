import '@kerfjs/ui/floating-toolbar.css';
import './terminal-dashboard.css';

import { Badge } from '@kerfjs/ui/badge';
import { px, uiColor } from '@kerfjs/ui/css-values';
import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import {
  CircleCheck,
  ClipboardPaste,
  Copy,
  Ellipsis,
  ExternalLink,
  Eye,
  EyeOff,
  Maximize2,
  MessageSquare,
  Minimize2,
  Minus,
  Plug,
  Plus,
  TriangleAlert,
  Unplug,
  X,
} from 'lucide';

import type { TerminalAiConnection, TerminalHalt } from '../api';
import { contextMenuAnchor } from '../context-menu-position';
import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
import type { MobileTerminalViewport } from '../mobile-terminal-focus';
import { aiConnectionLabel, type AiConnectionState } from '../terminal-ai-connection';
import {
  TERMINAL_TILE_HORIZONTAL_CHROME,
  TERMINAL_TILE_VERTICAL_CHROME,
  terminalDrawerGridLayout,
  terminalGridLayout,
  terminalPreviewText,
} from '../terminal-grid-layout';
import { terminalPhysicalScale } from '../terminal-viewport';
import {
  TERMINAL_VISIBILITY_TYPES,
  type TerminalVisibilityGroup,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import { TerminalKeyBar, type TerminalKeyBarProps } from './terminal-key-bar';

export interface TerminalDashboardSession {
  id: string;
  kind?: 'shell' | 'ai';
  projectId: string;
  projectName: string;
  title?: string;
  /** The derived default tab name a reset restores (HS2-2Q7KTX). */
  defaultTitle?: string;
  /** Whether a user rename (saved or in flight) supplies `title` rather than the default. */
  named?: boolean;
  alive: boolean;
  busy: boolean;
  cwd?: string;
  progress?: number;
  /** The AI session halted on an API error (HS2-HJ4D1H). */
  halt?: TerminalHalt;
  question?: { question: string; tool_use_id: string; at: string };
  /** The AI tool an `ai` terminal launched (for example `codex`). */
  tool?: string;
  /** The session's own `SessionStart` report (HS2-EV1XK3). */
  ai_connection?: TerminalAiConnection;
  last_hook_report?: TerminalAiConnection;
  /** Live `hotsheet-mcp` presence for this terminal (HS2-670ADP). */
  mcp_connected?: boolean;
  /** Whether the terminal's AI session reaches Hot Sheet, derived by `deriveAiConnectionStates`. */
  aiConnection?: AiConnectionState;
  /** The server advertises session push for this live terminal connection. */
  canMessage?: boolean;
  scrollback: string;
}

export interface TerminalDashboardGroup {
  projectId: string;
  projectName: string;
  sessions: TerminalDashboardSession[];
  chats?: WorkspaceGridChat[];
  itemOrder?: string[];
}

export interface WorkspaceGridChat {
  id: string;
  projectId: string;
  projectName: string;
  name: string;
  tool: string;
  busy?: boolean;
  summary?: string;
}

/** Phone-only presentation of the magnified terminal (HS2-WMN626): the overlay tracks the visual
 * viewport so the virtual keyboard shrinks it, and a top toolbar (close + text size) is shown only
 * while the keyboard is hidden. */
export interface MobileMagnifiedTerminal {
  viewport: MobileTerminalViewport;
  keyboardVisible: boolean;
  columns: number;
  /** Special-key accessory bar state, shown above the soft keyboard (HS2-CKS78M). */
  keyBar?: TerminalKeyBarProps;
}

export interface TerminalDashboardProps {
  groups: TerminalDashboardGroup[];
  width: number;
  height: number;
  fitAcross: number;
  fitHigh: number;
  fullscreen?: boolean;
  grouping?: 'project' | 'flow';
  layoutMode?: 'responsive' | 'drawer';
  visibilityGroups?: readonly TerminalVisibilityGroup[];
  activeVisibilityGroupId?: string;
  visibilityScope?: string;
  magnifiedKey?: string;
  mobileMagnified?: MobileMagnifiedTerminal;
  /**
   * Touch-first device at a desktop width (coarse primary pointer, HS2-5DHHPV): the magnified terminal's
   * toolbar carries the same Copy and Paste actions as the phone toolbar.
   */
  touchClipboard?: boolean;
  hiddenKeys?: readonly string[];
  visibleTypes?: readonly TerminalVisibilityType[];
  loading?: boolean;
  message?: string;
  contextMenu?: { key: string; x: number; y: number };
  /**
   * Actions offered by the tile More actions menu. The project drawer grid omits `hide`, because
   * terminal visibility is scoped to the workspace dashboard and the drawer never applies it.
   */
  contextMenuActions?: readonly TerminalContextMenuAction[];
}
export type TerminalContextMenuAction = 'open' | 'hide' | 'clear-halt';
const ALL_CONTEXT_MENU_ACTIONS: readonly TerminalContextMenuAction[] = ['open', 'hide', 'clear-halt'];

const keyFor = (session: TerminalDashboardSession) => `${session.projectId}:${session.id}`;
const chatKeyFor = (chat: WorkspaceGridChat) => `${chat.projectId}:${chat.id}`;
export const WORKSPACE_CHAT_PREVIEW_NATURAL_WIDTH = 300;
export const WORKSPACE_CHAT_PREVIEW_NATURAL_HEIGHT = 180;
/** A terminal tile's liveness as a labeled Kerf status dot: busy or working (success), idle or ready (neutral), or exited (danger). */
function TileState({ state }: { state: string }) {
  // The app-owned wrapper keeps the state's hover explanation; Kerf's Badge carries the accessible name.
  return (
    <span class="terminal-tile__state" title={state}>
      <Badge
        size="dot"
        tone={state === 'Busy' || state === 'Working' ? 'success' : state === 'Exited' ? 'danger' : 'neutral'}
        label={state}
      />
    </span>
  );
}

export function TerminalVisibilityControls({
  hiddenCount = 0,
  groups = [],
  activeId = 'default',
  scope = 'dashboard',
}: {
  hiddenCount?: number;
  groups?: readonly TerminalVisibilityGroup[];
  activeId?: string;
  scope?: string;
}) {
  const choices = groups.map((group) => ({ value: group.id, label: group.name }));
  return (
    <div class="terminal-dashboard-controls__visibility-group" data-visibility-scope={scope}>
      <Toolbar
        trailing={
          <>
            <ToolbarControlGroup single>
              <button
                type="button"
                {...TERMINALS_ACTIONS.openTerminalVisibility.attrs}
                aria-label="Manage workspace visibility"
                title="Manage Workspace Visibility"
              >
                <LucideIcon icon={Eye} name="eye" />
                {hiddenCount > 0 && (
                  <Badge placement="corner" tone="warning" appearance="solid" size="compact" ariaHidden>
                    {hiddenCount}
                  </Badge>
                )}
              </button>
            </ToolbarControlGroup>
            <ToolbarControlGroup single>
              <Select
                className="terminal-dashboard-controls__visibility-select"
                name="terminal-visibility-group"
                ariaLabel="Terminal Visibility Group"
                // The control group paints the pill chrome and focus ring around the trigger.
                presentation="toolbar-borderless"
                size="compact"
                focusRingOwner="group"
                value={activeId}
                minTriggerWidth={px(88)}
                maxTriggerWidth={px(160)}
                choices={choices}
                renderSelected={(choice) => <span>{choice.label}</span>}
              />
            </ToolbarControlGroup>
          </>
        }
      />
    </div>
  );
}

export function TerminalDashboardControls({
  hiddenCount = 0,
  visibilityGroups = [],
  activeVisibilityGroupId = 'default',
  visibilityScope = 'dashboard',
}: {
  hiddenCount?: number;
  visibilityGroups?: readonly TerminalVisibilityGroup[];
  activeVisibilityGroupId?: string;
  visibilityScope?: string;
}) {
  return (
    // Kerf Row hosts the visibility controls beside workspace header actions (HS2-1Q6GER).
    <Row vAlign="middle" gap="xs" rootAttributes={{ 'data-terminal-dashboard-controls': 'true' }}>
      <TerminalVisibilityControls
        hiddenCount={hiddenCount}
        groups={visibilityGroups}
        activeId={activeVisibilityGroupId}
        scope={visibilityScope}
      />
    </Row>
  );
}

/**
 * How a terminal card sizes itself: `grid` takes the tile size its grid sets, and `aspect` takes its
 * container's width and an intrinsic height with a 5:3 viewport (HS2-0X36TX).
 */
export type TerminalCardFit = 'grid' | 'aspect';

function TerminalCard({
  session,
  mode = 'preview',
  previewPaused = false,
  mobile,
  touchClipboard = false,
  fit = 'grid',
}: {
  session: TerminalDashboardSession;
  mode?: 'preview' | 'magnified';
  previewPaused?: boolean;
  mobile?: MobileMagnifiedTerminal;
  touchClipboard?: boolean;
  fit?: TerminalCardFit;
}) {
  const key = keyFor(session);
  const dashboardPreview = mode === 'preview',
    magnified = mode === 'magnified',
    // Phone chrome and touch-first desktops (HS2-FRB545, HS2-5DHHPV) put Copy and Paste in the toolbar.
    clipboardActions = magnified && (Boolean(mobile) || touchClipboard);
  const preview = terminalPreviewText(session.scrollback) || 'Terminal is ready.';
  const viewport =
    dashboardPreview && previewPaused ? (
      <div
        class="terminal-tile__preview-placeholder"
        data-component="terminal-preview-placeholder"
        data-project-id={session.projectId}
        data-terminal-id={session.id}
        aria-hidden="true"
      />
    ) : (
      <div
        class={`terminal-viewport${dashboardPreview ? ' terminal-viewport--scaled-preview' : ''}`}
        data-key={`${dashboardPreview ? 'preview' : 'viewport'}:${key}`}
        data-morph-skip
        data-component="terminal-viewport"
        data-project-id={session.projectId}
        data-terminal-id={session.id}
        data-display-mode={dashboardPreview ? 'scaled-preview' : 'interactive'}
        data-mount-policy={dashboardPreview ? 'visible-progressive' : 'immediate'}
        data-grid-policy={dashboardPreview ? 'dashboard-80x24' : undefined}
        data-geometry-ready="false"
        aria-hidden={dashboardPreview ? 'true' : undefined}
        aria-label={dashboardPreview ? undefined : `${session.title ?? session.id} interactive terminal`}
      />
    );
  return (
    <article
      class="terminal-tile"
      data-key={key}
      {...TERMINALS_TARGETS.terminalTile.attrs}
      data-fixed-aspect-terminal-card={mode}
      data-fit={fit}
      data-terminal-key={key}
      data-busy={String(session.busy)}
      data-alive={String(session.alive)}
      data-halted={session.halt ? 'true' : undefined}
      data-magnified={String(magnified)}
      data-mobile-chrome={magnified && mobile ? 'true' : undefined}
      data-keyboard-visible={magnified && mobile ? String(mobile.keyboardVisible) : undefined}
      data-preview-only={String(dashboardPreview)}
      data-preview-paused={String(dashboardPreview && previewPaused)}
      data-action={dashboardPreview ? 'preview-terminal' : undefined}
      tabindex={dashboardPreview ? '0' : undefined}
      aria-label={dashboardPreview ? `Preview ${session.title ?? session.id}` : undefined}
    >
      <div class="terminal-tile__preview">
        <pre>{preview}</pre>
        <div class="terminal-tile__viewport-frame">{viewport}</div>
      </div>
      {magnified && mobile?.keyboardVisible && mobile.keyBar && <TerminalKeyBar {...mobile.keyBar} />}
      {/* App-owned footer slot: it keeps the card's fixed footer band and phone ordering, and tunes the
          Kerf Toolbar inside it only through public toolbar tokens (HS2-XHA5QA). */}
      <div class="terminal-tile__footer">
        <Toolbar
          position="footer"
          label={`${session.title ?? session.id} Terminal`}
          leading={
            <>
              {magnified && mobile && (
                <ToolbarControlGroup single appearance="borderless" content="icon">
                  <button
                    type="button"
                    class="terminal-tile__close"
                    {...TERMINALS_ACTIONS.closeMagnifiedTerminal.attrs}
                    aria-label={`Close ${session.title ?? session.id}`}
                    title="Close"
                  >
                    <LucideIcon size="s" icon={X} name="x" />
                  </button>
                </ToolbarControlGroup>
              )}
              {session.halt ? (
                // A halted AI session (HS2-HJ4D1H) replaces the state dot: it is waiting for the user.
                <span
                  class="terminal-tile__halt"
                  role="img"
                  aria-label={`Stopped: ${session.halt.message}`}
                  title={`Stopped: ${session.halt.message}`}
                >
                  <LucideIcon size="s" icon={TriangleAlert} name="triangle-alert" color={uiColor('danger-on-quiet')} />
                </span>
              ) : (
                <>
                  <TileState state={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'} />
                  {session.aiConnection && (
                    <span
                      class="terminal-tile__ai-connection"
                      data-ai-connection={session.aiConnection}
                      role="img"
                      aria-label={aiConnectionLabel(
                        session.aiConnection,
                        session.ai_connection?.agent,
                        session.tool,
                        session.last_hook_report ?? session.ai_connection,
                        session.mcp_connected,
                      )}
                      title={aiConnectionLabel(
                        session.aiConnection,
                        session.ai_connection?.agent,
                        session.tool,
                        session.last_hook_report ?? session.ai_connection,
                        session.mcp_connected,
                      )}
                    >
                      {session.aiConnection === 'connected' ? (
                        <LucideIcon size="s" icon={Plug} name="plug" color={uiColor('neutral-on-quiet')} />
                      ) : (
                        <LucideIcon size="s" icon={Unplug} name="unplug" color={uiColor('warning-on-quiet')} />
                      )}
                    </span>
                  )}
                </>
              )}
              <button
                type="button"
                class="terminal-tile__identity"
                {...TERMINALS_ACTIONS.openTerminalProject.attrs}
                data-item-id={key}
                aria-label={`Open ${session.title ?? session.id} in ${session.projectName}`}
              >
                {magnified && mobile ? (
                  // Phone toolbar: the terminal's own name leads, with its project as a subtitle, so a narrow
                  // identity still names the terminal instead of truncating inside the project (HS2-8NQRJB).
                  <>
                    <strong>{session.title ?? session.id}</strong>
                    <span class="terminal-tile__identity-project">{session.projectName}</span>
                  </>
                ) : (
                  <strong>
                    {session.projectName}
                    <span aria-hidden="true"> › </span>
                    {session.title ?? session.id}
                  </strong>
                )}
              </button>
              {/* Progress follows the identity in the shrinking leading zone, so the trailing zone holds
                  one unwrappable action group and a narrow card truncates the identity instead. */}
              {session.progress !== undefined && (
                <Text variant="span" size="compact" tone="quiet">
                  {session.progress}%
                </Text>
              )}
            </>
          }
          trailing={
            <>
              {/* Kerf's borderless control group owns the actions' sizing, hover, and focus chrome. */}
              <ToolbarControlGroup
                appearance="borderless"
                content="icon"
                size={magnified && mobile ? 'default' : 'compact'}
                label="Terminal Actions"
              >
                {clipboardActions && (
                  <button
                    type="button"
                    class="terminal-tile__clipboard"
                    {...TERMINALS_ACTIONS.copyTerminalText.attrs}
                    aria-label="Copy Terminal Text"
                    title="Copy terminal text"
                  >
                    <LucideIcon size="s" icon={Copy} name="copy" />
                  </button>
                )}
                {clipboardActions && (
                  <button
                    type="button"
                    class="terminal-tile__clipboard"
                    {...TERMINALS_ACTIONS.pasteTerminalText.attrs}
                    aria-label="Paste"
                    title="Paste"
                  >
                    <LucideIcon size="s" icon={ClipboardPaste} name="clipboard-paste" />
                  </button>
                )}
                <button
                  type="button"
                  class="terminal-tile__menu"
                  {...TERMINALS_ACTIONS.openTerminalContextMenu.attrs}
                  data-item-id={key}
                  aria-label={`More Actions for ${session.title ?? session.id}`}
                  title="More actions"
                >
                  <LucideIcon size="s" icon={Ellipsis} name="ellipsis" />
                </button>
                {/* The phone toolbar leaves this to More actions → Open (and the identity), so the identity keeps
                    room beside Copy and Paste at 390px (HS2-8NQRJB). */}
                {magnified && !mobile && (
                  <button
                    type="button"
                    class="terminal-tile__open"
                    {...TERMINALS_ACTIONS.openTerminalProject.attrs}
                    data-terminal-key={key}
                    aria-label={`Open ${session.title ?? session.id} in Project Terminal Drawer`}
                    title="Open in project terminal drawer"
                  >
                    <LucideIcon size="s" icon={ExternalLink} name="external-link" />
                  </button>
                )}
              </ToolbarControlGroup>
            </>
          }
        />
      </div>
    </article>
  );
}

export function FixedAspectTerminalCard({
  session,
  mode = 'preview',
  mobile,
  touchClipboard,
  fit = 'grid',
}: {
  session: TerminalDashboardSession;
  mode?: 'preview' | 'magnified';
  mobile?: MobileMagnifiedTerminal;
  /** Touch-first desktop: show the Copy and Paste toolbar actions (HS2-5DHHPV). */
  touchClipboard?: boolean;
  /** `grid` (default) sizes from the enclosing grid; `aspect` fills the container's width at 5:3. */
  fit?: TerminalCardFit;
}) {
  return <TerminalCard session={session} mode={mode} mobile={mobile} touchClipboard={touchClipboard} fit={fit} />;
}

/**
 * A live, non-interactive terminal preview that fills its positioned container (HS2-148B5C): a
 * framed 1280×768 scaled viewport over the terminal background, with `fallback` text shown until the
 * live terminal connects. Containers tune it through `--terminal-preview-inset` (frame inset,
 * default `--kui-space-l`) and `--terminal-preview-radius` (frame radius, default
 * `--wa-border-radius-m`); the preview owns the frame, canvas, and fallback toggle. Once its mirrored
 * grid renders, it publishes the grid's own aspect as `--terminal-preview-grid-aspect` on its
 * container (and `data-grid-aspect` on its root), so a container can give the frame that aspect and
 * the grid fills it on both axes (HS2-RBS46R).
 */
export function TerminalPreview({
  projectId,
  terminalId,
  viewportKey,
  fallback = 'Connecting to the live terminal…',
}: {
  projectId: string;
  terminalId: string;
  /** Stable morph key of the live viewport, so a re-render keeps the mounted terminal. */
  viewportKey: string;
  fallback?: string;
}) {
  return (
    <div class="terminal-preview" data-component="terminal-preview">
      <div class="terminal-preview__frame">
        <div
          class="terminal-viewport terminal-viewport--scaled-preview"
          data-key={viewportKey}
          data-morph-skip
          data-component="terminal-viewport"
          data-project-id={projectId}
          data-terminal-id={terminalId}
          data-display-mode="scaled-preview"
          data-geometry-ready="false"
          aria-hidden="true"
        />
      </div>
      <p class="terminal-preview__fallback">{fallback}</p>
    </div>
  );
}

export function TerminalSession({
  session,
  active = true,
  mobile = false,
  focus = false,
}: {
  session: TerminalDashboardSession;
  active?: boolean;
  /** Phone presentation: the scaled xterm is clipped rather than scrolled by its viewport. */
  mobile?: boolean;
  /** Focus-mode presentation: the viewport drops its inset and fills the focused surface. */
  focus?: boolean;
}) {
  return (
    <section
      class="terminal-session"
      data-key={keyFor(session)}
      data-mobile={String(mobile)}
      data-focus={String(focus)}
      data-component="terminal-session"
      data-terminal-key={keyFor(session)}
      hidden={!active}
      aria-label={`${session.title ?? session.id} Terminal`}
    >
      <div
        class="terminal-viewport terminal-viewport--dedicated"
        data-key={`dedicated-viewport:${keyFor(session)}`}
        data-morph-skip
        data-component="terminal-viewport"
        data-display-mode="interactive"
        data-mount-policy="keep-alive"
        data-project-id={session.projectId}
        data-terminal-id={session.id}
        aria-label={`${session.title ?? session.id} interactive terminal`}
      />
    </section>
  );
}

/** The grid can cold-create a dedicated viewport before its drawer session has ever rendered. */
export function createDedicatedTerminalTileViewport(projectId: string, terminalId: string): HTMLElement {
  const element = document.createElement('div');
  element.className = 'terminal-viewport terminal-viewport--dedicated';
  element.dataset.key = `dedicated-viewport:${projectId}:${terminalId}`;
  element.setAttribute('data-morph-skip', '');
  element.dataset.component = 'terminal-viewport';
  element.dataset.projectId = projectId;
  element.dataset.terminalId = terminalId;
  element.dataset.displayMode = 'interactive';
  element.dataset.mountPolicy = 'keep-alive';
  element.dataset.geometryReady = 'false';
  element.inert = true;
  element.setAttribute('aria-hidden', 'true');
  element.setAttribute('aria-label', `${terminalId} interactive terminal`);
  return element;
}

export function WorkspaceGridChatCard({ chat, previewScale = 1 }: { chat: WorkspaceGridChat; previewScale?: number }) {
  const key = chatKeyFor(chat),
    state = chat.busy ? 'Working' : 'Ready',
    scale = Math.max(0, previewScale);
  return (
    <article
      class="terminal-tile workspace-chat-tile"
      data-key={key}
      {...TERMINALS_TARGETS.workspaceChatTile.attrs}
      data-chat-key={key}
      data-project-id={chat.projectId}
      data-chat-id={chat.id}
      data-busy={String(Boolean(chat.busy))}
      data-preview-only="true"
      {...TERMINALS_ACTIONS.openGridAiChat.attrs}
      data-item-id={key}
      tabindex="0"
      aria-label={`Open ${chat.name} in ${chat.projectName}`}
    >
      <div class="terminal-tile__preview workspace-chat-tile__preview">
        <div
          class="workspace-chat-tile__preview-surface"
          data-preview-scale={String(scale)}
          style={`--workspace-chat-preview-scale:${scale};--workspace-chat-preview-natural-width:${WORKSPACE_CHAT_PREVIEW_NATURAL_WIDTH}px;--workspace-chat-preview-natural-height:${WORKSPACE_CHAT_PREVIEW_NATURAL_HEIGHT}px`}
        >
          <span class="workspace-chat-tile__preview-icon">
            <LucideIcon icon={MessageSquare} name="message-square" size={8} color={uiColor('brand-on-quiet')} />
          </span>
          <div>
            <Text variant="span" size="medium" wrap="truncate">
              <strong>{chat.tool} AI Chat</strong>
            </Text>
            <Text variant="p" size="medium" tone="quiet" wrap="anywhere" maxLines={3} flush>
              {chat.summary || `Open ${chat.name} to continue the conversation.`}
            </Text>
          </div>
        </div>
      </div>
      <div class="terminal-tile__footer">
        <Toolbar
          position="footer"
          label={`${chat.name} AI Chat`}
          leading={
            <>
              <TileState state={state} />
              <button
                type="button"
                class="terminal-tile__identity"
                {...TERMINALS_ACTIONS.openGridAiChat.attrs}
                data-item-id={key}
                data-project-id={chat.projectId}
                data-chat-id={chat.id}
                aria-label={`Open ${chat.name} in ${chat.projectName}`}
              >
                <strong>
                  {chat.projectName}
                  <span aria-hidden="true"> › </span>
                  {chat.name}
                </strong>
              </button>
            </>
          }
        />
      </div>
    </article>
  );
}

type GridItem = { kind: 'terminal'; session: TerminalDashboardSession } | { kind: 'chat'; chat: WorkspaceGridChat };

function Grid({
  sessions,
  chats = [],
  itemOrder = [],
  layout,
  magnifiedKey,
}: {
  sessions: TerminalDashboardSession[];
  chats?: WorkspaceGridChat[];
  itemOrder?: string[];
  layout: ReturnType<typeof terminalGridLayout>;
  magnifiedKey?: string;
}) {
  const style = `--terminal-tile-width:${layout.tileWidth}px;--terminal-tile-height:${layout.tileHeight}px;--terminal-grid-fit:${layout.fit}`,
    chatPreviewScale = terminalPhysicalScale(
      WORKSPACE_CHAT_PREVIEW_NATURAL_WIDTH,
      WORKSPACE_CHAT_PREVIEW_NATURAL_HEIGHT,
      Math.max(1, layout.tileWidth - TERMINAL_TILE_HORIZONTAL_CHROME),
      Math.max(1, layout.tileHeight - TERMINAL_TILE_VERTICAL_CHROME),
    );
  const rank = new Map(itemOrder.map((id, index) => [id, index])),
    items: GridItem[] = [
      ...sessions.map((session) => ({ kind: 'terminal' as const, session })),
      ...chats.map((chat) => ({ kind: 'chat' as const, chat })),
    ];
  if (itemOrder.length)
    items.sort(
      (left, right) =>
        (rank.get(left.kind === 'terminal' ? left.session.id : left.chat.id) ?? Number.MAX_SAFE_INTEGER) -
        (rank.get(right.kind === 'terminal' ? right.session.id : right.chat.id) ?? Number.MAX_SAFE_INTEGER),
    );
  return (
    <div
      class="terminal-grid"
      data-component="terminal-grid"
      data-basis={layout.basis}
      data-fit={String(layout.fit)}
      style={style}
    >
      {items.map((item) =>
        item.kind === 'terminal' ? (
          <TerminalCard session={item.session} previewPaused={keyFor(item.session) === magnifiedKey} />
        ) : (
          <WorkspaceGridChatCard chat={item.chat} previewScale={chatPreviewScale} />
        ),
      )}
    </div>
  );
}

/**
 * The dashboard's empty, loading, and message copy (HS2-1Q6GER): a centered Kerf List of Text with no
 * icon slot, so a short (about 100px) drawer grid never clips the title as EmptyState's geometry did.
 * Loading and message copy is a polite status region.
 */
function DashboardFeedback({
  title,
  status = false,
  children,
}: {
  title?: string;
  status?: boolean;
  children: string;
}) {
  // The filled List gathers the title and copy in its middle with its gap (KF-3ZJ9PF, Kerf beta.106).
  return (
    <List
      fill
      gap="2xs"
      hAlign="center"
      vAlign="middle"
      rootAttributes={{ 'data-terminal-dashboard-feedback': 'true', ...(status ? { role: 'status' as const } : {}) }}
    >
      {title ? (
        <Text variant="p" align="center" flush>
          <strong>{title}</strong>
        </Text>
      ) : undefined}
      <Text variant="p" tone="quiet" align="center" flush>
        {children}
      </Text>
    </List>
  );
}

export function TerminalDashboard({
  groups,
  width,
  height,
  fitAcross,
  fitHigh,
  fullscreen = false,
  grouping = 'flow',
  layoutMode = 'responsive',
  magnifiedKey,
  mobileMagnified,
  touchClipboard = false,
  hiddenKeys = [],
  visibleTypes = TERMINAL_VISIBILITY_TYPES,
  loading = false,
  message = '',
  contextMenu,
  contextMenuActions = ALL_CONTEXT_MENU_ACTIONS,
}: TerminalDashboardProps) {
  const hidden = new Set(hiddenKeys);
  const visibleGroups = groups
    .map((group) => ({
      ...group,
      sessions: group.sessions.filter(
        (session) => visibleTypes.includes(session.kind === 'ai' ? 'ai' : 'shell') && !hidden.has(keyFor(session)),
      ),
      chats: (group.chats ?? []).filter((chat) => visibleTypes.includes('chat') && !hidden.has(chatKeyFor(chat))),
    }))
    .filter((group) => group.sessions.length + group.chats.length > 0);
  const sessions = visibleGroups.flatMap((group) => group.sessions);
  const chats = visibleGroups.flatMap((group) => group.chats);
  const menuSession = contextMenu && sessions.find((session) => keyFor(session) === contextMenu.key);
  const layout =
    layoutMode === 'drawer'
      ? terminalDrawerGridLayout(width, height, fitHigh)
      : terminalGridLayout(width, height, fitAcross, fitHigh);
  const magnified = groups.flatMap((group) => group.sessions).find((session) => keyFor(session) === magnifiedKey);
  // One menu signal serves every grid; only the grid that shows the targeted tile renders it.
  const menuTargetsThisGrid =
    contextMenu !== undefined &&
    (sessions.some((session) => keyFor(session) === contextMenu.key) ||
      chats.some((chat) => chatKeyFor(chat) === contextMenu.key));
  return (
    <section
      class="terminal-dashboard"
      data-component="terminal-dashboard"
      data-layout-mode={layoutMode}
      data-basis={layout.basis}
      data-fit={String(layout.fit)}
      aria-label="Workspace Grid"
    >
      <div
        class="terminal-dashboard__content"
        data-terminal-grid-measure="true"
        data-ticket-scroll-owner="terminal-grid"
      >
        {/* A refresh keeps the last snapshot's tiles on screen: replacing them with a loading state
            would unmount every live terminal and remount it once the refresh lands (HS2-AMNNBR). */}
        {loading && sessions.length + chats.length === 0 ? (
          <DashboardFeedback status>Loading workspace items…</DashboardFeedback>
        ) : message ? (
          <DashboardFeedback status>{message}</DashboardFeedback>
        ) : sessions.length + chats.length === 0 ? (
          <DashboardFeedback title="Nothing open yet">
            Open a project terminal or AI chat to add it to this grid.
          </DashboardFeedback>
        ) : grouping === 'flow' ? (
          <Grid
            sessions={sessions}
            chats={chats}
            itemOrder={visibleGroups.length === 1 ? visibleGroups[0].itemOrder : undefined}
            layout={layout}
            magnifiedKey={magnifiedKey}
          />
        ) : (
          visibleGroups.map((group) => (
            <section class="terminal-dashboard__project" data-key={group.projectId} data-project-id={group.projectId}>
              <ListHeader
                label={group.projectName}
                count={group.sessions.length + group.chats.length}
                countLabel={`${group.sessions.length + group.chats.length} workspace items`}
                density="compact"
              />
              <Grid
                sessions={group.sessions}
                chats={group.chats}
                itemOrder={group.itemOrder}
                layout={layout}
                magnifiedKey={magnifiedKey}
              />
            </section>
          ))
        )}
      </div>
      <div class="terminal-dashboard__zoom">
        <FloatingToolbar
          label={layoutMode === 'drawer' ? 'Workspace Tile Zoom' : 'Workspace Grid Controls'}
          position="bottom-end"
        >
          {layoutMode !== 'drawer' && (
            <ToolbarControlGroup single>
              <button
                type="button"
                {...TERMINALS_ACTIONS.toggleTerminalGridFullscreen.attrs}
                aria-label={fullscreen ? 'Exit workspace grid full screen' : 'Enter workspace grid full screen'}
                aria-pressed={String(fullscreen)}
                title={fullscreen ? 'Exit full screen' : 'Full screen'}
              >
                <LucideIcon icon={fullscreen ? Minimize2 : Maximize2} name={fullscreen ? 'minimize-2' : 'maximize-2'} />
              </button>
            </ToolbarControlGroup>
          )}
          <ToolbarControlGroup>
            <button
              type="button"
              {...TERMINALS_ACTIONS.zoomTerminalGrid.attrs}
              data-zoom-direction="out"
              disabled={layout.fit >= layout.max}
              aria-label={`Zoom Out, Fit More Items ${layout.basis}`}
              title="Zoom out"
            >
              <LucideIcon icon={Minus} name="minus" />
            </button>
            <button
              type="button"
              {...TERMINALS_ACTIONS.zoomTerminalGrid.attrs}
              data-zoom-direction="in"
              disabled={layout.fit <= 1}
              aria-label={`Zoom In, Fit Fewer Items ${layout.basis}`}
              title="Zoom in"
            >
              <LucideIcon icon={Plus} name="plus" />
            </button>
          </ToolbarControlGroup>
        </FloatingToolbar>
      </div>
      {magnified && (
        // A manual popover, opened by `wireTopLayerOverlays`, so the magnified terminal sits in the top
        // layer above the Workbench's rails, drawer clip, and every other surface (HS2-Z9PQSC).
        <div
          class="terminal-dashboard__magnified"
          popover="manual"
          data-top-layer-overlay
          role="dialog"
          aria-modal="true"
          aria-label={`Magnified ${magnified.title ?? magnified.id}`}
          {...TERMINALS_ACTIONS.dismissMagnifiedTerminal.attrs}
          data-mobile={String(Boolean(mobileMagnified))}
          data-keyboard-visible={mobileMagnified ? String(mobileMagnified.keyboardVisible) : undefined}
          style={
            mobileMagnified
              ? `--terminal-magnified-left:${mobileMagnified.viewport.left}px;--terminal-magnified-top:${mobileMagnified.viewport.top}px;--terminal-magnified-width:${mobileMagnified.viewport.width}px;--terminal-magnified-height:${mobileMagnified.viewport.height}px`
              : undefined
          }
        >
          <FixedAspectTerminalCard
            session={magnified}
            mode="magnified"
            mobile={mobileMagnified}
            touchClipboard={touchClipboard}
          />
        </div>
      )}
      {contextMenu && menuTargetsThisGrid && (
        <PopupMenu
          context
          anchor={contextMenuAnchor(contextMenu.x, contextMenu.y)}
          label="Terminal Actions"
          rootAttributes={{ 'data-context-menu': 'terminal', 'data-terminal-key': contextMenu.key }}
          items={[
            ...(contextMenuActions.includes('clear-halt') && menuSession?.halt
              ? [
                  {
                    label: 'Clear stopped state',
                    action: TERMINALS_ACTIONS.clearTerminalHalt.value,
                    icon: <LucideIcon icon={CircleCheck} name="circle-check" />,
                    attributes: { 'data-item-id': contextMenu.key },
                  },
                ]
              : []),
            ...(contextMenuActions.includes('open')
              ? [
                  {
                    label: 'Open',
                    action: 'open-terminal-project',
                    icon: <LucideIcon icon={ExternalLink} name="external-link" />,
                    attributes: { 'data-item-id': contextMenu.key },
                  },
                ]
              : []),
            ...(contextMenuActions.includes('hide')
              ? [
                  {
                    label: 'Hide Terminal',
                    action: 'hide-dashboard-terminal',
                    icon: <LucideIcon icon={EyeOff} name="eye-off" />,
                    attributes: { 'data-item-id': contextMenu.key },
                  },
                ]
              : []),
          ]}
        />
      )}
    </section>
  );
}
