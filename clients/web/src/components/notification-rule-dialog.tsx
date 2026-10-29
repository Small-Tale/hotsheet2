import { Button } from '@kerfjs/ui/button';
import { CheckboxGroup } from '@kerfjs/ui/checkbox-group';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import type { NotificationChannel, NotificationRule } from '../api';
import type { RuleEditorState } from '../features/notification-settings';
import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import { NOTIFICATION_EVENTS, NOTIFICATION_SEVERITIES, RULE_FIELD } from '../notification-settings-model';

export interface NotificationRuleDialogProps {
  editor: RuleEditorState;
  channels: readonly NotificationChannel[];
  /** The rule being edited; absent while adding one. */
  rule?: NotificationRule;
}

const join = (values: readonly string[] | undefined) => (values ?? []).join(', ');

/**
 * The add/edit rule dialog for App Settings → Notifications (HS2-DPBJJA): which events go to which
 * channels, narrowed by ticket text, tags, statuses, projects, and severity, with a repeat cooldown.
 */
export function NotificationRuleDialog({ editor, channels, rule }: NotificationRuleDialogProps) {
  const adding = !editor.id,
    filter = rule?.filter;
  return (
    <DialogSurface
      label={adding ? 'Add Rule' : 'Edit Rule'}
      rootAttributes={{
        ...NOTIFICATIONS_AND_LINKS_TARGETS.notificationRuleDialog.attrs,
        'data-controlled-open': 'true',
      }}
      footer={
        <Row gap="xs" hAlign="right" wrap>
          <Button emphasis="plain" action={NOTIFICATIONS_AND_LINKS_ACTIONS.closeNotificationRule.value}>
            Cancel
          </Button>
          <Button
            emphasis="primary"
            action={NOTIFICATIONS_AND_LINKS_ACTIONS.saveNotificationRule.value}
            disabled={editor.busy}
          >
            {adding ? 'Add Rule' : 'Save Rule'}
          </Button>
        </Row>
      }
    >
      <List gap="m">
        {adding ? <TextField name={RULE_FIELD.label} label="Name" placeholder="Release blockers" /> : null}
        <CheckboxGroup
          name={RULE_FIELD.events}
          label="When"
          orientation="horizontal"
          choices={NOTIFICATION_EVENTS.map((event): { value: string; label: string } => ({
            value: event.value,
            label: event.label,
          }))}
          value={rule?.events ?? []}
        />
        <CheckboxGroup
          name={RULE_FIELD.channels}
          label="Send To"
          orientation="horizontal"
          choices={channels.map((channel) => ({ value: channel.id, label: channel.label ?? channel.id }))}
          value={rule?.channels ?? []}
        />
        <Grid minColumnWidth={rem(14)} gap="m">
          <TextField
            name={RULE_FIELD.text}
            label="Ticket Text"
            hint="Matches a slug, title, or tag."
            value={filter?.text ?? ''}
          />
          <TextField
            name={RULE_FIELD.tags}
            label="Tags"
            hint="Any of these, separated by commas."
            value={join(filter?.tags)}
          />
          <TextField
            name={RULE_FIELD.statuses}
            label="Statuses"
            hint="For example completed, started."
            value={join(filter?.statuses)}
          />
          <TextField
            name={RULE_FIELD.projects}
            label="Projects"
            hint="Checkout or store ids; empty means every project."
            value={join(filter?.projects)}
          />
          <Select
            name={RULE_FIELD.minSeverity}
            label="Minimum Severity"
            value={filter?.min_severity ?? ''}
            choices={[
              { value: '', label: 'Any' },
              ...NOTIFICATION_SEVERITIES.map((severity) => ({
                value: severity,
                label: severity[0].toUpperCase() + severity.slice(1),
              })),
            ]}
          />
          <TextField
            name={RULE_FIELD.cooldown}
            label="Cooldown (Seconds)"
            type="number"
            min={0}
            hint="Skip repeats of the same event per channel."
            value={rule ? String(rule.cooldown_seconds) : ''}
          />
        </Grid>
        {editor.error ? (
          <Text tone="danger" role="alert">
            {editor.error}
          </Text>
        ) : null}
      </List>
    </DialogSurface>
  );
}
