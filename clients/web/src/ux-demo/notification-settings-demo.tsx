import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';

import type {
  NotificationChannel,
  NotificationConfig,
  NotificationDelivery,
  NotifierDescriptor,
  NotifierList,
} from '../api';
import { NotificationChannelDialog } from '../components/notification-channel-dialog';
import { NotificationRuleDialog } from '../components/notification-rule-dialog';
import { NotificationSettings } from '../components/notification-settings';
import { wireNotificationSettings } from '../components/notification-settings-interactions';
import { createNotificationSettings, type NotificationSettingsApi } from '../features/notification-settings';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import type { DemoModule } from './demo-module';

const capabilities = (formats: string[], body: number): NotifierDescriptor['capabilities'] => ({
  formats,
  max_title_bytes: 250,
  max_body_bytes: body,
  attachments: { max_count: 0, max_bytes: 0, max_total_bytes: 0, images_only: false },
  priority: true,
  click_url: true,
  audio: false,
});

const NOTIFIERS: NotifierList = {
  notifiers: [
    {
      id: 'speak',
      display_name: 'Speak Aloud',
      description: 'Reads notifications aloud on this machine with its text-to-speech voice.',
      capabilities: { ...capabilities(['plain'], 400), audio: true },
      settings: [{ key: 'voice', label: 'Voice', type: 'string', required: false }],
      source: { kind: 'built_in' },
    },
    {
      id: 'ntfy',
      display_name: 'ntfy Push',
      description: 'Sends push notifications to phones and desktops subscribed to an ntfy topic.',
      capabilities: capabilities(['markdown', 'plain'], 4096),
      settings: [
        { key: 'server', label: 'Server', type: 'string', required: false, default: 'https://ntfy.sh' },
        { key: 'topic', label: 'Topic', type: 'string', required: true },
        { key: 'token', label: 'Access Token', type: 'secret', required: false },
        {
          key: 'priority',
          label: 'Priority',
          type: 'choice',
          required: false,
          choices: ['auto', 'low', 'default', 'high', 'urgent'],
          default: 'auto',
        },
        { key: 'tags', label: 'Tags', type: 'list', required: false },
      ],
      source: { kind: 'built_in' },
    },
    {
      id: 'slack',
      display_name: 'Slack',
      description: 'Posts to a Slack channel through an incoming webhook.',
      capabilities: capabilities(['markdown'], 2800),
      settings: [{ key: 'webhook_url', label: 'Incoming Webhook URL', type: 'secret', required: true }],
      source: { kind: 'built_in' },
    },
    {
      id: 'pager',
      display_name: 'Team Pager',
      description: 'An external notifier plugin installed on this machine.',
      capabilities: capabilities(['plain'], 1000),
      settings: [{ key: 'escalate', label: 'Escalate Errors', type: 'bool', required: false }],
      source: { kind: 'external', dir: 'notifiers/pager' },
    },
  ],
  load_errors: [],
};

function fixtureConfig(): NotificationConfig {
  return {
    version: 1,
    link_base: 'https://hotsheet.example.net',
    project_activity: { grace_seconds: 120, idle_minutes: 30 },
    channels: [
      {
        id: 'phone',
        notifier: 'ntfy',
        label: 'My Phone',
        enabled: true,
        settings: { topic: 'hs-7f3k', token: { secret: 'notify-phone-token' }, priority: 'auto' },
        quiet_hours: { start: '22:00', end: '07:00', utc_offset: '-08:00', allow_errors: true },
      },
      {
        id: 'team',
        notifier: 'slack',
        label: 'Release Channel in the Platform Team Workspace',
        enabled: false,
        settings: { webhook_url: { secret: 'notify-team-webhook_url' } },
      },
      { id: 'pager', notifier: 'pager', enabled: true, settings: {}, plugin_digest: 'abc' },
    ],
    rules: [
      {
        id: 'attention',
        events: ['ticket.feedback_needed', 'permission.requested'],
        channels: ['phone'],
        enabled: true,
        filter: { min_severity: 'warning' },
        cooldown_seconds: 600,
      },
      {
        id: 'stopped',
        events: ['project.stopped'],
        channels: ['phone', 'team'],
        enabled: true,
        cooldown_seconds: 0,
      },
    ],
  };
}

const DELIVERIES: NotificationDelivery[] = [
  {
    id: 'd3',
    at: '2026-10-29T17:42:00Z',
    event: 'project.stopped',
    channel: 'phone',
    notifier: 'ntfy',
    outcome: 'delivered',
    attempts: 1,
    truncated: false,
    omitted_attachments: 0,
  },
  {
    id: 'd2',
    at: '2026-10-29T17:10:00Z',
    event: 'ticket.feedback_needed',
    channel: 'team',
    notifier: 'slack',
    outcome: 'failed',
    attempts: 3,
    error: 'Slack is unavailable (503)',
    ticket: 'HS2-7F3K9Q',
    truncated: false,
    omitted_attachments: 0,
  },
  {
    id: 'd1',
    at: '2026-10-29T03:15:00Z',
    event: 'permission.requested',
    channel: 'phone',
    notifier: 'ntfy',
    outcome: 'suppressed',
    attempts: 0,
    error: 'quiet hours 22:00-07:00 (-08:00)',
    truncated: false,
    omitted_attachments: 0,
  },
];

let config = fixtureConfig();

/** An in-memory server with the admin workflow's shapes, so the demo runs the real controller. */
const demoApi: NotificationSettingsApi = {
  notifiers: () => Promise.resolve(NOTIFIERS),
  notificationConfig: () => Promise.resolve(structuredClone(config)),
  notificationDeliveries: () => Promise.resolve(DELIVERIES),
  saveNotificationChannel: (id, body) => {
    const channel: NotificationChannel = { id, ...body };
    config.channels = [...config.channels.filter((item) => item.id !== id), channel];
    return Promise.resolve(channel);
  },
  saveNotificationChannelSecret: (id, field) => {
    const channel = config.channels.find((item) => item.id === id);
    if (channel) channel.settings[field] = { secret: `notify-${id}-${field}` };
    return Promise.resolve(undefined);
  },
  removeNotificationChannel: (id) => {
    config.channels = config.channels.filter((item) => item.id !== id);
    const removed = config.rules.filter((rule) => rule.channels.every((channel) => channel === id)).map((r) => r.id);
    config.rules = config.rules
      .filter((rule) => !removed.includes(rule.id))
      .map((rule) => ({ ...rule, channels: rule.channels.filter((channel) => channel !== id) }));
    return Promise.resolve({ removed: true, removed_rules: removed, deleted_secrets: [] });
  },
  testNotificationChannel: (id) => Promise.resolve({ ...DELIVERIES[0], id: `test-${id}`, event: 'test', channel: id }),
  trustNotificationChannel: (id) => Promise.resolve(config.channels.find((item) => item.id === id)!),
  saveNotificationRule: (id, rule) => {
    config.rules = [...config.rules.filter((item) => item.id !== id), { id, ...rule }];
    return Promise.resolve({ id, ...rule });
  },
  removeNotificationRule: (id) => {
    config.rules = config.rules.filter((item) => item.id !== id);
    return Promise.resolve(undefined);
  },
  saveNotificationLinkBase: (url) => {
    config.link_base = url ?? undefined;
    return Promise.resolve(undefined);
  },
  saveNotificationProjectTiming: (timing) => {
    config.project_activity = { ...config.project_activity, ...timing };
    return Promise.resolve(config.project_activity);
  },
};

const settings = createNotificationSettings({ api: () => demoApi });

function resetNotificationSettingsDemo() {
  config = fixtureConfig();
  settings.closeChannelEditor();
  settings.closeRuleEditor();
  settings.message.value = '';
  settings.status.value = '';
  void settings.refresh();
}

function liveProps() {
  return {
    notifiers: settings.notifiers.value,
    config: settings.config.value,
    deliveries: settings.deliveries.value,
    message: settings.message.value,
    status: settings.status.value,
    busyChannels: settings.busyChannels.value,
    channelEditor: settings.channelEditor.value,
    ruleEditor: settings.ruleEditor.value,
  };
}

export function NotificationSettingsDemo() {
  return (
    <>
      <CatalogExample label="Interactive">
        <NotificationSettings {...liveProps()} />
      </CatalogExample>
      <CatalogExample label="Loading">
        <NotificationSettings />
      </CatalogExample>
      <CatalogExample label="Empty">
        <NotificationSettings notifiers={NOTIFIERS} config={{ version: 1, channels: [], rules: [] }} deliveries={[]} />
      </CatalogExample>
    </>
  );
}

export function NotificationChannelDialogDemo() {
  const editor = settings.channelEditor.value;
  return (
    <CatalogExample align="glyph" label="Interactive">
      <Button action={NOTIFICATIONS_AND_LINKS_ACTIONS.addNotificationChannel.value}>Add Channel…</Button>
      <Button
        action={NOTIFICATIONS_AND_LINKS_ACTIONS.editNotificationChannel.value}
        rootAttributes={{ 'data-channel-id': 'phone' }}
      >
        Edit My Phone…
      </Button>
      {editor && settings.notifiers.value ? (
        <NotificationChannelDialog
          editor={editor}
          notifiers={settings.notifiers.value.notifiers}
          channel={settings.config.value?.channels.find((channel) => channel.id === editor.id)}
        />
      ) : null}
    </CatalogExample>
  );
}

export function NotificationRuleDialogDemo() {
  const editor = settings.ruleEditor.value;
  return (
    <CatalogExample align="glyph" label="Interactive">
      <Button action={NOTIFICATIONS_AND_LINKS_ACTIONS.addNotificationRule.value}>Add Rule…</Button>
      <Button
        action={NOTIFICATIONS_AND_LINKS_ACTIONS.editNotificationRule.value}
        rootAttributes={{ 'data-rule-id': 'attention' }}
      >
        Edit Attention Rule…
      </Button>
      {editor ? (
        <NotificationRuleDialog
          editor={editor}
          channels={settings.config.value?.channels ?? []}
          rule={settings.config.value?.rules.find((rule) => rule.id === editor.id)}
        />
      ) : null}
    </CatalogExample>
  );
}

/** The catalog entries this file renders (HS2-DPBJJA): the production controller and handlers over fixtures. */
export const demoModule: DemoModule = {
  entries: {
    'notification-settings': {
      catalogExamples: true,
      Demo: NotificationSettingsDemo,
      reset: resetNotificationSettingsDemo,
    },
    'notification-channel-dialog': {
      catalogExamples: true,
      Demo: NotificationChannelDialogDemo,
      reset: resetNotificationSettingsDemo,
    },
    'notification-rule-dialog': {
      catalogExamples: true,
      Demo: NotificationRuleDialogDemo,
      reset: resetNotificationSettingsDemo,
    },
  },
  wire: (root) => {
    void settings.refresh();
    return wireNotificationSettings(root, settings);
  },
};
