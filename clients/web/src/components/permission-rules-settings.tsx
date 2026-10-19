import { Button } from '@kerfjs/ui/button';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { ShieldCheck, ShieldX, Trash2 } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { type PermissionRule, permissionRuleKey } from '../permission-notifications';

export interface PermissionRulesSettingsProps {
  rules: readonly PermissionRule[];
  /** Rule keys whose revoke request is in flight. */
  revoking?: readonly string[];
  message?: string;
}

const SCOPE_LABELS: Record<PermissionRule['scope'], string> = {
  chat: 'This chat only, until it closes',
  session: 'This project, until the server restarts',
  always: 'This project, always',
};

/** Opaque ID distinguishes otherwise similar rules without exposing command/path text. */
function ruleLabel(tool: string, action: string) {
  const fingerprint = /^sha256:v1:[a-f0-9]{64}$/.test(action) ? action.slice(-8) : undefined;
  return fingerprint ? `${tool} request · ${fingerprint}` : `${tool} request`;
}

/**
 * Settings → Permissions: every answer Hot Sheet remembers for this project (HS2-JNHAYB) — "Allow for
 * this chat" rules, session rules, and durable "Always Allow" rules — each revocable. Chat rules
 * outrank project rules for their chat; revoking one makes the next identical request ask again (or
 * fall back to the project rule).
 */
export function PermissionRulesSettings({ rules, revoking = [], message = '' }: PermissionRulesSettingsProps) {
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
          <List gap="xs" rootAttributes={{ role: 'list', 'aria-label': 'Remembered Answers' }}>
            {rules.map((rule) => {
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
                    disabled={revoking.includes(key)}
                    ariaLabel={`Revoke ${label}`}
                    title="Forget this answer; the next identical request asks again"
                    rootAttributes={{ 'data-rule-key': key }}
                    icon={<LucideIcon icon={Trash2} name="trash-2" />}
                  />
                </Row>
              );
            })}
          </List>
        )}
        <Text tone="quiet" size="compact" role="status">
          {message}
        </Text>
      </List>
    </section>
  );
}
