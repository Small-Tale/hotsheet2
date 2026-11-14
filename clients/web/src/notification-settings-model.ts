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
  NotificationViewRef,
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
  quietMode: 'notification-channel-quiet-mode',
  quietStart: 'notification-channel-quiet-start',
  quietEnd: 'notification-channel-quiet-end',
  quietZone: 'notification-channel-quiet-zone',
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
  categories: 'notification-rule-categories',
  priorities: 'notification-rule-priorities',
  views: 'notification-rule-views',
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

/**
 * The value a schema choice `Select` shows (HS2-XCZY10): the person's choice in the open dialog while it
 * is still one of the field's choices, else the stored or default value, else the first choice. Kerf
 * `Select` is controlled, so a re-render without the open choice would snap back to the stored value.
 */
export function channelChoiceValue(field: ChannelFormField, chosen?: string): string {
  if (chosen !== undefined && field.choices.includes(chosen)) return chosen;
  return field.value || field.choices[0] || '';
}

/** The device's IANA time zone (HS2-9T27WQ), or `UTC` when the runtime cannot name it. */
export function deviceTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

/** Every IANA zone the runtime knows, or none on an engine without `Intl.supportedValuesOf`. */
export function supportedTimeZones(): readonly string[] {
  const intl = Intl as { supportedValuesOf?: (key: 'timeZone') => string[] };
  return typeof intl.supportedValuesOf === 'function' ? intl.supportedValuesOf('timeZone') : [];
}

/** The channel dialog's explicit quiet-time choice (HS2-WVJNYM): none, or a daily window. */
export const QUIET_TIME_CHOICES = [
  { value: 'none', label: 'No quiet time' },
  { value: 'daily', label: 'Daily quiet hours' },
];

/** A quiet-hours zone choice value that keeps a stored fixed offset (`offset:+09:00`). */
const OFFSET_CHOICE = 'offset:';

export interface QuietZoneChoices {
  value: string;
  choices: { value: string; label: string }[];
}

/**
 * The quiet-hours time zone choices (HS2-9T27WQ): the device's zone first, labeled as local, then UTC and
 * every other known zone. A saved zone stays selected even when this runtime does not list it, and a
 * window saved with only a fixed offset keeps that offset as its own choice until the person picks a zone.
 */
export function quietZoneChoices(
  device: string,
  saved?: NotificationQuietHours,
  zones: readonly string[] = supportedTimeZones(),
): QuietZoneChoices {
  const value = saved ? (saved.time_zone ?? `${OFFSET_CHOICE}${saved.utc_offset ?? '+00:00'}`) : device;
  const choices = [{ value: device, label: `Local time zone (${device.replaceAll('_', ' ')})` }];
  if (value.startsWith(OFFSET_CHOICE))
    choices.push({ value, label: `Fixed offset (UTC${value.slice(OFFSET_CHOICE.length)})` });
  const named = new Set([device]);
  for (const zone of [value, 'UTC', ...zones]) {
    if (zone.startsWith(OFFSET_CHOICE) || named.has(zone)) continue;
    named.add(zone);
    choices.push({ value: zone, label: zone.replaceAll('_', ' ') });
  }
  return { value, choices };
}

/**
 * A time of day (`HH:MM`) in the device's own clock format, 12- or 24-hour as the locale prefers
 * (HS2-NNC8BG). The clock is read in UTC so no zone or daylight-saving change can shift the label.
 */
export function deviceTimeLabel(hhmm: string, locale?: string): string {
  const [hours = 0, minutes = 0] = hhmm.split(':').map(Number);
  return new Intl.DateTimeFormat(locale, { timeStyle: 'short', timeZone: 'UTC' }).format(
    new Date(Date.UTC(2000, 0, 1, hours, minutes)),
  );
}

/**
 * Quiet-hours time choices every 15 minutes, valued `HH:MM` and labeled in the device's clock format
 * (HS2-NNC8BG). A saved time off that grid, such as one set from the CLI, stays a choice in order.
 */
export function quietTimeChoices(
  saved?: string,
  label: (hhmm: string) => string = (hhmm) => deviceTimeLabel(hhmm),
): { value: string; label: string }[] {
  const times: string[] = [];
  for (let minute = 0; minute < 24 * 60; minute += 15)
    times.push(`${String(Math.floor(minute / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`);
  if (saved && /^\d\d:\d\d$/.test(saved) && !times.includes(saved)) times.push(saved);
  return times.sort().map((value) => ({ value, label: label(value) }));
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
  // "No quiet time" saves no window whatever the time fields held; "Daily quiet hours" needs both times
  // (HS2-WVJNYM). Without the choice, the times alone decide.
  const mode = values[CHANNEL_FIELD.quietMode],
    start = mode === 'none' ? '' : text(values, CHANNEL_FIELD.quietStart),
    end = mode === 'none' ? '' : text(values, CHANNEL_FIELD.quietEnd);
  if (Boolean(start) !== Boolean(end) || (mode === 'daily' && !start))
    return { error: 'Quiet hours need both a start and an end time.' };
  const zone = text(values, CHANNEL_FIELD.quietZone) || deviceTimeZone();
  const quiet_hours: NotificationQuietHours | undefined =
    start && end
      ? {
          start,
          end,
          ...(zone.startsWith(OFFSET_CHOICE) ? { utc_offset: zone.slice(OFFSET_CHOICE.length) } : { time_zone: zone }),
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

/** A saved-view checkbox value: `<checkout id>/<view id>` (neither contains a slash). */
export const viewChoiceValue = (view: NotificationViewRef) => `${view.checkout}/${view.view}`;
const viewRefOf = (value: string): NotificationViewRef | undefined => {
  const slash = value.indexOf('/');
  return slash > 0 && slash < value.length - 1
    ? { checkout: value.slice(0, slash), view: value.slice(slash + 1) }
    : undefined;
};

/**
 * Read the rule form into a request body, or explain the first invalid value. A filter the form
 * did not show (an absent field, or `viewValues` undefined) is kept from `kept`, the edited rule's
 * filter, so saving here never drops it (HS2-G8NXNC, HS2-DNQFB8).
 */
export function ruleFromForm(
  values: FormValues,
  eventValues: readonly string[],
  channelValues: readonly string[],
  kept?: NotificationRuleFilter,
  viewValues?: readonly string[],
): Omit<NotificationRule, 'id'> | { error: string } {
  if (eventValues.length === 0) return { error: 'Choose at least one event.' };
  if (channelValues.length === 0) return { error: 'Choose at least one channel.' };
  const cooldownText = text(values, RULE_FIELD.cooldown);
  const cooldown = cooldownText ? Number(cooldownText) : 0;
  if (!Number.isInteger(cooldown) || cooldown < 0) return { error: 'Cooldown must be a whole number of seconds.' };
  const filter: NotificationRuleFilter = {};
  const shown = (name: string) => typeof values[name] === 'string';
  const categories = shown(RULE_FIELD.categories) ? list(text(values, RULE_FIELD.categories)) : kept?.categories,
    priorities = shown(RULE_FIELD.priorities) ? list(text(values, RULE_FIELD.priorities)) : kept?.priorities,
    views = viewValues ? viewValues.flatMap((value) => viewRefOf(value) ?? []) : kept?.views;
  if (categories?.length) filter.categories = categories;
  if (priorities?.length) filter.priorities = priorities;
  if (views?.length) filter.views = views;
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
  if (f?.categories?.length) filters.push(`category ${f.categories.join(', ')}`);
  if (f?.priorities?.length) filters.push(`priority ${f.priorities.join(', ')}`);
  if (f?.views?.length) filters.push(`view ${f.views.map(viewChoiceValue).join(', ')}`);
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
