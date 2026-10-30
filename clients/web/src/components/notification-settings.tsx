import { Badge } from '@kerfjs/ui/badge';
import { rem } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import {
  BellRing,
  CircleCheck,
  CircleMinus,
  CircleX,
  Mail,
  MessageCircle,
  MessageSquare,
  MoreHorizontal,
  Pencil,
  Plus,
  Power,
  PowerOff,
  Puzzle,
  RefreshCw,
  Send,
  ShieldCheck,
  Smartphone,
  Trash2,
  Volume2,
  Webhook,
} from 'lucide';

import type { NotificationChannel, NotificationConfig, NotificationDelivery, NotifierList } from '../api';
import type { ChannelEditorState, RuleEditorState, SavedViewChoice } from '../features/notification-settings';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { eventLabel, notifierIconName, OPTIONS_FIELD, ruleSummary } from '../notification-settings-model';
import { NotificationChannelDialog } from './notification-channel-dialog';
import { NotificationRuleDialog } from './notification-rule-dialog';

export interface NotificationSettingsProps {
  /** Installed notifiers; absent while loading. */
  notifiers?: NotifierList;
  /** Machine-wide channels and rules; absent while loading. */
  config?: NotificationConfig;
  deliveries?: readonly NotificationDelivery[];
  /** The last failure, shown until the next change. */
  message?: string;
  /** The last success, such as "Channel added." */
  status?: string;
  /** Channels with a test, toggle, trust, or removal in flight. */
  busyChannels?: readonly string[];
  channelEditor?: ChannelEditorState;
  ruleEditor?: RuleEditorState;
  /** The open projects' saved views a rule can name (HS2-DNQFB8). */
  viewChoices?: readonly SavedViewChoice[];
}

const ICONS = {
  'volume-2': Volume2,
  smartphone: Smartphone,
  mail: Mail,
  webhook: Webhook,
  'message-square': MessageSquare,
  'message-circle': MessageCircle,
  puzzle: Puzzle,
} as const;

const notifierIcon = (notifier: string) => {
  const name = notifierIconName(notifier) as keyof typeof ICONS;
  return <LucideIcon icon={ICONS[name]} name={name} />;
};

const ACTIONS = NOTIFICATIONS_AND_LINKS_ACTIONS;

const outcomeIcon = {
  delivered: <LucideIcon icon={CircleCheck} name="circle-check" />,
  failed: <LucideIcon icon={CircleX} name="circle-x" />,
  suppressed: <LucideIcon icon={CircleMinus} name="circle-minus" />,
} as const;

function deliveryTime(at: string): string {
  const date = new Date(at);
  return Number.isNaN(date.getTime())
    ? at
    : date.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
}

/**
 * App Settings → Notifications (HS2-DPBJJA, docs/32): delivery options, channels, rules, and recent
 * deliveries. Machine-wide, so it sits with the app settings. Every action is a delegated `data-action`
 * handled in `interactions/notifications-and-links.ts`.
 */
export function NotificationSettings({
  notifiers,
  config,
  deliveries,
  message = '',
  status = '',
  busyChannels = [],
  channelEditor,
  ruleEditor,
  viewChoices = [],
}: NotificationSettingsProps) {
  const loading = !config || !notifiers,
    channels = config?.channels ?? [],
    rules = config?.rules ?? [],
    descriptors = notifiers?.notifiers ?? [],
    displayName = (channel: NotificationChannel) =>
      descriptors.find((item) => item.id === channel.notifier)?.display_name ?? channel.notifier;
  return (
    <div data-component="notification-settings">
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Notifications</Text>
          <Text tone="quiet">
            Send Hot Sheet events to this computer’s voice, a phone, email, or team chat. These settings apply to every
            project on this machine.
          </Text>
          {notifiers?.load_errors.length ? (
            <Text tone="danger" size="compact" role="alert">
              {`Some notifier plugins could not load: ${notifiers.load_errors.join('; ')}`}
            </Text>
          ) : null}
        </List>
        <div {...ACTIONS.saveNotificationOption.attrs}>
          <Grid minColumnWidth={rem(15)} gap="m">
            <TextField
              name={OPTIONS_FIELD.linkBase}
              label="Link Origin"
              placeholder="https://hotsheet.example.net"
              hint="Links in notifications open Hot Sheet here. Use a network address to open them on a phone; empty uses this computer."
              value={config?.link_base ?? ''}
              disabled={loading}
            />
            <TextField
              name={OPTIONS_FIELD.graceSeconds}
              label="Stopped After (Seconds)"
              type="number"
              min={1}
              hint="How long a project may hold no ticket claim before it counts as stopped."
              value={String(config?.project_activity?.grace_seconds ?? 120)}
              disabled={loading}
            />
            <TextField
              name={OPTIONS_FIELD.idleMinutes}
              label="Stuck After (Minutes)"
              type="number"
              min={0}
              hint="How long claims may be held without progress; 0 turns this off."
              value={String(config?.project_activity?.idle_minutes ?? 30)}
              disabled={loading}
            />
          </Grid>
        </div>
        <List gap="xs">
          <ListHeader
            label="Channels"
            action={ACTIONS.addNotificationChannel.value}
            actionLabel="Add Channel…"
            actionIcon={<LucideIcon icon={Plus} name="plus" />}
            actionDisabled={loading}
          />
          {loading ? (
            <List gap={rem(0.125)}>
              <ListActionRow
                label="Loading"
                action={ACTIONS.editNotificationChannel.value}
                placeholder
                trailingMenu={{
                  label: 'Channel Actions',
                  icon: <LucideIcon icon={MoreHorizontal} name="more-horizontal" />,
                  items: [],
                }}
              />
            </List>
          ) : channels.length === 0 ? (
            <EmptyState
              title="No channels yet."
              detail="Add a channel to send notifications to speech, ntfy, email, or chat."
            />
          ) : (
            <List gap={rem(0.125)}>
              {channels.map((channel) => {
                const descriptor = descriptors.find((item) => item.id === channel.notifier),
                  external = descriptor?.source.kind === 'external',
                  name = channel.label ?? channel.id,
                  attributes = { 'data-channel-id': channel.id };
                return (
                  <ListActionRow
                    label={name}
                    description={displayName(channel)}
                    status={
                      channel.enabled ? undefined : (
                        <Badge tone="neutral" appearance="quiet" size="compact">
                          Disabled
                        </Badge>
                      )
                    }
                    icon={notifierIcon(channel.notifier)}
                    iconTile="neutral"
                    action={ACTIONS.editNotificationChannel.value}
                    itemId={channel.id}
                    busy={busyChannels.includes(channel.id)}
                    rootAttributes={{ 'data-notification-channel': channel.id }}
                    trailingMenu={{
                      label: `Actions for ${name}`,
                      icon: <LucideIcon icon={MoreHorizontal} name="more-horizontal" />,
                      items: [
                        {
                          label: 'Send Test',
                          action: ACTIONS.testNotificationChannel.value,
                          icon: <LucideIcon icon={Send} name="send" />,
                          attributes,
                        },
                        {
                          label: 'Edit…',
                          action: ACTIONS.editNotificationChannel.value,
                          icon: <LucideIcon icon={Pencil} name="pencil" />,
                          attributes,
                        },
                        {
                          label: channel.enabled ? 'Disable' : 'Enable',
                          action: ACTIONS.toggleNotificationChannel.value,
                          icon: channel.enabled ? (
                            <LucideIcon icon={PowerOff} name="power-off" />
                          ) : (
                            <LucideIcon icon={Power} name="power" />
                          ),
                          attributes,
                        },
                        ...(external
                          ? [
                              {
                                label: 'Approve Plugin',
                                action: ACTIONS.trustNotificationChannel.value,
                                icon: <LucideIcon icon={ShieldCheck} name="shield-check" />,
                                attributes,
                              },
                            ]
                          : []),
                        { kind: 'divider' as const },
                        {
                          label: 'Remove',
                          action: ACTIONS.removeNotificationChannel.value,
                          tone: 'danger' as const,
                          icon: <LucideIcon icon={Trash2} name="trash-2" />,
                          attributes,
                        },
                      ],
                    }}
                  />
                );
              })}
            </List>
          )}
        </List>
        <List gap="xs">
          <ListHeader
            label="Rules"
            action={ACTIONS.addNotificationRule.value}
            actionLabel="Add Rule…"
            actionIcon={<LucideIcon icon={Plus} name="plus" />}
            actionDisabled={loading || channels.length === 0}
            disabledReason={channels.length === 0 ? 'Add a channel first.' : undefined}
          />
          {!loading && rules.length === 0 ? (
            <EmptyState title="No rules yet." detail="Rules choose which events go to which channels." />
          ) : (
            <List gap={rem(0.125)}>
              {rules.map((rule) => {
                const name = rule.events.map(eventLabel).join(', '),
                  attributes = { 'data-rule-id': rule.id };
                return (
                  <ListActionRow
                    label={name}
                    description={ruleSummary(rule, channels)}
                    icon={<LucideIcon icon={BellRing} name="bell-ring" />}
                    iconTile="neutral"
                    action={ACTIONS.editNotificationRule.value}
                    itemId={rule.id}
                    rootAttributes={{ 'data-notification-rule': rule.id }}
                    trailingMenu={{
                      label: `Actions for ${name}`,
                      icon: <LucideIcon icon={MoreHorizontal} name="more-horizontal" />,
                      items: [
                        {
                          label: 'Edit…',
                          action: ACTIONS.editNotificationRule.value,
                          icon: <LucideIcon icon={Pencil} name="pencil" />,
                          attributes,
                        },
                        {
                          label: 'Remove',
                          action: ACTIONS.removeNotificationRule.value,
                          tone: 'danger',
                          icon: <LucideIcon icon={Trash2} name="trash-2" />,
                          attributes,
                        },
                      ],
                    }}
                  />
                );
              })}
            </List>
          )}
        </List>
        <List gap="xs">
          <ListHeader
            label="Recent Deliveries"
            action={ACTIONS.refreshNotificationDeliveries.value}
            actionLabel="Refresh"
            actionIcon={<LucideIcon icon={RefreshCw} name="refresh-cw" />}
            actionDisabled={loading}
          />
          {deliveries?.length ? (
            <ValueTable label="Recent Deliveries" density="compact" stackBelow="compact">
              {deliveries.map((delivery) => (
                <ValueTableRow
                  icon={outcomeIcon[delivery.outcome]}
                  label={`${deliveryTime(delivery.at)} · ${eventLabel(delivery.event)}`}
                  value={`${delivery.outcome[0].toUpperCase()}${delivery.outcome.slice(1)} to ${delivery.channel}${
                    delivery.ticket ? ` (${delivery.ticket})` : ''
                  }${delivery.error ? `: ${delivery.error}` : ''}`}
                />
              ))}
            </ValueTable>
          ) : (
            <Text tone="quiet" size="compact">
              {loading ? 'Loading deliveries…' : 'No deliveries yet.'}
            </Text>
          )}
        </List>
        <Text variant="span" tone={message ? 'danger' : 'quiet'} size="compact" role="status">
          {message || status}
        </Text>
      </List>
      {channelEditor && notifiers ? (
        <NotificationChannelDialog
          editor={channelEditor}
          notifiers={notifiers.notifiers}
          channel={channels.find((channel) => channel.id === channelEditor.id)}
        />
      ) : null}
      {ruleEditor ? (
        <NotificationRuleDialog
          editor={ruleEditor}
          channels={channels}
          viewChoices={viewChoices}
          rule={rules.find((rule) => rule.id === ruleEditor.id)}
        />
      ) : null}
    </div>
  );
}
