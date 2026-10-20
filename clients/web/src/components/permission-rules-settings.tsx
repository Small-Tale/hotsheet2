import './permission-rules-settings.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SegmentedControl } from '@kerfjs/ui/segmented-control';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { ChevronRight, ShieldCheck, ShieldX, Trash2 } from 'lucide';

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
  /** Rule keys whose revoke request is in flight. */
  revoking?: readonly string[];
  message?: string;
}

export const PERMISSION_RULE_BATCH_SIZE = 100;
export type PermissionRuleLifetimeFilter = 'permanent' | 'session';
export type PermissionRuleDecisionFilter = 'allow' | 'deny';
export interface PermissionRuleFilters {
  lifetime: PermissionRuleLifetimeFilter;
  decision: PermissionRuleDecisionFilter;
}

/** The API returns oldest first. Search only fields displayed to the user, never raw actions. */
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
        const safeText = `${ruleLabel(rule.tool, rule.action)} ${rule.decision} ${SCOPE_LABELS[rule.scope]}`;
        return safeText.toLocaleLowerCase().includes(term);
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

/** Opaque ID distinguishes otherwise similar rules without exposing command/path text. */
function ruleLabel(tool: string, action: string) {
  const fingerprint = /^hmac-sha256:v2:[a-f0-9]{16}:[a-f0-9]{64}$/.test(action) ? action.slice(-8) : undefined;
  return fingerprint ? `${tool} request · ${fingerprint}` : `${tool} request`;
}

/**
 * Settings → Permissions: every answer Hot Sheet remembers for this project (HS2-JNHAYB) — "Allow for
 * this chat" rules, session rules, and durable "Always Allow" rules — each revocable. Chat rules
 * outrank project rules for their chat; revoking one makes the next identical request ask again (or
 * fall back to the project rule).
 */
export function PermissionRulesSettings({ rules, dialog, revoking = [], message = '' }: PermissionRulesSettingsProps) {
  const ordered = permissionRulesList(rules, '', PERMISSION_RULE_BATCH_SIZE).matches,
    filters: PermissionRuleFilters = {
      lifetime: dialog?.lifetime ?? 'permanent',
      decision: dialog?.decision ?? 'allow',
    },
    list = dialog ? permissionRulesList(rules, dialog.query, dialog.visibleCount, filters) : undefined,
    selected = new Set(dialog?.selected ?? []),
    selectedCount = rules.filter((rule) => selected.has(permissionRuleKey(rule))).length;
  function ruleRows(items: readonly PermissionRule[], selectable = false) {
    return items.map((rule) => {
      const key = permissionRuleKey(rule),
        allowed = rule.decision === 'allow',
        label = ruleLabel(rule.tool, rule.action),
        targetScope = rule.scope === 'session' ? 'Persistent' : 'Session';
      return (
        <div
          class="permission-rules-settings__rule"
          data-permission-rule={key}
          data-permission-rule-scope={rule.scope}
          data-permission-rule-decision={rule.decision}
          data-rule-selected={selectable && selected.has(key) ? 'true' : undefined}
          data-rule-disabled={dialog?.busy || revoking.includes(key) ? 'true' : undefined}
          data-selectable={selectable ? '' : undefined}
        >
          <div class="permission-rules-settings__answer">
            <ListItem
              action={
                selectable
                  ? NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRuleRow.value
                  : NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRules.value
              }
              rootAttributes={{ 'data-rule-key': key }}
              density="compact"
              icon={<LucideIcon icon={allowed ? ShieldCheck : ShieldX} name={allowed ? 'shield-check' : 'shield-x'} />}
              label={
                <Text variant="span" wrap="truncate" truncate="middle" truncateKeep={8} flush>
                  {label}
                </Text>
              }
              description={`${allowed ? 'Allowed' : 'Denied'} · ${SCOPE_LABELS[rule.scope]}`}
              title={`${label} — Action details hidden to protect sensitive values`}
              pressed={selectable ? selected.has(key) : undefined}
              disabled={dialog?.busy || revoking.includes(key)}
              accessibleLabel={`${selectable ? 'Select' : 'View'} ${label}`}
            />
          </div>
          {rule.scope !== 'chat' && (
            <Button
              emphasis="plain"
              size="compact"
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.changePermissionRuleScope.value}
              rootAttributes={{
                'data-rule-key': key,
                'data-scope': rule.scope === 'session' ? 'always' : 'session',
              }}
              disabled={revoking.includes(key) || dialog?.busy}
              ariaLabel={`Set ${label} to ${targetScope}`}
              title={`Set to ${targetScope}`}
            >
              {`Set ${targetScope}`}
            </Button>
          )}
          <Button
            emphasis="plain"
            size="compact"
            action={NOTIFICATIONS_AND_LINKS_ACTIONS.revokePermissionRule.value}
            disabled={revoking.includes(key) || dialog?.busy}
            ariaLabel={`Revoke ${label}`}
            title="Forget this answer; the next identical request asks again"
            rootAttributes={{ 'data-rule-key': key }}
            icon={<LucideIcon icon={Trash2} name="trash-2" />}
          />
        </div>
      );
    });
  }
  return (
    <section
      class="permission-rules-settings"
      data-component="permission-rules-settings"
      aria-label="Remembered Permission Answers"
    >
      <List gap="xs">
        <ListHeader label="Remembered Answers" headingLevel={3} density="compact" />
        {rules.length === 0 ? (
          <EmptyState
            icon={<LucideIcon size="xl" icon={ShieldCheck} name="shield-check" />}
            title="No Remembered Answers"
            detail="Answers you give with Allow for This Chat or Always Allow appear here so you can revoke them."
          />
        ) : (
          <>
            <List gap="xs" rootAttributes={{ role: 'list', 'aria-label': 'Recent Remembered Answers' }}>
              {ruleRows(ordered.slice(0, 3))}
            </List>
            {ordered.length > 3 && (
              <ListItem
                action={NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRules.value}
                label={`Show all ${ordered.length} answers`}
                trailing={<LucideIcon icon={ChevronRight} name="chevron-right" size={16} />}
              />
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
              <Row gap="xs" wrap>
                <SegmentedControl<PermissionRuleLifetimeFilter>
                  id="permission-rule-lifetime"
                  label="Answer lifetime"
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
                  value={filters.decision}
                  choices={[
                    { value: 'allow', label: 'Allow' },
                    { value: 'deny', label: 'Disallow' },
                  ]}
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.filterPermissionRuleDecision.value}
                  disabled={dialog?.busy}
                />
              </Row>
              <TextField
                name={NOTIFICATIONS_AND_LINKS_TARGETS.permissionRulesSearch.value}
                type="search"
                ariaLabel="Search Remembered Answers"
                placeholder="Filter"
                value={dialog?.query ?? ''}
                clearable
                autofocus
              />
              <Row gap="xs" vAlign="middle" wrap>
                <Text tone="quiet" size="compact">
                  {`${list.matches.length} ${list.matches.length === 1 ? 'answer' : 'answers'}`}
                </Text>
                {selectedCount > 0 && <Text tone="quiet" size="compact">{`${selectedCount} selected`}</Text>}
                {selectedCount > 0 && (
                  <Button
                    emphasis="plain"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.clearPermissionRuleSelection.value}
                    disabled={dialog?.busy}
                  >
                    Clear
                  </Button>
                )}
              </Row>
              {selectedCount > 0 && (
                <Row gap="xs" vAlign="middle" wrap>
                  <Button
                    emphasis="secondary"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.editSelectedPermissionRules.value}
                    rootAttributes={{ 'data-decision': 'allow' }}
                    disabled={dialog?.busy}
                  >
                    Allow Selected
                  </Button>
                  <Button
                    emphasis="secondary"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.editSelectedPermissionRules.value}
                    rootAttributes={{ 'data-decision': 'deny' }}
                    disabled={dialog?.busy}
                  >
                    Deny Selected
                  </Button>
                  <Button
                    emphasis="plain"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.requestDeleteSelectedPermissionRules.value}
                    disabled={dialog?.busy}
                  >
                    Delete Selected
                  </Button>
                </Row>
              )}
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
                    disabled={dialog.busy}
                  >
                    Confirm Delete
                  </Button>
                  <Button
                    emphasis="plain"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.cancelDeleteSelectedPermissionRules.value}
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
                    {ruleRows(list.rows, true)}
                  </List>
                </div>
              ) : (
                <EmptyState title="No Matching Answers" detail="Try a tool, decision, scope, or visible fingerprint." />
              )}
            </List>
          </DialogSurface>
        </div>
      )}
    </section>
  );
}
