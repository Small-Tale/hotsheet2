import { signal } from 'kerfjs';

import type {
  CustomView,
  NotificationChannel,
  NotificationChannelRemoval,
  NotificationConfig,
  NotificationDelivery,
  NotificationProjectTiming,
  NotificationRule,
  NotifierList,
} from '../api';
import {
  channelDraftFromForm,
  channelIdFrom,
  type FormValues,
  RULE_FIELD,
  ruleFromForm,
  viewChoiceValue,
} from '../notification-settings-model';

/** The server calls the notification settings controller makes (HS2-DPBJJA). */
export interface NotificationSettingsApi {
  notifiers(): Promise<NotifierList>;
  notificationConfig(): Promise<NotificationConfig>;
  notificationDeliveries(limit?: number): Promise<NotificationDelivery[]>;
  saveNotificationChannel(
    id: string,
    body: Omit<NotificationChannel, 'id' | 'plugin_digest'>,
  ): Promise<NotificationChannel>;
  saveNotificationChannelSecret(id: string, field: string, value: string): Promise<unknown>;
  removeNotificationChannel(id: string): Promise<NotificationChannelRemoval>;
  testNotificationChannel(id: string): Promise<NotificationDelivery>;
  trustNotificationChannel(id: string): Promise<NotificationChannel>;
  saveNotificationRule(id: string, rule: Omit<NotificationRule, 'id'>): Promise<NotificationRule>;
  removeNotificationRule(id: string): Promise<unknown>;
  saveNotificationLinkBase(url: string | null): Promise<unknown>;
  saveNotificationProjectTiming(timing: NotificationProjectTiming): Promise<unknown>;
}

/** The open channel editor: a new channel (`id` absent) or an existing one. */
export interface ChannelEditorState {
  id?: string;
  notifier: string;
  busy: boolean;
  error?: string;
}
/** One open project's saved views, for the rule dialog's saved-view choices (HS2-DNQFB8). */
export interface ProjectSavedViews {
  checkout: string;
  project: string;
  views: readonly CustomView[];
}
/** A saved-view choice: `value` is `<checkout>/<view>`, `label` names the project and view. */
export interface SavedViewChoice {
  value: string;
  label: string;
}
export interface RuleEditorState {
  id?: string;
  busy: boolean;
  error?: string;
}

const messageOf = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * App Settings → Notifications (HS2-DPBJJA): loads the installed notifiers, machine-wide channels and
 * rules, and recent deliveries, and runs every change through the same REST admin workflow the CLI
 * uses. A refresh happens when the category opens and after each change; nothing polls.
 */
export function createNotificationSettings(dependencies: {
  api: () => NotificationSettingsApi | undefined;
  /** Saved views of the open projects; a project that fails to answer is left out. */
  savedViews?: () => Promise<readonly ProjectSavedViews[]>;
}) {
  const notifiers = signal<NotifierList | undefined>(undefined),
    config = signal<NotificationConfig | undefined>(undefined),
    deliveries = signal<NotificationDelivery[] | undefined>(undefined),
    message = signal(''),
    status = signal(''),
    channelEditor = signal<ChannelEditorState | undefined>(undefined),
    ruleEditor = signal<RuleEditorState | undefined>(undefined),
    busyChannels = signal<readonly string[]>([]),
    viewChoices = signal<readonly SavedViewChoice[]>([]);
  let generation = 0;

  async function refresh() {
    const api = dependencies.api();
    if (!api) return;
    const request = ++generation;
    try {
      const [list, current, recent] = await Promise.all([
        api.notifiers(),
        api.notificationConfig(),
        api.notificationDeliveries(20),
      ]);
      if (request !== generation) return;
      notifiers.value = list;
      config.value = current;
      deliveries.value = recent;
      const projects = (await dependencies.savedViews?.().catch(() => [])) ?? [];
      if (request !== generation) return;
      viewChoices.value = projects.flatMap((project) =>
        project.views.map((view) => ({
          value: viewChoiceValue({ checkout: project.checkout, view: view.id }),
          label: `${project.project} · ${view.name}`,
        })),
      );
    } catch (reason) {
      if (request === generation) message.value = messageOf(reason);
    }
  }

  /** Run one change, report its failure in place, and re-read what the server stored. */
  async function change(work: (api: NotificationSettingsApi) => Promise<string | undefined>) {
    const api = dependencies.api();
    if (!api) return false;
    message.value = '';
    status.value = '';
    try {
      const done = await work(api);
      if (done) status.value = done;
      return true;
    } catch (reason) {
      message.value = messageOf(reason);
      return false;
    } finally {
      await refresh();
    }
  }

  function openChannelEditor(id?: string) {
    const existing = id ? config.value?.channels.find((channel) => channel.id === id) : undefined;
    const notifier = existing?.notifier ?? notifiers.value?.notifiers[0]?.id ?? '';
    channelEditor.value = { id: existing?.id, notifier, busy: false };
  }

  function chooseNotifier(notifier: string) {
    const editor = channelEditor.value;
    if (editor && !editor.id) channelEditor.value = { ...editor, notifier, error: undefined };
  }

  function closeChannelEditor() {
    channelEditor.value = undefined;
  }

  async function saveChannel(values: FormValues) {
    const editor = channelEditor.value,
      api = dependencies.api(),
      descriptor = notifiers.value?.notifiers.find((item) => item.id === editor?.notifier);
    if (!editor || !api || !descriptor) return;
    const existing = editor.id ? config.value?.channels.find((channel) => channel.id === editor.id) : undefined;
    const draft = channelDraftFromForm(descriptor, values, existing);
    if ('error' in draft) {
      channelEditor.value = { ...editor, error: draft.error };
      return;
    }
    channelEditor.value = { ...editor, busy: true, error: undefined };
    const taken = config.value?.channels.map((channel) => channel.id) ?? [];
    const id = editor.id ?? channelIdFrom(draft.label ?? descriptor.display_name, taken);
    try {
      await api.saveNotificationChannel(id, {
        notifier: descriptor.id,
        label: draft.label,
        enabled: existing?.enabled ?? true,
        settings: draft.settings,
        quiet_hours: draft.quiet_hours,
        max_per_hour: draft.max_per_hour,
      });
      for (const [field, value] of Object.entries(draft.secrets))
        await api.saveNotificationChannelSecret(id, field, value);
      channelEditor.value = undefined;
      status.value = editor.id ? 'Channel saved.' : 'Channel added.';
    } catch (reason) {
      channelEditor.value = { ...editor, busy: false, error: messageOf(reason) };
    }
    await refresh();
  }

  async function withChannel(id: string, work: (api: NotificationSettingsApi) => Promise<string | undefined>) {
    busyChannels.value = [...busyChannels.value, id];
    try {
      return await change(work);
    } finally {
      busyChannels.value = busyChannels.value.filter((busy) => busy !== id);
    }
  }

  const testChannel = (id: string) =>
    withChannel(id, async (api) => {
      const record = await api.testNotificationChannel(id);
      if (record.outcome !== 'delivered') throw new Error(record.error ?? 'The test notification was not delivered.');
      return 'Test notification sent.';
    });

  const setChannelEnabled = (id: string, enabled: boolean) =>
    withChannel(id, async (api) => {
      const channel = config.value?.channels.find((item) => item.id === id);
      if (!channel) return undefined;
      await api.saveNotificationChannel(id, {
        notifier: channel.notifier,
        label: channel.label,
        enabled,
        settings: channel.settings,
        quiet_hours: channel.quiet_hours,
        max_per_hour: channel.max_per_hour,
      });
      return enabled ? 'Channel enabled.' : 'Channel disabled.';
    });

  const trustChannel = (id: string) =>
    withChannel(id, async (api) => {
      await api.trustNotificationChannel(id);
      return 'Plugin approved.';
    });

  const removeChannel = (id: string) =>
    withChannel(id, async (api) => {
      const removal = await api.removeNotificationChannel(id);
      return removal.removed_rules.length
        ? `Channel removed with ${removal.removed_rules.length === 1 ? '1 rule' : `${removal.removed_rules.length} rules`} that used only it.`
        : 'Channel removed.';
    });

  function openRuleEditor(id?: string) {
    ruleEditor.value = { id, busy: false };
  }

  function closeRuleEditor() {
    ruleEditor.value = undefined;
  }

  /** `views` is the checked saved-view values, or undefined when the dialog showed no choices. */
  async function saveRule(
    values: FormValues,
    events: readonly string[],
    channels: readonly string[],
    views?: readonly string[],
  ) {
    const editor = ruleEditor.value,
      api = dependencies.api();
    if (!editor || !api) return;
    const kept = editor.id ? config.value?.rules.find((item) => item.id === editor.id)?.filter : undefined;
    const rule = ruleFromForm(values, events, channels, kept, views);
    if ('error' in rule) {
      ruleEditor.value = { ...editor, error: rule.error };
      return;
    }
    ruleEditor.value = { ...editor, busy: true, error: undefined };
    const taken = config.value?.rules.map((item) => item.id) ?? [];
    const named = values[RULE_FIELD.label],
      label = typeof named === 'string' ? named.trim() : '';
    const id = editor.id ?? channelIdFrom(label || 'rule', taken);
    try {
      await api.saveNotificationRule(id, rule);
      ruleEditor.value = undefined;
      status.value = editor.id ? 'Rule saved.' : 'Rule added.';
    } catch (reason) {
      ruleEditor.value = { ...editor, busy: false, error: messageOf(reason) };
    }
    await refresh();
  }

  const removeRule = (id: string) =>
    change(async (api) => {
      await api.removeNotificationRule(id);
      return 'Rule removed.';
    });

  /** Save one delivery option; an empty link origin clears it, an empty number keeps the default. */
  function saveOption(name: 'link-base' | 'grace' | 'idle', raw: string) {
    const value = raw.trim();
    return change(async (api) => {
      if (name === 'link-base') {
        await api.saveNotificationLinkBase(value || null);
        return 'Link origin saved.';
      }
      const number = Number(value);
      if (!value || !Number.isInteger(number) || number < 0) throw new Error('Enter a whole number.');
      await api.saveNotificationProjectTiming(name === 'grace' ? { grace_seconds: number } : { idle_minutes: number });
      return 'Project timing saved.';
    });
  }

  return {
    notifiers,
    config,
    deliveries,
    message,
    status,
    channelEditor,
    ruleEditor,
    busyChannels,
    viewChoices,
    refresh,
    openChannelEditor,
    chooseNotifier,
    closeChannelEditor,
    saveChannel,
    testChannel,
    setChannelEnabled,
    trustChannel,
    removeChannel,
    openRuleEditor,
    closeRuleEditor,
    saveRule,
    removeRule,
    saveOption,
  };
}

/** The notification settings controller. */
export type NotificationSettings = ReturnType<typeof createNotificationSettings>;
