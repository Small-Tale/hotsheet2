import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './ai-conversation.css';
import '@kerfjs/ui/surface-scaffold/register';
import '@awesome.me/webawesome/dist/components/button/button.js';

import { Badge } from '@kerfjs/ui/badge';
import { Chip } from '@kerfjs/ui/chip';
import { Composer } from '@kerfjs/ui/composer';
import { ContentItem } from '@kerfjs/ui/content-item';
import { flex, px, rem, remify, uiColor } from '@kerfjs/ui/css-values';
import { deviceClass } from '@kerfjs/ui/device-class';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { computed, type ReadonlySignal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import {
  Activity,
  ArrowUp,
  Bot,
  Brain,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Copy,
  Download,
  Hourglass,
  Image,
  MessageSquare,
  Paperclip,
  Square,
  X,
} from 'lucide';

import {
  type ConversationActivity,
  conversationHeaderSummary,
  type ConversationMessage,
  conversationTimeline,
  type ConversationTurnProgress,
  type ConversationUsage,
  formatConversationCost,
  formatConversationTokens,
} from '../ai-conversation';
import { activityGroupExpanded } from '../conversation-activity-disclosure';
import { conversationSelectionHint } from '../conversation-selection';
import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';
import type { PermissionItem } from '../permission-notifications';
import { AIContentFeedback, AIContentLabel } from './ai-content-label';
import { MarkdownPreview } from './markdown-preview';
import { PermissionRequestCard } from './permission-request-card';
import { providerModelEffortEntries } from './provider-model-effort-menu';

export interface AIConversationProps {
  open: boolean;
  tool: string;
  sessionId?: string;
  selectionId?: string;
  selectedMessageIds?: readonly string[];
  messages: ConversationMessage[];
  /** Messages a reopened non-contiguous saved selection left out before each message (HS2-BZB93V). */
  skippedBefore?: Readonly<Record<string, number>>;
  draft: string;
  busy: boolean;
  progress?: string;
  /** The running turn's elapsed time, last activity, and in-flight tool or stuck state (HS2-XV1VCR). */
  turnProgress?: ConversationTurnProgress;
  interruptible: boolean;
  permissions?: PermissionItem[];
  activity?: ConversationActivity[];
  totalUsage?: ConversationUsage;
  error?: string;
  feedbackAvailable?: boolean;
  presentation?: 'dialog' | 'embedded';
  /**
   * The embedded conversation's measured height in CSS pixels (the drawer content it fills). A short
   * chat compacts its floating form so the transcript keeps room (HS2-SZZGC6); see
   * `embeddedComposerLayout`. Omitted (or in the dialog) the form keeps its full layout.
   */
  availableHeight?: number;
  foreground?: SafeHtml;
  providerId?: string;
  providers?: readonly { id: string; label: string }[];
  canChangeProvider?: boolean;
  model?: string;
  effort?: string;
  models?: readonly { id: string; label: string }[];
  efforts?: readonly string[];
  canChangeModel?: boolean;
  canChangeEffort?: boolean;
  /** The chat's active permission mode and the tool's declared modes (HS2-NHRDMW). */
  permissionMode?: string;
  permissionModes?: readonly { id: string; label: string; description?: string; warning?: string }[];
  canChangePermissionMode?: boolean;
  readOnly?: boolean;
  readOnlyContext?: 'saved' | 'preview';
  savedSource?: string;
  /** False on a remote client: host-only controls render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}

export function isConversationSurfaceLifecycleEvent(event: Pick<Event, 'target'>, surface: Element) {
  return event.target === surface;
}

function UsageLine({ usage }: { usage: ConversationUsage }) {
  const total = usage.tokensIn + usage.tokensOut;
  return (
    <p
      class="ai-conversation__usage"
      aria-label={`Turn usage: ${usage.tokensIn} input tokens, ${usage.tokensOut} output tokens, ${formatConversationCost(usage.costUsd)}`}
    >
      <span>{formatConversationTokens(total)} tokens</span>
      <span>{formatConversationCost(usage.costUsd)}</span>
      {usage.model && <span title={usage.model}>{usage.model}</span>}
    </p>
  );
}

export function ConversationMessages({
  tool,
  messages,
  feedbackAvailable = false,
  selectedIds,
  selectionAction,
}: {
  tool: string;
  messages: readonly ConversationMessage[];
  feedbackAvailable?: boolean;
  selectedIds?: ReadonlySet<string>;
  selectionAction?: string;
}) {
  return (
    <>
      {messages.map((message) => (
        // Kerf ContentItem chat bubbles (KF-M3HJK8, HS2-P16J04): the person's messages sit at the end on the
        // brand-loud fill, responses at the start; a selectable transcript makes each one a row of its
        // multi-select grid, announced with aria-selected (HS2-S9W63Q).
        <ContentItem
          bubble={message.role === 'user' ? 'end' : 'start'}
          appearance={message.status === 'failed' ? 'danger' : message.role === 'user' ? 'brand-loud' : 'neutral'}
          className={`ai-conversation__message ai-conversation__message--${message.role}`}
          ariaLabel={message.role === 'assistant' ? `AI-generated response by ${tool}` : undefined}
          {...(selectionAction
            ? {
                interactive: true as const,
                action: selectionAction,
                itemId: message.id,
                selectionMode: 'multiple' as const,
                selected: Boolean(selectedIds?.has(message.id)),
                rootAttributes: { 'data-message-id': message.id, 'data-status': message.status },
              }
            : { rootAttributes: { 'data-message-id': message.id, 'data-status': message.status } })}
        >
          {message.role === 'user' && <strong class="ai-conversation__message-author">You</strong>}
          {message.content ? (
            <MarkdownPreview
              source={message.content}
              tone={message.role === 'user' ? 'inverse' : 'inherit'}
              density="compact"
            />
          ) : message.role === 'assistant' && message.status === 'streaming' ? (
            // A thinking turn shows a labeled Kerf spinner inside its bubble, never an empty pill (HS2-77KJKR).
            <span class="ai-conversation__awaiting" role="status">
              <LoadingSpinner label="Awaiting response" size="s" />
            </span>
          ) : (
            <p class="ai-conversation__empty-message" aria-label="Empty response">
              {' '}
            </p>
          )}
          {message.files && message.files.length > 0 && (
            <ul class="ai-conversation__files" aria-label="Referenced files">
              {message.files.map((file) => (
                <li>
                  {/* Kerf link Chip (KF-H658D6) inherits the bubble foreground on both tones (HS2-94T958). */}
                  <Chip
                    href={file.url}
                    download={file.filename}
                    tone="inherit"
                    size="compact"
                    truncate
                    icon={
                      <LucideIcon
                        icon={file.kind === 'media' ? Image : Paperclip}
                        name={file.kind === 'media' ? 'image' : 'paperclip'}
                      />
                    }
                  >
                    {file.filename}
                  </Chip>
                </li>
              ))}
            </ul>
          )}
          {(message.usage || (message.role === 'assistant' && feedbackAvailable && message.status === 'completed')) && (
            <footer class="ai-conversation__message-meta">
              {message.usage && <UsageLine usage={message.usage} />}
              {message.role === 'assistant' && feedbackAvailable && message.status === 'completed' && (
                <AIContentFeedback tool={tool} feedbackTarget={`conversation:${message.id}`} />
              )}
            </footer>
          )}
        </ContentItem>
      ))}
    </>
  );
}

function ConversationActivityGroup({
  activity,
  feedbackAvailable,
  selected = false,
  connectionId,
}: {
  activity: readonly ConversationActivity[];
  feedbackAvailable: boolean;
  /** Between two selected messages: the only state that outlines the group (HS2-72GEX0). */
  selected?: boolean;
  connectionId?: string;
}) {
  // Collapsed by default to the latest item; the group keeps its first activity id while items
  // append, so a new item arriving while collapsed replaces the visible line (HS2-Y3SGH3).
  const groupId = activity[0].id,
    expanded = activityGroupExpanded(connectionId, groupId),
    visible = expanded ? activity : activity.slice(-1),
    listId = `ai-conversation-activity-${connectionId ?? 'conversation'}-${groupId}`,
    count = activity.length;
  return (
    // A borderless, transparent Kerf ContentItem like the sidebar's activity notes; it frames only
    // while it lies inside the selected message range (HS2-ADM62E, HS2-72GEX0).
    <ContentItem
      appearance="transparent"
      frame={selected ? 'framed' : 'none'}
      className="ai-conversation__activity"
      ariaLabel="AI-generated activity summaries; may contain errors"
      rootAttributes={{
        'data-selected-range': String(selected),
        'data-activity-group': groupId,
        'data-expanded': String(expanded),
      }}
    >
      {/* Clicking anywhere in the group toggles it; the header button is its keyboard and
          assistive-technology control. */}
      <div
        class="ai-conversation__activity-body"
        {...COMMANDS_AND_AI_ACTIONS.toggleConversationActivity.attrs}
        data-activity-group={groupId}
      >
        <header>
          <button
            type="button"
            class="ai-conversation__activity-toggle"
            aria-expanded={String(expanded)}
            aria-controls={listId}
            aria-label={
              expanded
                ? `Collapse activity to the latest of ${count} item${count === 1 ? '' : 's'}`
                : `Show all ${count} activity item${count === 1 ? '' : 's'}`
            }
          >
            <LucideIcon icon={Activity} name="activity" size={14.4} />
            <strong>Activity</strong>
            {count > 1 && <span class="ai-conversation__activity-count">{count}</span>}
            <LucideIcon
              icon={expanded ? ChevronDown : ChevronRight}
              name={expanded ? 'chevron-down' : 'chevron-right'}
              size={14.4}
            />
          </button>
          <span class="ai-conversation__activity-disclosure">
            <AIContentLabel mayContainErrors />
          </span>
        </header>
        <ol class="ai-conversation__activity-list" id={listId}>
          {visible.map((item) => (
            <li class="ai-conversation__activity-item" data-activity-id={item.id} data-importance={item.importance}>
              <MarkdownPreview source={item.summary} density="flush" />
              {feedbackAvailable && (
                <span class="ai-conversation__activity-feedback">
                  <AIContentFeedback tool={item.tool} feedbackTarget={`activity:${item.id}`} />
                </span>
              )}
            </li>
          ))}
        </ol>
      </div>
    </ContentItem>
  );
}

/**
 * Timeline positions of activity groups inside the selection: a group belongs to it only when the
 * nearest message before it and the nearest message after it are both selected, so the gaps of a
 * non-contiguous selection never outline skipped activity (HS2-72GEX0, HS2-WV88W3).
 */
export function selectedActivityGroups(
  timeline: ReturnType<typeof conversationTimeline>,
  selectedIds: ReadonlySet<string>,
): Set<number> {
  const inside = new Set<number>();
  let previousSelected = false,
    pending: number[] = [];
  timeline.forEach((group, index) => {
    if (group.kind !== 'message') {
      if (previousSelected) pending.push(index);
      return;
    }
    const selected = selectedIds.has(group.message.id);
    if (selected) for (const position of pending) inside.add(position);
    pending = [];
    previousSelected = selected;
  });
  return inside;
}

let handset: ReadonlySignal<boolean> | undefined;
/** Phones inset the floating composer 8px instead of 16px; read only the handset flag. */
function handsetDevice(): ReadonlySignal<boolean> {
  if (!handset) {
    const device = deviceClass();
    handset = computed(() => device.value.handset);
  }
  return handset;
}

/**
 * The conversation's toolbar controls: the provider/model/effort selector, Save, and Stop. The
 * dialog renders them in its header toolbar; the bottom drawer renders them in its tab toolbar,
 * so the embedded presentation carries no title bar or settings row (HS2-DMH8MX, HS2-59F7WK).
 * Every control dispatches the same delegated actions `wireAiConversation` handles.
 */
export function AIConversationToolbar({
  tool,
  messages,
  busy,
  interruptible,
  presentation = 'dialog',
  providerId,
  providers = [],
  canChangeProvider = false,
  model,
  effort,
  models = [],
  efforts = [],
  canChangeModel = false,
  canChangeEffort = false,
  permissionMode,
  permissionModes = [],
  canChangePermissionMode = false,
  readOnly = false,
  readOnlyContext = 'saved',
  hostActionsAvailable = hostActionsAvailableHere(),
}: AIConversationProps) {
  const previewOnly = readOnly && readOnlyContext === 'preview',
    // The drawer's tab toolbar uses compact borderless groups beside its own controls, and its
    // menu opens upward from the bottom drawer; the dialog header keeps contained default groups
    // and opens the menu downward.
    embedded = presentation === 'embedded',
    groupStyle = embedded ? ({ appearance: 'borderless', size: 'compact' } as const) : {};
  const saveAction = previewOnly ? null : (
    <button
      type="button"
      {...COMMANDS_AND_AI_ACTIONS.saveConversation.attrs}
      disabled={busy || messages.length === 0 || !hostActionsAvailable}
      aria-label="Save Conversation"
      title={
        hostOnlyDisabledReason('conversation-export', hostActionsAvailable) ??
        (busy ? 'Wait for the active response before saving' : 'Save conversation')
      }
    >
      <LucideIcon icon={Download} name="download" />
    </button>
  );
  const stopAction =
    busy && interruptible ? (
      <button
        type="button"
        {...COMMANDS_AND_AI_ACTIONS.stopConversation.attrs}
        aria-label={`Stop ${tool}`}
        title={`Stop ${tool}`}
      >
        <LucideIcon icon={Square} name="square" />
      </button>
    ) : null;
  const activeModel = models.find((item) => item.id === model),
    currentModelLabel = activeModel?.label ?? model ?? '',
    customModel = model && !activeModel ? model : undefined,
    currentEffort = effort ?? (efforts.length ? efforts[0] : undefined);
  const providerChoices = providers.filter((item) => item.id !== providerId),
    providerChangeable = canChangeProvider && providerChoices.length > 0,
    currentProviderLabel = providers.find((item) => item.id === providerId)?.label ?? tool,
    permissionChangeable = canChangePermissionMode && permissionModes.length > 0;
  const sessionControls =
    !readOnly &&
    (providerChangeable ||
      (canChangeModel && models.length) ||
      (canChangeEffort && efforts.length) ||
      permissionChangeable) ? (
      <>
        {/* The current-model label and its popup trigger group are siblings in the host toolbar zone,
            so the Kerf group keeps its cataloged Toolbar parent. */}
        <span class="ai-conversation__model" data-component="conversation-model-control">
          <LucideIcon icon={Brain} name="brain" color={uiColor('neutral-on-quiet')} />
          {canChangeModel && currentModelLabel && (
            <span class="ai-conversation__model-name" title={model}>
              {currentModelLabel}
            </span>
          )}
          {canChangeEffort && currentEffort && (
            <Badge
              className="ai-conversation__model-effort"
              tone="neutral"
              size="compact"
              label={`Effort ${currentEffort}`}
            >
              {currentEffort}
            </Badge>
          )}
        </span>
        <ToolbarControlGroup single nestedDropdown {...groupStyle}>
          <PopupMenu
            label={
              permissionChangeable
                ? 'Change Provider, Model, Effort, and Permissions'
                : 'Change Provider, Model, and Effort'
            }
            icon={<LucideIcon icon={ChevronDown} name="chevron-down" />}
            caret={false}
            placement={embedded ? 'top-end' : 'bottom-end'}
            rootAttributes={{ 'data-conversation-model-menu': 'true' }}
            items={providerModelEffortEntries({
              actions: {
                provider: 'select-conversation-provider',
                model: 'select-conversation-model',
                effort: 'select-conversation-effort',
                manualModel: 'open-conversation-manual-model',
                permissionMode: 'select-conversation-permission-mode',
              },
              providers: providerChangeable
                ? { choices: providers, currentId: providerId, currentLabel: currentProviderLabel }
                : undefined,
              model:
                canChangeModel && models.length > 0
                  ? { choices: models, currentId: activeModel?.id, currentLabel: currentModelLabel, customModel }
                  : undefined,
              effort: canChangeEffort && efforts.length > 0 ? { efforts, current: currentEffort } : undefined,
              permission: permissionChangeable ? { modes: permissionModes, current: permissionMode } : undefined,
            })}
          />
        </ToolbarControlGroup>
      </>
    ) : null;
  return (
    <>
      {sessionControls}
      {(saveAction || stopAction) && (
        <ToolbarControlGroup label={`${tool} Conversation Actions`} {...groupStyle}>
          <>
            {saveAction}
            {stopAction}
          </>
        </ToolbarControlGroup>
      )}
    </>
  );
}

/**
 * Non-message content between messages (activity groups, skipped-message dividers). Inside the
 * multi-select grid every child must be a row, so it becomes a non-selectable row with one cell;
 * outside the grid it renders as is (HS2-S9W63Q). Kerf has no non-selectable row yet (KF-RAEXQ7).
 */
function TranscriptRow({ grid, children }: { grid: boolean; children: SafeHtml }) {
  if (!grid) return children;
  return (
    <div role="row" class="ai-conversation__row">
      <div role="gridcell">{children}</div>
    </div>
  );
}

/**
 * Below this chat height the chat is compact: the stuck banner takes its one-line form, the
 * selection hint row steps aside while a turn's progress shows, and the draft grows to 3 rows.
 */
export const SHORT_CHAT_HEIGHT = 480;
/** Below this chat height (a default phone or desktop drawer) the draft grows to 2 rows. */
export const VERY_SHORT_CHAT_HEIGHT = 320;

/**
 * How large the embedded conversation's floating form may grow (HS2-SZZGC6). The form floats over
 * the transcript, so in a short drawer a full may-be-stuck banner plus a six-row draft would cover
 * every message. A short chat is therefore compact: the banner shows its one-line form, the
 * "Click to select" hint row above the transcript steps aside while a turn's progress line shows
 * (it returns when the turn ends, and the selection bar still appears once messages are selected),
 * and the draft's growth is capped (it scrolls inside the Composer beyond that). A tall chat or the
 * dialog keeps the full layout.
 */
export function embeddedComposerLayout(availableHeight: number | undefined): {
  maxRows: number;
  compact: boolean;
} {
  if (availableHeight === undefined || !Number.isFinite(availableHeight) || availableHeight >= SHORT_CHAT_HEIGHT)
    return { maxRows: 6, compact: false };
  return { maxRows: availableHeight < VERY_SHORT_CHAT_HEIGHT ? 2 : 3, compact: true };
}

/**
 * The running turn's progress beside the composer (HS2-XV1VCR): a spinner line naming what the
 * turn is doing and for how long, refreshed by the app's local render clock. After a bounded
 * silence it becomes a warning banner with the Stop action. The ticking line is not a live
 * region, so assistive technology is not interrupted every second; the stuck banner is a polite
 * status whose title changes at most once a minute. `compact` (a short drawer chat, HS2-SZZGC6)
 * gives the banner its one-line short form, still with the Stop action.
 */
export function ConversationTurnProgressLine({
  progress,
  tool,
  interruptible,
  compact = false,
}: {
  progress: ConversationTurnProgress;
  tool: string;
  interruptible: boolean;
  compact?: boolean;
}) {
  if (progress.state === 'stuck')
    return (
      <div class="ai-conversation__turn-progress" data-state="stuck" data-density={compact ? 'compact' : undefined}>
        <StateBanner
          title={compact ? progress.short : progress.label}
          detail={compact ? undefined : progress.detail}
          tone="warning"
          urgency="status"
          copyLayout="stacked"
          icon={<LucideIcon icon={Hourglass} name="hourglass" />}
          action={
            interruptible ? (
              <button
                type="button"
                {...COMMANDS_AND_AI_ACTIONS.stopConversation.attrs}
                aria-label={`Stop ${tool}`}
                title={`Stop ${tool}`}
              >
                Stop
              </button>
            ) : undefined
          }
        />
      </div>
    );
  return (
    <p class="ai-conversation__turn-progress" data-state={progress.state}>
      <LoadingSpinner size="s" />
      <span>{progress.label}</span>
    </p>
  );
}

function SkippedMessages({ count }: { count?: number }) {
  if (!count || count < 1) return <></>;
  return (
    <p class="ai-conversation__skipped" data-skipped-count={String(count)}>
      {`${count} message${count === 1 ? '' : 's'} skipped`}
    </p>
  );
}

export function AIConversation(props: AIConversationProps) {
  const {
    open,
    tool,
    selectionId,
    selectedMessageIds = [],
    messages,
    skippedBefore = {},
    draft,
    busy,
    progress,
    turnProgress,
    interruptible,
    permissions = [],
    activity = [],
    totalUsage,
    error,
    feedbackAvailable = false,
    presentation = 'dialog',
    availableHeight,
    foreground,
    model,
    effort,
    readOnly = false,
    readOnlyContext = 'saved',
    savedSource,
  } = props;
  const previewOnly = readOnly && readOnlyContext === 'preview',
    previewConfiguration = [model && `Model ${model}`, effort && `Effort ${effort}`].filter(Boolean).join(' · '),
    summary = previewOnly
      ? 'Read-only preview'
      : readOnly
        ? 'Saved read-only transcript'
        : busy
          ? (turnProgress?.short ?? `${tool} is working`)
          : messages.length === 0
            ? 'Ready for your first message'
            : // Totals sit inline with the count instead of in a separate header pill (HS2-P415F0).
              conversationHeaderSummary(messages.length, totalUsage);
  const inlinePermissions = presentation === 'dialog' && foreground ? [] : permissions,
    selectedIds = new Set(selectedMessageIds),
    selectionEnabled = Boolean(selectionId) && messages.length > 0,
    selectedCount = selectedIds.size;
  // The selection bar is its own row above the transcript scroller, never floating over messages
  // (HS2-C4RTVS); the dialog's fill-last body and the drawer's flex column give it its height.
  const formLayout = embeddedComposerLayout(presentation === 'embedded' ? availableHeight : undefined);
  // While a turn's progress shows in a compact (short) drawer chat, the hint row steps aside so the
  // transcript keeps its room; it returns when the turn ends (HS2-SZZGC6).
  const hintStepsAside = formLayout.compact && selectedCount === 0 && busy && Boolean(turnProgress) && !readOnly;
  const selectionStatus =
    selectionEnabled && !hintStepsAside ? (
      <div class="ai-conversation__selection" data-active={String(selectedCount > 0)}>
        <span>
          {selectedCount
            ? `${selectedCount} message${selectedCount === 1 ? '' : 's'} selected`
            : conversationSelectionHint()}
        </span>
        {selectedCount > 0 && (
          <span class="ai-conversation__selection-actions">
            <wa-button
              appearance="plain"
              size="small"
              {...COMMANDS_AND_AI_ACTIONS.copyConversationSelection.attrs}
              aria-label="Copy Selected Messages"
              title="Copy selected messages"
            >
              <LucideIcon size={14.4} icon={Copy} name="copy" />
            </wa-button>
            <wa-button
              appearance="plain"
              size="small"
              {...COMMANDS_AND_AI_ACTIONS.clearConversationSelection.attrs}
              aria-label="Clear Message Selection"
              title="Clear message selection"
            >
              <LucideIcon size={14.4} icon={X} name="x" />
            </wa-button>
          </span>
        )}
      </div>
    ) : null;
  const timeline = conversationTimeline(messages, activity),
    selectedActivity = selectedActivityGroups(timeline, selectedIds),
    timelineRows = (
      <>
        {timeline.map((group, index) =>
          group.kind === 'message' ? (
            <>
              {Boolean(skippedBefore[group.message.id]) && (
                <TranscriptRow grid={selectionEnabled}>
                  <SkippedMessages count={skippedBefore[group.message.id]} />
                </TranscriptRow>
              )}
              <ConversationMessages
                tool={tool}
                messages={[group.message]}
                feedbackAvailable={feedbackAvailable}
                selectedIds={selectedIds}
                selectionAction={selectionEnabled ? 'pick-conversation-message' : undefined}
              />
            </>
          ) : (
            <TranscriptRow grid={selectionEnabled}>
              <ConversationActivityGroup
                activity={group.activity}
                feedbackAvailable={feedbackAvailable}
                selected={selectedActivity.has(index)}
                connectionId={selectionId}
              />
            </TranscriptRow>
          ),
        )}
      </>
    ),
    transcript = (
      // Kerf List is the transcript scroller: its block-end scrollClearance lets messages scroll
      // beneath the floating Composer while the last one still rests above it. The dialog's
      // DialogSurface footer publishes that clearance (KF-913AT2, HS2-HVZVEA); in the drawer the
      // measured composer form does (HS2-W0KJVQ). The app-owned content column keeps its own inset.
      <List
        scrollable
        scrollClearance="block-end"
        flex={presentation === 'embedded' ? flex(1, 1, px(0)) : undefined}
        className="ai-conversation__transcript"
        rootAttributes={{ role: 'region', 'aria-label': 'Conversation Transcript', 'aria-live': 'polite' }}
      >
        <div class="ai-conversation__transcript-content">
          {messages.length === 0 && (
            <EmptyState
              className="ai-conversation__empty"
              icon={<LucideIcon size={28} icon={MessageSquare} name="message-square" />}
              title={readOnly ? 'No Conversation Messages Yet' : 'Start a Conversation'}
              detail={
                readOnly
                  ? 'This chat has not received any messages.'
                  : `Ask ${tool} about this project or send it a task.`
              }
            />
          )}
          {selectionEnabled ? (
            // While messages are selectable the timeline is Kerf's labeled multi-select grid, so
            // assistive technology announces it as one multi-selectable owner (HS2-S9W63Q).
            <List selectionMode="multiple" ariaLabel="Conversation Messages" gap={rem(0.8)}>
              {timelineRows}
            </List>
          ) : (
            timelineRows
          )}
          {inlinePermissions.map((item) => (
            <div data-permission-key={item.key}>
              <PermissionRequestCard item={item} presentation="list" />
            </div>
          ))}
          {busy && progress && !turnProgress && (
            <p class="ai-conversation__progress" role="status">
              <LucideIcon size="s" icon={Bot} name="bot" />
              {progress}
            </p>
          )}
          {error && (
            <StateBanner
              className="ai-conversation__error"
              tone="danger"
              urgency="alert"
              copyLayout="stacked"
              icon={<LucideIcon icon={CircleAlert} name="circle-alert" size={17.6} />}
              title="Conversation Unavailable"
              detail={error}
            />
          )}
        </div>
      </List>
    );
  const savedNotice = readOnly ? (
    <div class="ai-conversation__saved-notice">
      <StateBanner
        tone="neutral"
        copyLayout="stacked"
        title={previewOnly ? 'Read-only preview' : 'Saved transcript'}
        detail={
          previewOnly
            ? `Return to the AI chat to continue this conversation.${previewConfiguration ? ` ${previewConfiguration}` : ''}`
            : `${savedSource ? `Opened from ${savedSource}. ` : ''}This selected range is read-only; save it again or open a tail export with a resumable session to continue.`
        }
      />
    </div>
  ) : null;
  // Kerf Composer owns the raised frame, focus-within border, and the auto-growing borderless field;
  // `submitOnEnter` with `wireComposers` sends on Enter (KF-S5EG5K, HS2-94T958). The app form submits.
  // No visible keyboard hint: the field announces Enter through `aria-keyshortcuts` (HS2-3VQSZD).
  const composer = readOnly ? null : (
    <form
      slot={presentation === 'dialog' ? 'footer' : undefined}
      class="ai-conversation__composer"
      {...COMMANDS_AND_AI_ACTIONS.sendConversationTurn.attrs}
    >
      {busy && turnProgress && (
        <ConversationTurnProgressLine
          progress={turnProgress}
          tool={tool}
          interruptible={interruptible}
          compact={formLayout.compact}
        />
      )}
      <Composer
        label={`Message ${tool}`}
        name="conversation-draft"
        id={`ai-conversation-draft-${presentation}`}
        value={draft}
        placeholder={`Message ${tool}…`}
        maxRows={formLayout.maxRows}
        disabled={busy}
        submitOnEnter
        actions={
          <wa-button
            class="ai-conversation__send"
            appearance="accent"
            size="small"
            pill
            type="submit"
            disabled={busy || !draft.trim()}
            aria-label={`Send Message to ${tool}`}
            title={busy ? `${tool} is still working` : `Send message to ${tool}`}
          >
            <LucideIcon size="s" icon={ArrowUp} name="arrow-up" />
          </wa-button>
        }
      />
    </form>
  );
  if (presentation === 'embedded')
    return (
      <section
        class="ai-conversation ai-conversation--embedded"
        {...COMMANDS_AND_AI_TARGETS.aiConversation.attrs}
        data-presentation="embedded"
        data-read-only={String(readOnly)}
        data-selection-id={selectionId}
        aria-label={`${tool} Conversation`}
      >
        {/* No title bar: the drawer tab already names the chat, and its model, Save, and Stop controls
            render in the drawer's tab toolbar through AIConversationToolbar (HS2-DMH8MX, HS2-59F7WK). */}
        {savedNotice}
        {selectionStatus}
        {transcript}
        {composer}
      </section>
    );
  // Kerf DialogSurface owns the panel geometry: a 768px x 608px panel capped to the viewport less an
  // 8px gutter, with no body or footer inset (HS2-MWMMCN, HS2-ADM62E).
  return (
    <DialogSurface
      preferredWidth={remify(768)}
      height={remify(608)}
      viewportGutter={remify(8)}
      bodyInset="none"
      footerInset="none"
      bodyLayout="fill-last"
      footerPresentation="floating"
      footerOffset={handsetDevice().value ? remify(8) : remify(16)}
      rootAttributes={{ 'data-key': 'ai-conversation-surface' }}
    >
      <wa-dialog
        data-key="ai-conversation"
        class="ai-conversation"
        {...COMMANDS_AND_AI_TARGETS.aiConversation.attrs}
        data-presentation="dialog"
        data-read-only={String(readOnly)}
        data-selection-id={selectionId}
        label={`${tool} conversation`}
        open={open || undefined}
        data-controlled-open={String(open)}
        light-dismiss
        without-header
        with-footer={!readOnly}
      >
        <Toolbar
          dividerSides=""
          leading={
            <>
              <ToolbarControlGroup single tileTone="brand">
                <LucideIcon color={uiColor('brand-on-quiet')} icon={Bot} name="bot" />
              </ToolbarControlGroup>
              <ToolbarText
                text={`${tool} Conversation`}
                id="ai-conversation-title"
                size="xlarge"
                description={summary}
              />
            </>
          }
          trailing={<AIConversationToolbar {...props} presentation="dialog" />}
        />
        {foreground && <div class="ai-conversation__foreground">{foreground}</div>}
        {savedNotice}
        {selectionStatus}
        {transcript}
        {composer}
      </wa-dialog>
    </DialogSurface>
  );
}
