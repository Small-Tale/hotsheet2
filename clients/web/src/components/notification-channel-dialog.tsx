import { Button } from '@kerfjs/ui/button';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Switch } from '@kerfjs/ui/switch';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import type { NotificationChannel, NotifierDescriptor } from '../api';
import type { ChannelEditorState } from '../features/notification-settings';
import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import {
  CHANNEL_FIELD,
  type ChannelFormField,
  channelFormFields,
  deviceTimeZone,
  QUIET_TIME_CHOICES,
  quietTimeChoices,
  quietZoneChoices,
} from '../notification-settings-model';

export interface NotificationChannelDialogProps {
  editor: ChannelEditorState;
  notifiers: readonly NotifierDescriptor[];
  /** The channel being edited; absent while adding one. */
  channel?: NotificationChannel;
}

/** One generated control for a schema field. */
function settingControl(field: ChannelFormField) {
  if (field.type === 'bool')
    return <Switch name={field.name} label={field.label} checked={field.checked} hint={field.hint} />;
  if (field.type === 'choice')
    return (
      <Select
        name={field.name}
        label={field.label}
        hint={field.hint}
        value={field.value || field.choices[0] || ''}
        choices={field.choices.map((choice) => ({ value: choice, label: choice }))}
      />
    );
  if (field.type === 'secret')
    return (
      <TextField
        name={field.name}
        label={field.label}
        type="password"
        autocomplete="off"
        placeholder={field.stored ? 'Stored; leave empty to keep it' : ''}
        hint={field.hint}
        required={field.required && !field.stored}
      />
    );
  return (
    <TextField
      name={field.name}
      label={field.label}
      value={field.value}
      type={field.type === 'number' ? 'number' : 'text'}
      hint={field.type === 'list' ? [field.hint, 'Separate values with commas.'].filter(Boolean).join(' ') : field.hint}
      required={field.required}
    />
  );
}

/**
 * The add/edit channel dialog for App Settings → Notifications (HS2-DPBJJA). Its fields come from the
 * notifier's typed settings schema; credential inputs are write-only and stored through the secret
 * route. Saving validates the whole channel, so the dialog submits explicitly.
 */
export function NotificationChannelDialog({ editor, notifiers, channel }: NotificationChannelDialogProps) {
  const descriptor = notifiers.find((item) => item.id === editor.notifier),
    adding = !editor.id,
    fields = descriptor ? channelFormFields(descriptor, channel) : [],
    zone = quietZoneChoices(deviceTimeZone(), channel?.quiet_hours),
    // Quiet-hours selects are controlled: a re-render (a resize, a notifier change) shows what the person
    // chose, not the stored value (HS2-NNC8BG).
    chosen = (name: string, stored: string) => editor.fields?.[name] ?? stored,
    choose = NOTIFICATIONS_AND_LINKS_ACTIONS.chooseNotificationChannelField.attrs,
    // An explicit choice between no quiet time and a daily window (HS2-WVJNYM).
    quiet = chosen(CHANNEL_FIELD.quietMode, channel?.quiet_hours ? 'daily' : 'none') === 'daily';
  return (
    <DialogSurface
      label={adding ? 'Add Channel' : 'Edit Channel'}
      rootAttributes={{
        ...NOTIFICATIONS_AND_LINKS_TARGETS.notificationChannelDialog.attrs,
        'data-controlled-open': 'true',
      }}
      footer={
        <Row gap="xs" hAlign="right" wrap>
          <Button emphasis="plain" action={NOTIFICATIONS_AND_LINKS_ACTIONS.closeNotificationChannel.value}>
            Cancel
          </Button>
          <Button
            emphasis="primary"
            action={NOTIFICATIONS_AND_LINKS_ACTIONS.saveNotificationChannel.value}
            disabled={editor.busy || !descriptor}
          >
            {adding ? 'Add Channel' : 'Save Channel'}
          </Button>
        </Row>
      }
    >
      <List gap="m">
        {adding ? (
          <div {...NOTIFICATIONS_AND_LINKS_ACTIONS.chooseNotificationNotifier.attrs}>
            <Select
              name={CHANNEL_FIELD.notifier}
              label="Notifier"
              value={editor.notifier}
              choices={notifiers.map((item) => ({ value: item.id, label: item.display_name }))}
            />
          </div>
        ) : null}
        {descriptor?.description ? <Text tone="quiet">{descriptor.description}</Text> : null}
        <TextField
          name={CHANNEL_FIELD.label}
          label="Name"
          value={channel?.label ?? ''}
          placeholder={descriptor?.display_name ?? ''}
        />
        {fields.map(settingControl)}
        <ListHeader label="Quiet Hours and Limits" />
        <Select
          name={CHANNEL_FIELD.quietMode}
          label="Quiet Time"
          value={quiet ? 'daily' : 'none'}
          choices={QUIET_TIME_CHOICES}
          hint={
            quiet
              ? 'During quiet hours, rule deliveries are skipped; error notifications still go through.'
              : 'Rule deliveries go out at any time of day.'
          }
          rootAttributes={choose}
        />
        {quiet ? (
          <>
            <Grid minColumnWidth={rem(9)} gap="m">
              <Select
                name={CHANNEL_FIELD.quietStart}
                label="Quiet From"
                value={chosen(CHANNEL_FIELD.quietStart, channel?.quiet_hours?.start ?? '22:00')}
                choices={quietTimeChoices(channel?.quiet_hours?.start)}
                rootAttributes={choose}
              />
              <Select
                name={CHANNEL_FIELD.quietEnd}
                label="Quiet Until"
                value={chosen(CHANNEL_FIELD.quietEnd, channel?.quiet_hours?.end ?? '07:00')}
                choices={quietTimeChoices(channel?.quiet_hours?.end)}
                rootAttributes={choose}
              />
            </Grid>
            {/* Full width so the zone name is never truncated (HS2-9T27WQ). */}
            <Select
              name={CHANNEL_FIELD.quietZone}
              label="Time Zone"
              value={chosen(CHANNEL_FIELD.quietZone, zone.value)}
              choices={zone.choices}
              rootAttributes={choose}
            />
          </>
        ) : null}
        <TextField
          name={CHANNEL_FIELD.maxPerHour}
          label="Max per Hour"
          type="number"
          min={1}
          value={channel?.max_per_hour === undefined ? '' : String(channel.max_per_hour)}
        />
        {editor.error ? (
          <Text tone="danger" role="alert">
            {editor.error}
          </Text>
        ) : null}
      </List>
    </DialogSurface>
  );
}
