import './permission-request-card.css';

import { Button } from '@kerfjs/ui/button';
import { foregroundColorVar, uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Check, CircleAlert, Clock3, ExternalLink, Pause, ShieldCheck, X } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import type { PermissionHistoryItem, PermissionItem, PermissionPattern } from '../permission-notifications';
import { TOP_LAYER_DISMISS_ATTRIBUTE, TOP_LAYER_OVERLAY_ATTRIBUTE } from '../top-layer-overlay';
import { NotificationCard } from './notification-card';

export type PermissionRequestCardState = 'pending' | 'resolving' | 'failed' | 'disconnected';

/** Ephemeral confirmation state; never persisted or sent in notification events. */
interface RememberedPreview {
  scope: 'chat' | 'always';
  kind: 'exact' | 'template' | 'regex';
  expression: string;
  selected: number[];
}
const rememberedPreviews = new Map<string, RememberedPreview>();
export function rememberedPermissionPreview(key: string) {
  return rememberedPreviews.get(key);
}
export function setRememberedPermissionPreview(key: string, scope?: 'chat' | 'always', action = '') {
  if (scope) rememberedPreviews.set(key, { scope, kind: 'exact', expression: action, selected: [] });
  else rememberedPreviews.delete(key);
}
export function setRememberedPermissionPattern(key: string, kind: RememberedPreview['kind'], expression: string) {
  const preview = rememberedPreviews.get(key);
  if (preview) rememberedPreviews.set(key, { ...preview, kind, expression, selected: [] });
}
export function toggleRememberedPermissionSuggestion(item: PermissionItem, index: number) {
  const preview = rememberedPreviews.get(item.key);
  const suggestions = item.suggestions ?? [];
  if (!preview || !suggestions[index]) return;
  const selected = preview.selected.includes(index)
    ? preview.selected.filter((value) => value !== index)
    : [...preview.selected, index];
  const spans = selected.map((value) => suggestions[value]).sort((a, b) => a.start - b.start);
  let cursor = 0;
  let expression = '';
  for (const span of spans) {
    if (span.start < 0 || span.start < cursor || span.start >= span.end || span.end > item.action.length) return;
    expression += `${item.action.slice(cursor, span.start)}{{${span.placeholder}}}`;
    cursor = span.end;
  }
  expression += item.action.slice(cursor);
  rememberedPreviews.set(item.key, {
    ...preview,
    selected,
    kind: selected.length ? 'template' : 'exact',
    expression,
  });
}
export function reviewedPermissionPattern(key: string): PermissionPattern | undefined {
  const preview = rememberedPreviews.get(key);
  return preview && preview.kind !== 'exact'
    ? { version: 1, kind: preview.kind, expression: preview.expression }
    : undefined;
}

export interface PermissionRequestCardProps {
  item: PermissionItem | PermissionHistoryItem;
  presentation?: 'popup' | 'list';
  /** Use the containing dialog's shadow when its own scrollport would clip the card's blur. */
  popupShadow?: 'card' | 'container';
  state?: PermissionRequestCardState;
  explanation?: string;
  countdown?: string;
  countdownAction?: 'allow' | 'deny';
  error?: string;
  /** Allow Once already reached the tool; this surface remains for the reviewed rule. */
  reviewAlreadyAllowed?: boolean;
  /** Pending requests identical to this one, itself included (HS2-JNHAYB); one answer covers them all. */
  groupCount?: number;
}

function isHistory(item: PermissionItem | PermissionHistoryItem): item is PermissionHistoryItem {
  return 'decision' in item;
}

function operationLabel(item: PermissionItem): string {
  const tool = item.tool.toLowerCase();
  const target = item.action.split('\n', 1)[0];
  if (tool === 'edit' || tool === 'write' || tool.includes('filechange')) return `Wants permission to edit ${target}`;
  if (tool === 'read') return `Wants permission to read ${target}`;
  if (tool === 'bash' || tool.includes('command')) return 'Wants permission to run a command';
  return `Wants permission to use ${item.tool}`;
}

function historyLabel(item: PermissionHistoryItem): string {
  if (item.decision === 'external') return 'Decision made outside Hot Sheet';
  const prefix = item.automatic ? 'Automatically ' : '';
  if (item.decision === 'deny') return `${prefix}denied permission`;
  if (item.scope === 'always') return `${prefix}allowed this kind of request`;
  if (item.scope === 'chat') return `${prefix}allowed for this chat`;
  return `${prefix}allowed permission`;
}

function relativeTime(timestamp: number): string {
  const elapsed = Math.max(0, Date.now() - timestamp);
  if (elapsed < 60_000) return 'Now';
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m ago`;
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h ago`;
  return `${Math.floor(elapsed / 86_400_000)}d ago`;
}

/** Update only the timer text so unrelated Kerf/Web Awesome state stays mounted. */
export function updatePermissionCountdownText(root: ParentNode, key: string, value: string): boolean {
  for (const countdown of root.querySelectorAll<HTMLElement>('[data-permission-countdown-key]')) {
    if (countdown.dataset.permissionCountdownKey !== key) continue;
    const output = countdown.querySelector<HTMLElement>('[data-permission-countdown-value]');
    if (!output || output.textContent === value) return false;
    output.textContent = value;
    return true;
  }
  return false;
}

/** Shared permission presentation for floating prompts and notification history. */
export function PermissionRequestCard({
  item,
  presentation = 'list',
  popupShadow = 'card',
  state = 'pending',
  explanation,
  countdown,
  countdownAction = 'allow',
  error,
  reviewAlreadyAllowed = false,
  groupCount = 1,
}: PermissionRequestCardProps) {
  const history = isHistory(item);
  const alwaysSupported = !history && item.always_allow_supported === true;
  const preview = history ? undefined : rememberedPermissionPreview(item.key);
  const previewScope = preview?.scope;
  const statusLabel = history
    ? `${item.tool}: ${historyLabel(item)}`
    : reviewAlreadyAllowed
      ? `${item.tool}: this request was allowed`
      : operationLabel(item);
  const timestamp = history ? item.resolvedAt : item.receivedAt;
  const stateIcon = history
    ? item.decision === 'allow'
      ? Check
      : item.decision === 'deny'
        ? X
        : ExternalLink
    : state === 'failed' || state === 'disconnected'
      ? CircleAlert
      : state === 'resolving'
        ? Clock3
        : ShieldCheck;
  const iconName = history
    ? item.decision === 'allow'
      ? 'check'
      : item.decision === 'deny'
        ? 'x'
        : 'external-link'
    : state === 'failed' || state === 'disconnected'
      ? 'circle-alert'
      : state === 'resolving'
        ? 'clock-3'
        : 'shield-check';
  // The summary icon takes the card's accent, or a danger/neutral tone for refused, failed, and
  // externally resolved requests (HS2-GQ57YW: through LucideIcon's color prop, not wrapper CSS).
  const summaryState = history ? item.decision : state;
  const summaryColor =
    summaryState === 'deny' || summaryState === 'failed' || summaryState === 'disconnected'
      ? uiColor('danger-on-quiet')
      : summaryState === 'external'
        ? uiColor('neutral-on-quiet')
        : foregroundColorVar(
            history && item.decision === 'allow' && presentation === 'list'
              ? '--wa-color-success-fill-loud'
              : '--wa-color-brand-fill-loud',
          );

  const decisions = history ? undefined : previewScope ? (
    <div class="permission-request-card__confirmation" role="group" aria-label="Confirm remembered permission">
      <strong>Review the rule to remember</strong>
      {reviewAlreadyAllowed && (
        <p role="status">This request was allowed. Finish the rule to remember future matching requests.</p>
      )}
      <dl>
        <dt>Tool</dt>
        <dd>{item.tool}</dd>
        <dt>Exact action</dt>
        <dd class="permission-request-card__exact-action">{item.action}</dd>
        <dt>Match</dt>
        <dd>
          {item.suggestions && item.suggestions.length > 0 && (
            <div class="permission-request-card__suggestions">
              <strong>Possible secret spans</strong>
              <p>
                Suggestions are guesses. Choose each span you want to replace, then review or edit the pattern. Exact
                matching stays the default.
              </p>
              <ul>
                {item.suggestions.map((suggestion, index) => (
                  <li>
                    <span>
                      {suggestion.label}: <mark>{item.action.slice(suggestion.start, suggestion.end)}</mark>
                    </span>
                    <button
                      type="button"
                      data-permission-suggestion-index={index}
                      data-request-key={item.key}
                      aria-pressed={preview.selected.includes(index)}
                    >
                      {preview.selected.includes(index) ? 'Remove' : 'Use'}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
          <label>
            Rule type
            <select data-permission-pattern-kind={item.key} aria-label="Permission rule type">
              <option value="exact" selected={preview.kind === 'exact'}>
                Exact action
              </option>
              <option value="template" selected={preview.kind === 'template'}>
                Placeholders
              </option>
              <option value="regex" selected={preview.kind === 'regex'}>
                Constrained regex
              </option>
            </select>
          </label>
          {preview.kind !== 'exact' && (
            <>
              <label>
                Reviewed pattern
                <textarea
                  data-permission-pattern-expression={item.key}
                  aria-label="Reviewed permission pattern"
                  rows={3}
                >
                  {preview.expression}
                </textarea>
              </label>
              {preview.kind === 'template' && (
                <button type="button" data-permission-insert-placeholder={item.key}>
                  Replace selected text with {'{{token}}'}
                </button>
              )}
              {preview.kind === 'template' && (
                <p>
                  Select a secret span above, then replace it in the pattern with <code>{'{{token}}'}</code> (one
                  unquoted token) or <code>{'{{quoted}}'}</code> (text inside existing quotes). The surrounding command
                  stays literal.
                </p>
              )}
              {preview.kind === 'regex' && (
                <p>
                  Use <code>^</code> and <code>$</code> around the complete action. Repetitions must be bounded; broad
                  wildcards and alternation are rejected.
                </p>
              )}
              <p>
                <strong>Reviewed request:</strong> {item.action}
              </p>
              {preview.kind === 'template' && (
                <p>
                  <strong>Example future match:</strong>{' '}
                  {preview.expression.replaceAll('{{token}}', 'sample').replaceAll('{{quoted}}', 'sample')}
                </p>
              )}
              <p class="permission-request-card__pattern-warning">
                This rule may answer future matching actions without asking. Review the whole pattern and its project,
                tool, and chat reach.
              </p>
            </>
          )}
          {preview.kind === 'exact' && 'Exact action only'}
        </dd>
        <dt>Project</dt>
        <dd>{item.projectName}</dd>
        <dt>Applies to</dt>
        <dd>{previewScope === 'chat' ? 'This chat only' : 'All chats in this project'}</dd>
        <dt>Lifetime</dt>
        <dd>{previewScope === 'chat' ? 'Until this chat closes' : 'Persistent, until revoked'}</dd>
      </dl>
      <div class="permission-request-card__confirmation-actions">
        <Button
          emphasis="plain"
          size="compact"
          action={NOTIFICATIONS_AND_LINKS_ACTIONS.cancelRememberedPermission.value}
          rootAttributes={{ 'data-request-key': item.key }}
        >
          {reviewAlreadyAllowed ? 'Skip Rule' : 'Back'}
        </Button>
        <Button
          emphasis="primary"
          size="compact"
          action={NOTIFICATIONS_AND_LINKS_ACTIONS.resolvePermission.value}
          disabled={state === 'resolving'}
          rootAttributes={{
            'data-decision': 'allow',
            'data-scope': previewScope,
            'data-request-key': item.key,
            'data-rule-confirmed': 'true',
          }}
        >{`Confirm ${previewScope === 'chat' ? 'Allow for This Chat' : 'Always Allow'}`}</Button>
      </div>
    </div>
  ) : (
    <div class="permission-request-card__actions">
      <div class="permission-request-card__buttons">
        <span class="permission-request-card__decision permission-request-card__deny">
          <Button
            tone="danger"
            emphasis="secondary"
            size="compact"
            fill
            action={NOTIFICATIONS_AND_LINKS_ACTIONS.resolvePermission.value}
            disabled={state === 'resolving'}
            rootAttributes={{ 'data-decision': 'deny', 'data-scope': 'once', 'data-request-key': item.key }}
          >
            Deny
          </Button>
        </span>
        <div
          class="permission-request-card__allows"
          data-allow-count={alwaysSupported ? '3' : '2'}
          role="group"
          aria-label="Allow duration"
        >
          <span class="permission-request-card__decision">
            <Button
              emphasis="secondary"
              size="compact"
              fill
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.resolvePermission.value}
              disabled={state === 'resolving'}
              title="Allow this exact request again in this chat without asking, until the chat closes"
              rootAttributes={{ 'data-decision': 'allow', 'data-scope': 'chat', 'data-request-key': item.key }}
            >
              Allow for This Chat
            </Button>
          </span>
          {alwaysSupported && (
            <span class="permission-request-card__decision">
              <Button
                emphasis="secondary"
                size="compact"
                fill
                action={NOTIFICATIONS_AND_LINKS_ACTIONS.resolvePermission.value}
                disabled={state === 'resolving'}
                rootAttributes={{ 'data-decision': 'allow', 'data-scope': 'always', 'data-request-key': item.key }}
              >
                Always Allow
              </Button>
            </span>
          )}
          <span class="permission-request-card__decision">
            <Button
              tone={'brand'}
              emphasis="primary"
              size="compact"
              fill
              className={'permission-request-card__primary'}
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.resolvePermission.value}
              disabled={state === 'resolving'}
              rootAttributes={{ 'data-decision': 'allow', 'data-scope': 'once', 'data-request-key': item.key }}
            >
              {alwaysSupported ? 'Allow Once' : 'Allow'}
            </Button>
          </span>
        </div>
      </div>
      <div class="permission-request-card__utilities">
        <span class="permission-request-card__quiet-action permission-request-card__ignore">
          <Button
            emphasis="plain"
            size="compact"
            action={NOTIFICATIONS_AND_LINKS_ACTIONS.ignorePermission.value}
            rootAttributes={{ ...{ [TOP_LAYER_DISMISS_ATTRIBUTE]: '' }, 'data-request-key': item.key }}
          >
            Ignore
          </Button>
        </span>
        {countdown && (
          <div class="permission-request-card__countdown" data-permission-countdown-key={item.key}>
            <span
              class="permission-request-card__timer"
              role="timer"
              aria-label={`Automatic ${countdownAction} countdown`}
            >
              Auto-{countdownAction} in <strong data-permission-countdown-value>{countdown}</strong>
            </span>
            <Button
              emphasis="plain"
              size="compact"
              className={'permission-request-card__stop-automation'}
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.cancelPermissionAutomation.value}
              ariaLabel={`Stop auto-${countdownAction} Countdown`}
              title={`Stop auto-${countdownAction} countdown for this request`}
              rootAttributes={{ 'data-request-key': item.key }}
              icon={<LucideIcon size={14.4} icon={Pause} name="pause" />}
            />
          </div>
        )}
        {presentation === 'popup' && (
          // Pauses every interruptive notification in all projects; requests keep collecting in
          // Notifications until Resume (HS2-QYA9SC).
          <span class="permission-request-card__quiet-action permission-request-card__pause">
            <Button
              emphasis="plain"
              size="compact"
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications.value}
              title="Stop permission popups in every project until you resume"
            >
              Pause Notifications
            </Button>
          </span>
        )}
      </div>
    </div>
  );

  return (
    <NotificationCard
      rootAttributes={{
        'data-component': 'permission-request-card',
        'data-presentation': presentation,
        'data-state': history ? item.decision : state,
        'data-resolved': history ? 'true' : undefined,
        'data-request-key': item.key,
      }}
      elevation={
        presentation === 'popup'
          ? popupShadow === 'container'
            ? 'popup-contained'
            : 'popup'
          : history
            ? 'flat'
            : 'list'
      }
      footerLayout="stack"
      identity={item.agent}
      role={item.role}
      projectName={item.projectName}
      time={relativeTime(timestamp)}
      summary={{ icon: stateIcon, iconName, label: statusLabel, color: summaryColor }}
      details={{ text: item.action, bounded: true }}
      endActions={decisions}
    >
      <>
        {!history && groupCount > 1 && (
          <p class="permission-request-card__explanation" data-permission-group-count={String(groupCount)}>
            {`${groupCount} identical requests are waiting; your answer applies to all of them.`}
          </p>
        )}
        {explanation && <p class="permission-request-card__explanation">{explanation}</p>}
        {error && (
          <p class="permission-request-card__error" role="alert">
            {error}
          </p>
        )}
      </>
    </NotificationCard>
  );
}

export interface PermissionRequestPopupProps extends Omit<PermissionRequestCardProps, 'presentation' | 'popupShadow'> {
  /**
   * Where the popup paints. `inline` (default) anchors it to its positioned host's top-right corner.
   * `flow` places it in its host's layout, so the host positions it, as the AI conversation's
   * foreground does (HS2-M2W2DP). `top` renders a manual-popover `<dialog>` that `wireTopLayerOverlays`
   * lifts into the browser top layer, so the shell's popup sits above the Workbench's main-pane clip,
   * the terminal drawer, and the rails instead of being covered by them (HS2-ZESCM2), and stays
   * interactive above an open modal dialog (HS2-MAE27T).
   */
  layer?: 'inline' | 'flow' | 'top';
}

export function PermissionRequestPopup({ layer = 'inline', ...props }: PermissionRequestPopupProps) {
  const card = (
    <PermissionRequestCard {...props} presentation="popup" popupShadow={layer === 'top' ? 'container' : 'card'} />
  );
  // The shell's popup is a `<dialog>` so `wireTopLayerOverlays` can lift it with `showModal()` above an
  // open modal dialog, which would otherwise leave it painted on top but inert (HS2-MAE27T). `tabindex`
  // lets the watcher focus the popup itself on a lift, so Enter is never armed on a decision button.
  return layer === 'top' ? (
    <dialog
      class="permission-request-popup"
      data-component="permission-request-popup"
      data-layer={layer}
      popover="manual"
      {...{ [TOP_LAYER_OVERLAY_ATTRIBUTE]: '' }}
      tabindex={-1}
      aria-label="Permission Request"
    >
      {card}
    </dialog>
  ) : (
    <aside
      class="permission-request-popup"
      data-component="permission-request-popup"
      data-layer={layer}
      aria-label="Permission Request"
    >
      {card}
    </aside>
  );
}
