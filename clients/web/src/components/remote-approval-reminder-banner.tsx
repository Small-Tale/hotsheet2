import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CalendarClock } from 'lucide';

import { type ApprovalReminder, approvalReminderText } from '../features/remote-approval-reminders';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { ShellBanner } from './shell-banner';

/**
 * A remote approval nears its 90-day limit (HS2-4B0KSD): this browser's own session on a remote-mode
 * host, or this computer's approval on the remote server of the open tab. Renew asks the approving host
 * to renew in place; Dismiss hides the reminder until the next reminder day.
 */
export function RemoteApprovalReminderBanner({ reminder }: { reminder: ApprovalReminder }) {
  const scope = { 'data-remote-scope': reminder.scope };
  return (
    <ShellBanner
      component="remote-approval-reminder-banner"
      title={`Renew access for ${reminder.name}`}
      detail={approvalReminderText(reminder)}
      tone="warning"
      actionPlacement="below"
      icon={<LucideIcon icon={CalendarClock} name="calendar-clock" />}
      action={
        <>
          <button type="button" {...NAVIGATION_AND_TABS_ACTIONS.dismissRemoteApprovalReminder.attrs} {...scope}>
            Dismiss
          </button>
          {reminder.requested ? undefined : (
            <button type="button" {...NAVIGATION_AND_TABS_ACTIONS.renewRemoteApproval.attrs} {...scope}>
              Renew
            </button>
          )}
        </>
      }
    />
  );
}
