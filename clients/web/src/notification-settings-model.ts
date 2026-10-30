/**
 * Pure mapping between notifier descriptors, configured channels and rules, and the App Settings →
 * Notifications forms (HS2-DPBJJA, docs/32). The form is generated from each notifier's typed settings
 * schema, so a new or external notifier needs no client change. Credential values are write-only: a
 * secret field never shows its stored value, and an empty secret input keeps the stored credential.
 */
import type {
  NotificationChannel,
  NotificationQuietHours,
  NotificationRule,
  NotificationRuleFilter,
  NotifierDescriptor,
  NotifierSettingField,
} from './api';

/** Rule event kinds, in the order the rule editor offers them, with their labels. */
export const NOTIFICATION_EVENTS = [
  { value: 'ticket.status_changed', label: 'Status Changed' },
  { value: 'ticket.feedback_needed', label: 'Feedback Needed' },
  { value: 'ticket.blocked', label: 'Blocked' },
  { value: 'permission.requested', label: 'Permission Requested' },
  { value: 'project.stopped', label: 'Project Stopped or Stuck' },
  { value: 'notification', label: 'Published Notification' },
  { value: 'digest', label: 'Digest' },
] as const;

export const NOTIFICATION_SEVERITIES = ['info', 'success', 'warning', 'error'] as const;

/** Form-control names: `setting:<key>` for notifier settings, fixed names for everything else. */
export const CHANNEL_FIELD = {
  notifier: 'notification-channel-notifier',
  label: 'notification-channel-label',
  setting: (key: string) => `setting:${key}`,
  quietStart: 'notification-channel-quiet-start',
  quietEnd: 'notification-channel-quiet-end',
  quietOffset: 'notification-channel-quiet-offset',
  maxPerHour: 'notification-channel-max-per-hour',
} as const;
export const RULE_FIELD = {
  label: 'notification-rule-label',
  events: 'notification-rule-events',
  channels: 'notification-rule-channels',
  text: 'notification-rule-text',
  tags: 'notification-rule-tags',
  statuses: 'notification-rule-statuses',
  projects: 'notification-rule-projects',
  minSeverity: 'notification-rule-min-severity',
  cooldown: 'notification-rule-cooldown',
} as const;
export const OPTIONS_FIELD = {
  linkBase: 'notification-link-base',
  graceSeconds: 'notification-grace-seconds',
  idleMinutes: 'notification-idle-minutes',
} as const;

/** One generated form control. */
export interface ChannelFormField {
  key: string;
  name: string;
  label: string;
  type: NotifierSettingField['type'];
  value: string;
  checked: boolean;
  choices: readonly string[];
  hint?: string;
  required: boolean;
  /** A secret field already has a stored credential. */
  stored: boolean;
}

const secretName = (value: unknown): string | undefined =>
  typeof value === 'object' && value !== null && typeof (value as { secret?: unknown }).secret === 'string'
    ? (value as { secret: string }).secret
    : undefined;

function asText(value: unknown): string {
  if (Array.isArray(value)) return value.map(asText).join(', ');
  if (typeof value === 'string') return value;
  return typeof value === 'number' || typeof value === 'boolean' ? String(value) : '';
}

/** The form fields for `descriptor`, filled from `channel` when editing one. */
export function channelFormFields(descriptor: NotifierDescriptor, channel?: NotificationChannel): ChannelFormField[] {
  return descriptor.settings.map((field) => {
    const configured = channel?.settings[field.key];
    const value = configured ?? (field.type === 'secret' ? undefined : field.default);
    return {
      key: field.key,
      name: CHANNEL_FIELD.setting(field.key),
      label: field.label,
      type: field.type,
      value: field.type === 'secret' ? '' : asText(value),
      checked: value === true,
      choices: field.choices ?? [],
      hint: field.help,
      required: field.required,
      stored: field.type === 'secret' && secretName(configured) !== undefined,
    };
  });
}

/** Form values as the dialog reads them: text inputs by name, checkboxes as booleans. */
export type FormValues = Readonly<Record<string, string | boolean | undefined>>;

const text = (values: FormValues, name: string) => {
  const value = values[name];
  return typeof value === 'string' ? value.trim() : '';
};
const list = (value: string) =>
  value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);

export interface ChannelDraft {
  label?: string;
  settings: Record<string, unknown>;
  /** Credential values to store through the secret route, by field key. */
  secrets: Record<string, string>;
  quiet_hours?: NotificationQuietHours;
  max_per_hour?: number;
}

/** Read the channel form into a request body, or explain the first invalid value. */
export function channelDraftFromForm(
  descriptor: NotifierDescriptor,
  values: FormValues,
  existing?: NotificationChannel,
): ChannelDraft | { error: string } {
  const settings: Record<string, unknown> = {};
  const secrets: Record<string, string> = {};
  for (const field of descriptor.settings) {
    const name = CHANNEL_FIELD.setting(field.key);
    if (field.type === 'bool') {
      settings[field.key] = values[name] === true;
      continue;
    }
    const raw = text(values, name);
    if (field.type === 'secret') {
      if (raw) secrets[field.key] = raw;
      continue;
    }
    if (!raw) {
      if (field.required && field.default === undefined) return { error: `${field.label} is required.` };
      continue;
    }
    if (field.type === 'list') settings[field.key] = list(raw);
    else if (field.type === 'number') {
      const number = Number(raw);
      if (!Number.isFinite(number)) return { error: `${field.label} must be a number.` };
      settings[field.key] = number;
    } else if (field.type === 'choice' && field.choices && !field.choices.includes(raw))
      return { error: `${field.label} must be one of ${field.choices.join(', ')}.` };
    else settings[field.key] = raw;
  }
  const start = text(values, CHANNEL_FIELD.quietStart),
    end = text(values, CHANNEL_FIELD.quietEnd);
  if (Boolean(start) !== Boolean(end)) return { error: 'Quiet hours need both a start and an end time.' };
  const quiet_hours: NotificationQuietHours | undefined =
    start && end
      ? {
          start,
          end,
          utc_offset: text(values, CHANNEL_FIELD.quietOffset) || existing?.quiet_hours?.utc_offset || '+00:00',
          allow_errors: existing?.quiet_hours?.allow_errors ?? true,
        }
      : undefined;
  const limit = text(values, CHANNEL_FIELD.maxPerHour);
  let max_per_hour: number | undefined;
  if (limit) {
    max_per_hour = Number(limit);
    if (!Number.isInteger(max_per_hour) || max_per_hour < 1)
      return { error: 'Max per hour must be a whole number above 0.' };
  }
  const label = text(values, CHANNEL_FIELD.label) || undefined;
  return { label, settings, secrets, quiet_hours, max_per_hour };
}

/** A lowercase id derived from a label, unique among `taken` (`phone`, `phone-2`, …). */
export function channelIdFrom(label: string, taken: readonly string[]): string {
  const base =
    label
      .toLowerCase()
      .replace(/[^a-z0-9_-]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 40) || 'channel';
  let id = base;
  for (let n = 2; taken.includes(id); n += 1) id = `${base}-${n}`;
  return id;
}

/**
 * Read the rule form into a request body, or explain the first invalid value. Filters the form
 * does not show (categories, priorities, saved views) are kept from `kept`, the edited rule's
 * filter, so saving here never drops them (HS2-G8NXNC).
 */
export function ruleFromForm(
  values: FormValues,
  eventValues: readonly string[],
  channelValues: readonly string[],
  kept?: NotificationRuleFilter,
): Omit<NotificationRule, 'id'> | { error: string } {
  if (eventValues.length === 0) return { error: 'Choose at least one event.' };
  if (channelValues.length === 0) return { error: 'Choose at least one channel.' };
  const cooldownText = text(values, RULE_FIELD.cooldown);
  const cooldown = cooldownText ? Number(cooldownText) : 0;
  if (!Number.isInteger(cooldown) || cooldown < 0) return { error: 'Cooldown must be a whole number of seconds.' };
  const filter: NotificationRuleFilter = {};
  if (kept?.categories?.length) filter.categories = kept.categories;
  if (kept?.priorities?.length) filter.priorities = kept.priorities;
  if (kept?.views?.length) filter.views = kept.views;
  const tags = list(text(values, RULE_FIELD.tags)),
    statuses = list(text(values, RULE_FIELD.statuses)),
    projects = list(text(values, RULE_FIELD.projects)),
    match = text(values, RULE_FIELD.text),
    severity = text(values, RULE_FIELD.minSeverity);
  if (tags.length) filter.tags = tags;
  if (statuses.length) filter.statuses = statuses;
  if (projects.length) filter.projects = projects;
  if (match) filter.text = match;
  if ((NOTIFICATION_SEVERITIES as readonly string[]).includes(severity))
    filter.min_severity = severity as NotificationRuleFilter['min_severity'];
  return {
    events: [...eventValues],
    channels: [...channelValues],
    enabled: true,
    filter: Object.keys(filter).length ? filter : undefined,
    cooldown_seconds: cooldown,
  };
}

/** The summary a channel sends of what its quiet hours or hourly limit held (HS2-CFMSP2); not a rule event. */
export const HELD_SUMMARY_EVENT = { value: 'notifications.held', label: 'Held Notifications' } as const;

export const eventLabel = (event: string) =>
  [...NOTIFICATION_EVENTS, HELD_SUMMARY_EVENT].find((item) => item.value === event)?.label ?? event;

/** "Blocked, Status Changed → Phone · tag release" */
export function ruleSummary(rule: NotificationRule, channels: readonly NotificationChannel[]): string {
  const names = rule.channels.map((id) => channels.find((c) => c.id === id)?.label ?? id);
  const filters: string[] = [];
  const f = rule.filter;
  if (f?.text) filters.push(`matching “${f.text}”`);
  if (f?.tags?.length) filters.push(`tag ${f.tags.join(', ')}`);
  if (f?.statuses?.length) filters.push(`status ${f.statuses.join(', ')}`);
  if (f?.projects?.length) filters.push(`project ${f.projects.join(', ')}`);
  if (f?.views?.length) filters.push(`view ${f.views.map((v) => `${v.checkout}/${v.view}`).join(', ')}`);
  if (f?.min_severity) filters.push(`${f.min_severity} or higher`);
  if (rule.cooldown_seconds) filters.push(`cooldown ${rule.cooldown_seconds}s`);
  return `To ${names.join(', ')}${filters.length ? ` · ${filters.join(' · ')}` : ''}`;
}

/** The Lucide icon name for a notifier; external plugins share the plugin glyph. */
export function notifierIconName(notifier: string): string {
  switch (notifier) {
    case 'speak':
      return 'volume-2';
    case 'ntfy':
      return 'smartphone';
    case 'email':
      return 'mail';
    case 'webhook':
      return 'webhook';
    case 'slack':
    case 'teams':
      return 'message-square';
    case 'twilio':
      return 'message-circle';
    default:
      return 'puzzle';
  }
}
