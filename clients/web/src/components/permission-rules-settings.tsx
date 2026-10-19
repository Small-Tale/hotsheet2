import '@awesome.me/webawesome/dist/components/input/input.js';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { ShieldCheck, ShieldX, Trash2 } from 'lucide';

import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import { type PermissionRule, permissionRuleKey } from '../permission-notifications';

export interface PermissionRulesSettingsProps {
  rules: readonly PermissionRule[];
  dialog?: { query: string; page: number; selected?: readonly string[]; busy?: boolean; confirmingDelete?: boolean };
  /** Rule keys whose revoke request is in flight. */
  revoking?: readonly string[];
  message?: string;
}

const PAGE_SIZE = 5;

/** The API returns oldest first. Search only fields displayed to the user, never raw actions. */
export function permissionRulesPage(rules: readonly PermissionRule[], query: string, requestedPage: number) {
  const term = query.trim().toLocaleLowerCase(),
    matches = [...rules].reverse().filter((rule) => {
      const safeText = `${ruleLabel(rule.tool, rule.action)} ${rule.decision} ${SCOPE_LABELS[rule.scope]}`;
      return safeText.toLocaleLowerCase().includes(term);
    }),
    pageCount = Math.max(1, Math.ceil(matches.length / PAGE_SIZE)),
    page = Math.min(Math.max(1, requestedPage), pageCount);
  return { matches, page, pageCount, rows: matches.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE) };
}

const SCOPE_LABELS: Record<PermissionRule['scope'], string> = {
  chat: 'This chat only, until it closes',
  session: 'Until the server restarts',
  always: 'Always',
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
  const ordered = permissionRulesPage(rules, '', 1).matches,
    page = dialog ? permissionRulesPage(rules, dialog.query, dialog.page) : undefined,
    selected = new Set(dialog?.selected ?? []),
    selectedCount = rules.filter((rule) => selected.has(permissionRuleKey(rule))).length,
    pageSelected = page?.rows.length ? page.rows.every((rule) => selected.has(permissionRuleKey(rule))) : false;
  function ruleRows(items: readonly PermissionRule[], selectable = false) {
    return items.map((rule) => {
      const key = permissionRuleKey(rule),
        allowed = rule.decision === 'allow',
        label = ruleLabel(rule.tool, rule.action);
      return (
        <Row
          vAlign="middle"
          gap="m"
          rootAttributes={{
            'data-permission-rule': key,
            'data-permission-rule-scope': rule.scope,
          }}
        >
          {selectable && (
            <wa-checkbox
              {...NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRule.attrs}
              data-rule-key={key}
              checked={selected.has(key)}
              disabled={dialog?.busy}
              aria-label={`Select ${label}`}
            />
          )}
          <LucideIcon
            icon={allowed ? ShieldCheck : ShieldX}
            name={allowed ? 'shield-check' : 'shield-x'}
            label={allowed ? 'Allowed' : 'Denied'}
          />
          <List gap="none" flex>
            <Text wrap="truncate" title="Action details hidden to protect sensitive values" flush>
              {label}
            </Text>
            <Text tone="quiet" size="compact" flush>
              {`${allowed ? 'Allow' : 'Deny'} · ${SCOPE_LABELS[rule.scope]} · Details hidden`}
            </Text>
          </List>
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
        </Row>
      );
    });
  }
  return (
    <section data-component="permission-rules-settings" aria-label="Remembered Permission Answers">
      <List gap="xs">
        <Text variant="h3" size="compact">
          Remembered answers
        </Text>
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
              <Button emphasis="plain" action={NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRules.value}>
                {`Show all ${ordered.length} answers`}
              </Button>
            )}
          </>
        )}
        <Text tone="quiet" size="compact" role="status">
          {message}
        </Text>
      </List>
      {page && (
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
              <Row hAlign="space-between" vAlign="middle" gap="xs" wrap>
                <Text tone="quiet" size="compact">{`Page ${page.page} of ${page.pageCount}`}</Text>
                <Row gap="xs">
                  <Button
                    emphasis="secondary"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.previousPermissionRulesPage.value}
                    disabled={page.page === 1}
                  >
                    Previous
                  </Button>
                  <Button
                    emphasis="secondary"
                    action={NOTIFICATIONS_AND_LINKS_ACTIONS.nextPermissionRulesPage.value}
                    disabled={page.page === page.pageCount}
                  >
                    Next
                  </Button>
                </Row>
              </Row>
            }
          >
            <List gap="m">
              <wa-input
                {...NOTIFICATIONS_AND_LINKS_TARGETS.permissionRulesSearch.attrs}
                type="search"
                label="Search Remembered Answers"
                value={dialog?.query ?? ''}
                clearable
                autofocus
              />
              <Text tone="quiet" size="compact">{`${page.matches.length} matching answers · newest first`}</Text>
              <Row gap="xs" vAlign="middle" wrap>
                <Button
                  emphasis="secondary"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRulesPage.value}
                  disabled={page.rows.length === 0 || dialog?.busy}
                >
                  {pageSelected ? 'Deselect Page' : 'Select Page'}
                </Button>
                <Text tone="quiet" size="compact">{`${selectedCount} selected`}</Text>
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
              {page.rows.length ? (
                <List gap="xs" rootAttributes={{ role: 'list', 'aria-label': 'All Remembered Answers' }}>
                  {ruleRows(page.rows, true)}
                </List>
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
