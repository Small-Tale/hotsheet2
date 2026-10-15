import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './ai-conversation.css';
import '@kerfjs/ui/surface-scaffold/register';
import '@awesome.me/webawesome/dist/components/button/button.js';

import { Badge } from '@kerfjs/ui/badge';
import { Chip } from '@kerfjs/ui/chip';
import { Composer } from '@kerfjs/ui/composer';
import { ContentItem } from '@kerfjs/ui/content-item';
import { flex, px, remify, uiColor } from '@kerfjs/ui/css-values';
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
  CircleAlert,
  Copy,
  Download,
  Image,
  MessageSquare,
  Paperclip,
  Square,
  X,
} from 'lucide';

import {
  type ConversationActivity,
  type ConversationMessage,
  conversationTimeline,
  type ConversationUsage,
  formatConversationCost,
  formatConversationTokens,
} from '../ai-conversation';
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
  draft: string;
  busy: boolean;
  progress?: string;
  interruptible: boolean;
  permissions?: PermissionItem[];
  activity?: ConversationActivity[];
  totalUsage?: ConversationUsage;
  error?: string;
  feedbackAvailable?: boolean;
  presentation?: 'dialog' | 'embedded';
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
  readOnly?: boolean;
  readOnlyContext?: 'saved' | 'preview';
  savedSource?: string;
  /** False on a remote client: host-only controls render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}

export function isConversationSurfaceLifecycleEvent(event: Pick<Event, 'target'>, surface: Element) {
  return event.target === surface;
}

function UsageLine({ usage, label = 'Turn usage' }: { usage: ConversationUsage; label?: string }) {
  const total = usage.tokensIn + usage.tokensOut;
  return (
    <p
      class="ai-conversation__usage"
      aria-label={`${label}: ${usage.tokensIn} input tokens, ${usage.tokensOut} output tokens, ${formatConversationCost(usage.costUsd)}`}
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
        // brand-loud fill, responses at the start; a selectable transcript makes each one an option.
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
                selectionMode: 'single' as const,
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
}: {
  activity: readonly ConversationActivity[];
  feedbackAvailable: boolean;
}) {
  return (
    // A Kerf ContentItem frames the activity card (HS2-ADM62E); the app lays out only its own header and list.
    <ContentItem
      appearance="neutral"
      className="ai-conversation__activity"
      ariaLabel="AI-generated activity summaries; may contain errors"
    >
      <div class="ai-conversation__activity-body">
        <header>
          <span>
            <LucideIcon icon={Activity} name="activity" size={14.4} />
            <strong>Activity</strong>
          </span>
          <span class="ai-conversation__activity-disclosure">
            <AIContentLabel mayContainErrors />
          </span>
        </header>
        <ol class="ai-conversation__activity-list">
          {activity.map((item) => (
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

let handset: ReadonlySignal<boolean> | undefined;
/** Phones inset the floating composer 8px instead of 16px; read only the handset flag. */
function handsetDevice(): ReadonlySignal<boolean> {
  if (!handset) {
    const device = deviceClass();
    handset = computed(() => device.value.handset);
  }
  return handset;
}

export function AIConversation({
  open,
  tool,
  selectionId,
  selectedMessageIds = [],
  messages,
  draft,
  busy,
  progress,
  interruptible,
  permissions = [],
  activity = [],
  totalUsage,
  error,
  feedbackAvailable = false,
  presentation = 'dialog',
  foreground,
  providerId,
  providers = [],
  canChangeProvider = false,
  model,
  effort,
  models = [],
  efforts = [],
  canChangeModel = false,
  canChangeEffort = false,
  readOnly = false,
  readOnlyContext = 'saved',
  savedSource,
  hostActionsAvailable = hostActionsAvailableHere(),
}: AIConversationProps) {
  const previewOnly = readOnly && readOnlyContext === 'preview',
    previewConfiguration = [model && `Model ${model}`, effort && `Effort ${effort}`].filter(Boolean).join(' · '),
    summary = previewOnly
      ? 'Read-only preview'
      : readOnly
        ? 'Saved read-only transcript'
        : busy
          ? `${tool} is working`
          : messages.length === 0
            ? 'Ready for your first message'
            : `${messages.length} message${messages.length === 1 ? '' : 's'}`;
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
  const actions = (
    <>
      {totalUsage && (
        <span class="ai-conversation__header-usage">
          <UsageLine usage={totalUsage} label="Conversation usage" />
        </span>
      )}
      {saveAction}
      {stopAction}
    </>
  );
  const inlinePermissions = presentation === 'dialog' && foreground ? [] : permissions,
    selectedIds = new Set(selectedMessageIds),
    selectionEnabled = Boolean(selectionId) && messages.length > 0,
    selectedCount = selectedIds.size;
  const selectionStatus = selectionEnabled ? (
    <div class="ai-conversation__selection" data-active={String(selectedCount > 0)}>
      <span>
        {selectedCount
          ? `${selectedCount} message${selectedCount === 1 ? '' : 's'} selected`
          : 'Select a message, then another to select a range.'}
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
    transcript = (
      // Kerf List is the transcript scroller: in the dialog its block-end scrollClearance lets messages
      // scroll beneath the floating Composer footer while the last one still rests above it
      // (KF-913AT2, HS2-HVZVEA). The app-owned content column inside keeps the transcript's own inset.
      <List
        scrollable
        scrollClearance={presentation === 'dialog' ? 'block-end' : undefined}
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
          {selectionStatus}
          {timeline.map((group) =>
            group.kind === 'message' ? (
              <ConversationMessages
                tool={tool}
                messages={[group.message]}
                feedbackAvailable={feedbackAvailable}
                selectedIds={selectedIds}
                selectionAction={selectionEnabled ? 'pick-conversation-message' : undefined}
              />
            ) : (
              <ConversationActivityGroup activity={group.activity} feedbackAvailable={feedbackAvailable} />
            ),
          )}
          {inlinePermissions.map((item) => (
            <div data-permission-key={item.key}>
              <PermissionRequestCard item={item} presentation="list" />
            </div>
          ))}
          {busy && progress && (
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
  const activeModel = models.find((item) => item.id === model),
    currentModelLabel = activeModel?.label ?? model ?? '',
    customModel = model && !activeModel ? model : undefined,
    currentEffort = effort ?? (efforts.length ? efforts[0] : undefined);
  const providerChoices = providers.filter((item) => item.id !== providerId),
    providerChangeable = canChangeProvider && providerChoices.length > 0,
    currentProviderLabel = providers.find((item) => item.id === providerId)?.label ?? tool;
  const sessionControls =
    !readOnly && (providerChangeable || (canChangeModel && models.length) || (canChangeEffort && efforts.length)) ? (
      <div class="ai-conversation__model" data-component="conversation-model-control" aria-label="AI session settings">
        <span class="ai-conversation__model-current">
          <span class="ai-conversation__model-icon">
            <LucideIcon icon={Brain} name="brain" color={uiColor('neutral-on-quiet')} />
          </span>
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
        <PopupMenu
          label="Change Provider, Model, and Effort"
          icon={<LucideIcon icon={ChevronDown} name="chevron-down" />}
          caret={false}
          placement="top-end"
          rootAttributes={{ 'data-conversation-model-menu': 'true' }}
          items={providerModelEffortEntries({
            actions: {
              provider: 'select-conversation-provider',
              model: 'select-conversation-model',
              effort: 'select-conversation-effort',
              manualModel: 'open-conversation-manual-model',
            },
            providers: providerChangeable
              ? { choices: providers, currentId: providerId, currentLabel: currentProviderLabel }
              : undefined,
            model:
              canChangeModel && models.length > 0
                ? { choices: models, currentId: activeModel?.id, currentLabel: currentModelLabel, customModel }
                : undefined,
            effort: canChangeEffort && efforts.length > 0 ? { efforts, current: currentEffort } : undefined,
          })}
        />
      </div>
    ) : null;
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
      <Composer
        label={`Message ${tool}`}
        name="conversation-draft"
        id={`ai-conversation-draft-${presentation}`}
        value={draft}
        placeholder={`Message ${tool}…`}
        maxRows={6}
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
        {/* A compact (40px) Kerf Toolbar keeps the drawer header short enough that the transcript clears
            the floating composer (KF-0QDF0V, HS2-P16J04). */}
        <Toolbar
          density="compact"
          dividerSides="b"
          className="ai-conversation__embedded-header"
          leading={
            <>
              <LucideIcon icon={Bot} name="bot" size={16} />
              <ToolbarText text={`${tool} conversation`} />
            </>
          }
          trailing={
            saveAction || stopAction ? (
              <ToolbarControlGroup label={`${tool} Conversation Actions`} appearance="borderless" size="compact">
                <>
                  {saveAction}
                  {stopAction}
                </>
              </ToolbarControlGroup>
            ) : undefined
          }
        />
        {savedNotice}
        {sessionControls}
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
          trailing={<ToolbarControlGroup label={`${tool} Conversation Actions`}>{actions}</ToolbarControlGroup>}
        />
        {foreground && <div class="ai-conversation__foreground">{foreground}</div>}
        {savedNotice}
        {sessionControls}
        {transcript}
        {composer}
      </wa-dialog>
    </DialogSurface>
  );
}
