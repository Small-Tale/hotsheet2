import './ai-conversation.css';

import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { Chip } from '@kerfjs/ui/chip';
import { ContentItem } from '@kerfjs/ui/content-item';
import { flex, px, rem, remify, uiColor } from '@kerfjs/ui/css-values';
import { deviceClass } from '@kerfjs/ui/device-class';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { MessageComposer } from '@kerfjs/ui/message-composer';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { computed, type ReadonlySignal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import {
  ArrowUp,
  Bot,
  Brain,
  ChevronDown,
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
import { contextMenuAnchor } from '../context-menu-position';
import { activityGroupExpanded } from '../conversation-activity-disclosure';
import { conversationContextMenu } from '../conversation-context-menu';
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
  /** A separate, viewport-positioned surface that stays inside this modal's interaction scope. */
  sidecar?: SafeHtml;
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
    <Row
      gap="2xs"
      wrap
      rootAttributes={{
        'aria-label': `Turn usage: ${usage.tokensIn} input tokens, ${usage.tokensOut} output tokens, ${formatConversationCost(usage.costUsd)}`,
        'data-conversation-usage': 'true',
      }}
    >
      <Text variant="span" size="compact" tone="quiet">
        {formatConversationTokens(total)} tokens
      </Text>
      <Text variant="span" size="compact" tone="quiet">
        · {formatConversationCost(usage.costUsd)}
      </Text>
      {usage.model ? (
        <Text variant="span" size="compact" tone="quiet" wrap="truncate" title={usage.model}>
          · {usage.model}
        </Text>
      ) : null}
    </Row>
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
          {message.role === 'user' && (
            <div class="ai-conversation__message-author">
              <Text variant="span" size="compact">
                <strong>You</strong>
              </Text>
            </div>
          )}
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
            <div class="ai-conversation__files">
              <List gap="2xs" rootAttributes={{ role: 'list', 'aria-label': 'Referenced files' }}>
                {message.files.map((file) => (
                  <div class="ai-conversation__file" role="listitem">
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
                  </div>
                ))}
              </List>
            </div>
          )}
          {(message.usage || (message.role === 'assistant' && feedbackAvailable && message.status === 'completed')) && (
            // A Kerf Row spreads the usage line and the feedback actions; the footer keeps only its
            // offset from the message (HS2-ZA1EAG).
            <footer class="ai-conversation__message-meta">
              <Row hAlign="space-between" vAlign="middle" gap="2xs" wrap>
                {message.usage && <UsageLine usage={message.usage} />}
                {message.role === 'assistant' && feedbackAvailable && message.status === 'completed' && (
                  <AIContentFeedback tool={tool} feedbackTarget={`conversation:${message.id}`} />
                )}
              </Row>
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
  gridRow = false,
}: {
  activity: readonly ConversationActivity[];
  feedbackAvailable: boolean;
  /** Between two selected messages: the only state that outlines the group (HS2-72GEX0). */
  selected?: boolean;
  connectionId?: string;
  gridRow?: boolean;
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
      gridRow={gridRow}
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
        <Row hAlign="space-between" vAlign="middle" gap="2xs" wrap>
          <ListHeader
            label="Activity"
            inline
            density="compact"
            toggle
            action="toggle-conversation-activity"
            expanded={expanded}
            count={count}
            countLabel={`${count} activity item${count === 1 ? '' : 's'}`}
            triggerAttributes={{ 'aria-controls': listId }}
          />
          <AIContentLabel mayContainErrors />
        </Row>
        <ol class="ai-conversation__activity-list" id={listId}>
          {visible.map((item) => (
            <li class="ai-conversation__activity-item" data-activity-id={item.id} data-importance={item.importance}>
              <Row hAlign="space-between" vAlign="top" gap="xs">
                <div class="ai-conversation__activity-summary">
                  <MarkdownPreview source={item.summary} density="flush" />
                </div>
                {feedbackAvailable && <AIContentFeedback tool={item.tool} feedbackTarget={`activity:${item.id}`} />}
              </Row>
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
        <ToolbarControlGroup single nestedDropdown content="mixed" {...groupStyle}>
          <PopupMenu
            label={
              permissionChangeable
                ? 'Change Provider, Model, Effort, and Permissions'
                : 'Change Provider, Model, and Effort'
            }
            triggerContent={
              <span class="ai-conversation__model" data-component="conversation-model-control">
                <LucideIcon icon={Brain} name="brain" color={uiColor('neutral-on-quiet')} />
                {canChangeModel && currentModelLabel ? (
                  <span class="ai-conversation__model-name">
                    <Text variant="span" size="compact" wrap="truncate" title={model}>
                      {currentModelLabel}
                    </Text>
                  </span>
                ) : null}
                <LucideIcon icon={ChevronDown} name="chevron-down" color={uiColor('neutral-on-quiet')} />
              </span>
            }
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
 * Below this chat height the chat is compact: the stuck banner takes its one-line form, the
 * selection status appears only when messages are selected, and the draft grows to 3 rows.
 */
export const SHORT_CHAT_HEIGHT = 480;
/** Below this chat height (a default phone or desktop drawer) the draft grows to 2 rows. */
export const VERY_SHORT_CHAT_HEIGHT = 320;

/**
 * How large the embedded conversation's floating form may grow (HS2-SZZGC6). The form floats over
 * the transcript, so in a short drawer a full may-be-stuck banner plus a six-row draft would cover
 * every message. A short chat is therefore compact: the banner shows its one-line form, the
 * selection status appears only while messages are selected, and the draft's growth is capped
 * (it scrolls inside the MessageComposer beyond that). A tall chat or the
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
    <div class="ai-conversation__turn-progress" data-state={progress.state}>
      <Badge tone="neutral" appearance="outline" shape="pill" maxWidth={rem(20)} icon={<LoadingSpinner size="s" />}>
        {progress.label}
      </Badge>
    </div>
  );
}

function SkippedMessages({ count }: { count?: number }) {
  if (!count || count < 1) return <></>;
  return (
    <Row hAlign="full" vAlign="middle" gap="xs" rootAttributes={{ 'data-skipped-count': String(count) }}>
      <span class="ai-conversation__skipped-rule" aria-hidden="true" />
      <Text variant="span" tone="quiet" size="compact">
        <em>{`${count} message${count === 1 ? '' : 's'} skipped`}</em>
      </Text>
      <span class="ai-conversation__skipped-rule" aria-hidden="true" />
    </Row>
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
    sidecar,
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
  const inlinePermissions = presentation === 'dialog' && (foreground || sidecar) ? [] : permissions,
    selectedIds = new Set(selectedMessageIds),
    selectionEnabled = Boolean(selectionId) && messages.length > 0,
    selectedCount = selectedIds.size;
  // A selected-message bar has its own row above the transcript scroller, never floating over
  // messages (HS2-C4RTVS). With no selection the transcript uses that space (HS2-0T1TM5).
  const formLayout = embeddedComposerLayout(presentation === 'embedded' ? availableHeight : undefined);
  const selectionStatus =
    selectionEnabled && selectedCount > 0 ? (
      <div class="ai-conversation__selection">
        <span>{`${selectedCount} message${selectedCount === 1 ? '' : 's'} selected`}</span>
        <span class="ai-conversation__selection-actions">
          <Button
            emphasis="plain"
            size="compact"
            action={COMMANDS_AND_AI_ACTIONS.copyConversationSelection.value}
            ariaLabel={'Copy Selected Messages'}
            title="Copy selected messages"
            icon={<LucideIcon size={14.4} icon={Copy} name="copy" />}
          />
          <Button
            emphasis="plain"
            size="compact"
            action={COMMANDS_AND_AI_ACTIONS.downloadConversationSelection.value}
            ariaLabel={'Download Selected Messages'}
            title="Download selected messages"
            icon={<LucideIcon size={14.4} icon={Download} name="download" />}
          />
          <Button
            emphasis="plain"
            size="compact"
            action={COMMANDS_AND_AI_ACTIONS.clearConversationSelection.value}
            ariaLabel={'Clear Message Selection'}
            title="Clear message selection"
            icon={<LucideIcon size={14.4} icon={X} name="x" />}
          />
        </span>
      </div>
    ) : null;
  const contextState = conversationContextMenu.value,
    contextMenu =
      contextState && contextState.connectionId === selectionId ? (
        <PopupMenu
          context
          anchor={contextMenuAnchor(contextState.x, contextState.y)}
          label="Conversation Message Actions"
          rootAttributes={{ 'data-context-menu': 'conversation' }}
          items={[
            {
              label:
                contextState.messageIds.length === 1
                  ? 'Copy Message'
                  : `Copy ${contextState.messageIds.length} Messages`,
              action: 'conversation-context-action',
              icon: <LucideIcon icon={Copy} name="copy" />,
              attributes: { 'data-conversation-context-action': 'copy' },
            },
            {
              label:
                contextState.messageIds.length === 1
                  ? 'Download Message…'
                  : `Download ${contextState.messageIds.length} Messages…`,
              action: 'conversation-context-action',
              icon: <LucideIcon icon={Download} name="download" />,
              attributes: { 'data-conversation-context-action': 'download' },
            },
          ]}
        />
      ) : null;
  const timeline = conversationTimeline(messages, activity),
    selectedActivity = selectedActivityGroups(timeline, selectedIds),
    timelineRows = (
      <>
        {timeline.map((group, index) =>
          group.kind === 'message' ? (
            <>
              {Boolean(skippedBefore[group.message.id]) &&
                (selectionEnabled ? (
                  <ContentItem gridRow flush>
                    <SkippedMessages count={skippedBefore[group.message.id]} />
                  </ContentItem>
                ) : (
                  <SkippedMessages count={skippedBefore[group.message.id]} />
                ))}
              <ConversationMessages
                tool={tool}
                messages={[group.message]}
                feedbackAvailable={feedbackAvailable}
                selectedIds={selectedIds}
                selectionAction={selectionEnabled ? 'pick-conversation-message' : undefined}
              />
            </>
          ) : (
            <ConversationActivityGroup
              activity={group.activity}
              feedbackAvailable={feedbackAvailable}
              selected={selectedActivity.has(index)}
              connectionId={selectionId}
              gridRow={selectionEnabled}
            />
          ),
        )}
      </>
    ),
    transcript = (
      // Kerf List is the transcript scroller: its block-end scrollClearance lets messages scroll
      // beneath the floating MessageComposer while the last one still rests above it. The dialog's
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
            <Row gap="xs" vAlign="middle" rootAttributes={{ role: 'status', 'data-conversation-progress': 'true' }}>
              <LucideIcon size="s" icon={Bot} name="bot" />
              <Text variant="span" tone="quiet">
                {progress}
              </Text>
            </Row>
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
  // Kerf MessageComposer owns the raised frame, focus-within border, and the auto-growing borderless field;
  // `submitOnEnter` with `wireMessageComposers` sends on Enter (KF-S5EG5K, HS2-94T958). The app form submits.
  // No visible keyboard hint: the field announces Enter through `aria-keyshortcuts` (HS2-3VQSZD).
  const composer = readOnly ? null : (
    <form class="ai-conversation__composer" {...COMMANDS_AND_AI_ACTIONS.sendConversationTurn.attrs}>
      {busy && turnProgress && (
        <ConversationTurnProgressLine
          progress={turnProgress}
          tool={tool}
          interruptible={interruptible}
          compact={formLayout.compact}
        />
      )}
      <MessageComposer
        label={`Message ${tool}`}
        name="conversation-draft"
        id={`ai-conversation-draft-${presentation}`}
        value={draft}
        placeholder={`Message ${tool}…`}
        maxRows={formLayout.maxRows}
        disabled={busy}
        submitOnEnter
        trailing={
          <Button
            className={'ai-conversation__send'}
            emphasis="primary"
            size="compact"
            shape="pill"
            type="submit"
            disabled={busy || !draft.trim()}
            ariaLabel={`Send Message to ${tool}`}
            title={busy ? `${tool} is still working` : `Send message to ${tool}`}
            icon={<LucideIcon size="s" icon={ArrowUp} name="arrow-up" />}
          />
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
        {contextMenu}
      </section>
    );
  // Kerf owns the bottom sheet's viewport, keyboard, and resize bounds. A permission
  // sidecar shares short viewports with the chat, so it releases the usual 320px minimum.
  return (
    <DialogSurface
      presentation="bottom-sheet"
      resizable
      preferredWidth={remify(768)}
      height={remify(608)}
      minHeight={remify(sidecar ? 0 : 320)}
      viewportGutter={remify(8)}
      bodyInset="none"
      footerInset="none"
      bodyLayout="fill-last"
      footerPresentation="floating"
      footerOffset={handsetDevice().value ? remify(8) : remify(16)}
      label={`${tool} conversation`}
      lightDismiss
      withoutHeader
      className="ai-conversation"
      rootAttributes={{
        'data-key': 'ai-conversation',
        ...COMMANDS_AND_AI_TARGETS.aiConversationDialog.attrs,
        'data-read-only': String(readOnly),
        'data-selection-id': selectionId,
        'data-controlled-open': String(open),
      }}
      footer={composer ?? undefined}
    >
      <Toolbar
        dividerSides=""
        leading={
          <>
            {!handsetDevice().value && (
              <ToolbarControlGroup single tileTone="brand">
                <LucideIcon color={uiColor('brand-on-quiet')} icon={Bot} name="bot" />
              </ToolbarControlGroup>
            )}
            <ToolbarText
              text={handsetDevice().value ? tool : `${tool} Conversation`}
              id="ai-conversation-title"
              size={handsetDevice().value ? 'default' : 'xlarge'}
              description={summary}
            />
          </>
        }
        trailing={<AIConversationToolbar {...props} presentation="dialog" />}
      />
      {foreground && <div class="ai-conversation__foreground">{foreground}</div>}
      {sidecar && <div class="ai-conversation__sidecar">{sidecar}</div>}
      {savedNotice}
      {selectionStatus}
      {transcript}
      {contextMenu}
    </DialogSurface>
  );
}
