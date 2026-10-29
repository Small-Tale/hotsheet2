import { describe, expect, it } from 'vitest';

import type { NotificationChannel, NotificationConfig, NotificationDelivery, NotifierDescriptor } from '../api';
import { CHANNEL_FIELD, RULE_FIELD } from '../notification-settings-model';
import { createNotificationSettings, type NotificationSettingsApi } from './notification-settings';

const ntfy: NotifierDescriptor = {
  id: 'ntfy',
  display_name: 'ntfy Push',
  description: '',
  capabilities: {
    formats: ['markdown'],
    max_title_bytes: 250,
    max_body_bytes: 4096,
    attachments: { max_count: 1, max_bytes: 1, max_total_bytes: 1, images_only: false },
    priority: true,
    click_url: true,
    audio: false,
  },
  settings: [
    { key: 'topic', label: 'Topic', type: 'string', required: true },
    { key: 'token', label: 'Access Token', type: 'secret', required: false },
  ],
  source: { kind: 'built_in' },
};

/** A fake with the server's shapes that records calls and stores channels and rules like the admin workflow. */
function fakeApi(options: { failSave?: string; testOutcome?: NotificationDelivery['outcome'] } = {}) {
  const calls: string[] = [];
  const config: NotificationConfig = { version: 1, channels: [], rules: [] };
  const delivery = (outcome: NotificationDelivery['outcome']): NotificationDelivery => ({
    id: 'd1',
    at: '2026-10-29T00:00:00Z',
    event: 'test',
    channel: 'phone',
    notifier: 'ntfy',
    outcome,
    attempts: 1,
    truncated: false,
    omitted_attachments: 0,
    error: outcome === 'failed' ? 'ntfy refused the message (400)' : undefined,
  });
  const api: NotificationSettingsApi = {
    notifiers: () => Promise.resolve({ notifiers: [ntfy], load_errors: [] }),
    notificationConfig: () => Promise.resolve(structuredClone(config)),
    notificationDeliveries: () => Promise.resolve([]),
    saveNotificationChannel: (id, body) => {
      calls.push(`PUT channel ${id} ${JSON.stringify(body.settings)} enabled=${body.enabled}`);
      if (options.failSave) return Promise.reject(new Error(options.failSave));
      const channel: NotificationChannel = { id, ...body };
      config.channels = [...config.channels.filter((c) => c.id !== id), channel];
      return Promise.resolve(channel);
    },
    saveNotificationChannelSecret: (id, field) => {
      calls.push(`PUT secret ${id} ${field}`);
      return Promise.resolve(undefined);
    },
    removeNotificationChannel: (id) => {
      calls.push(`DELETE channel ${id}`);
      config.channels = config.channels.filter((c) => c.id !== id);
      const removed = config.rules.filter((r) => r.channels.every((c) => c === id)).map((r) => r.id);
      config.rules = config.rules.filter((r) => !removed.includes(r.id));
      return Promise.resolve({ removed: true, removed_rules: removed, deleted_secrets: [] });
    },
    testNotificationChannel: (id) => {
      calls.push(`POST test ${id}`);
      return Promise.resolve(delivery(options.testOutcome ?? 'delivered'));
    },
    trustNotificationChannel: (id) => {
      calls.push(`POST trust ${id}`);
      return Promise.resolve(config.channels[0]);
    },
    saveNotificationRule: (id, rule) => {
      calls.push(`PUT rule ${id} ${rule.events.join(',')}`);
      config.rules = [...config.rules.filter((r) => r.id !== id), { id, ...rule }];
      return Promise.resolve({ id, ...rule });
    },
    removeNotificationRule: (id) => {
      calls.push(`DELETE rule ${id}`);
      config.rules = config.rules.filter((r) => r.id !== id);
      return Promise.resolve(undefined);
    },
    saveNotificationLinkBase: (url) => {
      calls.push(`PUT link ${String(url)}`);
      return Promise.resolve(undefined);
    },
    saveNotificationProjectTiming: (timing) => {
      calls.push(`PUT timing ${JSON.stringify(timing)}`);
      return Promise.resolve(timing);
    },
  };
  return { api, calls, config };
}

const addPhone = async (settings: ReturnType<typeof createNotificationSettings>) => {
  settings.openChannelEditor();
  await settings.saveChannel({
    [CHANNEL_FIELD.label]: 'Phone',
    [CHANNEL_FIELD.setting('topic')]: 'hs',
    [CHANNEL_FIELD.setting('token')]: 'tk',
  });
};

describe('notification settings controller (HS2-DPBJJA)', () => {
  it('adds a channel, stores its credential separately, closes the editor, and reloads', async () => {
    const fake = fakeApi();
    const settings = createNotificationSettings({ api: () => fake.api });
    await settings.refresh();
    expect(settings.notifiers.value?.notifiers[0]?.id).toBe('ntfy');
    await addPhone(settings);
    expect(fake.calls).toEqual(['PUT channel phone {"topic":"hs"} enabled=true', 'PUT secret phone token']);
    expect(settings.channelEditor.value).toBeUndefined();
    expect(settings.config.value?.channels.map((c) => c.id)).toEqual(['phone']);
    expect(settings.status.value).toBe('Channel added.');
  });

  it('keeps the editor open with the reason on invalid input or a server refusal', async () => {
    const fake = fakeApi({ failSave: 'channel phone: setting topic must be a string' });
    const settings = createNotificationSettings({ api: () => fake.api });
    await settings.refresh();
    settings.openChannelEditor();
    await settings.saveChannel({});
    expect(settings.channelEditor.value?.error).toBe('Topic is required.');
    expect(fake.calls).toEqual([]);
    await settings.saveChannel({ [CHANNEL_FIELD.setting('topic')]: 'hs' });
    expect(settings.channelEditor.value).toMatchObject({
      busy: false,
      error: 'channel phone: setting topic must be a string',
    });
    settings.closeChannelEditor();
    expect(settings.channelEditor.value).toBeUndefined();
  });

  it('edits keep the channel id and notifier, and the notifier can change only while adding', async () => {
    const fake = fakeApi();
    const settings = createNotificationSettings({ api: () => fake.api });
    await settings.refresh();
    await addPhone(settings);
    settings.openChannelEditor('phone');
    settings.chooseNotifier('email');
    expect(settings.channelEditor.value).toMatchObject({ id: 'phone', notifier: 'ntfy' });
    await settings.saveChannel({ [CHANNEL_FIELD.setting('topic')]: 'hs2' });
    expect(fake.calls.at(-1)).toBe('PUT channel phone {"topic":"hs2"} enabled=true');
    expect(settings.status.value).toBe('Channel saved.');
    settings.openChannelEditor();
    settings.chooseNotifier('email');
    expect(settings.channelEditor.value?.notifier).toBe('email');
  });

  it('tests, disables, trusts, and removes, reporting a failed test in place', async () => {
    const fake = fakeApi({ testOutcome: 'failed' });
    const settings = createNotificationSettings({ api: () => fake.api });
    await settings.refresh();
    await addPhone(settings);
    await settings.testChannel('phone');
    expect(settings.message.value).toBe('ntfy refused the message (400)');
    expect(settings.busyChannels.value).toEqual([]);
    await settings.setChannelEnabled('phone', false);
    expect(settings.config.value?.channels[0]?.enabled).toBe(false);
    expect(settings.message.value).toBe('');
    await settings.trustChannel('phone');
    expect(settings.status.value).toBe('Plugin approved.');
    await settings.saveRule({}, [], []);
    settings.openRuleEditor();
    await settings.saveRule({ [RULE_FIELD.label]: 'Stopped' }, ['project.stopped'], ['phone']);
    expect(settings.config.value?.rules.map((r) => r.id)).toEqual(['stopped']);
    await settings.removeChannel('phone');
    expect(settings.status.value).toBe('Channel removed with 1 rule that used only it.');
    expect(settings.config.value).toMatchObject({ channels: [], rules: [] });
  });

  it('validates and saves delivery options', async () => {
    const fake = fakeApi();
    const settings = createNotificationSettings({ api: () => fake.api });
    await settings.saveOption('link-base', ' https://hs.lan ');
    await settings.saveOption('link-base', '');
    await settings.saveOption('idle', '0');
    await settings.saveOption('grace', 'soon');
    expect(fake.calls).toEqual(['PUT link https://hs.lan', 'PUT link null', 'PUT timing {"idle_minutes":0}']);
    expect(settings.message.value).toBe('Enter a whole number.');
  });

  it('a rule editor rejects an empty selection without a request', async () => {
    const fake = fakeApi();
    const settings = createNotificationSettings({ api: () => fake.api });
    settings.openRuleEditor();
    await settings.saveRule({}, [], ['phone']);
    expect(settings.ruleEditor.value?.error).toBe('Choose at least one event.');
    settings.closeRuleEditor();
    await settings.removeRule('none');
    expect(fake.calls).toEqual(['DELETE rule none']);
  });

  it('does nothing without a server', async () => {
    const settings = createNotificationSettings({ api: () => undefined });
    await settings.refresh();
    expect(await settings.saveOption('idle', '5')).toBe(false);
    expect(settings.config.value).toBeUndefined();
  });
});
