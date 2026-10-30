import { describe, expect, it } from 'vitest';

import type { NotificationChannel, NotifierDescriptor } from './api';
import {
  CHANNEL_FIELD,
  channelDraftFromForm,
  channelFormFields,
  channelIdFrom,
  eventLabel,
  notifierIconName,
  RULE_FIELD,
  ruleFromForm,
  ruleSummary,
} from './notification-settings-model';

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
    { key: 'server', label: 'Server', type: 'string', required: false, default: 'https://ntfy.sh' },
    { key: 'topic', label: 'Topic', type: 'string', required: true },
    { key: 'token', label: 'Access Token', type: 'secret', required: false },
    { key: 'priority', label: 'Priority', type: 'choice', required: false, choices: ['auto', 'high'], default: 'auto' },
    { key: 'tags', label: 'Tags', type: 'list', required: false },
    { key: 'rate', label: 'Rate', type: 'number', required: false },
    { key: 'loud', label: 'Loud', type: 'bool', required: false },
  ],
  source: { kind: 'built_in' },
};

const phone: NotificationChannel = {
  id: 'phone',
  notifier: 'ntfy',
  label: 'My Phone',
  enabled: true,
  settings: { topic: 'hs', token: { secret: 'notify-phone-token' }, tags: ['a', 'b'], loud: true },
};

describe('channelFormFields (HS2-DPBJJA)', () => {
  it('fills values, defaults, lists, and booleans, and never shows a stored credential', () => {
    const fields = Object.fromEntries(channelFormFields(ntfy, phone).map((field) => [field.key, field]));
    expect(fields.server.value).toBe('https://ntfy.sh');
    expect(fields.topic.value).toBe('hs');
    expect(fields.tags.value).toBe('a, b');
    expect(fields.loud.checked).toBe(true);
    expect(fields.token.value).toBe('');
    expect(fields.token.stored).toBe(true);
    expect(fields.priority.choices).toEqual(['auto', 'high']);
    expect(channelFormFields(ntfy).find((field) => field.key === 'token')?.stored).toBe(false);
  });
});

describe('channelDraftFromForm (HS2-DPBJJA)', () => {
  const values = (extra: Record<string, string | boolean>) => ({
    [CHANNEL_FIELD.setting('topic')]: ' hs ',
    ...extra,
  });

  it('types values by schema and routes credentials to the secret route only', () => {
    const draft = channelDraftFromForm(
      ntfy,
      values({
        [CHANNEL_FIELD.label]: 'Phone',
        [CHANNEL_FIELD.setting('tags')]: 'x, y,',
        [CHANNEL_FIELD.setting('rate')]: '3',
        [CHANNEL_FIELD.setting('loud')]: true,
        [CHANNEL_FIELD.setting('token')]: 'tk',
        [CHANNEL_FIELD.setting('priority')]: 'high',
      }),
    );
    expect(draft).toEqual({
      label: 'Phone',
      settings: { topic: 'hs', tags: ['x', 'y'], rate: 3, loud: true, priority: 'high' },
      secrets: { token: 'tk' },
      quiet_hours: undefined,
      max_per_hour: undefined,
    });
  });

  it('keeps a stored credential when its input is left empty', () => {
    const draft = channelDraftFromForm(ntfy, values({}), phone);
    expect('error' in draft ? draft.error : draft.secrets).toEqual({});
    expect('error' in draft ? undefined : draft.settings).not.toHaveProperty('token');
  });

  it('reads quiet hours and the hourly limit', () => {
    const draft = channelDraftFromForm(
      ntfy,
      values({
        [CHANNEL_FIELD.quietStart]: '22:00',
        [CHANNEL_FIELD.quietEnd]: '07:00',
        [CHANNEL_FIELD.quietOffset]: '+09:00',
        [CHANNEL_FIELD.maxPerHour]: '6',
      }),
    );
    expect('error' in draft ? draft : draft.quiet_hours).toEqual({
      start: '22:00',
      end: '07:00',
      utc_offset: '+09:00',
      allow_errors: true,
    });
    expect('error' in draft ? undefined : draft.max_per_hour).toBe(6);
  });

  it('explains the first invalid value', () => {
    expect(channelDraftFromForm(ntfy, {})).toEqual({ error: 'Topic is required.' });
    expect(channelDraftFromForm(ntfy, values({ [CHANNEL_FIELD.setting('rate')]: 'fast' }))).toEqual({
      error: 'Rate must be a number.',
    });
    expect(channelDraftFromForm(ntfy, values({ [CHANNEL_FIELD.setting('priority')]: 'loudest' }))).toEqual({
      error: 'Priority must be one of auto, high.',
    });
    expect(channelDraftFromForm(ntfy, values({ [CHANNEL_FIELD.quietStart]: '22:00' }))).toEqual({
      error: 'Quiet hours need both a start and an end time.',
    });
    expect(channelDraftFromForm(ntfy, values({ [CHANNEL_FIELD.maxPerHour]: '0' }))).toEqual({
      error: 'Max per hour must be a whole number above 0.',
    });
  });
});

describe('channelIdFrom', () => {
  it('slugs labels and avoids taken ids', () => {
    expect(channelIdFrom('My Phone!', [])).toBe('my-phone');
    expect(channelIdFrom('My Phone', ['my-phone', 'my-phone-2'])).toBe('my-phone-3');
    expect(channelIdFrom('***', [])).toBe('channel');
  });
});

describe('rules (HS2-DPBJJA)', () => {
  it('reads events, channels, filters, and cooldown', () => {
    const rule = ruleFromForm(
      {
        [RULE_FIELD.text]: 'release',
        [RULE_FIELD.tags]: 'ship, urgent',
        [RULE_FIELD.minSeverity]: 'warning',
        [RULE_FIELD.cooldown]: '600',
      },
      ['ticket.blocked'],
      ['phone'],
    );
    expect(rule).toEqual({
      events: ['ticket.blocked'],
      channels: ['phone'],
      enabled: true,
      filter: { text: 'release', tags: ['ship', 'urgent'], min_severity: 'warning' },
      cooldown_seconds: 600,
    });
    expect(ruleFromForm({}, ['digest'], ['phone'])).toMatchObject({ filter: undefined, cooldown_seconds: 0 });
    expect(ruleFromForm({}, [], ['phone'])).toEqual({ error: 'Choose at least one event.' });
    expect(ruleFromForm({}, ['digest'], [])).toEqual({ error: 'Choose at least one channel.' });
    expect(ruleFromForm({ [RULE_FIELD.cooldown]: '-1' }, ['digest'], ['phone'])).toEqual({
      error: 'Cooldown must be a whole number of seconds.',
    });
  });

  it('keeps filters the form does not show when a rule is edited (HS2-G8NXNC)', () => {
    const kept = {
      tags: ['old'],
      categories: ['bug'],
      priorities: ['high'],
      views: [{ checkout: 'web-1a2b', view: 'needs-docs' }],
    };
    expect(ruleFromForm({ [RULE_FIELD.tags]: 'new' }, ['ticket.blocked'], ['phone'], kept)).toMatchObject({
      filter: {
        tags: ['new'],
        categories: ['bug'],
        priorities: ['high'],
        views: [{ checkout: 'web-1a2b', view: 'needs-docs' }],
      },
    });
    expect(ruleFromForm({}, ['ticket.blocked'], ['phone'], { tags: ['old'] })).toMatchObject({ filter: undefined });
  });

  it('summarizes a rule with channel labels and filters', () => {
    expect(
      ruleSummary(
        {
          id: 'r',
          events: ['ticket.blocked'],
          channels: ['phone', 'gone'],
          enabled: true,
          filter: {
            tags: ['release'],
            min_severity: 'error',
            views: [{ checkout: 'web-1a2b', view: 'needs-docs' }],
          },
          cooldown_seconds: 60,
        },
        [phone],
      ),
    ).toBe('To My Phone, gone · tag release · view web-1a2b/needs-docs · error or higher · cooldown 60s');
  });

  it('labels rule events and the held-notification summary (HS2-CFMSP2)', () => {
    expect(eventLabel('ticket.blocked')).toBe('Blocked');
    expect(eventLabel('notifications.held')).toBe('Held Notifications');
    expect(eventLabel('custom.event')).toBe('custom.event');
  });

  it('maps notifiers to icons with a plugin fallback', () => {
    expect(notifierIconName('email')).toBe('mail');
    expect(notifierIconName('custom-pager')).toBe('puzzle');
  });
});
