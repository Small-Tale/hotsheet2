import './permission-rules-settings.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { SegmentedControl } from '@kerfjs/ui/segmented-control';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Copy, Ellipsis, ShieldCheck, ShieldX, Trash2 } from 'lucide';

import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import { type PermissionRule, permissionRuleKey } from '../permission-notifications';

export interface PermissionRulesSettingsProps {
  rules: readonly PermissionRule[];
  dialog?: {
    query: string;
    visibleCount: number;
    lifetime?: PermissionRuleLifetimeFilter;
    decision?: PermissionRuleDecisionFilter;
    selected?: readonly string[];
    busy?: boolean;
    confirmingDelete?: boolean;
  };
  preview?: {
    selected?: readonly string[];
    busy?: boolean;
    confirmingDelete?: boolean;
  };
  /** Rule keys whose revoke request is in flight. */
  revoking?: readonly string[];
  message?: string;
  locked?: boolean;
}

export const PERMISSION_RULE_BATCH_SIZE = 100;
export type PermissionRuleLifetimeFilter = 'permanent' | 'session';
export type PermissionRuleDecisionFilter = 'allow' | 'deny';
export interface PermissionRuleFilters {
  lifetime: PermissionRuleLifetimeFilter;
  decision: PermissionRuleDecisionFilter;
}

/** The API returns oldest first. Search the action and its visible context. */
export function permissionRulesList(
  rules: readonly PermissionRule[],
  query: string,
  requestedVisibleCount: number,
  filters?: PermissionRuleFilters,
) {
  const term = query.trim().toLocaleLowerCase(),
    matches = [...rules]
      .reverse()
      .filter(
        (rule) =>
          (!filters || (filters.lifetime === 'permanent' ? rule.scope === 'always' : rule.scope !== 'always')) &&
          (!filters || rule.decision === filters.decision),
      )
      .filter((rule) => {
        return ruleLabel(rule).toLocaleLowerCase().includes(term);
      }),
    visibleCount = Math.min(
      Math.max(
        PERMISSION_RULE_BATCH_SIZE,
        Math.ceil(requestedVisibleCount / PERMISSION_RULE_BATCH_SIZE) * PERMISSION_RULE_BATCH_SIZE,
      ),
      Math.max(PERMISSION_RULE_BATCH_SIZE, matches.length),
    ),
    rows = matches.slice(0, visibleCount);
  return { matches, visibleCount, rows, remainingCount: matches.length - rows.length };
}

const SCOPE_LABELS: Record<PermissionRule['scope'], string> = {
  chat: 'This chat only, until it closes',
  session: 'Session (until restart)',
  always: 'Persistent',
};

function ruleLabel(rule: PermissionRule) {
  return `${rule.tool} · ${rule.exact_action ?? 'Action unavailable'} · ${rule.project} · ${SCOPE_LABELS[rule.scope]} · ${rule.match_type ?? 'exact'} match · ${rule.pattern?.expression ?? ''}`;
}

function matchLabel(rule: PermissionRule) {
  return rule.match_type === 'template' ? 'Placeholder match' : rule.match_type === 'regex' ? 'Regex match' : undefined;
}

/**
 * Settings → Permissions: every answer Hot Sheet remembers for this project (HS2-JNHAYB) — "Allow for
 * this chat" rules, session rules, and durable "Always Allow" rules — each revocable. Chat rules
 * outrank project rules for their chat; revoking one makes the next identical request ask again (or
 * fall back to the project rule).
 */
export function PermissionRulesSettings({
  rules,
  dialog,
  preview,
  revoking = [],
  message = '',
  locked = false,
}: PermissionRulesSettingsProps) {
  const ordered = permissionRulesList(rules, '', PERMISSION_RULE_BATCH_SIZE).matches,
    filters: PermissionRuleFilters = {
      lifetime: dialog?.lifetime ?? 'permanent',
      decision: dialog?.decision ?? 'allow',
    },
    list = dialog ? permissionRulesList(rules, dialog.query, dialog.visibleCount, filters) : undefined,
    dialogSelected = new Set(dialog?.selected ?? []),
    previewSelected = new Set(preview?.selected ?? []),
    selectedCount = rules.filter((rule) => dialogSelected.has(permissionRuleKey(rule))).length,
    previewSelectedCount = rules.filter((rule) => previewSelected.has(permissionRuleKey(rule))).length;
  function ruleRows(items: readonly PermissionRule[], surface: 'preview' | 'dialog') {
    return items.map((rule) => {
      const key = permissionRuleKey(rule),
        allowed = rule.decision === 'allow',
        label = ruleLabel(rule),
        selected = surface === 'dialog' ? dialogSelected.has(key) : previewSelected.has(key),
        busy = surface === 'dialog' ? dialog?.busy : preview?.busy;
      return (
        <div
          class="permission-rules-settings__rule"
          data-permission-rule={key}
          data-permission-rule-scope={rule.scope}
          data-permission-rule-decision={rule.decision}
          data-rule-selected={selected ? 'true' : undefined}
          data-rule-disabled={busy || revoking.includes(key) ? 'true' : undefined}
          data-selectable=""
          data-permission-selection-surface={surface}
        >
          <div class="permission-rules-settings__answer">
            <ListItem
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRuleRow.value}
              rootAttributes={{ 'data-rule-key': key }}
              density="compact"
              icon={<LucideIcon icon={allowed ? ShieldCheck : ShieldX} name={allowed ? 'shield-check' : 'shield-x'} />}
              label={rule.tool}
              description={allowed ? undefined : 'Denied'}
              title={label}
              pressed={selected}
              disabled={busy || revoking.includes(key)}
              accessibleLabel={`Select ${label}${allowed ? '; allowed' : ''}`}
            />
            <div class="permission-rules-settings__details">
              <code data-exact-action>{rule.exact_action ?? 'Action unavailable — revoke and review again'}</code>
              <span>{rule.scope === 'chat' ? `This chat in ${rule.project}` : `All chats in ${rule.project}`}</span>
              <span>
                {SCOPE_LABELS[rule.scope]}
                {matchLabel(rule) && ` · ${matchLabel(rule)}`}
              </span>
              {rule.pattern && <code data-permission-pattern>{rule.pattern.expression}</code>}
              {rule.exact_action && (
                <details data-permission-pattern-editor={key}>
                  <summary>Edit match</summary>
                  <label>
                    Rule type
                    <select data-permission-rule-pattern-kind={key} aria-label={`Rule type for ${rule.tool}`}>
                      <option value="exact" selected={!rule.pattern}>
                        Exact action
                      </option>
                      <option value="template" selected={rule.pattern?.kind === 'template'}>
                        Placeholders
                      </option>
                      <option value="regex" selected={rule.pattern?.kind === 'regex'}>
                        Constrained regex
                      </option>
                    </select>
                  </label>
                  <label>
                    Pattern
                    <textarea
                      data-permission-rule-pattern-expression={key}
                      aria-label={`Pattern for ${rule.tool}`}
                      rows={3}
                    >
                      {rule.pattern?.expression ?? rule.exact_action}
                    </textarea>
                  </label>
                  <small>
                    The complete action must match. Template placeholders: {'{{token}}'} or {'{{quoted}}'}. This can
                    change which future requests are answered automatically.
                  </small>
                  <button type="button" data-permission-rule-pattern-save={key}>
                    Save Reviewed Match
                  </button>
                </details>
              )}
            </div>
          </div>
          {rule.exact_action && (
            <Button
              emphasis="plain"
              size="compact"
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.copyPermissionRuleAction.value}
              rootAttributes={{ 'data-rule-key': key }}
              icon={<LucideIcon icon={Copy} name="copy" />}
              ariaLabel={`Copy Exact Action for ${rule.tool}`}
            >
              Copy
            </Button>
          )}
        </div>
      );
    });
  }
  function selectedActions(surface: 'preview' | 'dialog', count: number, busy = false) {
    const items: PopupMenuEntry[] = [
      {
        label: 'Allow Selected',
        action: NOTIFICATIONS_AND_LINKS_ACTIONS.editSelectedPermissionRules.value,
        attributes: { 'data-decision': 'allow' },
      },
      {
        label: 'Deny Selected',
        action: NOTIFICATIONS_AND_LINKS_ACTIONS.editSelectedPermissionRules.value,
        attributes: { 'data-decision': 'deny' },
      },
      { kind: 'divider' },
      {
        label: 'Set Selected to Session',
        action: NOTIFICATIONS_AND_LINKS_ACTIONS.changeSelectedPermissionRuleScope.value,
        attributes: { 'data-scope': 'session' },
      },
      {
        label: 'Set Selected to Persistent',
        action: NOTIFICATIONS_AND_LINKS_ACTIONS.changeSelectedPermissionRuleScope.value,
        attributes: { 'data-scope': 'always' },
      },
      { kind: 'divider' },
      {
        label: 'Delete Selected',
        action: NOTIFICATIONS_AND_LINKS_ACTIONS.requestDeleteSelectedPermissionRules.value,
        tone: 'danger',
        icon: <LucideIcon icon={Trash2} name="trash-2" />,
      },
    ];
    return (
      <ToolbarControlGroup single>
        <PopupMenu
          label="Selected Answer Actions"
          icon={<LucideIcon icon={Ellipsis} name="ellipsis" />}
          caret={false}
          placement="bottom-end"
          disabled={count === 0 || busy}
          rootAttributes={{ 'data-permission-bulk-menu': '', 'data-permission-selection-surface': surface }}
          items={items}
        />
      </ToolbarControlGroup>
    );
  }
  return (
    <section
      class="permission-rules-settings"
      data-component="permission-rules-settings"
      aria-label="Remembered Permission Answers"
    >
      <List gap="xs">
        <ListHeader label="Remembered Answers" headingLevel={3} density="compact" />
        {locked ? (
          <EmptyState
            icon={<LucideIcon size="xl" icon={ShieldX} name="shield-x" />}
            title="Remembered Answers Locked"
            detail="The permission key or encrypted rule data is unavailable. Saved rules cannot approve requests until access is restored."
          />
        ) : rules.length === 0 ? (
          <EmptyState
            icon={<LucideIcon size="xl" icon={ShieldCheck} name="shield-check" />}
            title="No Remembered Answers"
            detail="Answers you give with Allow for This Chat or Always Allow appear here so you can revoke them."
          />
        ) : (
          <>
            <Row gap="xs" hAlign="right" vAlign="middle" wrap>
              {previewSelectedCount > 0 && (
                <Text tone="quiet" size="compact">{`${previewSelectedCount} selected`}</Text>
              )}
              {previewSelectedCount > 0 && (
                <Button
                  emphasis="plain"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.clearPermissionRuleSelection.value}
                  rootAttributes={{ 'data-permission-selection-surface': 'preview' }}
                  disabled={preview?.busy}
                >
                  Clear
                </Button>
              )}
              {selectedActions('preview', previewSelectedCount, preview?.busy)}
            </Row>
            {preview?.confirmingDelete && previewSelectedCount > 0 && (
              <Row
                gap="xs"
                vAlign="middle"
                wrap
                rootAttributes={{ role: 'group', 'aria-label': 'Confirm preview deletion' }}
              >
                <Text size="compact">{`Delete ${previewSelectedCount} remembered answers? Future matching requests will ask again.`}</Text>
                <Button
                  emphasis="primary"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.confirmDeleteSelectedPermissionRules.value}
                  rootAttributes={{ 'data-permission-selection-surface': 'preview' }}
                  disabled={preview.busy}
                >
                  Confirm Delete
                </Button>
                <Button
                  emphasis="plain"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.cancelDeleteSelectedPermissionRules.value}
                  rootAttributes={{ 'data-permission-selection-surface': 'preview' }}
                  disabled={preview.busy}
                >
                  Cancel
                </Button>
              </Row>
            )}
            <List
              gap="xs"
              className="permission-rules-settings__recent"
              rootAttributes={{ role: 'list', 'aria-label': 'Recent Remembered Answers' }}
            >
              {ruleRows(ordered.slice(0, 3), 'preview')}
            </List>
            {ordered.length > 0 && (
              <Row hAlign="center" controlInsets="tb">
                <Button
                  type="button"
                  emphasis="secondary"
                  shape="pill"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRules.value}
                >
                  {`Show all ${ordered.length} ${ordered.length === 1 ? 'answer' : 'answers'}`}
                </Button>
              </Row>
            )}
          </>
        )}
        <Text tone="quiet" size="compact" role="status">
          {message}
        </Text>
      </List>
      {list && (
        <div data-key="permission-rules-dialog">
          <DialogSurface
            label="Remembered answers"
            preferredWidth={remify(640)}
            viewportGutter={remify(16)}
            rootAttributes={{
              'data-key': 'permission-rules-dialog',
              ...NOTIFICATIONS_AND_LINKS_TARGETS.permissionRulesDialog.attrs,
              'data-controlled-open': 'true',
            }}
            footer={
              list.remainingCount > 0 ? (
                <Button
                  emphasis="plain"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.loadMorePermissionRules.value}
                  disabled={dialog?.busy}
                >
                  {`Load ${Math.min(PERMISSION_RULE_BATCH_SIZE, list.remainingCount)} more answers`}
                </Button>
              ) : undefined
            }
          >
            <List gap="m">
              <TextField
                name={NOTIFICATIONS_AND_LINKS_TARGETS.permissionRulesSearch.value}
                type="search"
                ariaLabel="Search Remembered Answers"
                placeholder="Filter"
                value={dialog?.query ?? ''}
                clearable
                autofocus
              />
              <div class="permission-rules-settings__filters">
                <SegmentedControl<PermissionRuleLifetimeFilter>
                  id="permission-rule-lifetime"
                  label="Answer lifetime"
                  layout="equal"
                  value={filters.lifetime}
                  choices={[
                    { value: 'permanent', label: 'Permanent' },
                    { value: 'session', label: 'Session', title: 'Session and chat-only answers' },
                  ]}
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.filterPermissionRuleLifetime.value}
                  disabled={dialog?.busy}
                />
                <SegmentedControl<PermissionRuleDecisionFilter>
                  id="permission-rule-decision"
                  label="Answer decision"
                  layout="equal"
                  value={filters.decision}
                  choices={[
                    { value: 'allow', label: 'Allow' },
                    { value: 'deny', label: 'Disallow' },
                  ]}
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.filterPermissionRuleDecision.value}
                  disabled={dialog?.busy}
                />
              </div>
              <Row gap="xs" hAlign="right" vAlign="middle" wrap>
                {selectedCount > 0 && <Text tone="quiet" size="compact">{`${selectedCount} selected`}</Text>}
                {selectedCount > 0 && (
                  <Button
                    emphasis="plain"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.clearPermissionRuleSelection.value}
                    rootAttributes={{ 'data-permission-selection-surface': 'dialog' }}
                    disabled={dialog?.busy}
                  >
                    Clear
                  </Button>
                )}
                {selectedActions('dialog', selectedCount, dialog?.busy)}
              </Row>
              {dialog?.confirmingDelete && selectedCount > 0 && (
                <Row
                  gap="xs"
                  vAlign="middle"
                  wrap
                  rootAttributes={{ role: 'group', 'aria-label': 'Confirm bulk deletion' }}
                >
                  <Text size="compact">{`Delete ${selectedCount} remembered answers? Future matching requests will ask again.`}</Text>
                  <Button
                    emphasis="primary"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.confirmDeleteSelectedPermissionRules.value}
                    rootAttributes={{ 'data-permission-selection-surface': 'dialog' }}
                    disabled={dialog.busy}
                  >
                    Confirm Delete
                  </Button>
                  <Button
                    emphasis="plain"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.cancelDeleteSelectedPermissionRules.value}
                    rootAttributes={{ 'data-permission-selection-surface': 'dialog' }}
                    disabled={dialog.busy}
                  >
                    Cancel
                  </Button>
                </Row>
              )}
              <Text tone="quiet" size="compact" role="status">
                {dialog?.busy ? 'Updating selected answers…' : message}
              </Text>
              {list.rows.length ? (
                <div class="permission-rules-settings__all" tabindex="0" aria-label="Scrollable Remembered Answers">
                  <List gap="xs" rootAttributes={{ role: 'list', 'aria-label': 'All Remembered Answers' }}>
                    {ruleRows(list.rows, 'dialog')}
                  </List>
                </div>
              ) : (
                <EmptyState
                  title="No Matching Answers"
                  detail="Try a tool or visible fingerprint, or change the filters."
                />
              )}
            </List>
          </DialogSurface>
        </div>
      )}
    </section>
  );
}
