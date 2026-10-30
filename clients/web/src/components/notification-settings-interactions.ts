import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { NotificationSettings } from '../features/notification-settings';
import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import type { Control } from '../interactions/types';
import { CHANNEL_FIELD, OPTIONS_FIELD, RULE_FIELD } from '../notification-settings-model';

/**
 * Delegated handlers for App Settings → Notifications (HS2-DPBJJA): options save on change, rows and
 * menus open, test, toggle, approve, and remove, and the channel and rule dialogs submit explicitly.
 * The application and the catalog demo install the same handlers over the same controller.
 */
export function wireNotificationSettings(root: HTMLElement, settings: NotificationSettings): () => void {
  const lifetime = createScope();
  const notificationId = (target: Element, row: string, item: string) =>
    target.closest<HTMLElement>(`[${row}]`)?.getAttribute(row) ??
    target.getAttribute(item) ??
    target.getAttribute('data-item-id') ??
    undefined;
  const channelOf = (target: Element) => notificationId(target, 'data-notification-channel', 'data-channel-id');
  const ruleOf = (target: Element) => notificationId(target, 'data-notification-rule', 'data-rule-id');
  /** A dialog's named controls: text values by name, a lone checkbox as a boolean, and checkbox groups listed. */
  function dialogValues(dialog: Element) {
    const values: Record<string, string | boolean> = {},
      checked: Record<string, string[]> = {};
    for (const control of dialog.querySelectorAll<Control>('[name]')) {
      const name = control.getAttribute('name') ?? '';
      // A Select's own inner controls belong to it; read the Select itself.
      if (control.parentElement?.closest('kui-select')) continue;
      if (control instanceof HTMLInputElement && control.type === 'checkbox') {
        if (control.closest('fieldset')) {
          checked[name] ??= [];
          if (control.checked) checked[name].push(control.value);
        } else values[name] = control.checked;
      } else values[name] = control.value;
    }
    return { values, checked };
  }
  lifetime.add(
    delegate(root, 'change', NOTIFICATIONS_AND_LINKS_ACTIONS.saveNotificationOption.selector, (event) => {
      const control = event.target as Control | null,
        name = control?.getAttribute('name');
      const option =
        name === OPTIONS_FIELD.linkBase
          ? 'link-base'
          : name === OPTIONS_FIELD.graceSeconds
            ? 'grace'
            : name === OPTIONS_FIELD.idleMinutes
              ? 'idle'
              : undefined;
      if (control && option) void settings.saveOption(option, control.value);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.addNotificationChannel.selector, () => {
      settings.openChannelEditor();
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.editNotificationChannel.selector, (_event, target) => {
      const id = channelOf(target);
      if (id && settings.config.value?.channels.some((channel) => channel.id === id)) settings.openChannelEditor(id);
    }),
  );
  lifetime.add(
    delegate(root, 'change', NOTIFICATIONS_AND_LINKS_ACTIONS.chooseNotificationNotifier.selector, (_event, target) => {
      const value = target.querySelector<Control>(`[name="${CHANNEL_FIELD.notifier}"]`)?.value;
      if (value) settings.chooseNotifier(value);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.saveNotificationChannel.selector, (_event, target) => {
      const dialog = target.closest(NOTIFICATIONS_AND_LINKS_TARGETS.notificationChannelDialog.selector);
      if (dialog) void settings.saveChannel(dialogValues(dialog).values);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.closeNotificationChannel.selector, () => {
      settings.closeChannelEditor();
    }),
  );
  lifetime.add(
    delegateCapture(
      root,
      'kui-request-close',
      NOTIFICATIONS_AND_LINKS_TARGETS.notificationChannelDialog.selector,
      () => {
        settings.closeChannelEditor();
      },
    ),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.testNotificationChannel.selector, (_event, target) => {
      const id = channelOf(target);
      if (id) void settings.testChannel(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.toggleNotificationChannel.selector, (_event, target) => {
      const id = channelOf(target),
        channel = settings.config.value?.channels.find((item) => item.id === id);
      if (channel) void settings.setChannelEnabled(channel.id, !channel.enabled);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.trustNotificationChannel.selector, (_event, target) => {
      const id = channelOf(target);
      if (id) void settings.trustChannel(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.removeNotificationChannel.selector, (_event, target) => {
      const id = channelOf(target);
      if (id) void settings.removeChannel(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.addNotificationRule.selector, () => {
      settings.openRuleEditor();
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.editNotificationRule.selector, (_event, target) => {
      const id = ruleOf(target);
      if (id && settings.config.value?.rules.some((rule) => rule.id === id)) settings.openRuleEditor(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.saveNotificationRule.selector, (_event, target) => {
      const dialog = target.closest(NOTIFICATIONS_AND_LINKS_TARGETS.notificationRuleDialog.selector);
      if (!dialog) return;
      const { values, checked } = dialogValues(dialog);
      void settings.saveRule(
        values,
        checked[RULE_FIELD.events] ?? [],
        checked[RULE_FIELD.channels] ?? [],
        checked[RULE_FIELD.views],
      );
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.closeNotificationRule.selector, () => {
      settings.closeRuleEditor();
    }),
  );
  lifetime.add(
    delegateCapture(root, 'kui-request-close', NOTIFICATIONS_AND_LINKS_TARGETS.notificationRuleDialog.selector, () => {
      settings.closeRuleEditor();
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.removeNotificationRule.selector, (_event, target) => {
      const id = ruleOf(target);
      if (id) void settings.removeRule(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.refreshNotificationDeliveries.selector, () => {
      void settings.refresh();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
